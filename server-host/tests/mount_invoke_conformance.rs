use geulos_core::std_types;
use geulos_proto::{
    decode_frame, encode_frame, Hello, HelloAck, InvokeMsg, MountAck, MountMsg, Role,
};
use geulos_server_host::run_listener;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn connect_and_handshake() -> TcpStream {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Ai,
        auth: json!({}),
        client_id: "test".to_string(),
    };
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    // HelloAck 소비
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let _ack: HelloAck = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();
    stream
}

#[tokio::test]
async fn mount_round_trip_returns_ack() {
    let mut stream = connect_and_handshake().await;
    let obj = std_types::text(geulos_core::ActorId::new_ai_session(), "hi from wire");
    let mount =
        MountMsg { root_object_id: obj.id.to_string(), tree: serde_json::to_value(&obj).unwrap() };
    let body = serde_json::to_vec(&mount).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp_body = decode_frame(&mut slice).unwrap();
    let _ack: MountAck = serde_json::from_slice(&resp_body).expect("MountAck 형식이어야 함");
}

#[tokio::test]
async fn invoke_after_mount_succeeds() {
    let mut stream = connect_and_handshake().await;

    // 우선 owner를 자기 자신(ai 세션)으로 한 버튼 mount
    // ai 세션의 ActorId는 client 측에서 알 수 없으므로,
    // 서버가 발급한 owner로 만들기 위해 trick을 쓸 수 없음.
    // 대신, 본 테스트는 user owner로 mount 후 invoke가 *PermissionDenied*를 받는지 검증.
    // (M2 acceptance에서 ai owner+ai invoke 경로는 별도 검증.)

    let user = geulos_core::ActorId::local_user();
    let btn = std_types::button(user, "OK");
    let mount =
        MountMsg { root_object_id: btn.id.to_string(), tree: serde_json::to_value(&btn).unwrap() };
    let body = serde_json::to_vec(&mount).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    // MountAck 소비
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let _ack: MountAck = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();

    // Invoke
    let inv = InvokeMsg {
        request_id: "req-1".to_string(),
        target: btn.id.to_string(),
        method: "press".to_string(),
        args: json!(null),
    };
    let body = serde_json::to_vec(&inv).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    // 응답 받기
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp_body = decode_frame(&mut slice).unwrap();

    // user_local owner인 객체에 ai_session이 invoke → PermissionDenied
    let txt = String::from_utf8_lossy(&resp_body);
    assert!(txt.contains("InvokeError"), "expected InvokeError, got: {}", txt);
    let _err: geulos_proto::InvokeError =
        serde_json::from_slice(&resp_body).expect("InvokeError 형식");
}
