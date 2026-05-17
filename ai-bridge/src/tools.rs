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
            description: "Fetch full details (props, state, methods, ACL) of an object by UUID."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "object_id": {"type": "string"}
                },
                "required": ["object_id"]
            }),
        },
        ToolDef {
            name: "invoke_method".to_string(),
            description: "Invoke a method on an object. Returns event_id or error info."
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
            description: "Call exactly once when finished. Provide a 3-5 sentence summary."
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
            Ok(DispatchResult::Output(json!({ "object_ids": ids })))
        }
        "get_object" => {
            let id = input.get("object_id").and_then(|v| v.as_str()).unwrap_or("");
            match wire.get_object(id).await {
                Ok(obj) => Ok(DispatchResult::Output(json!({ "object": obj }))),
                Err(e) => Ok(DispatchResult::Output(json!({ "error": e.to_string() }))),
            }
        }
        "invoke_method" => {
            let target = input.get("target").and_then(|v| v.as_str()).unwrap_or("");
            let method = input.get("method").and_then(|v| v.as_str()).unwrap_or("");
            let args = input.get("args").cloned().unwrap_or(Value::Null);
            match wire.invoke(target, method, args).await {
                Ok(eid) => Ok(DispatchResult::Output(json!({ "ok": true, "event_id": eid }))),
                Err(e) => Ok(DispatchResult::Output(json!({ "ok": false, "error": e.to_string() }))),
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
            let summary =
                input.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string();
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
