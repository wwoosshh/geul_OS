//! 한 클라이언트 연결의 read/write 루프.

use geulos_core::ActorId;
use geulos_proto::{
    decode_frame, encode_frame, DecodeError, Hello, HelloAck, HelloReject, InvokeMsg, MountMsg,
    QueryMsg, Role,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use uuid::Uuid;

use crate::dispatch::{handle_invoke, handle_mount, handle_query};
use crate::ObjectServerHandle;

pub async fn handle_connection(mut stream: TcpStream, handle: ObjectServerHandle) {
    let actor_id = match read_and_handle_hello(&mut stream).await {
        Ok(id) => id,
        Err(e) => {
            eprintln!("handshake failed: {}", e);
            return;
        }
    };

    // 메시지 read 루프
    let mut accum: Vec<u8> = Vec::new();
    let mut tmp = vec![0u8; 4096];
    loop {
        let n = match stream.read(&mut tmp).await {
            Ok(0) => return,
            Ok(n) => n,
            Err(_) => return,
        };
        accum.extend_from_slice(&tmp[..n]);

        loop {
            let mut slice = accum.as_slice();
            match decode_frame(&mut slice) {
                Ok(body) => {
                    let consumed = accum.len() - slice.len();
                    let body = body.clone();
                    accum.drain(..consumed);

                    let resp = dispatch_message(&handle, &actor_id, &body).await;
                    if let Some(resp_val) = resp {
                        let resp_body = serde_json::to_vec(&resp_val).unwrap_or_default();
                        let _ = stream.write_all(&encode_frame(&resp_body)).await;
                    }
                }
                Err(DecodeError::Incomplete) => break,
                Err(DecodeError::TooLarge(_)) => return,
            }
        }
    }
}

/// 메시지 종류에 따라 dispatch. 응답이 있으면 JSON Value 반환.
async fn dispatch_message(
    handle: &ObjectServerHandle,
    actor: &ActorId,
    body: &[u8],
) -> Option<serde_json::Value> {
    let raw: serde_json::Value = serde_json::from_slice(body).ok()?;
    let kind = raw.get("kind").and_then(|v| v.as_str())?;

    match kind {
        "Mount" => {
            let m: MountMsg = serde_json::from_value(raw).ok()?;
            Some(handle_mount(handle, m).await)
        }
        "Invoke" => {
            let m: InvokeMsg = serde_json::from_value(raw).ok()?;
            Some(handle_invoke(handle, m, actor.clone()).await)
        }
        "Query" => {
            let m: QueryMsg = serde_json::from_value(raw).ok()?;
            Some(handle_query(handle, m).await)
        }
        _ => None, // Subscribe 등은 Task 7
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
    stream.write_all(&encode_frame(&body)).await.map_err(|e| e.to_string())?;
    Ok(())
}
