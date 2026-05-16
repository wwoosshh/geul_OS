//! M3 acceptance: echo-app subprocess + 외부 클라이언트가 press → count 증가 관찰.
//!
//! `#[ignore]` 처리 — subprocess를 spawn하므로 일반 CI 실행에서는 제외.
//! 수동 실행: `cargo test -p geulos-server-host --test m3_acceptance --include-ignored`

use geulos_proto::*;
use geulos_server_host::run_listener;
use serde_json::json;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::time::timeout;

#[tokio::test]
#[ignore = "spawns subprocess; run with --include-ignored"]
async fn echo_app_button_press_increments_counter() -> Result<(), Box<dyn std::error::Error>> {
    // 1) 서버 띄우기
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(run_listener(listener));

    // 2) echo-app subprocess spawn
    // CARGO_BIN_EXE_<name> is set by Cargo at runtime for integration tests when
    // the binary's package is listed in [dev-dependencies].
    // Fall back to a computed path from the workspace root.
    let echo_exe = std::env::var("CARGO_BIN_EXE_geulos-echo-app").unwrap_or_else(|_| {
        // Fallback: navigate from server-host manifest dir up to workspace root / target/debug
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_dir.parent().expect("server-host parent = workspace root");
        let ext = if cfg!(windows) { ".exe" } else { "" };
        workspace_root
            .join("target")
            .join("debug")
            .join(format!("geulos-echo-app{}", ext))
            .to_string_lossy()
            .into_owned()
    });
    let mut child = Command::new(echo_exe)
        .arg(addr.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    // echo-app이 mount + subscribe 완료할 시간 부여
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 3) 외부 클라이언트 (geulosh 역할)로 접속
    let mut stream = TcpStream::connect(addr).await?;
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Ai,
        auth: json!({}),
        client_id: "external".to_string(),
    };
    let body = serde_json::to_vec(&hello)?;
    stream.write_all(&encode_frame(&body)).await?;
    let mut buf = vec![0u8; 16384];
    let n = stream.read(&mut buf).await?;
    let mut slice = &buf[..n];
    let _ack: HelloAck = serde_json::from_slice(&decode_frame(&mut slice)?)?;

    // 4) query type aios.std/Button@1 으로 button 찾기
    let q = QueryMsg {
        request_id: "q-1".to_string(),
        query: QueryPredicate::ByType { type_uri: "aios.std/Button@1".to_string() },
    };
    let body = serde_json::to_vec(&q)?;
    stream.write_all(&encode_frame(&body)).await?;
    let n = stream.read(&mut buf).await?;
    let mut slice = &buf[..n];
    let qres: QueryResult = serde_json::from_slice(&decode_frame(&mut slice)?)?;
    assert!(!qres.objects.is_empty(), "echo-app의 button을 찾지 못함");
    let button_id = qres.objects[0].clone();

    // 5) Text 찾기
    let q2 = QueryMsg {
        request_id: "q-2".to_string(),
        query: QueryPredicate::ByType { type_uri: "aios.std/Text@1".to_string() },
    };
    let body = serde_json::to_vec(&q2)?;
    stream.write_all(&encode_frame(&body)).await?;
    let n = stream.read(&mut buf).await?;
    let mut slice = &buf[..n];
    let qres2: QueryResult = serde_json::from_slice(&decode_frame(&mut slice)?)?;
    assert!(!qres2.objects.is_empty(), "echo-app의 text를 찾지 못함");
    let text_id = qres2.objects[0].clone();

    // 6) Subscribe to text (StateSet 필터)
    let sub = SubscribeMsg {
        subscription_id: "obs-text".to_string(),
        target: text_id.clone(),
        kinds: vec![EventKindFilterWire::StateSet],
        include_initial: false,
    };
    let body = serde_json::to_vec(&sub)?;
    stream.write_all(&encode_frame(&body)).await?;
    let n = stream.read(&mut buf).await?;
    let mut slice = &buf[..n];
    let _: SubscribeAck = serde_json::from_slice(&decode_frame(&mut slice)?)?;

    // 7) 버튼 press 호출
    //    button.acl 에 wildcard Allow 가 있으므로 InvokeAck 기대.
    let inv = InvokeMsg {
        request_id: "ext-1".to_string(),
        target: button_id.clone(),
        method: "press".to_string(),
        args: json!(null),
    };
    let body = serde_json::to_vec(&inv)?;
    stream.write_all(&encode_frame(&body)).await?;

    // InvokeAck / InvokeError 응답 수신 (버퍼에 쌓음)
    let n = timeout(Duration::from_millis(1000), stream.read(&mut buf)).await??;
    {
        let mut slice = &buf[..n];
        if let Ok(body) = decode_frame(&mut slice) {
            let raw: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let kind = raw.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            assert_ne!(kind, "InvokeError", "press が拒否された: {:?}", raw);
        }
    }

    // 8) Subscribe로 StateSet 이벤트 기다리기 (최대 3초)
    //
    // 타이밍:
    //   - echo-app이 100ms마다 이벤트를 drain → press 이벤트 수신
    //   - echo-app이 StateSet 전송 → 서버 처리
    //   - 외부 클라이언트가 100ms마다 text 구독 drain → StateSet 이벤트 수신
    //   총 지연: 최대 ~200ms. 3초 타임아웃은 충분히 여유 있음.
    //
    // EventMsg.event 는 core::Event 를 JSON으로 직렬화한 Value 이다.
    // core::Event.kind 는 EventKind (serde tag = "kind") 로,
    // {"kind": "StateSet", "key": "content", "value": "count: 1"} 형태.
    let mut got_state_set = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline && !got_state_set {
        let n = match timeout(Duration::from_millis(300), stream.read(&mut buf)).await {
            Ok(Ok(0)) => break, // 연결 종료
            Ok(Ok(n)) => n,
            Ok(Err(_)) => break,
            Err(_) => continue, // timeout — 다시 시도
        };
        let mut slice = &buf[..n];
        while let Ok(body) = decode_frame(&mut slice) {
            let raw: serde_json::Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => continue,
            };
            // 최상위 kind == "Event" 인 메시지가 EventMsg
            if raw.get("kind").and_then(|v| v.as_str()) == Some("Event") {
                let ev: EventMsg = match serde_json::from_value(raw) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                // ev.event 는 core::Event 의 직렬화.
                // ev.event["kind"] = {"kind": "StateSet", "key": ..., "value": ...}
                let event_kind_obj = ev.event.get("kind");
                if event_kind_obj.and_then(|k| k.get("kind")).and_then(|v| v.as_str())
                    == Some("StateSet")
                {
                    got_state_set = true;
                    break;
                }
            }
        }
    }

    let _ = child.kill().await;

    assert!(
        got_state_set,
        "Text의 StateSet 이벤트를 못 받음 — echo-app이 press에 반응하지 않은 듯"
    );

    Ok(())
}
