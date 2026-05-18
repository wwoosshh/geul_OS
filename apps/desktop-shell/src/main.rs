//! desktop-shell 진입점 — server-host 연결 + 워크스페이스 스캔 + Desktop 트리 mount.
//!
//! 흐름:
//! 1. 워크스페이스 루트 확보 (없으면 생성).
//! 2. server-host(127.0.0.1:5550)에 TCP 연결, Hello 전송.
//! 3. HelloAck에서 ActorId 받아옴.
//! 4. Desktop / FileTree / Canvas + 가상 root Folder + 워크스페이스 스캔 결과(Folder/File)를 한꺼번에 mount.
//! 5. FileTree·Canvas·root Folder·하위 Folder/File에 Invoke 구독 → 컴포지터/AI 클릭이 도착하면
//!    invoke_handler::* 또는 fs_ops::* 로 처리하고 StateSetMsg / MountMsg로 broadcast.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use geulos_core::{
    std_types, AclEffect, AclEntry, ActorId, ActorPattern, MethodPattern, Object, ObjectId,
};
use geulos_desktop_shell::cli_handler::{self, SpecialAction};
use geulos_desktop_shell::{fs_ops, invoke_handler, scan, workspace};
use geulos_proto::{
    decode_frame, encode_frame, EventKindFilterWire, EventMsg, Hello, HelloAck, MountAck, MountMsg,
    Role, StateSetMsg, SubscribeAck, SubscribeMsg,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SERVER_ADDR: &str = "127.0.0.1:5550";

/// 단방향 디스크 기록 시점에 preview에 담을 최대 바이트 수.
/// scan.rs와 같은 512 — 시각적 일관성 유지.
const PREVIEW_BYTES: usize = 512;

/// 텍스트 확장자 → MIME 매핑. scan.rs의 TEXT_EXTS를 단순화/축약해 inline.
/// AI가 만드는 파일은 거의 .md/.txt 라 큰 표는 불필요.
const TEXT_EXTS: &[(&str, &str)] = &[
    ("txt", "text/plain"),
    ("md", "text/markdown"),
    ("toml", "text/plain"),
    ("json", "text/json"),
    ("rs", "text/rust"),
    ("py", "text/python"),
    ("js", "text/javascript"),
    ("html", "text/html"),
    ("css", "text/css"),
    ("yaml", "text/yaml"),
    ("yml", "text/yaml"),
];

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

/// 파일명에서 MIME 추정 (확장자 lookup, 기본 octet-stream).
fn mime_for(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    TEXT_EXTS.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m).unwrap_or("application/octet-stream")
}

/// 주어진 ID의 Folder 객체에서 `path` prop을 꺼낸다. 없으면 None.
fn lookup_folder_path(objects: &[Object], id: ObjectId) -> Option<PathBuf> {
    let obj = objects.iter().find(|o| o.id == id)?;
    if obj.type_uri.as_str() != "aios.std/Folder@1" {
        return None;
    }
    obj.props.get("path").and_then(|v| v.as_str()).map(PathBuf::from)
}

/// 주어진 ID의 File 객체에서 `path` prop을 꺼낸다. 없으면 None.
fn lookup_file_path(objects: &[Object], id: ObjectId) -> Option<PathBuf> {
    let obj = objects.iter().find(|o| o.id == id)?;
    if obj.type_uri.as_str() != "aios.std/File@1" {
        return None;
    }
    obj.props.get("path").and_then(|v| v.as_str()).map(PathBuf::from)
}

/// UTF-8 경계 안전한 prefix (scan.rs::utf8_safe_slice 와 동일한 알고리즘).
/// preview 생성용. AI가 보낸 content가 멀티바이트 경계에 잘릴 수 있어 필수.
fn utf8_safe_prefix(bytes: &[u8], max: usize) -> &[u8] {
    let mut end = max.min(bytes.len());
    if end == bytes.len() {
        return &bytes[..end];
    }
    while end > 0 && (bytes[end - 1] & 0b1100_0000) == 0b1000_0000 {
        end -= 1;
    }
    if end > 0 && bytes[end - 1] >= 0b1100_0000 {
        end -= 1;
    }
    &bytes[..end]
}

/// 새 File 객체 생성 — fs_ops::create_empty_file 직후 호출.
/// path는 절대 경로, parent_id는 mount 시 부모로 사용.
fn build_new_file_object(
    owner: &ActorId,
    parent_id: ObjectId,
    path: &Path,
    actor: &str,
    now_ms: i64,
) -> Object {
    let name = path.file_name().and_then(|s| s.to_str()).map(String::from).unwrap_or_else(|| {
        // 이론상 발생 불가 (safe_join이 normal 컴포넌트만 통과시킴).
        path.display().to_string()
    });
    let mime = mime_for(&name);
    let mut file_obj =
        std_types::file(owner.clone(), path.to_string_lossy().as_ref(), &name, mime, now_ms);
    add_wildcard_acl(&mut file_obj);
    file_obj.parent = Some(parent_id);
    // 갓 만든 빈 파일이므로 size=0, preview="" (std_types::file 기본값과 동일).
    // 그러나 last_change_actor는 system이 아니라 호출자 actor로 갱신해야
    // 컴포지터 노란 점이 뜬다.
    file_obj.set_state("last_change_actor", json!(actor));
    file_obj.set_state("last_change_ms", json!(now_ms));
    file_obj
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
                "aios.builtin/Cli@1",
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

    // Desktop = [FileTree, Canvas, Cli] 세 자식. 컴포지터가 3분할로 그림 (T7.5).
    // ADR-023 — Cli는 데스크톱 셸의 3번째 자식 (4번째 builtin 타입 — Desktop 자체 포함),
    // 항상 보임.
    let mut desktop = std_types::desktop(owner.clone());
    let mut file_tree = std_types::file_tree(owner.clone(), &root.to_string_lossy());
    let mut canvas = std_types::canvas(owner.clone());
    let mut cli = std_types::cli(owner.clone());
    file_tree.parent = Some(desktop.id);
    canvas.parent = Some(desktop.id);
    cli.parent = Some(desktop.id);
    desktop.children = vec![file_tree.id, canvas.id, cli.id];

    // TODO(T8): 컴포지터(외부 actor)가 expand/select/set_file/submit_input invoke할 수
    // 있어야 함. CLI 객체 ACL도 함께 매니페스트 기반 권한으로 교체 예정.
    add_wildcard_acl(&mut desktop);
    add_wildcard_acl(&mut file_tree);
    add_wildcard_acl(&mut canvas);
    add_wildcard_acl(&mut cli);

    let file_tree_id = file_tree.id;
    let canvas_id = canvas.id;
    let cli_id = cli.id;

    // T7: 가상 root Folder — 워크스페이스 자체를 Folder로 노출해 AI가 루트에도
    // create_file 할 수 있게 한다. (FileTree에 직접 메서드를 추가하지 않는 이유:
    // T1 스펙 고정. 디자인 갭은 가상 Folder로 메운다.)
    // 표시명은 절대 경로 — 사용자가 트리 root에서 워크스페이스 위치를 명확히 인지하도록.
    // 자식 Folder/File은 scan.rs가 basename으로 채우므로 트리는 root만 절대, 자식은 상대.
    let root_display = root.to_string_lossy().to_string();
    let mut root_folder = std_types::folder(owner.clone(), &root_display, &root_display, now_ms);
    add_wildcard_acl(&mut root_folder);
    root_folder.parent = Some(file_tree_id);
    let root_folder_id = root_folder.id;
    file_tree.children = vec![root_folder_id];

    // 워크스페이스 스캔 — 루트 직계는 parent=None으로 돌아오므로 root_folder id로 채움.
    let scan_result = scan::scan_tree(&owner, &root)?;
    let mut all_objects: Vec<Object> =
        vec![desktop.clone(), file_tree.clone(), canvas.clone(), cli.clone(), root_folder.clone()];
    let mut top_level_ids = Vec::new();
    for mut obj in scan_result.objects {
        if obj.parent.is_none() {
            obj.parent = Some(root_folder_id);
            top_level_ids.push(obj.id);
        }
        all_objects.push(obj);
    }
    if let Some(rf) = all_objects.iter_mut().find(|o| o.id == root_folder_id) {
        rf.state.insert("child_count".into(), json!(top_level_ids.len()));
        rf.children = top_level_ids;
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

    // Invoke 구독 — FileTree·Canvas·Cli + 모든 Folder + 모든 File.
    // root_folder도 Folder 타입이므로 자동 포함됨. AI가 어디든 create_file/write 가능.
    // CLI는 submit_input / clear / append_line invoke를 받음 (ADR-023).
    let mut subscribe_targets: Vec<ObjectId> = vec![file_tree_id, canvas_id, cli_id];
    for obj in &all_objects {
        let uri = obj.type_uri.as_str();
        if uri == "aios.std/Folder@1" || uri == "aios.std/File@1" {
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
        "[desktop-shell] subscribed to {} targets (FileTree, Canvas, Cli, Folders, Files)",
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
            // core::Event.actor — "ai-bridge", "compositor" 등 ActorId 문자열.
            // 노란 점 트리거는 컴포지터에서 "ai" 접두 매칭이므로, actor 자체를
            // 그대로 last_change_actor에 담으면 된다.
            let actor =
                ev.event.get("actor").and_then(|v| v.as_str()).unwrap_or("system").to_string();

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
                // ─────────────────────── T7: 단방향 디스크 기록 ───────────────────────
                "create_file" => {
                    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    if name.is_empty() {
                        eprintln!("[desktop-shell] create_file: 빈 name 무시");
                        invoke_handler::InvokeOutcome::empty()
                    } else if let Some(folder_path) =
                        lookup_folder_path(&mounted_objects, target_id)
                    {
                        match fs_ops::safe_join(&folder_path, name) {
                            Ok(safe_path) => match fs_ops::create_empty_file(&safe_path) {
                                Ok(()) => {
                                    let now = chrono::Utc::now().timestamp_millis();
                                    let mut new_file = build_new_file_object(
                                        &owner, target_id, &safe_path, &actor, now,
                                    );
                                    // 모든 File도 invoke 받을 수 있게 method 시그니처는
                                    // std_types::file이 이미 채워줌. (write/delete 포함)
                                    // wildcard ACL 추가는 build_new_file_object 안에서.
                                    let new_id = new_file.id;
                                    // mount.
                                    let mm = MountMsg {
                                        root_object_id: new_id.to_string(),
                                        tree: serde_json::to_value(&new_file)?,
                                    };
                                    if let Err(e) = stream
                                        .write_all(&encode_frame(&serde_json::to_vec(&mm)?))
                                        .await
                                    {
                                        eprintln!(
                                            "[desktop-shell] new file mount 송신 실패: {}",
                                            e
                                        );
                                    }
                                    // 새 파일에도 Invoke 구독 추가.
                                    req_seq += 1;
                                    let sub = SubscribeMsg {
                                        subscription_id: format!("sub-runtime-{}", req_seq),
                                        target: new_id.to_string(),
                                        kinds: vec![EventKindFilterWire::Invoke],
                                        include_initial: false,
                                    };
                                    if let Err(e) = stream
                                        .write_all(&encode_frame(&serde_json::to_vec(&sub)?))
                                        .await
                                    {
                                        eprintln!(
                                            "[desktop-shell] new file subscribe 송신 실패: {}",
                                            e
                                        );
                                    }
                                    // mounted_objects + 부모 children 갱신.
                                    let parent_children_len = {
                                        if let Some(parent) =
                                            mounted_objects.iter_mut().find(|o| o.id == target_id)
                                        {
                                            parent.children.push(new_id);
                                            parent.children.len()
                                        } else {
                                            0
                                        }
                                    };
                                    new_file.children = vec![];
                                    mounted_objects.push(new_file);

                                    // 부모 Folder state 갱신.
                                    let mut outs = vec![
                                        (
                                            target_id,
                                            "child_count".to_string(),
                                            json!(parent_children_len),
                                        ),
                                        (target_id, "last_change_ms".to_string(), json!(now)),
                                        (target_id, "last_change_actor".to_string(), json!(actor)),
                                    ];
                                    // 새 파일에도 last_change_*를 명시적으로 broadcast —
                                    // mount의 state는 일부 컴포지터 구현에서 변경 이벤트로
                                    // 인식되지 않을 수 있으므로 안전하게 한 번 더.
                                    outs.push((new_id, "last_change_ms".to_string(), json!(now)));
                                    outs.push((
                                        new_id,
                                        "last_change_actor".to_string(),
                                        json!(actor),
                                    ));
                                    invoke_handler::InvokeOutcome { state_sets: outs }
                                }
                                Err(e) => {
                                    eprintln!(
                                        "[desktop-shell] create_empty_file 실패 {}: {}",
                                        safe_path.display(),
                                        e
                                    );
                                    invoke_handler::InvokeOutcome::empty()
                                }
                            },
                            Err(e) => {
                                eprintln!("[desktop-shell] create_file safe_join 거부: {}", e);
                                invoke_handler::InvokeOutcome::empty()
                            }
                        }
                    } else {
                        eprintln!(
                            "[desktop-shell] create_file: target {} 이 Folder 아님 또는 미존재",
                            target_id
                        );
                        invoke_handler::InvokeOutcome::empty()
                    }
                }
                "write" => {
                    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    if let Some(file_path) = lookup_file_path(&mounted_objects, target_id) {
                        match fs_ops::atomic_write(&file_path, content.as_bytes()) {
                            Ok(()) => {
                                let now = chrono::Utc::now().timestamp_millis();
                                let bytes = content.as_bytes();
                                let preview_slice = utf8_safe_prefix(bytes, PREVIEW_BYTES);
                                let preview =
                                    std::str::from_utf8(preview_slice).unwrap_or("").to_string();
                                let size = bytes.len() as u64;
                                // 로컬 캐시도 갱신 — 이후 lookup의 정확성.
                                if let Some(f) =
                                    mounted_objects.iter_mut().find(|o| o.id == target_id)
                                {
                                    f.state.insert("size_bytes".into(), json!(size));
                                    f.state.insert("preview".into(), json!(preview));
                                    f.state.insert("last_change_ms".into(), json!(now));
                                    f.state.insert("last_change_actor".into(), json!(actor));
                                }
                                invoke_handler::InvokeOutcome {
                                    state_sets: vec![
                                        (target_id, "size_bytes".to_string(), json!(size)),
                                        (target_id, "preview".to_string(), json!(preview)),
                                        (target_id, "last_change_ms".to_string(), json!(now)),
                                        (target_id, "last_change_actor".to_string(), json!(actor)),
                                    ],
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "[desktop-shell] atomic_write 실패 {}: {}",
                                    file_path.display(),
                                    e
                                );
                                invoke_handler::InvokeOutcome::empty()
                            }
                        }
                    } else {
                        eprintln!(
                            "[desktop-shell] write: target {} 이 File 아님 또는 미존재",
                            target_id
                        );
                        invoke_handler::InvokeOutcome::empty()
                    }
                }
                "delete" => {
                    if let Some(file_path) = lookup_file_path(&mounted_objects, target_id) {
                        match fs_ops::delete_file(&file_path) {
                            Ok(()) => {
                                let now = chrono::Utc::now().timestamp_millis();
                                if let Some(f) =
                                    mounted_objects.iter_mut().find(|o| o.id == target_id)
                                {
                                    f.state.insert("deleted".into(), json!(true));
                                    f.state.insert("last_change_ms".into(), json!(now));
                                    f.state.insert("last_change_actor".into(), json!(actor));
                                }
                                invoke_handler::InvokeOutcome {
                                    state_sets: vec![
                                        (target_id, "deleted".to_string(), json!(true)),
                                        (target_id, "last_change_ms".to_string(), json!(now)),
                                        (target_id, "last_change_actor".to_string(), json!(actor)),
                                    ],
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "[desktop-shell] delete_file 실패 {}: {}",
                                    file_path.display(),
                                    e
                                );
                                invoke_handler::InvokeOutcome::empty()
                            }
                        }
                    } else {
                        eprintln!(
                            "[desktop-shell] delete: target {} 이 File 아님 또는 미존재",
                            target_id
                        );
                        invoke_handler::InvokeOutcome::empty()
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
