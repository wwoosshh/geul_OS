//! desktop-shell 진입점 — server-host 연결 + 워크스페이스 스캔 + Desktop 트리 mount.
//!
//! 흐름:
//! 1. 워크스페이스 루트 확보 (없으면 생성).
//! 2. server-host(127.0.0.1:5550)에 TCP 연결, Hello 전송.
//! 3. HelloAck에서 ActorId 받아옴.
//! 4. Desktop / FileTree / Canvas + 워크스페이스 스캔 결과(Folder/File)를 한꺼번에 mount.
//! 5. FileTree·Canvas에 Invoke 구독 → 컴포지터 클릭이 도착하면 invoke_handler::*로
//!    처리하고 StateSetMsg로 broadcast.

use std::str::FromStr;

use geulos_core::{
    std_types, AclEffect, AclEntry, ActorId, ActorPattern, MethodPattern, Object, ObjectId,
};
use geulos_desktop_shell::{invoke_handler, scan, workspace};
use geulos_proto::{
    decode_frame, encode_frame, EventKindFilterWire, EventMsg, Hello, HelloAck, MountAck, MountMsg,
    Role, StateSetMsg, SubscribeAck, SubscribeMsg,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SERVER_ADDR: &str = "127.0.0.1:5550";

/// TODO(T8): wildcard ACL은 임시. 매니페스트 기반 권한으로 교체 예정.
fn add_wildcard_acl(obj: &mut Object) {
    obj.acl.push(AclEntry {
        actor: ActorPattern::Wildcard,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
}

/// 문자열에서 ObjectId 파싱 (serde_json 경유 — core가 FromStr 미구현).
fn parse_object_id(s: &str) -> Option<ObjectId> {
    serde_json::from_str(&format!("\"{}\"", s)).ok()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace::resolve()?;
    workspace::ensure_exists(&root)?;
    println!("[desktop-shell] workspace root: {}", root.display());

    let addr = std::env::args().nth(1).unwrap_or_else(|| SERVER_ADDR.to_string());
    println!("[desktop-shell] connecting to {}...", addr);
    let mut stream = TcpStream::connect(&addr).await?;

    // Hello — manifest는 인라인. 데스크톱 셸이 표시할 빌트인 UI 타입 목록을 노출.
    let manifest = json!({
        "manifest": {
            "id": "desktop-shell",
            "permissions": [],
            "ui_types": [
                "aios.builtin/Desktop@1",
                "aios.builtin/FileTree@1",
                "aios.builtin/Canvas@1",
                "aios.std/Folder@1",
                "aios.std/File@1",
            ]
        }
    });
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::App,
        auth: manifest,
        client_id: "desktop-shell".to_string(),
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&hello)?)).await?;

    let mut buf = vec![0u8; 16384];
    let mut accum: Vec<u8> = Vec::new();
    let actor_str = loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Err("closed before HelloAck".into());
        }
        accum.extend_from_slice(&buf[..n]);
        let mut slice = accum.as_slice();
        if let Ok(body) = decode_frame(&mut slice) {
            let consumed = accum.len() - slice.len();
            accum.drain(..consumed);
            let ack: HelloAck = serde_json::from_slice(&body)?;
            println!("[desktop-shell] HelloAck: actor={}", ack.actor_id);
            break ack.actor_id;
        }
    };
    let owner = ActorId::from_str(&actor_str)?;

    // Desktop = [FileTree, Canvas] 두 패널. T7에서 컴포지터가 좌/우 분할로 그림.
    let mut desktop = std_types::desktop(owner.clone());
    let mut file_tree = std_types::file_tree(owner.clone(), &root.to_string_lossy());
    let mut canvas = std_types::canvas(owner.clone());
    file_tree.parent = Some(desktop.id);
    canvas.parent = Some(desktop.id);
    desktop.children = vec![file_tree.id, canvas.id];

    // TODO(T8): 컴포지터(외부 actor)가 expand/select/set_file invoke할 수 있어야 함.
    add_wildcard_acl(&mut desktop);
    add_wildcard_acl(&mut file_tree);
    add_wildcard_acl(&mut canvas);

    let file_tree_id = file_tree.id;
    let canvas_id = canvas.id;

    // 워크스페이스 스캔 — 루트 직계는 parent=None으로 돌아오므로 FileTree id로 채움.
    let scan_result = scan::scan_tree(&owner, &root)?;
    let mut all_objects: Vec<Object> = vec![desktop.clone(), file_tree.clone(), canvas.clone()];
    let mut top_level_ids = Vec::new();
    for mut obj in scan_result.objects {
        if obj.parent.is_none() {
            obj.parent = Some(file_tree_id);
            top_level_ids.push(obj.id);
        }
        all_objects.push(obj);
    }
    if let Some(ft) = all_objects.iter_mut().find(|o| o.id == file_tree_id) {
        ft.children = top_level_ids;
    }

    for obj in &all_objects {
        let msg = MountMsg { root_object_id: obj.id.to_string(), tree: serde_json::to_value(obj)? };
        stream.write_all(&encode_frame(&serde_json::to_vec(&msg)?)).await?;
        loop {
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                return Err("closed during mount".into());
            }
            accum.extend_from_slice(&buf[..n]);
            let mut slice = accum.as_slice();
            if let Ok(b) = decode_frame(&mut slice) {
                let consumed = accum.len() - slice.len();
                accum.drain(..consumed);
                let _: MountAck = serde_json::from_slice(&b)?;
                break;
            }
        }
    }
    println!("[desktop-shell] mounted {} objects", all_objects.len());

    // FileTree·Canvas에 Invoke 구독 — 클릭이 컴포지터→server→여기로 흐름.
    let subscribe_targets = [file_tree_id, canvas_id];
    for (i, target_id) in subscribe_targets.iter().enumerate() {
        let sub = SubscribeMsg {
            subscription_id: format!("sub-{}", i),
            target: target_id.to_string(),
            kinds: vec![EventKindFilterWire::Invoke],
            include_initial: false,
        };
        stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
        loop {
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                return Err("closed during subscribe".into());
            }
            accum.extend_from_slice(&buf[..n]);
            let mut slice = accum.as_slice();
            if let Ok(b) = decode_frame(&mut slice) {
                let consumed = accum.len() - slice.len();
                accum.drain(..consumed);
                let _: SubscribeAck = serde_json::from_slice(&b)?;
                break;
            }
        }
    }
    println!("[desktop-shell] subscribed to FileTree and Canvas invoke events");

    // 이벤트 루프 — Invoke를 받아 invoke_handler로 처리하고 StateSetMsg를 송신.
    let mut tracked_expanded: Vec<ObjectId> = Vec::new();
    let mut req_seq: u64 = 0;
    loop {
        let n = match stream.read(&mut buf).await {
            Ok(n) => n,
            Err(e) => {
                eprintln!("[desktop-shell] read error: {}", e);
                break;
            }
        };
        if n == 0 {
            break;
        }
        accum.extend_from_slice(&buf[..n]);
        loop {
            let mut slice = accum.as_slice();
            let body = match decode_frame(&mut slice) {
                Ok(b) => b,
                Err(_) => break,
            };
            let consumed = accum.len() - slice.len();
            accum.drain(..consumed);
            let raw: serde_json::Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let frame_kind = raw.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if frame_kind != "Event" {
                continue;
            }
            let ev: EventMsg = match serde_json::from_value(raw) {
                Ok(e) => e,
                Err(_) => continue,
            };
            let event_kind_obj = ev.event.get("kind");
            let is_invoke = event_kind_obj.and_then(|k| k.get("kind")).and_then(|v| v.as_str())
                == Some("Invoke");
            if !is_invoke {
                continue;
            }
            let target_str = ev.event.get("target").and_then(|v| v.as_str()).unwrap_or("");
            let target_id = match parse_object_id(target_str) {
                Some(id) => id,
                None => continue,
            };
            let method =
                event_kind_obj.and_then(|k| k.get("method")).and_then(|v| v.as_str()).unwrap_or("");
            let args = event_kind_obj
                .and_then(|k| k.get("args"))
                .cloned()
                .unwrap_or(serde_json::Value::Null);

            let outcome = match method {
                "expand" => {
                    let fid_str = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    match parse_object_id(fid_str) {
                        Some(fid) => {
                            let outcome = invoke_handler::handle_file_tree_expand(
                                target_id,
                                &tracked_expanded,
                                fid,
                            );
                            if !tracked_expanded.contains(&fid) {
                                tracked_expanded.push(fid);
                            }
                            outcome
                        }
                        None => invoke_handler::InvokeOutcome::empty(),
                    }
                }
                "collapse" => {
                    let fid_str = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    match parse_object_id(fid_str) {
                        Some(fid) => {
                            let outcome = invoke_handler::handle_file_tree_collapse(
                                target_id,
                                &tracked_expanded,
                                fid,
                            );
                            tracked_expanded.retain(|x| *x != fid);
                            outcome
                        }
                        None => invoke_handler::InvokeOutcome::empty(),
                    }
                }
                "select" => {
                    let nid_str = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    match parse_object_id(nid_str) {
                        Some(nid) => invoke_handler::handle_file_tree_select(target_id, nid),
                        None => invoke_handler::InvokeOutcome::empty(),
                    }
                }
                "set_file" => {
                    let fid_str = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    match parse_object_id(fid_str) {
                        Some(fid) => invoke_handler::handle_canvas_set_file(target_id, fid),
                        None => invoke_handler::InvokeOutcome::empty(),
                    }
                }
                _ => invoke_handler::InvokeOutcome::empty(),
            };

            for (oid, key, val) in outcome.state_sets {
                req_seq += 1;
                let ss = StateSetMsg {
                    request_id: format!("r-{}", req_seq),
                    target: oid.to_string(),
                    key,
                    value: val,
                };
                stream.write_all(&encode_frame(&serde_json::to_vec(&ss)?)).await?;
            }
        }
    }
    println!("[desktop-shell] exit");
    Ok(())
}
