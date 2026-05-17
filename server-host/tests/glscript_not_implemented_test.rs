//! M5 T8 — Glscript 메시지가 NotImplemented + M5.5 가이드 반환.

use geulos_proto::{decode_frame, encode_frame, GlscriptMsg, Hello, HelloAck, Role};
use geulos_server_host::run_listener;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn glscript_returns_not_implemented_with_m5_5_guidance() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Ai,
        auth: json!({}),
        client_id: "t".to_string(),
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&hello).unwrap())).await.unwrap();

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let _: HelloAck = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();

    let g = GlscriptMsg {
        request_id: "g-1".to_string(),
        source: "let x = 1.".to_string(),
        budget: json!({}),
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&g).unwrap())).await.unwrap();
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp_bytes = decode_frame(&mut slice).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&resp_bytes).unwrap();
    assert_eq!(v["kind"], "GlscriptError");
    assert_eq!(v["error_kind"], "not_implemented");
    let detail = v["detail"].as_str().unwrap();
    assert!(detail.contains("M5.5"), "detail should mention M5.5; got: {}", detail);
    assert!(detail.contains("ADR-015"), "detail should mention ADR-015; got: {}", detail);
}
