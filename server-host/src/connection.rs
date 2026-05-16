//! 한 클라이언트 연결의 read/write 루프.

use geulos_core::ActorId;
use geulos_proto::{decode_frame, encode_frame, DecodeError, Hello, HelloAck, HelloReject, Role};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use uuid::Uuid;

use crate::ObjectServerHandle;

/// 한 연결을 처리.
pub async fn handle_connection(mut stream: TcpStream, _handle: ObjectServerHandle) {
    // 핸드셰이크
    let actor_id = match read_and_handle_hello(&mut stream).await {
        Ok(id) => id,
        Err(e) => {
            eprintln!("handshake failed: {}", e);
            return;
        }
    };
    let _ = actor_id; // Task 6에서 메시지 디스패치에 사용

    // M2 Task 5는 핸드셰이크까지만. Task 6+에서 read 루프 추가.
    let mut buf = vec![0u8; 4096];
    loop {
        let n = match stream.read(&mut buf).await {
            Ok(0) => return,
            Ok(n) => n,
            Err(_) => return,
        };
        // 후속 태스크에서 메시지 디스패치
        let _ = n;
    }
}

async fn read_and_handle_hello(stream: &mut TcpStream) -> Result<ActorId, String> {
    let mut accum = Vec::new();
    let mut tmp = vec![0u8; 4096];
    loop {
        let n = stream.read(&mut tmp).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("connection closed before Hello".to_string());
        }
        accum.extend_from_slice(&tmp[..n]);
        let mut slice = accum.as_slice();
        match decode_frame(&mut slice) {
            Ok(body) => {
                let consumed = accum.len() - slice.len();
                let body = body.clone();
                accum.drain(..consumed);

                let hello: Hello = match serde_json::from_slice(&body) {
                    Ok(h) => h,
                    Err(e) => {
                        let rej = HelloReject {
                            reason: "malformed_hello".to_string(),
                            detail: e.to_string(),
                        };
                        write_message(stream, &rej).await?;
                        return Err(format!("malformed Hello: {}", e));
                    }
                };

                if hello.version != "0.1" {
                    let rej = HelloReject {
                        reason: "version_mismatch".to_string(),
                        detail: format!("server: 0.1, client: {}", hello.version),
                    };
                    write_message(stream, &rej).await?;
                    return Err("version mismatch".to_string());
                }

                let actor = match hello.role {
                    Role::Ai => ActorId::new_ai_session(),
                    Role::App => ActorId::new_app(
                        hello
                            .auth
                            .get("manifest")
                            .and_then(|m| m.get("id"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown"),
                    ),
                    Role::Compositor => ActorId::system_compositor(),
                };

                let ack = HelloAck {
                    session_id: Uuid::new_v4().to_string(),
                    actor_id: actor.as_str().to_string(),
                    server_version: "0.1".to_string(),
                    capabilities: vec![
                        "mount".to_string(),
                        "invoke".to_string(),
                        "subscribe".to_string(),
                        "query".to_string(),
                    ],
                };
                write_message(stream, &ack).await?;
                return Ok(actor);
            }
            Err(DecodeError::Incomplete) => continue,
            Err(DecodeError::TooLarge(n)) => return Err(format!("frame too large: {}", n)),
        }
    }
}

async fn write_message<T: serde::Serialize>(stream: &mut TcpStream, msg: &T) -> Result<(), String> {
    let body = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    let frame = encode_frame(&body);
    stream.write_all(&frame).await.map_err(|e| e.to_string())?;
    Ok(())
}
