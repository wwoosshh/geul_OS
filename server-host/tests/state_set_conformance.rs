use geulos_core::{std_types, ActorId};
use geulos_proto::{
    decode_frame, encode_frame, Hello, HelloAck, MountAck, MountMsg, Role, StateSetError,
    StateSetMsg,
};
use geulos_server_host::run_listener;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn state_set_by_non_owner_returns_permission_denied() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    // connect as ai actor
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Ai,
        auth: json!({}),
        client_id: "t".to_string(),
    };
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let _ack: HelloAck = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();

    // mount a text object owned by user:local — NOT the ai session actor
    let txt = std_types::text(ActorId::local_user(), "before");
    let target = txt.id.to_string();
    let mount =
        MountMsg { root_object_id: target.clone(), tree: serde_json::to_value(&txt).unwrap() };
    let body = serde_json::to_vec(&mount).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let _ack: MountAck = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();

    // ai session tries StateSet on user:local owned object → permission denied
    let ss = StateSetMsg {
        request_id: "r-1".to_string(),
        target: target.clone(),
        key: "content".to_string(),
        value: json!("after"),
    };
    let body = serde_json::to_vec(&ss).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp_body = decode_frame(&mut slice).unwrap();
    let err: StateSetError = serde_json::from_slice(&resp_body).expect("expected StateSetError");
    assert_eq!(err.kind, "permission");
}
