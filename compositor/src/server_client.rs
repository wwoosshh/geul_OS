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

/// 표준 타입 URI 목록 — 컴포지터가 처음 query 할 것들.
///
/// M4 표준 4종 + M7+M8 데스크톱 셸 8종 (Desktop/FileTree/Canvas/Cli/Window/Explorer +
/// Folder/File).
/// 이 목록에 없는 타입은 컴포지터가 트리에서 보지 못한다 — desktop-shell이 mount해도
/// 화면에 안 나옴. 새 표준/빌트인 타입 추가 시 std_types_query_coverage_smoke 테스트
/// 갱신 필수.
const STD_TYPES: &[&str] = &[
    "aios.std/Container@1",
    "aios.std/Text@1",
    "aios.std/Button@1",
    "aios.std/Toggle@1",
    "aios.builtin/Desktop@1",
    "aios.builtin/FileTree@1",
    "aios.builtin/Canvas@1",
    "aios.builtin/Cli@1",
    "aios.builtin/Window@1",
    "aios.builtin/Explorer@1",
    "aios.std/Folder@1",
    "aios.std/File@1",
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
    stream.write_all(&encode_frame(&body)).await.map_err(|e| e.to_string())?;

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
        let g = GetMsg { request_id: format!("g-{}", i), target: id_str.clone() };
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

    // 4.5) 각 표준 타입에 *type-level* Subscribe — KI-004 해소.
    //
    // startup 후 desktop-shell이 lazy_mount로 만드는 새 Folder/File/Window는 위의
    // ID-based subscribe가 cover 못함 (그 ID가 아직 존재하지 않았음). type-level
    // 구독은 그 type의 *모든* 객체 (현재+미래) 이벤트를 받는다. Lifecycle만 구독해도
    // 충분 — Created 도착 시 handle_server_frame이 Get으로 본문 fetch + ID-based
    // subscribe를 추가로 등록한다 (StateSet/Invoke 수신을 위해).
    for (i, t) in STD_TYPES.iter().enumerate() {
        let s = SubscribeMsg {
            subscription_id: format!("type-sub-{}", i),
            target: format!("type:{}", t),
            kinds: vec![EventKindFilterWire::Lifecycle],
            include_initial: false,
        };
        write_msg(&mut stream, &s).await?;
        let _ack = read_response_body(&mut stream, &mut accum, &mut buf).await?;
    }

    // 5) 동시 루프: 서버 → 클라 event 수신 + UI → 서버 Invoke 송신
    //
    // `dyn_sub_seq`: 동적으로 mount된 객체에 추가하는 ID-based subscribe의 sequence 번호.
    // type-level subscribe로 Created를 받으면 그 ID에도 ID-based subscribe를 등록해
    // StateSet/Invoke를 수신한다.
    let mut dyn_sub_seq: u64 = 0;
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
                            handle_server_frame(
                                &body,
                                &event_tx,
                                &mut stream,
                                &mut accum,
                                &mut buf,
                                &mut dyn_sub_seq,
                            )
                            .await;
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

/// 서버에서 받은 한 프레임 처리.
///
/// Created 이벤트 도착 시 *동기적으로* (loop 안에서) Get 요청을 보내고 응답을 받아
/// 본문을 fetch한 뒤 ObjectUpserted로 전달. 더불어 그 ID에 ID-based subscribe를
/// 추가 등록해 StateSet/Invoke 수신 path를 확보 (type-level은 Lifecycle만 구독).
///
/// Get/Subscribe 응답 대기 사이에 다른 이벤트 프레임이 server에서 도착하면 그것도
/// 같은 stream에 쌓여 read_response_body가 *response 가 아닌* event 프레임을
/// 먼저 꺼낼 위험이 있다. 현재 server-host는 push task가 100ms 간격으로 동작하므로
/// Get 요청 → ack의 round-trip이 그 사이에 끝날 확률이 매우 높지만, 완벽 보장은
/// 아니다 (KI-013 후속 부채로 known-issues.md 등록 검토). M8 회귀 fix 범위에선
/// 충분.
async fn handle_server_frame(
    body: &[u8],
    event_tx: &mpsc::Sender<ServerEvent>,
    stream: &mut TcpStream,
    accum: &mut Vec<u8>,
    buf: &mut [u8],
    dyn_sub_seq: &mut u64,
) {
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
        let kind_str =
            ev.event.get("kind").and_then(|k| k.get("kind")).and_then(|v| v.as_str()).unwrap_or("");
        match kind_str {
            "StateSet" => {
                let kind_obj = ev.event.get("kind").unwrap();
                let key = kind_obj.get("key").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let value = kind_obj.get("value").cloned().unwrap_or(serde_json::Value::Null);
                let _ = event_tx.send(ServerEvent::StateSet { id: target_id, key, value }).await;
            }
            "Lifecycle" => {
                let lifecycle = ev
                    .event
                    .get("kind")
                    .and_then(|k| k.get("Lifecycle"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match lifecycle {
                    "Created" => {
                        // KI-004 해소: type-level subscribe로 도착한 신규 객체. Get으로
                        // 본문 fetch + ID-based subscribe 추가 (StateSet/Invoke 수신).
                        let target_str_owned = target_str.to_string();
                        let g = GetMsg {
                            request_id: format!("g-created-{}", target_id),
                            target: target_str_owned.clone(),
                        };
                        if let Err(e) = write_msg(stream, &g).await {
                            eprintln!("[compositor] Get for Created 실패: {}", e);
                            return;
                        }
                        match read_typed::<GetResult>(stream, accum, buf).await {
                            Ok(gr) => {
                                if let Ok(obj) = serde_json::from_value::<Object>(gr.object) {
                                    let _ = event_tx.send(ServerEvent::ObjectUpserted(obj)).await;
                                }
                            }
                            Err(e) => {
                                eprintln!("[compositor] Get response 디코딩 실패: {}", e);
                                return;
                            }
                        }
                        // 그 새 ID에 ID-based subscribe 추가 — StateSet/Invoke 수신.
                        *dyn_sub_seq += 1;
                        let s = SubscribeMsg {
                            subscription_id: format!("dyn-sub-{}", *dyn_sub_seq),
                            target: target_str_owned,
                            kinds: vec![
                                EventKindFilterWire::Invoke,
                                EventKindFilterWire::StateSet,
                                EventKindFilterWire::Lifecycle,
                            ],
                            include_initial: false,
                        };
                        if let Err(e) = write_msg(stream, &s).await {
                            eprintln!("[compositor] dyn Subscribe 실패: {}", e);
                            return;
                        }
                        let _ = read_response_body(stream, accum, buf).await;
                    }
                    "Destroyed" => {
                        let _ = event_tx.send(ServerEvent::ObjectRemoved(target_id)).await;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

async fn write_msg<T: serde::Serialize>(stream: &mut TcpStream, msg: &T) -> Result<(), String> {
    let body = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    stream.write_all(&encode_frame(&body)).await.map_err(|e| e.to_string())?;
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

#[cfg(test)]
mod tests {
    use super::STD_TYPES;
    use geulos_core::std_types as st;
    use geulos_core::ActorId;

    /// 새 표준/빌트인 타입 팩토리가 추가됐는데 STD_TYPES 갱신을 잊으면 컴포지터가
    /// 그 객체를 query하지 못해 화면에 안 나타난다 (T7.5에서 Cli@1로 실제 발생한 회귀).
    /// 이 테스트는 핵심 팩토리들의 type_uri가 모두 STD_TYPES에 있는지 강제한다.
    #[test]
    fn std_types_query_coverage_smoke() {
        use geulos_core::ObjectId;
        let owner = ActorId::local_user();
        let factories: Vec<String> = vec![
            st::container(owner.clone()).type_uri.as_str().to_string(),
            st::text(owner.clone(), "").type_uri.as_str().to_string(),
            st::button(owner.clone(), "").type_uri.as_str().to_string(),
            st::toggle(owner.clone(), false).type_uri.as_str().to_string(),
            st::desktop(owner.clone()).type_uri.as_str().to_string(),
            st::file_tree(owner.clone(), "/").type_uri.as_str().to_string(),
            st::canvas(owner.clone()).type_uri.as_str().to_string(),
            st::cli(owner.clone()).type_uri.as_str().to_string(),
            st::window(owner.clone(), "", ObjectId::new(), 0, 0, 200, 120)
                .type_uri
                .as_str()
                .to_string(),
            st::explorer(owner.clone()).type_uri.as_str().to_string(),
            st::folder(owner.clone(), "/", "/", 0).type_uri.as_str().to_string(),
            st::file(owner.clone(), "/", "x", "text/plain", 0).type_uri.as_str().to_string(),
        ];
        for uri in &factories {
            assert!(
                STD_TYPES.contains(&uri.as_str()),
                "STD_TYPES에 {} 누락 — 새 표준 타입 추가 시 server_client.rs:STD_TYPES도 갱신 필요",
                uri
            );
        }
    }
}
