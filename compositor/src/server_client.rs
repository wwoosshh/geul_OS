//! 별 tokio 스레드에서 server-host와 TCP로 대화.
//!
//! winit 메인 스레드는 mpsc 채널로:
//! - 입력 → UiAction 송신
//! - ServerEvent 수신 → 트리 갱신 + 윈도우 redraw 요청

use std::sync::Arc;
use std::time::Duration;

use geulos_core::{Object, ObjectId};
use geulos_proto::{
    decode_frame, encode_frame, EventKindFilterWire, EventMsg, GetMsg, GetResult, Hello, HelloAck,
    InvokeMsg, QueryMsg, QueryPredicate, QueryResult, Role, SubscribeMsg,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use winit::event_loop::EventLoopProxy;

use crate::messages::{ServerEvent, UiAction};

/// 표준 타입 URI 목록 — M4에서 컴포지터가 처음 query 할 것들.
const STD_TYPES: &[&str] = &[
    "aios.std/Container@1",
    "aios.std/Text@1",
    "aios.std/Button@1",
    "aios.std/Toggle@1",
];

/// 컴포지터의 redraw/quit 신호를 winit에 보내는 user_event 타입.
#[derive(Debug, Clone)]
pub enum UserEvent {
    Redraw,
    Quit,
}

/// 컴포지터 측 server-host 클라이언트 실행.
///
/// 별 tokio 스레드에서 호출. server에 접속, query+get으로 초기 트리를 가져옴,
/// 모든 객체 subscribe, 이벤트 받아서 event_tx로 전달.
/// 같이 ui_rx로 UiAction(예: 클릭에 의한 Invoke)을 받아 wire로 전송.
pub async fn run_server_client(
    addr: String,
    event_tx: mpsc::Sender<ServerEvent>,
    mut ui_rx: mpsc::Receiver<UiAction>,
    proxy: Arc<EventLoopProxy<UserEvent>>,
) -> Result<(), String> {
    let mut stream = TcpStream::connect(&addr).await.map_err(|e| e.to_string())?;

    // 1) Hello as Compositor
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Compositor,
        auth: json!({}),
        client_id: "compositor".to_string(),
    };
    let body = serde_json::to_vec(&hello).map_err(|e| e.to_string())?;
    stream
        .write_all(&encode_frame(&body))
        .await
        .map_err(|e| e.to_string())?;

    let mut accum: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 16384];
    // HelloAck 수신
    let _ack: HelloAck = read_typed(&mut stream, &mut accum, &mut buf).await?;
    let _ = proxy.send_event(UserEvent::Redraw);

    // 2) 표준 타입별 Query → 객체 ID 모으기
    let mut all_ids: Vec<String> = Vec::new();
    for (i, t) in STD_TYPES.iter().enumerate() {
        let q = QueryMsg {
            request_id: format!("q-{}", i),
            query: QueryPredicate::ByType { type_uri: t.to_string() },
        };
        write_msg(&mut stream, &q).await?;
        let qr: QueryResult = read_typed(&mut stream, &mut accum, &mut buf).await?;
        all_ids.extend(qr.objects);
    }

    // 3) 각 ID에 대해 Get 후 ServerEvent::ObjectUpserted 전송
    for (i, id_str) in all_ids.iter().enumerate() {
        let g = GetMsg {
            request_id: format!("g-{}", i),
            target: id_str.clone(),
        };
        write_msg(&mut stream, &g).await?;
        let gr: GetResult = read_typed(&mut stream, &mut accum, &mut buf).await?;
        if let Ok(obj) = serde_json::from_value::<Object>(gr.object) {
            let _ = event_tx.send(ServerEvent::ObjectUpserted(obj)).await;
        }
    }
    let _ = proxy.send_event(UserEvent::Redraw);

    // 4) 각 객체에 Subscribe (Invoke + StateSet + Lifecycle)
    for (i, id_str) in all_ids.iter().enumerate() {
        let s = SubscribeMsg {
            subscription_id: format!("sub-{}", i),
            target: id_str.clone(),
            kinds: vec![
                EventKindFilterWire::Invoke,
                EventKindFilterWire::StateSet,
                EventKindFilterWire::Lifecycle,
            ],
            include_initial: false,
        };
        write_msg(&mut stream, &s).await?;
        let _ack = read_response_body(&mut stream, &mut accum, &mut buf).await?;
    }

    // 5) 동시 루프: 서버 → 클라 event 수신 + UI → 서버 Invoke 송신
    loop {
        tokio::select! {
            r = stream.read(&mut buf) => {
                let n = match r {
                    Ok(0) => { let _ = event_tx.send(ServerEvent::Disconnected).await; return Ok(()); }
                    Ok(n) => n,
                    Err(_) => { let _ = event_tx.send(ServerEvent::Disconnected).await; return Ok(()); }
                };
                accum.extend_from_slice(&buf[..n]);
                loop {
                    let mut slice = accum.as_slice();
                    match decode_frame(&mut slice) {
                        Ok(body) => {
                            let consumed = accum.len() - slice.len();
                            accum.drain(..consumed);
                            handle_server_frame(&body, &event_tx).await;
                            let _ = proxy.send_event(UserEvent::Redraw);
                        }
                        Err(_) => break,
                    }
                }
            }
            Some(action) = ui_rx.recv() => {
                match action {
                    UiAction::Invoke { target, method, args } => {
                        let req_id = format!("inv-{}", target);
                        let m = InvokeMsg {
                            request_id: req_id,
                            target: target.to_string(),
                            method,
                            args,
                        };
                        let _ = write_msg(&mut stream, &m).await;
                    }
                    UiAction::Quit => {
                        let _ = proxy.send_event(UserEvent::Quit);
                        return Ok(());
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                // 살아있음 — 별 작업 없음
            }
        }
    }
}

async fn handle_server_frame(body: &[u8], event_tx: &mpsc::Sender<ServerEvent>) {
    let raw: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return,
    };
    let kind = raw.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    if kind == "Event" {
        let ev: EventMsg = match serde_json::from_value(raw) {
            Ok(e) => e,
            Err(_) => return,
        };
        // 이벤트 종류별 분석
        let target_str = ev.event.get("target").and_then(|v| v.as_str()).unwrap_or("");
        let target_id: ObjectId = match serde_json::from_str(&format!("\"{}\"", target_str)) {
            Ok(t) => t,
            Err(_) => return,
        };
        let kind_str = ev.event.get("kind").and_then(|k| k.get("kind"))
            .and_then(|v| v.as_str()).unwrap_or("");
        match kind_str {
            "StateSet" => {
                let kind_obj = ev.event.get("kind").unwrap();
                let key = kind_obj.get("key").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let value = kind_obj.get("value").cloned().unwrap_or(serde_json::Value::Null);
                let _ = event_tx.send(ServerEvent::StateSet { id: target_id, key, value }).await;
            }
            "Lifecycle" => {
                // Destroyed → 제거
                let lifecycle = ev.event.get("kind").and_then(|k| k.get("Lifecycle"))
                    .and_then(|v| v.as_str()).unwrap_or("");
                if lifecycle == "Destroyed" {
                    let _ = event_tx.send(ServerEvent::ObjectRemoved(target_id)).await;
                }
            }
            _ => {}
        }
    }
}

async fn write_msg<T: serde::Serialize>(
    stream: &mut TcpStream,
    msg: &T,
) -> Result<(), String> {
    let body = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    stream
        .write_all(&encode_frame(&body))
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn read_response_body(
    stream: &mut TcpStream,
    accum: &mut Vec<u8>,
    buf: &mut [u8],
) -> Result<Vec<u8>, String> {
    loop {
        let mut slice = accum.as_slice();
        if let Ok(body) = decode_frame(&mut slice) {
            let consumed = accum.len() - slice.len();
            accum.drain(..consumed);
            return Ok(body);
        }
        let n = stream.read(buf).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("closed".to_string());
        }
        accum.extend_from_slice(&buf[..n]);
    }
}

async fn read_typed<T: serde::de::DeserializeOwned>(
    stream: &mut TcpStream,
    accum: &mut Vec<u8>,
    buf: &mut [u8],
) -> Result<T, String> {
    let body = read_response_body(stream, accum, buf).await?;
    serde_json::from_slice(&body).map_err(|e| format!("decode: {}", e))
}
