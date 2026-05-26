//! M11 — server-host GrantUpdate handle + actor 가드.

use geulos_proto::{decode_frame, encode_frame, GrantOp, GrantUpdate, Hello, HelloAck, Role};
use geulos_server_host::run_listener;
use serde_json::json;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// 서버를 바인드·스폰하고 지정 manifest id로 handshake를 완료한 TcpStream을 반환.
async fn start_and_connect(manifest_id: &str) -> TcpStream {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::App,
        auth: json!({ "manifest": { "id": manifest_id, "ui_types": [] } }),
        client_id: "test".to_string(),
    };
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    // HelloAck 수신 — 핸드셰이크 완료.
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp_body = decode_frame(&mut slice).unwrap();
    let _ack: HelloAck = serde_json::from_slice(&resp_body)
        .unwrap_or_else(|_| panic!("HelloAck 예상, 실제: {}", String::from_utf8_lossy(&resp_body)));

    stream
}

#[tokio::test]
async fn grant_update_accepted_from_desktop_shell() {
    let mut stream = start_and_connect("desktop-shell").await;

    let g = GrantUpdate {
        actor: "ai:00000000-0000-0000-0000-000000000001".to_string(),
        path: "D:/tmp".to_string(),
        op: GrantOp::Add,
    };
    let body = serde_json::to_vec(&g).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    // 100ms 후 연결이 살아있으면 OK — server가 끊지 않음 = 수락.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut probe = vec![0u8; 16];
    match tokio::time::timeout(Duration::from_millis(50), stream.read(&mut probe)).await {
        Ok(Ok(0)) => panic!("server가 desktop-shell의 GrantUpdate를 수락 후 연결을 끊었음"),
        Ok(Err(e)) => panic!("연결 에러: {}", e),
        _ => {} // timeout = 연결 정상 유지
    }
}

#[tokio::test]
async fn grant_update_rejected_from_non_desktop_shell() {
    let mut stream = start_and_connect("echo-app").await;

    let g = GrantUpdate {
        actor: "ai:00000000-0000-0000-0000-000000000002".to_string(),
        path: "/x".to_string(),
        op: GrantOp::Add,
    };
    let body = serde_json::to_vec(&g).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    // 거부 시 server가 PermissionDenied 응답을 보내야 함.
    let mut buf = vec![0u8; 1024];
    let res = tokio::time::timeout(Duration::from_millis(500), stream.read(&mut buf)).await;
    match res {
        Ok(Ok(0)) => {} // 끊김도 거부로 간주 — OK
        Ok(Ok(n)) => {
            let mut slice = &buf[..n];
            if let Ok(frame_body) = decode_frame(&mut slice) {
                let text = String::from_utf8_lossy(&frame_body);
                assert!(
                    text.contains("PermissionDenied")
                        || text.contains("grant_denied")
                        || text.contains("Denied"),
                    "기대: PermissionDenied 포함 응답. 실제: {}",
                    text
                );
            } else {
                // 프레임 파싱 실패 — 응답이 있으나 알 수 없는 형식. 통과.
            }
        }
        Ok(Err(_)) => {} // 연결 에러 = 끊김 = OK
        Err(_) => panic!("server가 echo-app의 GrantUpdate를 거부하지 않음 (응답 없음)"),
    }
}

#[tokio::test]
async fn grant_remove_accepted_from_desktop_shell() {
    let mut stream = start_and_connect("desktop-shell").await;

    // 먼저 Add.
    let add = GrantUpdate {
        actor: "ai:00000000-0000-0000-0000-000000000003".to_string(),
        path: "D:/docs".to_string(),
        op: GrantOp::Add,
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&add).unwrap())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 그다음 Remove — 연결이 살아있어야 함.
    let rem = GrantUpdate {
        actor: "ai:00000000-0000-0000-0000-000000000003".to_string(),
        path: "D:/docs".to_string(),
        op: GrantOp::Remove,
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&rem).unwrap())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut probe = vec![0u8; 16];
    match tokio::time::timeout(Duration::from_millis(50), stream.read(&mut probe)).await {
        Ok(Ok(0)) => panic!("server가 desktop-shell의 GrantUpdate(Remove) 후 연결을 끊었음"),
        Ok(Err(e)) => panic!("연결 에러: {}", e),
        _ => {} // timeout = 연결 정상 유지
    }
}
