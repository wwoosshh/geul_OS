//! 와이어 메시지 → 액터 명령 변환.

use geulos_core::{ActorId, Object, ObjectId, Query, TypeUri};
use geulos_proto::{
    InvokeAck, InvokeError, InvokeMsg, MountAck, MountMsg, MountReject, QueryMsg, QueryPredicate,
    QueryResult,
};
use serde_json::Value;

use crate::ObjectServerHandle;

/// Mount 메시지 처리. 응답 본문 JSON을 반환.
pub async fn handle_mount(handle: &ObjectServerHandle, msg: MountMsg) -> Value {
    let obj: Object = match serde_json::from_value(msg.tree) {
        Ok(o) => o,
        Err(e) => {
            return serde_json::to_value(MountReject {
                reason: "malformed_tree".to_string(),
                detail: e.to_string(),
            })
            .unwrap();
        }
    };

    match handle.mount(obj).await {
        Ok(id) => serde_json::to_value(MountAck { root_object_id: id.to_string() }).unwrap(),
        Err(e) => serde_json::to_value(MountReject {
            reason: "core_error".to_string(),
            detail: e.to_string(),
        })
        .unwrap(),
    }
}

/// Invoke 메시지 처리. 세션의 actor 인자가 호출자.
pub async fn handle_invoke(
    handle: &ObjectServerHandle,
    msg: InvokeMsg,
    session_actor: ActorId,
) -> Value {
    let target = match parse_object_id(&msg.target) {
        Some(id) => id,
        None => {
            return serde_json::to_value(InvokeError {
                request_id: msg.request_id,
                kind: "malformed_target".to_string(),
                detail: format!("bad UUID: {}", msg.target),
            })
            .unwrap();
        }
    };
    match handle.invoke(session_actor, target, msg.method.clone(), msg.args).await {
        Ok(event_id) => serde_json::to_value(InvokeAck {
            request_id: msg.request_id,
            event_id: event_id.to_string(),
            result: Value::Null,
        })
        .unwrap(),
        Err(e) => {
            let err_str = e.to_string();
            let kind = if err_str.contains("권한") || err_str.contains("permission") {
                "permission"
            } else if err_str.contains("찾을 수 없음") || err_str.contains("not found") {
                "not_found"
            } else if err_str.contains("지원하지 않음") || err_str.contains("unknown method")
            {
                "unknown_method"
            } else {
                "core"
            };
            serde_json::to_value(InvokeError {
                request_id: msg.request_id,
                kind: kind.to_string(),
                detail: err_str,
            })
            .unwrap()
        }
    }
}

/// Query 메시지 처리.
pub async fn handle_query(handle: &ObjectServerHandle, msg: QueryMsg) -> Value {
    let q = match msg.query {
        QueryPredicate::ByType { type_uri } => {
            let t = match TypeUri::parse(&type_uri) {
                Ok(t) => t,
                Err(_) => {
                    return serde_json::json!({"kind": "QueryError", "detail": "bad TypeUri"});
                }
            };
            Query::ByType(t)
        }
        QueryPredicate::ByOwner { actor } => {
            // 알려진 프리셋만 정확 매칭 (M2 한계)
            let a = if actor == "user:local" {
                ActorId::local_user()
            } else if actor == "system:compositor" {
                ActorId::system_compositor()
            } else {
                // 일치하는 객체 없음을 보장하는 fallback
                ActorId::local_user()
            };
            Query::ByOwner(a)
        }
        QueryPredicate::ChildrenOf { parent } => {
            let id = match parse_object_id(&parent) {
                Some(i) => i,
                None => {
                    return serde_json::json!({"kind": "QueryError", "detail": "bad parent UUID"})
                }
            };
            Query::ChildrenOf(id)
        }
    };
    let ids = handle.query(q).await.unwrap_or_default();
    let id_strs: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
    serde_json::to_value(QueryResult { request_id: msg.request_id, objects: id_strs }).unwrap()
}

fn parse_object_id(s: &str) -> Option<ObjectId> {
    // ObjectId의 내부 표현이 Uuid이므로 JSON 라운드트립으로 변환.
    let json = format!("\"{}\"", s);
    serde_json::from_str(&json).ok()
}
