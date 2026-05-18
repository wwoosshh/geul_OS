//! desktop-shell 진입점 — server-host 연결 + 드라이브 자동 mount + Desktop 트리 mount.
//!
//! 흐름 (M8 ADR-026/027/028):
//! 1. 시스템의 모든 드라이브 열거 (drives::list_drives).
//! 2. server-host(127.0.0.1:5550)에 TCP 연결, Hello 전송.
//! 3. HelloAck에서 ActorId 받아옴.
//! 4. Desktop / FileTree / Explorer / Cli + 각 드라이브 Folder(children=[])를 mount.
//! 5. FileTree·Explorer·Cli·Desktop·드라이브 Folder들에 Invoke 구독.
//! 6. expand / navigate_to invoke 도착 시 lazy_mount::expand_folder로 직계 자식만
//!    동적으로 mount + subscribe.
//!
//! M8 read-only: create_file / write / delete invoke 핸들러는 제거됨. fs_ops 모듈은
//! M9 권한 다이얼로그 마일스톤에서 재활성 예정이라 dead code로 보존.

use std::path::PathBuf;
use std::str::FromStr;

use geulos_core::{
    std_types, AclEffect, AclEntry, ActorId, ActorPattern, MethodPattern, Object, ObjectId,
};
use geulos_desktop_shell::cli_handler::{self, SpecialAction};
use geulos_desktop_shell::{drives, explorer_ops, invoke_handler, lazy_mount};
use geulos_proto::{
    decode_frame, encode_frame, EventKindFilterWire, EventMsg, Hello, HelloAck, MountAck, MountMsg,
    Role, StateSetMsg, SubscribeAck, SubscribeMsg,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SERVER_ADDR: &str = "127.0.0.1:5550";

/// TODO(T8.12): wildcard ACL은 M8 동안 유지 — read-only로 자연 보호.
/// 매니페스트 기반 권한으로 교체 예정.
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

/// 주어진 ID의 Folder 객체에서 `path` prop을 꺼낸다. 없으면 None.
///
/// lazy_expand_if_needed에서 폴더 디스크 경로를 알아낼 때 사용. 다른 헬퍼들(write/create_file
/// 분기에서 쓰던 lookup_file_path 등)은 M8에서 dead라 제거됨.
fn lookup_folder_path(objects: &[Object], id: ObjectId) -> Option<PathBuf> {
    let obj = objects.iter().find(|o| o.id == id)?;
    if obj.type_uri.as_str() != "aios.std/Folder@1" {
        return None;
    }
    obj.props.get("path").and_then(|v| v.as_str()).map(PathBuf::from)
}

/// CLI lines 히스토리 최대 보관 라인 수 (오래된 라인은 잘림).
const CLI_LINES_CAP: usize = 1000;

/// CLI 입력 dispatch 결과를 Cli.state.lines에 반영하고 StateSet 출력 생성.
///
/// `input_echo`가 비어있지 않으면 첫 라인으로 `> {input_echo}`를 추가해 사용자 입력
/// 자체도 출력 히스토리에 남김 (전형적 셸 동작). special이 Clear면 기존 라인 다 비우고
/// echo·output_lines도 무시 — clear 명령은 깨끗한 상태가 목적. 사용자 입력 `clear`의
/// input echo도 의도적으로 drop — POSIX `clear`와 일관.
///
/// mounted_objects의 Cli 객체에서 현재 lines를 읽고 capped된 새 배열을 만들어
/// state_sets로 반환. mounted_objects도 동기화 갱신.
fn handle_cli_outcome(
    mounted_objects: &mut [Object],
    cli_target: ObjectId,
    input_echo: &str,
    output_lines: Vec<String>,
    special: Option<SpecialAction>,
) -> invoke_handler::InvokeOutcome {
    // Clear는 lines를 빈 배열로 set — 입력 echo·output_lines 무시.
    if let Some(SpecialAction::Clear) = special {
        if let Some(cli) = mounted_objects.iter_mut().find(|o| o.id == cli_target) {
            cli.state.insert("lines".into(), json!([] as [&str; 0]));
        }
        return invoke_handler::InvokeOutcome {
            state_sets: vec![(cli_target, "lines".into(), json!([] as [&str; 0]))],
        };
    }

    // 일반 동작 — 현재 lines 읽어 input_echo + output_lines append, cap 적용.
    let mut current: Vec<String> = mounted_objects
        .iter()
        .find(|o| o.id == cli_target)
        .and_then(|o| o.state.get("lines"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if !input_echo.is_empty() {
        current.push(format!("> {}", input_echo));
    }
    for line in output_lines {
        current.push(line);
    }
    // cap — 가장 오래된 라인부터 잘라냄.
    if current.len() > CLI_LINES_CAP {
        let drop = current.len() - CLI_LINES_CAP;
        current.drain(..drop);
    }
    let new_value = json!(current);
    if let Some(cli) = mounted_objects.iter_mut().find(|o| o.id == cli_target) {
        cli.state.insert("lines".into(), new_value.clone());
    }
    invoke_handler::InvokeOutcome { state_sets: vec![(cli_target, "lines".into(), new_value)] }
}

/// 폴더 lazy expand — children이 비어있으면 lazy_mount + mount/subscribe 처리.
///
/// 부모 Folder.children도 갱신. 새 자식 id들의 mount/subscribe wire 메시지를 전송.
/// 호출 후 부모는 children 갱신, 새 자식 객체들이 `mounted_objects`에 추가됨.
///
/// Borrow 노트: stream/mounted_objects/req_seq 모두 mutable로 받지만 매개변수가 서로
/// 독립이라 borrow checker는 만족. mounted_objects를 push할 때 부모 갱신은 push 이후
/// 별도 `iter_mut().find` 로 분리되어 있어 동시 mutable borrow가 발생하지 않는다.
async fn lazy_expand_if_needed(
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    folder_id: ObjectId,
    req_seq: &mut u64,
) -> Result<(), Box<dyn std::error::Error>> {
    if !explorer_ops::needs_expand(mounted_objects, folder_id) {
        return Ok(());
    }
    let folder_path = match lookup_folder_path(mounted_objects, folder_id) {
        Some(p) => p,
        None => return Ok(()),
    };
    let now = chrono::Utc::now().timestamp_millis();
    let children = match lazy_mount::expand_folder(owner, &folder_path, now) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[desktop-shell] expand_folder 실패 {}: {}", folder_id, e);
            return Ok(());
        }
    };
    let mut child_ids = Vec::new();
    for mut child in children {
        child.parent = Some(folder_id);
        add_wildcard_acl(&mut child);
        let child_id = child.id;
        let child_is_folder = child.type_uri.as_str() == "aios.std/Folder@1";
        child_ids.push(child_id);
        let mm =
            MountMsg { root_object_id: child_id.to_string(), tree: serde_json::to_value(&child)? };
        stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
        // Folder만 invoke subscribe (File은 클릭 시점에 별도 처리 — T8.7).
        if child_is_folder {
            *req_seq += 1;
            let sub = SubscribeMsg {
                subscription_id: format!("sub-runtime-{}", req_seq),
                target: child_id.to_string(),
                kinds: vec![EventKindFilterWire::Invoke],
                include_initial: false,
            };
            stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
        }
        mounted_objects.push(child);
    }
    if let Some(parent) = mounted_objects.iter_mut().find(|o| o.id == folder_id) {
        parent.children = child_ids;
        // child_count state도 갱신.
        let len = parent.children.len();
        parent.state.insert("child_count".to_string(), serde_json::json!(len));
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let drive_paths = drives::list_drives();
    println!("[desktop-shell] {} 드라이브 mount", drive_paths.len());

    let addr = std::env::args().nth(1).unwrap_or_else(|| SERVER_ADDR.to_string());
    println!("[desktop-shell] connecting to {}...", addr);
    let mut stream = TcpStream::connect(&addr).await?;

    // Hello — manifest는 인라인. 데스크톱 셸이 표시할 빌트인 UI 타입 목록을 노출.
    // M8: Window/Explorer 추가. Canvas는 legacy로 STD_TYPES에 남아있어 호환 차원에서 유지.
    let manifest = json!({
        "manifest": {
            "id": "desktop-shell",
            "permissions": [],
            "ui_types": [
                "aios.builtin/Desktop@1",
                "aios.builtin/FileTree@1",
                "aios.builtin/Canvas@1",
                "aios.builtin/Cli@1",
                "aios.builtin/Window@1",
                "aios.builtin/Explorer@1",
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

    let now_ms = chrono::Utc::now().timestamp_millis();

    // Desktop = [FileTree, Explorer, Cli, Window*] — Window는 런타임에 추가 (T8.7).
    // FileTree는 multi-root 드라이브를 가지므로 root_path 의미가 약함 — 마커로 "/" 유지.
    let mut desktop = std_types::desktop(owner.clone());
    let mut file_tree = std_types::file_tree(owner.clone(), "/");
    let mut explorer = std_types::explorer(owner.clone());
    let mut cli = std_types::cli(owner.clone());
    file_tree.parent = Some(desktop.id);
    explorer.parent = Some(desktop.id);
    cli.parent = Some(desktop.id);

    // 드라이브 Folder mount — 각각 children=[]로 지연 mount (lazy expand).
    let mut drive_folders: Vec<Object> = drive_paths
        .iter()
        .map(|p| {
            let mut f = std_types::folder(
                owner.clone(),
                p.to_string_lossy().as_ref(),
                p.to_string_lossy().as_ref(),
                now_ms,
            );
            f.parent = Some(file_tree.id);
            f
        })
        .collect();
    file_tree.children = drive_folders.iter().map(|f| f.id).collect();
    desktop.children = vec![file_tree.id, explorer.id, cli.id];

    // TODO(T8.12): wildcard ACL은 M8 동안 유지. read-only로 자연 보호.
    for o in [&mut desktop, &mut file_tree, &mut explorer, &mut cli] {
        add_wildcard_acl(o);
    }
    for f in &mut drive_folders {
        add_wildcard_acl(f);
    }

    let file_tree_id = file_tree.id;
    let explorer_id = explorer.id;
    let cli_id = cli.id;
    let desktop_id = desktop.id;

    let mut all_objects: Vec<Object> =
        vec![desktop.clone(), file_tree.clone(), explorer.clone(), cli.clone()];
    all_objects.extend(drive_folders);

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

    // Invoke 구독 — FileTree·Explorer·Cli·Desktop + 모든 Folder (드라이브 포함).
    // Desktop도 subscribe — Window mount/close 시 자식 변경 추적용 (T8.7).
    // File은 *초기 subscribe X* — Explorer.open_file 시점에 별도 처리 (T8.7).
    let mut subscribe_targets: Vec<ObjectId> = vec![file_tree_id, explorer_id, cli_id, desktop_id];
    for obj in &all_objects {
        if obj.type_uri.as_str() == "aios.std/Folder@1" {
            subscribe_targets.push(obj.id);
        }
    }

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
    println!(
        "[desktop-shell] subscribed to {} targets (Desktop, FileTree, Explorer, Cli, Folders)",
        subscribe_targets.len()
    );

    // mount 후에도 객체 정보가 필요 — invoke 처리 시 path/parent 조회용.
    let mut mounted_objects: Vec<Object> = all_objects.clone();

    // 이벤트 루프 — Invoke를 받아 dispatch하고 결과를 StateSet/Mount로 broadcast.
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
                            lazy_expand_if_needed(
                                &mut stream,
                                &mut mounted_objects,
                                &owner,
                                fid,
                                &mut req_seq,
                            )
                            .await?;
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
                "navigate_to" => {
                    let fid_str = args.get("folder_id").and_then(|v| v.as_str()).unwrap_or("");
                    match parse_object_id(fid_str) {
                        Some(fid) => {
                            lazy_expand_if_needed(
                                &mut stream,
                                &mut mounted_objects,
                                &owner,
                                fid,
                                &mut req_seq,
                            )
                            .await?;
                            explorer_ops::handle_navigate_to(target_id, fid)
                        }
                        None => invoke_handler::InvokeOutcome::empty(),
                    }
                }
                // ─────────────────────── T7.5: 하단 CLI 패널 ───────────────────────
                "submit_input" => {
                    // 컴포지터에서 받은 사용자 입력 텍스트. dispatch_command로 파싱하고
                    // 결과 라인을 Cli.state.lines에 append.
                    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
                    let outcome = cli_handler::dispatch_command(text);
                    handle_cli_outcome(
                        &mut mounted_objects,
                        target_id,
                        text,
                        outcome.output_lines,
                        outcome.special,
                    )
                }
                "clear" => {
                    // 외부에서 직접 clear 호출 — lines 비움.
                    handle_cli_outcome(
                        &mut mounted_objects,
                        target_id,
                        "",
                        vec![],
                        Some(SpecialAction::Clear),
                    )
                }
                "append_line" => {
                    // 외부(AI bridge 등)에서 한 라인 추가.
                    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
                    handle_cli_outcome(
                        &mut mounted_objects,
                        target_id,
                        "",
                        vec![text.to_string()],
                        None,
                    )
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
                let bytes = match serde_json::to_vec(&ss) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("[desktop-shell] StateSet 직렬화 실패: {}", e);
                        continue;
                    }
                };
                if let Err(e) = stream.write_all(&encode_frame(&bytes)).await {
                    eprintln!("[desktop-shell] StateSet 송신 실패: {}", e);
                    break; // 송신 실패는 stream 끊김 → break 합리.
                }
            }
        }
    }
    println!("[desktop-shell] exit");
    Ok(())
}
