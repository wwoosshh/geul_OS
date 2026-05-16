use geulos_proto::{decode_frame, encode_frame, Hello, HelloAck, Role};
use geulos_server_host::run_listener;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn server_accepts_hello_and_returns_ack() {
    // 사용 가능한 포트로 서버 시작
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        run_listener(listener).await;
    });

    // 클라가 접속
    let mut stream = TcpStream::connect(addr).await.unwrap();

    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Ai,
        auth: json!({"token": "test"}),
        client_id: "test-client".to_string(),
    };
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    // HelloAck 받기
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp_body = decode_frame(&mut slice).unwrap();
    let ack: HelloAck = serde_json::from_slice(&resp_body).expect("HelloAck 형식이어야 함");

    assert_eq!(ack.server_version, "0.1");
    assert!(!ack.session_id.is_empty());
    assert!(ack.actor_id.starts_with("ai:"));
}
