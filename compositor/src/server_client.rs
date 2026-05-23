//! 별 tokio 스레드에서 server-host와 TCP로 대화.
//!
//! winit 메인 스레드는 mpsc 채널로:
//! - 입력 → UiAction 송신
//! - ServerEvent 수신 → 트리 갱신 + 윈도우 redraw 요청

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use geulos_core::{Object, ObjectId};
use geulos_proto::{
    decode_frame, encode_frame, EventKindFilterWire, EventMsg, GetMsg, GetResult, Hello, HelloAck,
    InvokeMsg, QueryMsg, QueryPredicate, QueryResult, Role, StateSetMsg, SubscribeMsg,
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
    "aios.builtin/Dialog@1",
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
    //
    // `pending_gets`: KI-013 해소 — Created 분기에서 Get을 fire-and-forget으로 송신한 후,
    // 응답(GetResult)이 다음 select! tick의 stream.read에서 frame 단위로 도착할 때 어떤
    // ObjectId의 응답인지 매칭하기 위한 request_id → target_id 맵. Get/Event interleave
    // race를 근본적으로 해소.
    let mut dyn_sub_seq: u64 = 0;
    let mut pending_gets: HashMap<String, ObjectId> = HashMap::new();
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
                                &mut dyn_sub_seq,
                                &mut pending_gets,
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
                    UiAction::SetState { target, key, value } => {
                        // M8 T8.17: scroll_y 같은 viewer-only 상태 직접 SetState.
                        // StateSetAck는 stream에 흘러와 handle_server_frame의 `_ => {}` 분기로
                        // silent drop (handle_server_frame이 "StateSetAck" kind을 모름 — 무시).
                        let req_id = format!("ss-{}", target);
                        let m = StateSetMsg {
                            request_id: req_id,
                            target: target.to_string(),
                            key,
                            value,
                        };
                        let _ = write_msg(&mut stream, &m).await;
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
/// KI-013 해소 (2026-05-18): 이전 구현은 Created 분기에서 *동기적으로* Get을 송신하고
/// `read_typed::<GetResult>`로 응답을 *그 자리에서 stream.read* 했다. 그러나 server-host의
/// push task는 100ms 간격으로 *큐된 모든 이벤트를 한꺼번에 drain*해 stream에 연속 push
/// 한다. 폴더가 expand되며 자식 N개가 mount될 경우 N개의 EventMsg가 연속 push되는데,
/// Get을 보낸 직후 *다음 frame이 EventMsg* 면 GetResult deserialize 실패 → 그 객체가
/// 영영 트리에 안 들어옴 (사용자 증상: "폴더 열어도 자식 안 보임").
///
/// 새 접근 — *fire-and-forget*:
/// 1. Created 도착 시 Get만 송신 (응답 대기 X). request_id → target_id를 `pending_gets`에
///    저장 + 즉시 return.
/// 2. 다음 select! tick에서 stream.read이 GetResult frame을 받으면 GetResult 분기가
///    pending_gets lookup → ObjectUpserted + dyn Subscribe 송신.
/// 3. dyn Subscribe ack도 기다리지 않음 — server-host가 처리하면 됨. SubscribeAck는
///    그냥 stream에 흘러와 `_ => {}` 분기로 silent drop.
///
/// 이렇게 하면 모든 frame이 *select! loop의 stream.read*만으로 받아져 순서대로
/// handle_server_frame에 전달된다 — interleave 가능성 자체가 사라짐.
async fn handle_server_frame(
    body: &[u8],
    event_tx: &mpsc::Sender<ServerEvent>,
    stream: &mut TcpStream,
    dyn_sub_seq: &mut u64,
    pending_gets: &mut HashMap<String, ObjectId>,
) {
    let raw: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return,
    };
    let kind = raw.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "Event" => {
            handle_event_frame(raw, event_tx, stream, pending_gets).await;
        }
        "GetResult" => {
            // Created 분기에서 보낸 Get의 응답. request_id로 어떤 객체 응답인지 매칭.
            let request_id =
                raw.get("request_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let target_id = match pending_gets.remove(&request_id) {
                Some(id) => id,
                // 우리가 보내지 않은 Get의 응답 (또는 이미 처리된 중복). 안전하게 무시.
                None => return,
            };
            let object_val = match raw.get("object") {
                Some(v) => v.clone(),
                None => return,
            };
            let obj: Object = match serde_json::from_value(object_val) {
                Ok(o) => o,
                Err(e) => {
                    eprintln!("[compositor] GetResult.object 디코딩 실패: {}", e);
                    return;
                }
            };
            eprintln!(
                "[compositor] ObjectUpserted id={} type={} parent={:?}",
                obj.id,
                obj.type_uri.as_str(),
                obj.parent
            );
            let _ = event_tx.send(ServerEvent::ObjectUpserted(obj)).await;
            // 그 새 ID에 ID-based subscribe 추가 — StateSet/Invoke 수신을 위해.
            // (type-level subscribe는 Lifecycle만 cover.)
            *dyn_sub_seq += 1;
            let s = SubscribeMsg {
                subscription_id: format!("dyn-sub-{}", *dyn_sub_seq),
                target: target_id.to_string(),
                kinds: vec![
                    EventKindFilterWire::Invoke,
                    EventKindFilterWire::StateSet,
                    EventKindFilterWire::Lifecycle,
                ],
                include_initial: false,
            };
            if let Err(e) = write_msg(stream, &s).await {
                eprintln!("[compositor] dyn Subscribe 송신 실패: {}", e);
            }
            // SubscribeAck는 stream에 흘러와 `_ => {}` 분기로 무시 — 별 처리 불필요.
        }
        "GetError" => {
            // Get이 실패했다. pending entry만 정리 — 메모리 누수 방지.
            let request_id =
                raw.get("request_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if pending_gets.remove(&request_id).is_some() {
                eprintln!("[compositor] Get for Created 실패 (request {})", request_id);
            }
        }
        _ => {
            // SubscribeAck, MountAck 등 다른 응답들 — 우리가 처리할 필요 없음.
        }
    }
}

/// `Event` kind frame 처리 (Lifecycle/StateSet/Invoke).
///
/// `Lifecycle::Created`에서는 Get을 *fire-and-forget*으로 송신만 한다. 응답은
/// 다음 select! tick에서 GetResult 분기가 처리. 자세한 race 해소 이유는
/// `handle_server_frame` 주석 참고.
async fn handle_event_frame(
    raw: serde_json::Value,
    event_tx: &mpsc::Sender<ServerEvent>,
    stream: &mut TcpStream,
    pending_gets: &mut HashMap<String, ObjectId>,
) {
    let ev: EventMsg = match serde_json::from_value(raw) {
        Ok(e) => e,
        Err(_) => return,
    };
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
            // 실제 직렬화 형식: `{"kind": "Lifecycle", "Created": null}` 또는 `{"kind": "Lifecycle", "Destroyed": null}`.
            // serde의 internally tagged enum + newtype variant(LifecycleKind) 동작 — variant 이름이 *키*로,
            // value는 null. 따라서 *키 존재*로 판정 (기존 코드의 `get("Lifecycle")`은 그런 키가 없어서 영원히
            // None이었음 — Created 분기에 한 번도 못 들어가 KI-004 fix 효과가 다 묻혀있었다).
            let kind_obj = match ev.event.get("kind") {
                Some(k) => k,
                None => return,
            };
            let lifecycle = if kind_obj.get("Created").is_some() {
                "Created"
            } else if kind_obj.get("Destroyed").is_some() {
                "Destroyed"
            } else {
                ""
            };
            match lifecycle {
                "Created" => {
                    eprintln!("[compositor] Lifecycle::Created 도착 id={}", target_id);
                    // KI-013 해소: Get *송신만* — 응답은 다음 stream.read에서 GetResult
                    // 분기가 pending_gets lookup으로 처리. interleave race 차단.
                    let request_id = format!("g-created-{}", target_id);
                    let g =
                        GetMsg { request_id: request_id.clone(), target: target_str.to_string() };
                    if let Err(e) = write_msg(stream, &g).await {
                        eprintln!("[compositor] Get 송신 실패 (target {}): {}", target_id, e);
                        return;
                    }
                    // 같은 객체에 대해 Created가 중복 도착하면 같은 request_id로 덮어쓴다.
                    // 첫 응답이 와도 target_id는 동일하므로 무해.
                    pending_gets.insert(request_id, target_id);
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
            // M9 T8: Dialog@1 — desktop-shell이 AI 저장 confirm 등에 mount.
            // 누락 시 compositor가 query/type-subscribe에서 받지 못해 화면에 안 나타남.
            st::dialog(owner.clone(), "", "", "confirm", vec!["허용".to_string()])
                .type_uri
                .as_str()
                .to_string(),
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
