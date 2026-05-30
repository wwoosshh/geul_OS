//! Claude 도구 정의 + dispatch — probe.py의 TOOLS와 동등 + subscribe/drain 추가.

use serde_json::{json, Value};

use crate::adapter::ToolDef;
use crate::error::{BridgeError, BridgeResult};
use crate::wire::WireClient;

/// 표준 6개 도구 정의.
pub fn standard_tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "list_objects_by_type".to_string(),
            description: "List all object IDs matching a type URI. \
                          Standard types: aios.std/Container@1, aios.std/Text@1, \
                          aios.std/Button@1, aios.std/Toggle@1."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "type_uri": {"type": "string"}
                },
                "required": ["type_uri"]
            }),
        },
        ToolDef {
            name: "get_object".to_string(),
            description: "Fetch object details. Optional `fields` (top-level only: \
                          'state'/'props'/'methods'/'acl'/'children'/'type_uri'/'name') to \
                          limit response and save tokens. Omit `fields` for full object."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "object_id": {"type": "string"},
                    "fields": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Subset of top-level fields. e.g. ['state'] for read result only."
                    }
                },
                "required": ["object_id"]
            }),
        },
        ToolDef {
            name: "invoke_method".to_string(),
            description: "Invoke a method on an object. Returns event_id. \
                          For read-only methods (`read`, `read_external`) the result is returned \
                          inline as `state`. For mutation methods (`save`, `delete`, `rename`, \
                          `create_file`, `create_folder`) a `status` field appears: `completed` \
                          (user approved + state changed) or `awaiting_user` (Dialog pending, \
                          retry get_object later). `create_*` also returns `new_child_id` when \
                          unambiguous. No separate polling needed in the happy path."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string"},
                    "method": {"type": "string"},
                    "args": {}
                },
                "required": ["target", "method"]
            }),
        },
        ToolDef {
            name: "subscribe".to_string(),
            description: "Subscribe to events on an object. \
                          Kinds: Invoke, StateSet, Lifecycle, ChildChange. \
                          Returns subscription_id."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string"},
                    "kinds": {
                        "type": "array",
                        "items": {"type": "string", "enum": ["Invoke", "StateSet", "Lifecycle", "ChildChange"]}
                    }
                },
                "required": ["target", "kinds"]
            }),
        },
        ToolDef {
            name: "drain".to_string(),
            description: "Drain queued events for a subscription (returns up to ~150ms worth)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "subscription_id": {"type": "string"}
                },
                "required": ["subscription_id"]
            }),
        },
        ToolDef {
            name: "report_done".to_string(),
            description: "Call exactly once when finished. Keep `summary` to ≤2 short sentences \
                          (~30 words). The detailed reply lives in your ai_text response; the \
                          summary is just a one-line action log."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "summary": {"type": "string"}
                },
                "required": ["summary"]
            }),
        },
    ]
}

/// 한 도구 호출을 dispatch. report_done은 특별 처리 (`DispatchResult::Done`).
pub async fn dispatch_tool(
    wire: &mut WireClient,
    name: &str,
    input: &Value,
) -> BridgeResult<DispatchResult> {
    use geulos_proto::EventKindFilterWire;

    match name {
        "list_objects_by_type" => {
            let t = input.get("type_uri").and_then(|v| v.as_str()).unwrap_or("");
            let ids = wire.query_by_type(t).await?;
            // M11.1 후속: AI 효율성 위해 각 id의 *기본 props (name/path/type_uri)*를
            // inline. 기존엔 ID만 반환해 AI가 path 매칭 위해 get_object N번 호출
            // → turn 폭주 (사용자 JSONL 진단). 이제 한 번 호출로 path 매칭 가능.
            //
            // 기존 호환 위해 object_ids 필드도 유지.
            let mut objects: Vec<Value> = Vec::with_capacity(ids.len());
            for id in &ids {
                match wire.get_object(id).await {
                    Ok(obj) => {
                        // props에서 name/path만 추출 (전체 props 보내면 응답 비대).
                        let name = obj
                            .get("props")
                            .and_then(|p| p.get("name"))
                            .cloned()
                            .unwrap_or(Value::Null);
                        let path = obj
                            .get("props")
                            .and_then(|p| p.get("path"))
                            .cloned()
                            .unwrap_or(Value::Null);
                        let type_uri = obj.get("type_uri").cloned().unwrap_or(Value::Null);
                        objects.push(json!({
                            "id": id,
                            "type_uri": type_uri,
                            "name": name,
                            "path": path,
                        }));
                    }
                    Err(_) => {
                        // 어느 id의 get이 실패해도 ID 자체는 결과에 포함 (AI가 fallback).
                        objects.push(json!({ "id": id, "error": "get_object 실패" }));
                    }
                }
            }
            Ok(DispatchResult::Output(json!({
                "object_ids": ids,  // 기존 호환
                "objects": objects, // 신규 — id + 기본 props
            })))
        }
        "get_object" => {
            let id = input.get("object_id").and_then(|v| v.as_str()).unwrap_or("");
            // B: top-level fields 필터 — 명시되면 그 키만 응답에 포함 (토큰 절감).
            let fields: Vec<String> = input
                .get("fields")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            match wire.get_object(id).await {
                Ok(obj) => {
                    let filtered = if fields.is_empty() {
                        obj
                    } else if let Value::Object(map) = &obj {
                        let mut out = serde_json::Map::new();
                        for k in &fields {
                            if let Some(v) = map.get(k) {
                                out.insert(k.clone(), v.clone());
                            }
                        }
                        Value::Object(out)
                    } else {
                        obj
                    };
                    Ok(DispatchResult::Output(json!({ "object": filtered })))
                }
                Err(e) => Ok(DispatchResult::Output(json!({ "error": e.to_string() }))),
            }
        }
        "invoke_method" => {
            let target = input.get("target").and_then(|v| v.as_str()).unwrap_or("");
            let method = input.get("method").and_then(|v| v.as_str()).unwrap_or("");
            let args = input.get("args").cloned().unwrap_or(Value::Null);
            let path_arg = args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            // rename_external은 path 대신 from/to. mutation polling 매칭에 사용.
            let to_arg = args
                .get("to")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // mutation은 Dialog 승인 대기 — invoke 전에 target snapshot을 찍어 두고
            // polling 시 변화 비교에 사용 (save: dirty flip, delete: destroyed,
            // rename: props.name 변화, create_*: state.child_count 증가).
            let is_mutation = matches!(
                method,
                "save"
                    | "delete"
                    | "rename"
                    | "create_file"
                    | "create_folder"
                    | "write_external"
                    | "delete_external"
                    | "rename_external"
            );
            let pre_snapshot = if is_mutation {
                wire.get_object(target).await.ok()
            } else {
                None
            };
            // method별 매칭 인자 — path / from / to.
            let write_external_path = if method == "write_external" { path_arg.clone() } else { String::new() };
            let delete_external_path = if method == "delete_external" { path_arg.clone() } else { String::new() };
            let rename_external_to = if method == "rename_external" { to_arg.clone() } else { String::new() };

            match wire.invoke(target, method, args).await {
                Ok(eid) => {
                    // A (read): inline state. wire.invoke 완료는 *InvokeAck* 시점 —
                    // 실제 handler의 SetState broadcast 도착까지 짧은 polling으로 race 흡수.
                    let auto_fetch_read = matches!(method, "read_external" | "read");
                    if auto_fetch_read {
                        for _ in 0..20 {
                            if let Ok(obj) = wire.get_object(target).await {
                                let state = obj.get("state").cloned().unwrap_or(Value::Null);
                                let ready = match method {
                                    "read_external" => {
                                        state.get("last_read_path").and_then(|v| v.as_str())
                                            == Some(path_arg.as_str())
                                            && state
                                                .get("last_read_content")
                                                .map(|v| !v.is_null())
                                                .unwrap_or(false)
                                    }
                                    "read" => state
                                        .get("content")
                                        .map(|v| !v.is_null())
                                        .unwrap_or(false),
                                    _ => true,
                                };
                                if ready {
                                    return Ok(DispatchResult::Output(json!({
                                        "ok": true,
                                        "event_id": eid,
                                        "state": state,
                                    })));
                                }
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        }
                        if let Ok(obj) = wire.get_object(target).await {
                            let state = obj.get("state").cloned().unwrap_or(Value::Null);
                            return Ok(DispatchResult::Output(json!({
                                "ok": true,
                                "event_id": eid,
                                "state": state,
                                "stale": true,
                            })));
                        }
                    }

                    // mutation polling: 사용자 Dialog 응답 대기.
                    // 2초/100ms 간격 — 즉시 [허용] 시나리오만 inline 반환, timeout 시
                    // status="awaiting_user" + AI가 후속 turn에서 polling. spec 결정 C.
                    if is_mutation {
                        for _ in 0..20 {
                            if let Ok(cur) = wire.get_object(target).await {
                                if let Some(status) =
                                    check_mutation_ready(
                                        method,
                                        &pre_snapshot,
                                        &cur,
                                        &write_external_path,
                                        &delete_external_path,
                                        &rename_external_to,
                                    )
                                {
                                    let mut out = json!({
                                        "ok": true,
                                        "event_id": eid,
                                        "status": status,
                                        "state": cur.get("state").cloned().unwrap_or(Value::Null),
                                    });
                                    // create_*은 새 child id 식별 (parent.children diff).
                                    if matches!(method, "create_file" | "create_folder") {
                                        if let (Some(pre), Some(out_obj)) =
                                            (pre_snapshot.as_ref(), out.as_object_mut())
                                        {
                                            let new_ids = diff_children(pre, &cur);
                                            if new_ids.len() == 1 {
                                                out_obj.insert(
                                                    "new_child_id".to_string(),
                                                    json!(new_ids[0]),
                                                );
                                            } else if new_ids.len() > 1 {
                                                out_obj.insert(
                                                    "new_child_ids".to_string(),
                                                    json!(new_ids),
                                                );
                                                out_obj
                                                    .insert("ambiguous".to_string(), json!(true));
                                            }
                                        }
                                    }
                                    return Ok(DispatchResult::Output(out));
                                }
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                        // 2초 timeout — 사용자 미응답. AI에게 명시 pending 신호.
                        return Ok(DispatchResult::Output(json!({
                            "ok": true,
                            "event_id": eid,
                            "status": "awaiting_user",
                            "hint": "사용자가 아직 Dialog에 응답 안 함. 다음 turn에서 get_object로 재확인하거나 사용자에게 안내.",
                        })));
                    }

                    Ok(DispatchResult::Output(json!({ "ok": true, "event_id": eid })))
                }
                Err(e) => {
                    Ok(DispatchResult::Output(json!({ "ok": false, "error": e.to_string() })))
                }
            }
        }
        "subscribe" => {
            let target = input.get("target").and_then(|v| v.as_str()).unwrap_or("");
            let kinds_arr =
                input.get("kinds").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            let mut kinds = Vec::new();
            for k in &kinds_arr {
                if let Some(s) = k.as_str() {
                    let kf = match s {
                        "Invoke" => EventKindFilterWire::Invoke,
                        "StateSet" => EventKindFilterWire::StateSet,
                        "Lifecycle" => EventKindFilterWire::Lifecycle,
                        "ChildChange" => EventKindFilterWire::ChildChange,
                        _ => continue,
                    };
                    kinds.push(kf);
                }
            }
            let sid = wire.subscribe(target, &kinds).await?;
            Ok(DispatchResult::Output(json!({ "subscription_id": sid })))
        }
        "drain" => {
            let sid = input.get("subscription_id").and_then(|v| v.as_str()).unwrap_or("");
            let events = wire.drain(sid).await?;
            Ok(DispatchResult::Output(json!({ "events": events })))
        }
        "report_done" => {
            let summary = input.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Ok(DispatchResult::Done { summary })
        }
        other => Err(BridgeError::Config(format!("unknown tool: {}", other))),
    }
}

/// 도구 호출 결과.
#[derive(Debug)]
pub enum DispatchResult {
    /// 정상 결과 (JSON value).
    Output(Value),
    /// `report_done` 호출 — 세션 종료 신호.
    Done {
        /// 모델이 제공한 요약.
        summary: String,
    },
}

/// mutation polling의 ready 판정 — pre/cur snapshot 비교.
///
/// 반환: `Some("completed")` / `Some("rejected")` / `None` (아직 결과 미확정).
/// rejected 감지는 V2 — 현재는 ChildChange/state 변화로만 completed 판정.
/// timeout 시 caller가 "awaiting_user" 반환.
fn check_mutation_ready(
    method: &str,
    pre: &Option<Value>,
    cur: &Value,
    write_external_path: &str,
    delete_external_path: &str,
    rename_external_to: &str,
) -> Option<&'static str> {
    let cur_state = cur.get("state");
    match method {
        "save" => {
            // dirty가 true → false로 변경되면 save 완료.
            let pre_dirty = pre
                .as_ref()
                .and_then(|p| p.get("state").and_then(|s| s.get("dirty")).and_then(|v| v.as_bool()))
                .unwrap_or(true);
            let cur_dirty = cur_state
                .and_then(|s| s.get("dirty"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if pre_dirty && !cur_dirty {
                Some("completed")
            } else {
                None
            }
        }
        "delete" => {
            let destroyed = cur
                .get("destroyed")
                .and_then(|v| v.as_bool())
                .or_else(|| {
                    cur_state.and_then(|s| s.get("destroyed")).and_then(|v| v.as_bool())
                })
                .unwrap_or(false);
            if destroyed {
                Some("completed")
            } else {
                None
            }
        }
        "rename" => {
            let pre_name = pre
                .as_ref()
                .and_then(|p| p.get("props").and_then(|p| p.get("name"))).cloned();
            let cur_name = cur.get("props").and_then(|p| p.get("name")).cloned();
            if pre_name.is_some() && pre_name != cur_name {
                Some("completed")
            } else {
                None
            }
        }
        "create_file" | "create_folder" => {
            // target은 parent Folder. child_count 또는 children 길이 증가 감지.
            let pre_count = pre
                .as_ref()
                .and_then(|p| {
                    p.get("state").and_then(|s| s.get("child_count")).and_then(|v| v.as_u64())
                })
                .or_else(|| {
                    pre.as_ref()
                        .and_then(|p| p.get("children").and_then(|c| c.as_array()))
                        .map(|a| a.len() as u64)
                });
            let cur_count = cur_state
                .and_then(|s| s.get("child_count"))
                .and_then(|v| v.as_u64())
                .or_else(|| cur.get("children").and_then(|c| c.as_array()).map(|a| a.len() as u64));
            match (pre_count, cur_count) {
                (Some(p), Some(c)) if c > p => Some("completed"),
                _ => None,
            }
        }
        "write_external" => {
            // Filesystem@1.state.last_write_path가 args.path와 일치하면 승인+write 완료.
            let last_write =
                cur_state.and_then(|s| s.get("last_write_path")).and_then(|v| v.as_str());
            if last_write == Some(write_external_path) {
                Some("completed")
            } else {
                None
            }
        }
        "delete_external" => {
            let last_delete =
                cur_state.and_then(|s| s.get("last_delete_path")).and_then(|v| v.as_str());
            if last_delete == Some(delete_external_path) {
                Some("completed")
            } else {
                None
            }
        }
        "rename_external" => {
            let last_rename_to =
                cur_state.and_then(|s| s.get("last_rename_to_path")).and_then(|v| v.as_str());
            if last_rename_to == Some(rename_external_to) {
                Some("completed")
            } else {
                None
            }
        }
        _ => None,
    }
}

/// create_*의 새 child id 식별 — pre/cur children id 집합 diff.
fn diff_children(pre: &Value, cur: &Value) -> Vec<String> {
    let pre_ids: std::collections::HashSet<String> = pre
        .get("children")
        .and_then(|c| c.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let cur_ids: Vec<String> = cur
        .get("children")
        .and_then(|c| c.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    cur_ids.into_iter().filter(|id| !pre_ids.contains(id)).collect()
}
