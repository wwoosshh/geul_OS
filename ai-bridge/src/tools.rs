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
            description: "Invoke a method on an object. Returns event_id. For read-only methods \
                          (`read`, `read_external`) the result is also returned inline as \
                          `state` — no separate get_object polling needed."
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
            match wire.invoke(target, method, args).await {
                Ok(eid) => {
                    // A: read-only 메서드는 결과를 inline 반환해 폴링 turn 제거.
                    // wire.invoke 완료는 *InvokeAck* 시점 — 실제 handler의 SetState broadcast가
                    // 아직 도착 안 했을 수 있음 (race). 짧은 polling으로 state 갱신 대기.
                    let auto_fetch = matches!(method, "read_external" | "read");
                    if auto_fetch {
                        // 최대 20×10ms = 200ms 대기. 일반적으로 1-3 iter에 잡힘.
                        for _ in 0..20 {
                            if let Ok(obj) = wire.get_object(target).await {
                                let state = obj.get("state").cloned().unwrap_or(Value::Null);
                                let ready = match method {
                                    "read_external" => {
                                        // last_read_path가 args.path와 일치 + content non-null.
                                        state
                                            .get("last_read_path")
                                            .and_then(|v| v.as_str())
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
                        // timeout — best-effort state 반환 + stale 표시.
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
