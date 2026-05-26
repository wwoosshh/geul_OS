//! M11 — AI invoke가 GrantUpdate 후 AllowIfGrantedDir 통과하는지 end-to-end 회귀.
//!
//! 시나리오:
//! 1. server 바인드 + 스폰
//! 2. desktop-shell role 연결 + AllowIfGrantedDir ACL이 붙은 Folder 객체 mount
//! 3. AI role 연결
//! 4. grant 없이 AI invoke → InvokeError (권한 없음)
//! 5. desktop-shell이 GrantUpdate(Add) 송신
//! 6. AI invoke 재시도 → InvokeAck (통과)
//!
//! 파일명에 "update" 키워드 없음 — Windows installer/UAC 탐지 회피.

use geulos_core::{std_types, AclEffect, AclEntry, ActorId, ActorPattern, MethodPattern};
use geulos_proto::{
    decode_frame, encode_frame, GrantOp, GrantUpdate, Hello, HelloAck, InvokeMsg, MountMsg, Role,
};
use geulos_server_host::run_listener;
use serde_json::json;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// server를 바인드·스폰하고 랜덤 포트를 반환.
async fn start_server() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));
    addr
}

/// 지정 역할로 연결하고 HelloAck를 반환.
async fn connect_as(addr: std::net::SocketAddr, hello: Hello) -> (TcpStream, HelloAck) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let ack = read_frame_typed::<HelloAck>(&mut stream).await;
    (stream, ack)
}

/// TcpStream에서 하나의 프레임을 읽어 T로 역직렬화.
async fn read_frame_typed<T: serde::de::DeserializeOwned>(stream: &mut TcpStream) -> T {
    let mut accum: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 16384];
    loop {
        let n = stream.read(&mut buf).await.unwrap();
        assert!(n > 0, "연결이 예상보다 일찍 닫힘");
        accum.extend_from_slice(&buf[..n]);
        let mut slice = accum.as_slice();
        if let Ok(body) = decode_frame(&mut slice) {
            return serde_json::from_slice(&body).unwrap_or_else(|e| {
                panic!("역직렬화 실패: {e}\n원본: {}", String::from_utf8_lossy(&body))
            });
        }
    }
}

/// TcpStream에서 하나의 프레임을 읽어 raw JSON Value로 반환.
async fn read_frame_value(stream: &mut TcpStream) -> serde_json::Value {
    let mut accum: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 16384];
    loop {
        let n = stream.read(&mut buf).await.unwrap();
        assert!(n > 0, "연결이 예상보다 일찍 닫힘");
        accum.extend_from_slice(&buf[..n]);
        let mut slice = accum.as_slice();
        if let Ok(body) = decode_frame(&mut slice) {
            return serde_json::from_slice(&body).unwrap_or_else(|e| panic!("JSON 파싱 실패: {e}"));
        }
    }
}

#[tokio::test]
async fn ai_can_invoke_folder_in_granted_dir_only() {
    // 1. server 바인드 + 스폰
    let addr = start_server().await;

    // 2. desktop-shell 연결
    let shell_hello = Hello {
        version: "0.1".to_string(),
        role: Role::App,
        auth: json!({ "manifest": { "id": "desktop-shell", "ui_types": [] } }),
        client_id: "shell".to_string(),
    };
    let (mut shell, _shell_ack) = connect_as(addr, shell_hello).await;

    // 3. AllowIfGrantedDir ACL이 붙은 Folder 객체 mount
    let owner = ActorId::local_user();
    let mut folder = std_types::folder(owner.clone(), "D:/granted/foo", "foo", 0);
    folder.acl.push(AclEntry {
        actor: ActorPattern::AiSession,
        method: MethodPattern::Wildcard,
        effect: AclEffect::AllowIfGrantedDir,
    });
    let folder_id = folder.id;

    let mount = MountMsg {
        root_object_id: folder_id.to_string(),
        tree: serde_json::to_value(&folder).unwrap(),
    };
    let mount_body = serde_json::to_vec(&mount).unwrap();
    shell.write_all(&encode_frame(&mount_body)).await.unwrap();

    // Mount는 MountAck 또는 MountReject 응답이 옴 — 읽어서 버림 (성공 여부만 확인).
    let mount_resp = read_frame_value(&mut shell).await;
    assert_eq!(
        mount_resp.get("kind").and_then(|v| v.as_str()),
        Some("MountAck"),
        "Folder mount 실패: {mount_resp}"
    );

    // 4. AI 연결
    let ai_hello = Hello {
        version: "0.1".to_string(),
        role: Role::Ai,
        auth: json!({}),
        client_id: "ai".to_string(),
    };
    let (mut ai, ai_ack) = connect_as(addr, ai_hello).await;
    let ai_actor_str = ai_ack.actor_id.clone();

    // 5. grant 없는 상태 — AI invoke 거부 기대
    let inv1 = InvokeMsg {
        request_id: "r1".to_string(),
        target: folder_id.to_string(),
        method: "list".to_string(),
        args: json!({}),
    };
    ai.write_all(&encode_frame(&serde_json::to_vec(&inv1).unwrap())).await.unwrap();

    let resp1 = read_frame_value(&mut ai).await;
    assert_eq!(resp1.get("request_id").and_then(|v| v.as_str()), Some("r1"), "request_id 불일치");
    assert_eq!(
        resp1.get("kind").and_then(|v| v.as_str()),
        Some("InvokeError"),
        "grant 없이 AI invoke가 거부되지 않음: {resp1}"
    );

    // 6. desktop-shell이 GrantUpdate(Add) 송신
    let grant = GrantUpdate {
        actor: ai_actor_str.clone(),
        path: "D:/granted/foo".to_string(),
        op: GrantOp::Add,
    };
    shell.write_all(&encode_frame(&serde_json::to_vec(&grant).unwrap())).await.unwrap();

    // GrantUpdate는 응답 없음 — server가 반영할 시간 대기.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 7. AI invoke 재시도 — grant 후 통과 기대
    let inv2 = InvokeMsg {
        request_id: "r2".to_string(),
        target: folder_id.to_string(),
        method: "list".to_string(),
        args: json!({}),
    };
    ai.write_all(&encode_frame(&serde_json::to_vec(&inv2).unwrap())).await.unwrap();

    let resp2 = read_frame_value(&mut ai).await;
    assert_eq!(resp2.get("request_id").and_then(|v| v.as_str()), Some("r2"), "request_id 불일치");
    assert_eq!(
        resp2.get("kind").and_then(|v| v.as_str()),
        Some("InvokeAck"),
        "grant 후 AI invoke가 통과되지 않음: {resp2}"
    );
}
