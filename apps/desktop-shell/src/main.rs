//! desktop-shell 진입점 — server-host 연결 + 워크스페이스 스캔 + Desktop 트리 mount.
//!
//! 흐름:
//! 1. 워크스페이스 루트 확보 (없으면 생성).
//! 2. server-host(127.0.0.1:5550)에 TCP 연결, Hello 전송.
//! 3. HelloAck에서 ActorId 받아옴.
//! 4. Desktop / FileTree / Canvas + 워크스페이스 스캔 결과(Folder/File)를 한꺼번에 mount.
//! 5. idle 루프 — 추후 T6/T7에서 invoke 디스패치로 교체.

use std::str::FromStr;

use geulos_core::{std_types, ActorId, Object};
use geulos_desktop_shell::{scan, workspace};
use geulos_proto::{decode_frame, encode_frame, Hello, HelloAck, MountAck, MountMsg, Role};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SERVER_ADDR: &str = "127.0.0.1:5550";

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

    // 워크스페이스 스캔 — 루트 직계는 parent=None으로 돌아오므로 FileTree id로 채움.
    let scan_result = scan::scan_tree(&owner, &root)?;
    let file_tree_id = file_tree.id;
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

    // idle 유지 — T6/T7에서 invoke 핸들러로 교체.
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
            if decode_frame(&mut slice).is_err() {
                break;
            }
            let consumed = accum.len() - slice.len();
            accum.drain(..consumed);
        }
    }
    println!("[desktop-shell] exit");
    Ok(())
}
