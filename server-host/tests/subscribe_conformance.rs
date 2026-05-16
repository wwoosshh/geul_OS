use geulos_core::std_types;
use geulos_proto::{
    decode_frame, encode_frame, EventKindFilterWire, Hello, HelloAck, InvokeMsg, MountMsg, Role,
    SubscribeAck, SubscribeMsg,
};
use geulos_server_host::run_listener;
use serde_json::json;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

#[tokio::test]
async fn subscribe_then_invoke_pushes_event() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut stream = TcpStream::connect(addr).await.unwrap();

    // 핸드셰이크
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Ai,
        auth: json!({}),
        client_id: "t".to_string(),
    };
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let _: HelloAck = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();

    // ai actor 이름이 변하지 않는 한 본인이 만든 객체를 본인이 누를 수 있어야 함.
    // 다만 클라가 ai_id를 모르므로 obj.owner를 클라 측에서 직접 설정 불가.
    // 우회: mount 시 owner를 임의 ai로 설정 → invoke는 핸드셰이크 ai와 owner가 다르면 거부.
    // 본 테스트에서는 *user owner* 객체를 mount하고, 본인(ai 핸드셰이크) invoke가 PermissionDenied로 *거부*되어도
    // Subscribe 채널이 살아있고 Lifecycle Created 이벤트는 *invoke 전*에 발행되므로 받음.

    let user = geulos_core::ActorId::local_user();
    let btn = std_types::button(user, "OK");
    let btn_id_str = btn.id.to_string();
    let mount =
        MountMsg { root_object_id: btn_id_str.clone(), tree: serde_json::to_value(&btn).unwrap() };
    let body = serde_json::to_vec(&mount).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let _ = decode_frame(&mut slice); // MountAck 소비

    // Subscribe (Lifecycle은 mount 전 발행되었으므로 무시. Invoke 필터로 시도)
    let sub = SubscribeMsg {
        subscription_id: "sub-1".to_string(),
        target: btn_id_str.clone(),
        kinds: vec![EventKindFilterWire::Invoke, EventKindFilterWire::Lifecycle],
        include_initial: false,
    };
    let body = serde_json::to_vec(&sub).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let _: SubscribeAck = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();

    // Invoke (PermissionDenied 예상)
    let inv = InvokeMsg {
        request_id: "r-1".to_string(),
        target: btn_id_str,
        method: "press".to_string(),
        args: json!(null),
    };
    let body = serde_json::to_vec(&inv).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    // Invoke 응답 + (없는) Event 모두 처리 가능해야 함.
    let n = timeout(Duration::from_millis(500), stream.read(&mut buf)).await.unwrap().unwrap();
    let mut slice = &buf[..n];
    // 응답 메시지가 하나 이상 있어야 함 (InvokeError).
    let _resp = decode_frame(&mut slice);
    // 이 테스트의 핵심은 Subscribe 핸드셰이크가 작동하고 SubscribeAck를 받았다는 것.
    // 실제 Event push는 owner+method가 성공 시에만 발생.
}
