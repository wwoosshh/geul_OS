//! KI-032 회귀: wire request()가 서버 무응답 시 deadline으로 빠져나오는지.

use std::time::Duration;

use geulos_ai_bridge::wire::{WireClient, WireError};
use geulos_proto::{encode_frame, HelloAck};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn request_times_out_when_server_silent() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let _ = sock.read(&mut buf).await.unwrap();
        let ack = HelloAck {
            session_id: "sess:test".to_string(),
            actor_id: "ai:test".to_string(),
            server_version: "0.1".to_string(),
            capabilities: Vec::new(),
        };
        let body = serde_json::to_vec(&ack).unwrap();
        sock.write_all(&encode_frame(&body)).await.unwrap();
        // 핸드셰이크 후 침묵 → 후속 RPC가 deadline으로 빠져나와야 함.
        tokio::time::sleep(Duration::from_secs(5)).await;
    });

    let mut client = WireClient::connect_as_ai(&addr.to_string())
        .await
        .unwrap()
        .with_request_timeout(Duration::from_millis(200));

    let start = std::time::Instant::now();
    let res = client.get_object("00000000-0000-0000-0000-000000000000").await;
    let elapsed = start.elapsed();

    assert!(matches!(res, Err(WireError::Timeout(_))), "expected Timeout, got {res:?}");
    assert!(elapsed < Duration::from_secs(2), "should fail fast, took {elapsed:?}");

    server.abort();
}
