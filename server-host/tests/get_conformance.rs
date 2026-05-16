use geulos_core::{std_types, ActorId};
use geulos_proto::*;
use geulos_server_host::run_listener;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn get_returns_full_object() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Ai,
        auth: json!({}),
        client_id: "c".to_string(),
    };
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let _: HelloAck = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();

    // 객체 mount
    let txt = std_types::text(ActorId::local_user(), "hi");
    let target = txt.id.to_string();
    let mount =
        MountMsg { root_object_id: target.clone(), tree: serde_json::to_value(&txt).unwrap() };
    let body = serde_json::to_vec(&mount).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let _: MountAck = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();

    // Get
    let g = GetMsg { request_id: "g-1".to_string(), target: target.clone() };
    let body = serde_json::to_vec(&g).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp: GetResult = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();
    assert_eq!(resp.request_id, "g-1");
    assert!(resp.object.get("id").is_some());
}
