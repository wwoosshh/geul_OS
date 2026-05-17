//! notepad-app 진입점.
//!
//! M7 T2 — 스캐폴드만. server-host에 Hello + 최소 mount(MemoList + TextArea)까지
//! 수행. T3에서 fs 로딩·이벤트 루프·메서드 핸들러를 본격 추가.

use geulos_notepad_app::build_initial_tree;
use geulos_proto::{decode_frame, encode_frame, Hello, HelloAck, MountAck, MountMsg, Role};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SERVER_ADDR: &str = "127.0.0.1:5550";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::args().nth(1).unwrap_or_else(|| SERVER_ADDR.to_string());
    println!("notepad-app connecting to {}...", addr);

    let mut stream = TcpStream::connect(&addr).await?;

    // Hello — manifest는 src/manifest.toml의 *요약 형태*를 인라인. T3에서 파일 로드로 교체.
    let manifest = json!({
        "manifest": {
            "id": "notepad",
            "permissions": [],
            "ui_types": [
                "aios.std/MemoList@1",
                "aios.std/Memo@1",
                "aios.std/TextArea@1",
            ]
        }
    });
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::App,
        auth: manifest,
        client_id: "notepad-app".to_string(),
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&hello)?)).await?;

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
            println!("[notepad-app] HelloAck: actor={}", actor_str);
            break;
        }
    }

    let owner = <geulos_core::ActorId as std::str::FromStr>::from_str(&actor_str)?;
    let (mut memo_list, mut text_area) = build_initial_tree(owner);
    text_area.parent = Some(memo_list.id);
    memo_list.children.push(text_area.id);

    for obj in [&memo_list, &text_area] {
        let msg = MountMsg { root_object_id: obj.id.to_string(), tree: serde_json::to_value(obj)? };
        stream.write_all(&encode_frame(&serde_json::to_vec(&msg)?)).await?;
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
    println!("[notepad-app] mounted: memo_list={}, text_area={}", memo_list.id, text_area.id);
    println!("[notepad-app] T2 scaffold complete — T3에서 메서드 핸들러 추가");

    // T2: idle 상태로 유지. 연결 끊기 전까지 대기. (echo-app 패턴 따름)
    loop {
        let n = match stream.read(&mut buf).await {
            Ok(n) => n,
            Err(e) => {
                eprintln!("read error: {}", e);
                break;
            }
        };
        if n == 0 {
            break;
        }
        // T2에선 들어오는 메시지를 *소비만* 하고 처리 안 함. T3에서 디스패치.
        accum.extend_from_slice(&buf[..n]);
        loop {
            let mut slice = accum.as_slice();
            if decode_frame(&mut slice).is_err() {
                break;
            }
            let consumed = accum.len() - slice.len();
            accum.drain(..consumed);
        }
    }
    println!("[notepad-app] exit");
    Ok(())
}
