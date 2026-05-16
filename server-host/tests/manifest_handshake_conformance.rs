use geulos_proto::{decode_frame, encode_frame, Hello, HelloAck, HelloReject, Role};
use geulos_server_host::run_listener;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn app_hello_with_valid_manifest_succeeds() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::App,
        auth: json!({
            "manifest": {
                "id": "test-app",
                "permissions": [],
                "ui_types": ["aios.std/Text@1"]
            }
        }),
        client_id: "c".to_string(),
    };
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp_body = decode_frame(&mut slice).unwrap();
    let ack: HelloAck = serde_json::from_slice(&resp_body).unwrap_or_else(|_| {
        panic!("expected HelloAck, got: {}", String::from_utf8_lossy(&resp_body))
    });
    assert!(ack.actor_id.starts_with("app:test-app:"));
}

#[tokio::test]
async fn app_hello_with_missing_manifest_rejected() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::App,
        auth: json!({}), // no manifest
        client_id: "c".to_string(),
    };
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp_body = decode_frame(&mut slice).unwrap();
    let rej: HelloReject = serde_json::from_slice(&resp_body).expect("expected HelloReject");
    assert_eq!(rej.reason, "missing_manifest");
}

#[tokio::test]
async fn app_hello_with_invalid_manifest_rejected() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::App,
        auth: json!({"manifest": {"id": "x", "permissions": [], "ui_types": ["bad type uri"]}}),
        client_id: "c".to_string(),
    };
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp_body = decode_frame(&mut slice).unwrap();
    let rej: HelloReject = serde_json::from_slice(&resp_body).expect("expected HelloReject");
    assert_eq!(rej.reason, "invalid_manifest");
}
