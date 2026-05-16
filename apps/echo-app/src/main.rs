//! echo-app: count 버튼 + 텍스트 라벨.
//!
//! 동작:
//! 1. 서버에 App role + 매니페스트로 접속
//! 2. Container > [Text, Button] mount
//! 3. Button을 invoke 필터로 subscribe
//! 4. press 이벤트가 오면 카운터를 증가시키고 Text.content StateSet
//!
//! 외부 클라이언트가 Button을 invoke press 하면 Text가 갱신되어야 함.

use std::time::Duration;

use geulos_echo_app::{build_ui, next_count};
use geulos_proto::{
    decode_frame, encode_frame, EventKindFilterWire, EventMsg, Hello, HelloAck, MountAck, MountMsg,
    Role, StateSetAck, StateSetMsg, SubscribeAck, SubscribeMsg,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SERVER_ADDR: &str = "127.0.0.1:5550";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::args().nth(1).unwrap_or_else(|| SERVER_ADDR.to_string());
    println!("echo-app connecting to {}...", addr);

    let mut stream = TcpStream::connect(&addr).await?;

    // 1) Hello (App + manifest)
    let manifest = json!({
        "manifest": {
            "id": "echo",
            "permissions": [],
            "ui_types": [
                "aios.std/Container@1",
                "aios.std/Text@1",
                "aios.std/Button@1"
            ]
        }
    });
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::App,
        auth: manifest,
        client_id: "echo-app".to_string(),
    };
    let body = serde_json::to_vec(&hello)?;
    stream.write_all(&encode_frame(&body)).await?;

    let mut buf = vec![0u8; 16384];
    let mut accum: Vec<u8> = Vec::new();
    let actor_str: String;
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Err("closed before HelloAck".into());
        }
        accum.extend_from_slice(&buf[..n]);
        let mut slice = accum.as_slice();
        if let Ok(body) = decode_frame(&mut slice) {
            let consumed = accum.len() - slice.len();
            accum.drain(..consumed);
            let ack: HelloAck = serde_json::from_slice(&body)?;
            actor_str = ack.actor_id.clone();
            println!("[echo-app] HelloAck: actor={}", actor_str);
            break;
        }
    }

    // 2) UI 구성 + mount
    let owner = <geulos_core::ActorId as std::str::FromStr>::from_str(&actor_str)?;
    let (container, text, button) = build_ui(owner.clone());
    let text_id = text.id;
    let button_id = button.id;

    for obj in [&container, &text, &button] {
        let msg = MountMsg { root_object_id: obj.id.to_string(), tree: serde_json::to_value(obj)? };
        let body = serde_json::to_vec(&msg)?;
        stream.write_all(&encode_frame(&body)).await?;
        // MountAck 소비
        loop {
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                return Err("closed".into());
            }
            accum.extend_from_slice(&buf[..n]);
            let mut slice = accum.as_slice();
            if let Ok(b) = decode_frame(&mut slice) {
                let consumed = accum.len() - slice.len();
                accum.drain(..consumed);
                let _: MountAck = serde_json::from_slice(&b)?;
                break;
            }
        }
    }
    println!(
        "[echo-app] mounted: container={}, text={}, button={}",
        container.id, text_id, button_id
    );

    // 3) Subscribe to button.invoke
    let sub = SubscribeMsg {
        subscription_id: "sub-button".to_string(),
        target: button_id.to_string(),
        kinds: vec![EventKindFilterWire::Invoke],
        include_initial: false,
    };
    let body = serde_json::to_vec(&sub)?;
    stream.write_all(&encode_frame(&body)).await?;
    // SubscribeAck 소비
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Err("closed".into());
        }
        accum.extend_from_slice(&buf[..n]);
        let mut slice = accum.as_slice();
        if let Ok(b) = decode_frame(&mut slice) {
            let consumed = accum.len() - slice.len();
            accum.drain(..consumed);
            let _: SubscribeAck = serde_json::from_slice(&b)?;
            break;
        }
    }
    println!("[echo-app] subscribed to button events");

    // 4) 이벤트 루프
    let mut count: i64 = 0;
    let mut req_seq: u64 = 0;
    loop {
        let n = match tokio::time::timeout(Duration::from_secs(60), stream.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                eprintln!("read error: {}", e);
                break;
            }
            Err(_) => {
                println!("[echo-app] idle 60s, exiting");
                break;
            }
        };
        if n == 0 {
            break;
        }
        accum.extend_from_slice(&buf[..n]);
        loop {
            let mut slice = accum.as_slice();
            match decode_frame(&mut slice) {
                Ok(body) => {
                    let consumed = accum.len() - slice.len();
                    accum.drain(..consumed);
                    let raw: serde_json::Value = match serde_json::from_slice(&body) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let kind = raw.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                    if kind == "Event" {
                        let ev: EventMsg = match serde_json::from_value(raw) {
                            Ok(e) => e,
                            Err(_) => continue,
                        };
                        // press 이벤트 감지 → count 증가 + Text 갱신
                        // ev.event["kind"] = {"kind": "Invoke", "method": "press", "args": null}
                        // (EventKind 는 #[serde(tag = "kind")] 로 직렬화됨)
                        let event_kind = ev.event.get("kind");
                        let is_invoke =
                            event_kind.and_then(|k| k.get("kind")).and_then(|v| v.as_str())
                                == Some("Invoke");
                        let method = if is_invoke {
                            event_kind
                                .and_then(|k| k.get("method"))
                                .and_then(|m| m.as_str())
                                .unwrap_or("")
                        } else {
                            ""
                        };
                        if method == "press" {
                            let (new_count, new_text) = next_count(count);
                            count = new_count;
                            req_seq += 1;
                            let ss = StateSetMsg {
                                request_id: format!("r-{}", req_seq),
                                target: text_id.to_string(),
                                key: "content".to_string(),
                                value: json!(new_text),
                            };
                            let body = serde_json::to_vec(&ss)?;
                            stream.write_all(&encode_frame(&body)).await?;
                            println!("[echo-app] count -> {}", new_count);
                        }
                    } else if kind == "StateSetAck" {
                        let _: StateSetAck = match serde_json::from_value(raw) {
                            Ok(a) => a,
                            Err(_) => continue,
                        };
                    }
                }
                Err(_) => break,
            }
        }
    }
    println!("[echo-app] exit");
    Ok(())
}
