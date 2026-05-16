//! 한 클라이언트 연결의 read/write 루프 + 이벤트 푸시.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use geulos_core::{ActorId, EventKindFilter, ObjectId, SubscriptionId};
use geulos_proto::{
    decode_frame, encode_frame, DecodeError, EventKindFilterWire, EventMsg, GetMsg, Hello,
    HelloAck, HelloReject, InvokeMsg, MountMsg, QueryMsg, Role, StateSetMsg, SubscribeAck,
    SubscribeMsg, UnsubscribeMsg,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::dispatch::{handle_get, handle_invoke, handle_mount, handle_query, handle_state_set};
use crate::ObjectServerHandle;

/// 한 연결의 구독 매핑: 클라이언트 subscription_id → 서버 SubscriptionId.
type SubMap = Arc<Mutex<HashMap<String, SubscriptionId>>>;

pub async fn handle_connection(stream: TcpStream, handle: ObjectServerHandle) {
    let (mut reader, writer) = stream.into_split();
    let writer = Arc::new(Mutex::new(writer));
    let sub_map: SubMap = Arc::new(Mutex::new(HashMap::new()));

    // 핸드셰이크
    let actor_id = match read_and_handle_hello_split(&mut reader, &writer).await {
        Ok(id) => id,
        Err(e) => {
            eprintln!("handshake failed: {}", e);
            return;
        }
    };

    // 푸시 task: 100ms마다 모든 구독을 drain → EventMsg로 보냄
    let push_handle = handle.clone();
    let push_sub_map = sub_map.clone();
    let push_writer = writer.clone();
    let push_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let subs: Vec<(String, SubscriptionId)> = {
                let m = push_sub_map.lock().await;
                m.iter().map(|(k, v)| (k.clone(), *v)).collect()
            };
            for (client_sub_id, server_sub_id) in subs {
                let evs = push_handle.drain(server_sub_id).await.unwrap_or_default();
                for ev in evs {
                    let msg = EventMsg {
                        subscription_id: client_sub_id.clone(),
                        event: serde_json::to_value(&ev).unwrap_or(serde_json::Value::Null),
                    };
                    let body = serde_json::to_vec(&msg).unwrap_or_default();
                    let frame = encode_frame(&body);
                    let mut w = push_writer.lock().await;
                    if w.write_all(&frame).await.is_err() {
                        return;
                    }
                }
            }
        }
    });

    // Read 루프
    let mut accum: Vec<u8> = Vec::new();
    let mut tmp = vec![0u8; 4096];
    loop {
        let n = match reader.read(&mut tmp).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        accum.extend_from_slice(&tmp[..n]);
        loop {
            let mut slice = accum.as_slice();
            match decode_frame(&mut slice) {
                Ok(body) => {
                    let consumed = accum.len() - slice.len();
                    accum.drain(..consumed);
                    dispatch_one(&handle, &actor_id, &sub_map, &writer, &body).await;
                }
                Err(DecodeError::Incomplete) => break,
                Err(DecodeError::TooLarge(_)) => return,
            }
        }
    }
    push_task.abort();
    let _ = handle.disconnect_actor(actor_id).await;
}

async fn dispatch_one(
    handle: &ObjectServerHandle,
    actor: &ActorId,
    sub_map: &SubMap,
    writer: &Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    body: &[u8],
) {
    let raw: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return,
    };
    let kind = raw.get("kind").and_then(|v| v.as_str()).unwrap_or("");

    let response: Option<serde_json::Value> = match kind {
        "Mount" => {
            let m: MountMsg = match serde_json::from_value(raw) {
                Ok(m) => m,
                Err(_) => return,
            };
            Some(handle_mount(handle, m).await)
        }
        "Invoke" => {
            let m: InvokeMsg = match serde_json::from_value(raw) {
                Ok(m) => m,
                Err(_) => return,
            };
            Some(handle_invoke(handle, m, actor.clone()).await)
        }
        "Query" => {
            let m: QueryMsg = match serde_json::from_value(raw) {
                Ok(m) => m,
                Err(_) => return,
            };
            Some(handle_query(handle, m).await)
        }
        "Subscribe" => {
            let m: SubscribeMsg = match serde_json::from_value(raw) {
                Ok(m) => m,
                Err(_) => return,
            };
            let target = match parse_obj_id(&m.target) {
                Some(t) => t,
                None => return,
            };
            let filters: Vec<EventKindFilter> = m
                .kinds
                .iter()
                .map(|k| match k {
                    EventKindFilterWire::Invoke => EventKindFilter::Invoke,
                    EventKindFilterWire::StateSet => EventKindFilter::StateSet,
                    EventKindFilterWire::Lifecycle => EventKindFilter::Lifecycle,
                    EventKindFilterWire::ChildChange => EventKindFilter::ChildChange,
                })
                .collect();
            let sid = match handle.subscribe(actor.clone(), target, filters).await {
                Ok(s) => s,
                Err(_) => return,
            };
            sub_map.lock().await.insert(m.subscription_id.clone(), sid);
            Some(serde_json::to_value(SubscribeAck { subscription_id: m.subscription_id }).unwrap())
        }
        "Unsubscribe" => {
            let m: UnsubscribeMsg = match serde_json::from_value(raw) {
                Ok(m) => m,
                Err(_) => return,
            };
            let server_sid = sub_map.lock().await.remove(&m.subscription_id);
            if let Some(s) = server_sid {
                let _ = handle.unsubscribe(s).await;
            }
            None
        }
        "StateSet" => {
            let m: StateSetMsg = match serde_json::from_value(raw) {
                Ok(m) => m,
                Err(_) => return,
            };
            Some(handle_state_set(handle, m, actor.clone()).await)
        }
        "Get" => {
            let m: GetMsg = match serde_json::from_value(raw) {
                Ok(m) => m,
                Err(_) => return,
            };
            Some(handle_get(handle, m).await)
        }
        "Glscript" => {
            // M5에서 구현
            let req_id = raw.get("request_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Some(serde_json::json!({
                "kind": "GlscriptError",
                "request_id": req_id,
                "kind_error": "not_implemented",
                "detail": "Glscript는 M5 마일스톤에서 구현됩니다"
            }))
        }
        _ => None,
    };

    if let Some(resp) = response {
        let body = serde_json::to_vec(&resp).unwrap_or_default();
        let frame = encode_frame(&body);
        let mut w = writer.lock().await;
        let _ = w.write_all(&frame).await;
    }
}

fn parse_obj_id(s: &str) -> Option<ObjectId> {
    let json = format!("\"{}\"", s);
    serde_json::from_str(&json).ok()
}

async fn read_and_handle_hello_split(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    writer: &Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
) -> Result<ActorId, String> {
    let mut accum = Vec::new();
    let mut tmp = vec![0u8; 4096];
    loop {
        let n = reader.read(&mut tmp).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("closed before Hello".to_string());
        }
        accum.extend_from_slice(&tmp[..n]);
        let mut slice = accum.as_slice();
        match decode_frame(&mut slice) {
            Ok(body) => {
                let consumed = accum.len() - slice.len();
                accum.drain(..consumed);
                let hello: Hello = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
                if hello.version != "0.1" {
                    let rej = HelloReject {
                        reason: "version_mismatch".to_string(),
                        detail: format!("server 0.1, client {}", hello.version),
                    };
                    let body = serde_json::to_vec(&rej).unwrap();
                    let mut w = writer.lock().await;
                    let _ = w.write_all(&encode_frame(&body)).await;
                    return Err("version".to_string());
                }
                let actor = match hello.role {
                    Role::Ai => ActorId::new_ai_session(),
                    Role::App => {
                        // Require auth.manifest with valid id and valid ui_types.
                        let manifest_val = match hello.auth.get("manifest") {
                            Some(m) => m,
                            None => {
                                let rej = HelloReject {
                                    reason: "missing_manifest".to_string(),
                                    detail: "Role::App requires auth.manifest".to_string(),
                                };
                                let body = serde_json::to_vec(&rej).unwrap();
                                let mut w = writer.lock().await;
                                let _ = w.write_all(&encode_frame(&body)).await;
                                return Err("missing_manifest".to_string());
                            }
                        };

                        let raw_id = manifest_val.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        if raw_id.is_empty() {
                            let rej = HelloReject {
                                reason: "invalid_manifest".to_string(),
                                detail: "manifest.id is required".to_string(),
                            };
                            let body = serde_json::to_vec(&rej).unwrap();
                            let mut w = writer.lock().await;
                            let _ = w.write_all(&encode_frame(&body)).await;
                            return Err("invalid_manifest".to_string());
                        }

                        let raw_ui_types: Vec<String> = manifest_val
                            .get("ui_types")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();

                        for t in &raw_ui_types {
                            if geulos_core::TypeUri::parse(t).is_err() {
                                let rej = HelloReject {
                                    reason: "invalid_manifest".to_string(),
                                    detail: format!("bad TypeUri in ui_types: '{}'", t),
                                };
                                let body = serde_json::to_vec(&rej).unwrap();
                                let mut w = writer.lock().await;
                                let _ = w.write_all(&encode_frame(&body)).await;
                                return Err("invalid_manifest".to_string());
                            }
                        }

                        ActorId::new_app(raw_id)
                    }
                    Role::Compositor => ActorId::system_compositor(),
                };
                let ack = HelloAck {
                    session_id: Uuid::new_v4().to_string(),
                    actor_id: actor.as_str().to_string(),
                    server_version: "0.1".to_string(),
                    capabilities: vec![
                        "mount".to_string(),
                        "invoke".to_string(),
                        "subscribe".to_string(),
                        "query".to_string(),
                    ],
                };
                let body = serde_json::to_vec(&ack).unwrap();
                let mut w = writer.lock().await;
                w.write_all(&encode_frame(&body)).await.map_err(|e| e.to_string())?;
                return Ok(actor);
            }
            Err(DecodeError::Incomplete) => continue,
            Err(DecodeError::TooLarge(n)) => return Err(format!("too large: {}", n)),
        }
    }
}
