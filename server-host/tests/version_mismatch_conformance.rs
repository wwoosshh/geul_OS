use geulos_proto::*;
use geulos_server_host::run_listener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn version_mismatch_returns_hello_reject() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let hello = Hello {
        version: "0.2".to_string(), // mismatch
        role: Role::Ai,
        auth: serde_json::json!({}),
        client_id: "t".to_string(),
    };
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp_body = decode_frame(&mut slice).unwrap();
    let rej: HelloReject = serde_json::from_slice(&resp_body).expect("HelloReject");
    assert_eq!(rej.reason, "version_mismatch");
}
