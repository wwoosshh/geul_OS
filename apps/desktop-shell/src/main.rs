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
//! invoke 분기는 `handlers/` 모듈로 분리되어 있고 본 파일은 *얇은 dispatch*만 유지.
//! 단, **submit_input**은 `ai_session`/`chat_session`/`awaiting_api_key` 상태가
//! main loop의 mutable local과 강하게 결합되어 main에 잔존 (해당 분기만 ~400 lines).

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use geulos_core::{std_types, ActorId, Object, ObjectId};
use geulos_desktop_shell::ai_session::{self, CliChatSession};
use geulos_desktop_shell::cli_handler::{self, SpecialAction};
use geulos_desktop_shell::fs_watcher::{FsChange, FsWatcher};
use geulos_desktop_shell::handlers::{
    add_container_acl, add_filesystem_acl, add_fs_object_acl, add_ui_object_acl, cli_methods,
    dialog_methods, explorer_methods, external_methods, find_object_by_path, fs_methods,
    handle_cli_outcome, parse_object_id, window_methods,
};
use geulos_desktop_shell::{dialog_ops, drives, granted_dirs, invoke_handler, lazy_mount};
use geulos_proto::{
    decode_frame, encode_frame, EventKindFilterWire, EventMsg, Hello, HelloAck, MountAck, MountMsg,
    Role, StateSetMsg, SubscribeAck, SubscribeMsg,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SERVER_ADDR: &str = "127.0.0.1:5550";

/// AI 응답이 spawned task에서 main loop로 전달되는 메시지. M11.1 신규.
struct AiResult {
    /// 응답을 append할 Cli 객체 id.
    cli_target: ObjectId,
    /// AI 응답 본문 또는 에러 메시지.
    result: Result<String, String>,
    /// echo/sentinel 라인 추적 — 응답 도착 시점에 제거할 sentinel string.
    sentinel: String,
    /// 응답 lines 앞에 붙일 prompt prefix (예: "[ai:foo] > ").
    prompt_prefix: String,
}

#[allow(dead_code)] // T5에서 spawned task에서 사용 — 현재 T4 단독에서는 미사용.
const AI_WAITING_SENTINEL: &str = "(응답 대기 중...)";

/// `/ai start` (is_start=true) 또는 `/ai load` (is_start=false) 분기 공통 helper.
///
/// 이미 resolve된 `key`(env/저장 파일/방금 사용자가 입력)를 받아 wire 연결 +
/// CliChatSession 생성·로드. 성공 시 chat_session에 새 세션을 대입하고 사용자에게
/// 보여줄 안내 한 줄을 반환. 실패 시 에러를 propagate — caller가
/// `[AI start/load 실패: ...]` 메시지로 변환한다.
///
/// **T7.9 (ADR-032):** key resolution은 *호출 전*에 caller가 담당. None이면 caller가
/// awaiting_api_key mode로 분기 — 본 helper에는 도달하지 않는다.
async fn start_or_load_session(
    server_addr: &str,
    key: String,
    name: &str,
    is_start: bool,
    chat_session: &std::sync::Arc<tokio::sync::Mutex<Option<CliChatSession>>>,
) -> Result<String, String> {
    let wire = ai_session::connect_wire(server_addr).await.map_err(|e| e.to_string())?;
    let system = ai_session::DEFAULT_CLI_SYSTEM_PROMPT.to_string();
    if is_start {
        let session = CliChatSession::start(key, wire, system, name.to_string());
        *chat_session.lock().await = Some(session);
        Ok(format!("(새 AI 세션 시작: {})", name))
    } else {
        let session = CliChatSession::load(key, wire, system, name).map_err(|e| e.to_string())?;
        *chat_session.lock().await = Some(session);
        Ok(format!("(AI 세션 로드: {})", name))
    }
}

/// Cli 객체의 현재 mode/session_name 기반으로 입력 echo prompt prefix를 만든다 (T7.8).
///
/// - shell 모드 → `"> "`
/// - AI 모드 + session_name 있음 → `"[ai:<name>] > "`
/// - AI 모드 + session_name 없음 (이론상 비정상) → `"[ai] > "`
fn prompt_prefix_for(mounted_objects: &[Object], cli_target: ObjectId) -> String {
    let cli = match mounted_objects.iter().find(|o| o.id == cli_target) {
        Some(o) => o,
        None => return "> ".to_string(),
    };
    let mode = cli.state.get("mode").and_then(|v| v.as_str()).unwrap_or("shell");
    if mode == "ai" {
        match cli.state.get("session_name").and_then(|v| v.as_str()) {
            Some(name) => format!("[ai:{}] > ", name),
            None => "[ai] > ".to_string(),
        }
    } else {
        "> ".to_string()
    }
}

/// **T7.9 (ADR-032)** awaiting_api_key mode 진입 helper.
///
/// `Cli.state.mode = "awaiting_api_key"` + `pending_action = "<encoded>"`로 SetState,
/// 그리고 안내 라인 출력. caller는 *이 함수의 반환 (lines, extra_sets)을 handle_cli_outcome
/// 에 전달*한다.
///
/// pending 인코딩:
/// - `"start"` — `/ai start` (이름 생략).
/// - `"start NAME"` — `/ai start NAME`.
/// - `"load NAME"` — `/ai load NAME`.
fn enter_awaiting_mode(
    mounted_objects: &mut [Object],
    cli_target: ObjectId,
    pending: String,
) -> Vec<(ObjectId, String, serde_json::Value)> {
    if let Some(cli) = mounted_objects.iter_mut().find(|o| o.id == cli_target) {
        cli.state.insert("mode".into(), json!("awaiting_api_key"));
        cli.state.insert("pending_action".into(), json!(pending));
    }
    vec![
        (cli_target, "mode".to_string(), json!("awaiting_api_key")),
        (cli_target, "pending_action".to_string(), json!(pending)),
    ]
}

/// **T7.9 (ADR-032)** awaiting → shell 복귀 helper. mode/session_name/pending_action 모두 리셋.
fn exit_awaiting_mode(
    mounted_objects: &mut [Object],
    cli_target: ObjectId,
) -> Vec<(ObjectId, String, serde_json::Value)> {
    if let Some(cli) = mounted_objects.iter_mut().find(|o| o.id == cli_target) {
        cli.state.insert("mode".into(), json!("shell"));
        cli.state.insert("session_name".into(), json!(null));
        cli.state.insert("pending_action".into(), json!(null));
    }
    vec![
        (cli_target, "mode".to_string(), json!("shell")),
        (cli_target, "session_name".to_string(), json!(null)),
        (cli_target, "pending_action".to_string(), json!(null)),
    ]
}

/// 외부 fs 변경 이벤트 처리 — Created/Modified/Removed를 적절한 mount/SetState/destroy
/// 와이어 흐름으로 변환 (M10 Phase 2 / ADR-036).
///
/// **Created**: 부모 폴더가 mounted면 새 File/Folder 객체 mount + subscribe + parent.children
/// 갱신. 부모 폴더가 아직 expand되지 않았으면 *아무 것도 안 함* — 사용자가 expand 시점에
/// lazy_mount::expand_folder가 자연스럽게 새 자식까지 본다.
///
/// **Modified**: 그 path의 mounted File 객체를 찾아 `last_change_ms` / `last_change_actor=
/// "external"` SetState broadcast. Window content reload는 v2 — 사용자가 편집 중일 때
/// 덮어쓰기 위험이 있어 신중한 정책 필요 (editor_state.window_id 비활성 조건 추가).
///
/// **Removed**: 그 path의 mounted 객체를 destroyed=true SetState로 표시 + mounted_objects
/// 에서 제거 + 부모.children에서 제거. SetState destroyed는 KI-011 tombstone 패턴 — 컴포지터
/// layout이 자동으로 skip한다.
async fn handle_fs_change(
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    req_seq: &mut u64,
    change: FsChange,
) -> Result<(), Box<dyn std::error::Error>> {
    match change {
        FsChange::Created(path) => {
            // 이미 mount되어 있으면 (우리가 막 만든 후 watcher가 늦게 알린 케이스 또는 echo
            // 캐시 만료 후 도착) skip — 중복 mount 방지.
            if find_object_by_path(mounted_objects, &path).is_some() {
                return Ok(());
            }
            let parent_path = match path.parent() {
                Some(p) => p,
                None => return Ok(()),
            };
            let (parent_id, _) = match find_object_by_path(mounted_objects, parent_path) {
                Some(v) => v,
                None => {
                    // 부모가 아직 expand되지 않은 폴더 — skip. 나중에 사용자가 expand하면
                    // lazy_mount가 새 자식까지 자연스럽게 포함.
                    return Ok(());
                }
            };
            let meta = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => return Ok(()), // 이벤트 도착 시점에 이미 삭제 — silent skip.
            };
            let now = chrono::Utc::now().timestamp_millis();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(s) => s.to_string(),
                None => return Ok(()),
            };
            let mut new_obj = if meta.is_dir() {
                std_types::folder(owner.clone(), path.to_string_lossy().as_ref(), &name, now)
            } else if meta.is_file() {
                let mime = lazy_mount::guess_mime(&name);
                let mut f = std_types::file(
                    owner.clone(),
                    path.to_string_lossy().as_ref(),
                    &name,
                    mime,
                    now,
                );
                f.set_state("size_bytes", serde_json::json!(meta.len()));
                f
            } else {
                return Ok(());
            };
            new_obj.parent = Some(parent_id);
            new_obj.set_state("last_change_actor", serde_json::json!("external"));
            new_obj.set_state("last_change_ms", serde_json::json!(now));
            // M11 T9: fs_watcher가 생성하는 객체는 Folder/File — fs_object ACL 적용.
            add_fs_object_acl(&mut new_obj);
            let new_id = new_obj.id;
            let mm = MountMsg {
                root_object_id: new_id.to_string(),
                tree: serde_json::to_value(&new_obj)?,
            };
            stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
            *req_seq += 1;
            let sub = SubscribeMsg {
                subscription_id: format!("sub-runtime-{}", req_seq),
                target: new_id.to_string(),
                kinds: vec![EventKindFilterWire::Invoke],
                include_initial: false,
            };
            stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
            if let Some(p) = mounted_objects.iter_mut().find(|o| o.id == parent_id) {
                p.children.push(new_id);
                let len = p.children.len();
                p.state.insert("child_count".to_string(), serde_json::json!(len));
                // 부모 child_count SetState broadcast — UI가 즉시 새 자식 인식.
                *req_seq += 1;
                let ss = StateSetMsg {
                    request_id: format!("r-{}", req_seq),
                    target: parent_id.to_string(),
                    key: "child_count".to_string(),
                    value: serde_json::json!(len),
                };
                stream.write_all(&encode_frame(&serde_json::to_vec(&ss)?)).await?;
            }
            mounted_objects.push(new_obj);
            // **부모 Folder를 통째 re-mount** — Object.children은 wire SetState가 아닌 *Object
            // mount frame*으로만 전파됨. compositor의 tree_model이 부모.children에 새 child를
            // 자동 push하지만 그 retrofit이 race/타이밍으로 누락될 수 있어 (사용자 보고 — test1
            // 폴더 안 외부 변경이 Explorer에 안 보임), 명시적으로 부모 객체 전체 frame을 한 번
            // 더 보낸다. core::mount의 dedup 덕에 서버 측 children 중복 안 생기고, compositor
            // upsert가 *기존 부모 객체 덮어쓰기*로 새 children 리스트 확실히 적용.
            if let Some(p) = mounted_objects.iter().find(|o| o.id == parent_id) {
                let mm_parent = MountMsg {
                    root_object_id: parent_id.to_string(),
                    tree: serde_json::to_value(p)?,
                };
                stream.write_all(&encode_frame(&serde_json::to_vec(&mm_parent)?)).await?;
            }
            eprintln!("[desktop-shell] fs_watcher Created → mount {}", path.display());
        }
        FsChange::Modified(path) => {
            // mounted File만 갱신 — Folder Modified는 무의미 (자식 list는 Create/Remove로 감지).
            // Windows ReadDirectoryChangesW가 *Create를 Modified로 보낼 때*가 있어 (notify-rs 알려진
            // 동작), Modified path가 *mount 안 된 새 파일*이면 Created로 fallback해 mount 시도.
            let now = chrono::Utc::now().timestamp_millis();
            let target_lookup = mounted_objects
                .iter()
                .find(|o| {
                    o.props
                        .get("path")
                        .and_then(|v| v.as_str())
                        .map(|p| Path::new(p) == path)
                        .unwrap_or(false)
                })
                .map(|o| (o.id, o.type_uri.as_str().to_string()));
            let (target_id, ty) = match target_lookup {
                Some(v) => v,
                None => {
                    eprintln!(
                        "[desktop-shell] fs_watcher Modified → 기존 mount X, Created로 fallback {}",
                        path.display()
                    );
                    // 직접 Created 분기 재호출 — 코드 중복 피하기 위해 재귀.
                    return Box::pin(handle_fs_change(
                        stream,
                        mounted_objects,
                        owner,
                        req_seq,
                        FsChange::Created(path),
                    ))
                    .await;
                }
            };
            if ty != "aios.std/File@1" {
                return Ok(());
            }
            if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
                o.state.insert("last_change_ms".into(), serde_json::json!(now));
                o.state.insert("last_change_actor".into(), serde_json::json!("external"));
            }
            *req_seq += 1;
            let ss1 = StateSetMsg {
                request_id: format!("r-{}", req_seq),
                target: target_id.to_string(),
                key: "last_change_ms".to_string(),
                value: serde_json::json!(now),
            };
            stream.write_all(&encode_frame(&serde_json::to_vec(&ss1)?)).await?;
            *req_seq += 1;
            let ss2 = StateSetMsg {
                request_id: format!("r-{}", req_seq),
                target: target_id.to_string(),
                key: "last_change_actor".to_string(),
                value: serde_json::json!("external"),
            };
            stream.write_all(&encode_frame(&serde_json::to_vec(&ss2)?)).await?;
            // content + size도 자동 reload — 외부 수정 시 AI/Window가 *fresh content*를
            // 즉시 본다. 1MB 이하만 (큰 파일은 viewer/editor 흐름에서 별도 처리). dirty=true
            // Window의 file이면 사용자 편집 덮어쓰기 위험이지만 v1 단순화로 항상 reload —
            // 사용자가 외부 수정과 동시에 GeulOS 편집은 충돌이라 알려진 한계.
            if let Ok(content) = std::fs::read_to_string(&path) {
                if content.len() <= 1024 * 1024 {
                    let size = content.len() as i64;
                    if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
                        o.state.insert("content".into(), serde_json::json!(&content));
                        o.state.insert("size".into(), serde_json::json!(size));
                    }
                    *req_seq += 1;
                    let ssc = StateSetMsg {
                        request_id: format!("r-{}", req_seq),
                        target: target_id.to_string(),
                        key: "content".to_string(),
                        value: serde_json::json!(content),
                    };
                    stream.write_all(&encode_frame(&serde_json::to_vec(&ssc)?)).await?;
                    *req_seq += 1;
                    let sss = StateSetMsg {
                        request_id: format!("r-{}", req_seq),
                        target: target_id.to_string(),
                        key: "size".to_string(),
                        value: serde_json::json!(size),
                    };
                    stream.write_all(&encode_frame(&serde_json::to_vec(&sss)?)).await?;
                }
            }
            eprintln!("[desktop-shell] fs_watcher Modified → SetState {}", path.display());
        }
        FsChange::Removed(path) => {
            let (target_id, parent_id_opt) = match find_object_by_path(mounted_objects, &path) {
                Some(v) => v,
                None => return Ok(()),
            };
            mounted_objects.retain(|o| o.id != target_id);
            if let Some(parent_id) = parent_id_opt {
                if let Some(p) = mounted_objects.iter_mut().find(|o| o.id == parent_id) {
                    p.children.retain(|c| *c != target_id);
                    let len = p.children.len();
                    p.state.insert("child_count".to_string(), serde_json::json!(len));
                    *req_seq += 1;
                    let ss = StateSetMsg {
                        request_id: format!("r-{}", req_seq),
                        target: parent_id.to_string(),
                        key: "child_count".to_string(),
                        value: serde_json::json!(len),
                    };
                    stream.write_all(&encode_frame(&serde_json::to_vec(&ss)?)).await?;
                }
                // 부모 Folder 재mount — children에서 옛 child id가 빠진 새 리스트를 compositor에 broadcast.
                if let Some(p) = mounted_objects.iter().find(|o| o.id == parent_id) {
                    let mm_parent = MountMsg {
                        root_object_id: parent_id.to_string(),
                        tree: serde_json::to_value(p)?,
                    };
                    stream.write_all(&encode_frame(&serde_json::to_vec(&mm_parent)?)).await?;
                }
            }
            *req_seq += 1;
            let ss = StateSetMsg {
                request_id: format!("r-{}", req_seq),
                target: target_id.to_string(),
                key: "destroyed".to_string(),
                value: serde_json::json!(true),
            };
            stream.write_all(&encode_frame(&serde_json::to_vec(&ss)?)).await?;
            eprintln!("[desktop-shell] fs_watcher Removed → destroy {}", path.display());
        }
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
                // M10 Phase 3 (ADR-036): cwd 밖 escape hatch singleton.
                "aios.builtin/Filesystem@1",
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

    // M10 Phase 3 (ADR-036): cwd 결정 — process 시작 시 한 번. 이후 read_external/write_external
    // 분기에서 cwd 안/밖 판정에 사용 (cwd 안은 거부, 밖만 통과). 실패 시 "." (현재 dir) fallback.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    println!("[desktop-shell] cwd = {}", cwd.display());

    // Desktop = [FileTree, Explorer, Cli, Filesystem, Window*] — Window는 런타임에 추가 (T8.7).
    // FileTree는 multi-root 드라이브를 가지므로 root_path 의미가 약함 — 마커로 "/" 유지.
    let mut desktop = std_types::desktop(owner.clone());
    let mut file_tree = std_types::file_tree(owner.clone(), "/");
    let mut explorer = std_types::explorer(owner.clone());
    let mut cli = std_types::cli(owner.clone());
    // M10 Phase 3: Filesystem@1 escape hatch singleton.
    let mut filesystem_obj = std_types::filesystem(owner.clone(), &cwd.to_string_lossy());
    file_tree.parent = Some(desktop.id);
    explorer.parent = Some(desktop.id);
    cli.parent = Some(desktop.id);
    filesystem_obj.parent = Some(desktop.id);

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
    desktop.children = vec![file_tree.id, explorer.id, cli.id, filesystem_obj.id];

    // M11 T9: 객체 타입별 typed ACL helper 적용. add_wildcard_acl(KI-001/016) 제거.
    add_container_acl(&mut desktop);
    add_ui_object_acl(&mut file_tree);
    add_ui_object_acl(&mut explorer);
    add_ui_object_acl(&mut cli);
    add_filesystem_acl(&mut filesystem_obj);
    // M11 T9: drive Folder도 fs_object — compositor 무조건 + AI는 grant 시만.
    for f in &mut drive_folders {
        add_fs_object_acl(f);
    }

    let file_tree_id = file_tree.id;
    let explorer_id = explorer.id;
    let cli_id = cli.id;
    let desktop_id = desktop.id;
    let filesystem_id = filesystem_obj.id;

    let mut all_objects: Vec<Object> = vec![
        desktop.clone(),
        file_tree.clone(),
        explorer.clone(),
        cli.clone(),
        filesystem_obj.clone(),
    ];
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

    // Invoke 구독 — FileTree·Explorer·Cli·Desktop·Filesystem + 모든 Folder (드라이브 포함).
    // Desktop도 subscribe — Window mount/close 시 자식 변경 추적용 (T8.7).
    // Filesystem@1 (M10 Phase 3) — read_external/write_external invoke 수신.
    // File은 *초기 subscribe X* — Explorer.open_file 시점에 별도 처리 (T8.7).
    let mut subscribe_targets: Vec<ObjectId> =
        vec![file_tree_id, explorer_id, cli_id, desktop_id, filesystem_id];
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

    // ─────────────────── T7.8 (ADR-031): CLI AI chat mode + 영속 세션 ───────────────────
    // T7.7과 달리 *시작 시 세션 생성 안 함* — `/ai start [name]` / `/ai load <name>` 시점에
    // lazy 생성. process 생애 동안 chat_session: Option<...>은 활성 세션을 보유.
    //
    // wire는 server-host에 *별도 TCP connection* (Role::Ai로) — desktop-shell의 connection과
    // 분리. last_change_actor가 AI actor_id로 기록돼 T5 노란 점 시각화가 자연스럽게 동작.
    // (`/ai start`/`load` 시점에 매번 새 wire를 연결 — 한 desktop-shell 안에서 여러 세션을
    // 순차로 열어도 각자 깨끗한 wire를 가진다.)
    //
    // API key는 같은 시점(start/load)에 환경 변수에서 읽어 graceful 실패 메시지. `/ai list`는
    // 디렉터리 read만 필요 — key 없이도 정상 작동.
    let chat_session: std::sync::Arc<tokio::sync::Mutex<Option<CliChatSession>>> =
        std::sync::Arc::new(tokio::sync::Mutex::new(None));
    println!(
        "[desktop-shell] CLI 시작 (shell 모드). /ai start | /ai load | /ai list | /exit 으로 AI 모드 진입/탈출."
    );

    // M11.1 T4: AI 응답 channel — spawned task → main loop.
    let (ai_response_tx, mut ai_response_rx) = tokio::sync::mpsc::channel::<AiResult>(16);
    // T5에서 spawn task에서 사용. 본 T4 단독 commit에서는 unused warning 회피.
    let _ai_response_tx_retain_for_t5 = ai_response_tx.clone();

    // 이벤트 루프 — Invoke를 받아 dispatch하고 결과를 StateSet/Mount로 broadcast.
    let mut tracked_expanded: Vec<ObjectId> = Vec::new();
    let mut req_seq: u64 = 0;
    // M9 T8: AI write 등 ConfirmRequired invoke가 Dialog 응답을 기다리는 동안 보관.
    // v1은 *동기 처리* — respond 분기가 도착 시 PendingMap.take + 동기로 file_write::save
    // 호출 후 Dialog destroy. PendingEntry.tx (oneshot) 채널은 미래 비동기 흐름 인프라로
    // 보존만 — 실제 사용 X (`_ = p.tx`로 drop). main loop의 stream/mounted_objects 동시
    // borrow race를 회피하기 위함.
    let pending = dialog_ops::PendingMap::new();
    // M10 T7: AI에게 부여된 디렉터리 grant (path-aware ACL 캐시 — ADR-036). 한 dir에 대해
    // [허용] Dialog를 한 번 처리하면 그 dir 안 후속 write/create/rename은 confirm 없이 통과.
    // process 종료 시 자연 reset. judge_with_path가 이를 참조.
    let granted = granted_dirs::GrantedDirs::new();

    // ─────────── M10 Phase 2 (ADR-036): 외부 fs 변경 감지 watcher ───────────
    // lazy expand된 폴더만 *비-재귀* 등록. notify-rs RecommendedWatcher가 OS 백엔드
    // (Windows ReadDirectoryChangesW / Linux inotify / macOS FSEvents) 추상화.
    // 초기화 실패 시 None — 외부 변경 감지는 비활성이지만 나머지 기능은 정상 동작 (fail-open).
    // *우리 자신의* fs op (folder_ops/file_ops/file_write::save) 직후 mark_self_op로
    // path를 echo_cache에 등록 → 같은 path 이벤트는 1초 동안 무시되어 무한 루프 차단.
    let mut fs_watcher = match FsWatcher::new() {
        Ok(w) => Some(w),
        Err(e) => {
            eprintln!("[desktop-shell] fs_watcher 초기화 실패 — 외부 변경 감지 비활성: {}", e);
            None
        }
    };
    // 100ms 주기로 watcher.drain() 호출 — 사용자 인식 임계 (200ms+) 아래라 UX 영향 미미.
    let mut watcher_tick = tokio::time::interval(Duration::from_millis(100));
    // Tokio interval은 시작 직후 *즉시* 한 번 발화 — 그 첫 tick은 무시되어도 무방 (Skip 정책).
    watcher_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        let n = tokio::select! {
            // 우선순위: stream read를 우선해 backpressure 회피. select!는 기본 random이라
            // biased를 명시.
            biased;
            read_res = stream.read(&mut buf) => match read_res {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("[desktop-shell] read error: {}", e);
                    break;
                }
            },
            // M11.1: spawned AI task가 보낸 응답을 받아 lines에 append.
            Some(ai_result) = ai_response_rx.recv() => {
                handle_ai_response(ai_result, &mut stream, &mut mounted_objects, &mut req_seq).await;
                continue;
            }
            _ = watcher_tick.tick() => {
                if let Some(w) = fs_watcher.as_ref() {
                    let changes = w.drain();
                    for change in changes {
                        if let Err(e) = handle_fs_change(
                            &mut stream,
                            &mut mounted_objects,
                            &owner,
                            &mut req_seq,
                            change,
                        )
                        .await
                        {
                            eprintln!("[desktop-shell] fs_change 처리 실패: {}", e);
                        }
                    }
                }
                continue;
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
            // M9 T8: invoker actor — Event.actor 필드 (server-host invoke.rs가 actor.clone()을
            // emit 시 그대로 넣음). permission::judge에 전달해 사용자 vs AI를 구분.
            // 파싱 실패 시 ActorId::local_user() — fail-closed 아닌 fail-open이지만, 이는
            // *서버가 보낸 우리 자신의 trusted event*이므로 안전. ai-bridge 등 외부 actor가
            // 임의로 위장할 수 없음 (server-host가 connection의 actor를 강제 매핑).
            let sender_actor = ev
                .event
                .get("actor")
                .and_then(|v| v.as_str())
                .and_then(|s| ActorId::from_str(s).ok())
                .unwrap_or_else(ActorId::local_user);

            // ─────────────────── invoke dispatch — handlers/<카테고리> 모듈로 위임 ───────────────────
            // submit_input만 main에 잔존 (ai_session/chat_session/awaiting_api_key 상태가
            // main loop의 local과 강하게 결합 — 분리 시 LOC 절감 효과 < borrow checker 부담).
            let outcome = match method {
                // ───── explorer_methods ─────
                "expand" => {
                    explorer_methods::handle_expand(
                        target_id,
                        &args,
                        &mut stream,
                        &mut mounted_objects,
                        &owner,
                        &mut tracked_expanded,
                        fs_watcher.as_mut(),
                        &mut req_seq,
                    )
                    .await?
                }
                "collapse" => {
                    explorer_methods::handle_collapse(target_id, &args, &mut tracked_expanded)
                }
                "navigate_to" => {
                    explorer_methods::handle_navigate_to(
                        target_id,
                        &args,
                        &mut stream,
                        &mut mounted_objects,
                        &owner,
                        fs_watcher.as_mut(),
                        &mut req_seq,
                    )
                    .await?
                }
                "navigate_up" => {
                    explorer_methods::handle_navigate_up(target_id, &mut mounted_objects)
                }
                "open_file" => {
                    explorer_methods::handle_open_file(
                        target_id,
                        &args,
                        &mut stream,
                        &mut mounted_objects,
                        &owner,
                        desktop_id,
                        &mut req_seq,
                    )
                    .await?
                }
                // ───── window_methods ─────
                "move" => window_methods::handle_move(target_id, &args, &mut mounted_objects),
                "resize" => window_methods::handle_resize(target_id, &args, &mut mounted_objects),
                "focus" => window_methods::handle_focus(target_id, &mut mounted_objects),
                "close" => {
                    window_methods::handle_close(target_id, desktop_id, &mut mounted_objects)
                }
                "close_confirm" => window_methods::handle_close_confirm(
                    target_id,
                    desktop_id,
                    &mut mounted_objects,
                ),
                // ───── cli_methods ─────
                // submit_input — main에 잔존 (T7.8/T7.9: ai_session/chat_session/awaiting 결합).
                "submit_input" => {
                    handle_submit_input(
                        target_id,
                        &args,
                        &mut stream,
                        &mut mounted_objects,
                        &chat_session,
                        &addr,
                        &mut req_seq,
                    )
                    .await?
                }
                "clear" => cli_methods::handle_clear(target_id, &mut mounted_objects),
                "append_line" => {
                    cli_methods::handle_append_line(target_id, &args, &mut mounted_objects)
                }
                // ───── fs_methods ─────
                "save_to_file" => {
                    fs_methods::handle_save_to_file(target_id, &args, &mut mounted_objects)
                }
                "save" => {
                    fs_methods::handle_save(
                        target_id,
                        &args,
                        &mut stream,
                        &mut mounted_objects,
                        &owner,
                        desktop_id,
                        &sender_actor,
                        &granted,
                        &pending,
                        fs_watcher.as_ref(),
                        &mut req_seq,
                    )
                    .await?
                }
                "create_file" => {
                    fs_methods::handle_create_file(
                        target_id,
                        &args,
                        &mut stream,
                        &mut mounted_objects,
                        &owner,
                        desktop_id,
                        &sender_actor,
                        &granted,
                        &pending,
                        fs_watcher.as_ref(),
                        &mut req_seq,
                    )
                    .await?
                }
                "create_folder" => {
                    fs_methods::handle_create_folder(
                        target_id,
                        &args,
                        &mut stream,
                        &mut mounted_objects,
                        &owner,
                        desktop_id,
                        &sender_actor,
                        &granted,
                        &pending,
                        fs_watcher.as_ref(),
                        &mut req_seq,
                    )
                    .await?
                }
                "delete" => {
                    fs_methods::handle_delete(
                        target_id,
                        &args,
                        &mut stream,
                        &mut mounted_objects,
                        &owner,
                        desktop_id,
                        &sender_actor,
                        &pending,
                        &mut req_seq,
                    )
                    .await?
                }
                "rename" => {
                    fs_methods::handle_rename(
                        target_id,
                        &args,
                        &mut stream,
                        &mut mounted_objects,
                        &owner,
                        desktop_id,
                        &sender_actor,
                        &granted,
                        &pending,
                        fs_watcher.as_ref(),
                        &mut req_seq,
                    )
                    .await?
                }
                "read" => fs_methods::handle_read(target_id, &mut mounted_objects),
                "list" => {
                    fs_methods::handle_list(
                        target_id,
                        &mut stream,
                        &mut mounted_objects,
                        &owner,
                        fs_watcher.as_mut(),
                        &mut req_seq,
                    )
                    .await?
                }
                // ───── dialog_methods ─────
                "respond" => {
                    dialog_methods::handle_respond(
                        target_id,
                        &args,
                        &mut stream,
                        &mut mounted_objects,
                        &owner,
                        desktop_id,
                        &pending,
                        &granted,
                        fs_watcher.as_ref(),
                        &mut req_seq,
                    )
                    .await?
                }
                // ───── external_methods ─────
                "read_external" => external_methods::handle_read_external(
                    target_id,
                    &args,
                    &mut mounted_objects,
                    filesystem_id,
                    &cwd,
                ),
                "write_external" => {
                    external_methods::handle_write_external(
                        target_id,
                        &args,
                        &mut stream,
                        &mut mounted_objects,
                        &owner,
                        desktop_id,
                        filesystem_id,
                        &cwd,
                        &sender_actor,
                        &pending,
                        &mut req_seq,
                    )
                    .await?
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

/// Cli.submit_input — T7.8/T7.9 awaiting_api_key + AI start/load/list/send/exit 분기.
///
/// `ai_session`/`chat_session`/`addr` 등 main loop의 local 상태와 강하게 결합되어 있어
/// handlers/cli_methods로 분리하지 않고 main 내 helper로 잔존. 분기는 두 단계:
/// 1. mode == "awaiting_api_key" → 입력을 key로 처리 (`/exit`은 취소, valid key는 pending
///    action 재실행).
/// 2. 그 외 → `cli_handler::dispatch_command` 또는 `dispatch_chat` (mode에 따라). 결과의
///    SpecialAction을 보고 AiStart/Load/List/Exit/Send/Clear별 분기.
///
/// **T7.10:** awaiting 모드 입력은 *원본 echo X* (API key 노출 차단).
///
/// 반환은 `InvokeOutcome` 표준 — main의 broadcast 루프가 그대로 처리. 단 awaiting 분기는
/// 즉시 StateSet 전송 + `continue` 패턴이 있어 *함수가 직접 wire 송신 + 빈 outcome 반환*하는
/// 경우도 있다 (분기 단순화). 분리 전 main 분기와 *완전히 동일* 동작.
#[allow(clippy::too_many_arguments)]
async fn handle_submit_input(
    target_id: ObjectId,
    args: &serde_json::Value,
    stream: &mut TcpStream,
    mounted_objects: &mut [Object],
    chat_session: &std::sync::Arc<tokio::sync::Mutex<Option<CliChatSession>>>,
    addr: &str,
    req_seq: &mut u64,
) -> Result<invoke_handler::InvokeOutcome, Box<dyn std::error::Error>> {
    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let current_mode = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|o| o.state.get("mode").and_then(|v| v.as_str()))
        .unwrap_or("shell")
        .to_string();
    let prompt_prefix = prompt_prefix_for(mounted_objects, target_id);

    // T7.9 (ADR-032): awaiting_api_key 모드는 *dispatch 외부에서* 처리.
    // 입력을 key로 취급해 검증·저장·resume. dispatch_command/dispatch_chat에는
    // 진입하지 않는다.
    if current_mode == "awaiting_api_key" {
        let trimmed = text.trim();
        // /exit 또는 빈 입력 → cancel.
        if trimmed.is_empty() || trimmed == "/exit" {
            let extra = exit_awaiting_mode(mounted_objects, target_id);
            // T7.10: input_echo는 *항상 빈 문자열* — awaiting 모드의 입력은
            // 잠재적 API key. /exit 같은 무해한 토큰도 lines에 안 남겨야 *AI tool로
            // get_object(cli) 호출 시 키 유출 위험을 구조적으로 0으로* 만든다
            // (분기 분석을 의존하지 않고 *항상 skip*하는 게 안전).
            let mut combined = handle_cli_outcome(
                mounted_objects,
                target_id,
                &prompt_prefix,
                "",
                vec!["(API key 입력 취소 — 셸 모드로 복귀)".to_string()],
                None,
            );
            combined.state_sets.extend(extra);
            send_state_sets(stream, req_seq, combined.state_sets).await;
            return Ok(invoke_handler::InvokeOutcome::empty());
        }
        // 입력을 key로 처리. echo는 *masking은 v2* — 일단 plain ("(입력)" 자리표시도 고려했지만
        // 사용자가 자기 화면에 잘못 입력했는지 보고싶을 수 있어 그대로 노출).
        // 입력 echo는 prompt prefix + 텍스트.
        let key = trimmed.to_string();
        match geulos_ai_bridge::api_key::validate(&key).await {
            Ok(_) => {
                let mut lines = vec![];
                match geulos_ai_bridge::api_key::save_to_file(&key) {
                    Ok(_) => lines.push("[저장됨 ~/.geulos/api_key]".to_string()),
                    Err(e) => {
                        // 저장 실패해도 메모리에는 보관해 진행. 사용자에게 안내만.
                        lines.push(format!(
                            "[저장 실패(메모리만 보관, 다음 실행에는 재입력 필요): {}]",
                            e
                        ));
                    }
                }
                // pending action을 읽어 재실행.
                let pending_str = mounted_objects
                    .iter()
                    .find(|o| o.id == target_id)
                    .and_then(|o| o.state.get("pending_action").and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string();
                // mode/pending_action SetState — shell로 일단 복귀.
                let mut extra = exit_awaiting_mode(mounted_objects, target_id);
                // pending 실행.
                let (is_start, name): (bool, Option<String>) = {
                    let mut parts = pending_str.splitn(2, ' ');
                    let sub = parts.next().unwrap_or("");
                    let arg = parts.next().map(|s| s.trim().to_string());
                    match sub {
                        "start" => (true, arg),
                        "load" => (false, arg),
                        _ => (true, None),
                    }
                };
                let session_name = if is_start {
                    name.unwrap_or_else(ai_session::auto_name)
                } else {
                    name.unwrap_or_default()
                };
                if !is_start && session_name.is_empty() {
                    lines.push("[pending 액션이 비어있어 셸 모드로 복귀]".to_string());
                } else {
                    match start_or_load_session(
                        addr,
                        key.clone(),
                        &session_name,
                        is_start,
                        chat_session,
                    )
                    .await
                    {
                        Ok(msg) => {
                            lines.push(msg);
                            if chat_session.lock().await.is_some() {
                                if let Some(cli) =
                                    mounted_objects.iter_mut().find(|o| o.id == target_id)
                                {
                                    cli.state.insert("mode".into(), json!("ai"));
                                    cli.state.insert("session_name".into(), json!(&session_name));
                                }
                                // 기 push된 mode=shell을 ai로 덮어쓰기 — 같은 key의
                                // 마지막 set이 승. session_name도 갱신.
                                extra.push((target_id, "mode".to_string(), json!("ai")));
                                extra.push((
                                    target_id,
                                    "session_name".to_string(),
                                    json!(session_name),
                                ));
                            }
                        }
                        Err(e) => {
                            let label = if is_start { "start" } else { "load" };
                            lines.push(format!("[AI {} 실패: {}]", label, e));
                        }
                    }
                }
                // T7.10: 사용자가 방금 입력한 *원본 텍스트*는 API key 본문이므로
                // 절대 lines에 echo하지 않는다. AI tool로 get_object(cli) 호출 시
                // 키가 노출되는 경로를 차단 (구조적 fix).
                let mut combined =
                    handle_cli_outcome(mounted_objects, target_id, &prompt_prefix, "", lines, None);
                combined.state_sets.extend(extra);
                send_state_sets(stream, req_seq, combined.state_sets).await;
                return Ok(invoke_handler::InvokeOutcome::empty());
            }
            Err(e) => {
                // 검증 실패 — mode 유지, 안내 출력.
                // T7.10: 입력 echo 빈 문자열 (key 본문 lines 노출 방지).
                let combined = handle_cli_outcome(
                    mounted_objects,
                    target_id,
                    &prompt_prefix,
                    "",
                    vec![format!("[검증 실패: {}] 다시 입력하거나 /exit으로 취소.", e)],
                    None,
                );
                send_state_sets(stream, req_seq, combined.state_sets).await;
                return Ok(invoke_handler::InvokeOutcome::empty());
            }
        }
    }

    let dispatch_outcome = if current_mode == "ai" {
        cli_handler::dispatch_chat(&text)
    } else {
        cli_handler::dispatch_command(&text)
    };

    // mode/session_name SetState를 별도로 누적해 outcome에 합친다.
    let mut extra_sets: Vec<(ObjectId, String, serde_json::Value)> = Vec::new();
    let outcome = match dispatch_outcome.special {
        Some(SpecialAction::AiStart(name_opt)) => {
            // 이전 세션은 매 send 후 dump되므로 그대로 drop으로 OK.
            let name = name_opt.unwrap_or_else(ai_session::auto_name);
            // T7.9 (ADR-032): key chain — 없으면 awaiting_api_key 모드 진입.
            match ai_session::resolve_api_key() {
                Some(key) => {
                    let lines =
                        match start_or_load_session(addr, key, &name, true, chat_session).await {
                            Ok(msg) => vec![msg],
                            Err(e) => vec![format!("[AI start 실패: {}]", e)],
                        };
                    // 성공 시 mode=ai + session_name=name SetState.
                    if chat_session.lock().await.is_some() {
                        if let Some(cli) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
                            cli.state.insert("mode".into(), json!("ai"));
                            cli.state.insert("session_name".into(), json!(&name));
                        }
                        extra_sets.push((target_id, "mode".to_string(), json!("ai")));
                        extra_sets.push((target_id, "session_name".to_string(), json!(name)));
                    }
                    handle_cli_outcome(
                        mounted_objects,
                        target_id,
                        &prompt_prefix,
                        &text,
                        lines,
                        None,
                    )
                }
                None => {
                    let pending = format!("start {}", name);
                    let extra = enter_awaiting_mode(mounted_objects, target_id, pending);
                    extra_sets.extend(extra);
                    handle_cli_outcome(
                        mounted_objects,
                        target_id,
                        &prompt_prefix,
                        &text,
                        vec!["[ANTHROPIC_API_KEY 미설정] CLI에 키를 입력 후 Enter (취소: /exit)"
                            .to_string()],
                        None,
                    )
                }
            }
        }
        Some(SpecialAction::AiLoad(name)) => {
            // T7.9 (ADR-032): key chain — 없으면 awaiting_api_key 모드 진입.
            match ai_session::resolve_api_key() {
                Some(key) => {
                    let lines =
                        match start_or_load_session(addr, key, &name, false, chat_session).await {
                            Ok(msg) => vec![msg],
                            Err(e) => vec![format!("[AI load 실패: {}]", e)],
                        };
                    if chat_session.lock().await.is_some() {
                        if let Some(cli) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
                            cli.state.insert("mode".into(), json!("ai"));
                            cli.state.insert("session_name".into(), json!(&name));
                        }
                        extra_sets.push((target_id, "mode".to_string(), json!("ai")));
                        extra_sets.push((target_id, "session_name".to_string(), json!(name)));
                    }
                    handle_cli_outcome(
                        mounted_objects,
                        target_id,
                        &prompt_prefix,
                        &text,
                        lines,
                        None,
                    )
                }
                None => {
                    let pending = format!("load {}", name);
                    let extra = enter_awaiting_mode(mounted_objects, target_id, pending);
                    extra_sets.extend(extra);
                    handle_cli_outcome(
                        mounted_objects,
                        target_id,
                        &prompt_prefix,
                        &text,
                        vec!["[ANTHROPIC_API_KEY 미설정] CLI에 키를 입력 후 Enter (취소: /exit)"
                            .to_string()],
                        None,
                    )
                }
            }
        }
        Some(SpecialAction::AiList) => {
            let lines = match CliChatSession::list_sessions() {
                Ok(items) if items.is_empty() => {
                    vec!["(저장된 AI 세션 없음)".to_string()]
                }
                Ok(items) => {
                    let mut out = vec![format!("저장된 세션 ({}):", items.len())];
                    for (name, count) in items {
                        out.push(format!("  {}  ({} 메시지)", name, count));
                    }
                    out
                }
                Err(e) => vec![format!("[AI list 실패: {}]", e)],
            };
            handle_cli_outcome(mounted_objects, target_id, &prompt_prefix, &text, lines, None)
        }
        Some(SpecialAction::AiExit) => {
            // 세션은 매 send 후 dump됨 — drop으로 OK.
            *chat_session.lock().await = None;
            // T7.9 (ADR-032): pending_action도 항상 null로 리셋 — awaiting에서
            // /ai start로 들어왔다가 다시 /exit하는 등 잔재 방지.
            if let Some(cli) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
                cli.state.insert("mode".into(), json!("shell"));
                cli.state.insert("session_name".into(), json!(null));
                cli.state.insert("pending_action".into(), json!(null));
            }
            extra_sets.push((target_id, "mode".to_string(), json!("shell")));
            extra_sets.push((target_id, "session_name".to_string(), json!(null)));
            extra_sets.push((target_id, "pending_action".to_string(), json!(null)));
            handle_cli_outcome(
                mounted_objects,
                target_id,
                &prompt_prefix,
                &text,
                vec!["(셸 모드로 복귀)".to_string()],
                None,
            )
        }
        Some(SpecialAction::AiSend(prompt)) => {
            let lines = {
                let mut guard = chat_session.lock().await;
                if let Some(session) = guard.as_mut() {
                    eprintln!("[desktop-shell] AI prompt: {}", prompt);
                    match session.send(&prompt).await {
                        Ok(reply) if reply.is_empty() => vec!["[AI: (빈 응답)]".to_string()],
                        Ok(reply) => reply.lines().map(String::from).collect(),
                        Err(e) => vec![format!("[AI 오류: {}]", e)],
                    }
                } else {
                    // 이론상 mode=ai인데 chat_session=None은 발생 안 함.
                    // 방어적으로 안내.
                    vec!["[AI 세션 없음 — /ai start로 시작]".to_string()]
                }
            }; // guard drop
            handle_cli_outcome(mounted_objects, target_id, &prompt_prefix, &text, lines, None)
        }
        Some(SpecialAction::Clear) | None => handle_cli_outcome(
            mounted_objects,
            target_id,
            &prompt_prefix,
            &text,
            dispatch_outcome.output_lines,
            dispatch_outcome.special,
        ),
    };
    // mode/session_name SetState를 outcome 뒤에 합쳐 한 번에 broadcast.
    let mut combined = outcome;
    combined.state_sets.extend(extra_sets);
    Ok(combined)
}

/// spawned AI task가 응답을 보내오면 호출. sentinel 라인 제거 + AI 응답 (또는 에러)
/// lines에 append + SetState broadcast.
async fn handle_ai_response(
    ai_result: AiResult,
    stream: &mut TcpStream,
    mounted_objects: &mut [Object],
    req_seq: &mut u64,
) {
    let AiResult { cli_target, result, sentinel, prompt_prefix } = ai_result;

    // 1) sentinel 제거 — lines 중 sentinel 포함 항목 모두 제거.
    let mut current: Vec<String> = mounted_objects
        .iter()
        .find(|o| o.id == cli_target)
        .and_then(|o| o.state.get("lines"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    current.retain(|line| !line.contains(&sentinel));

    // 2) AI 응답 또는 에러를 lines에 append (한 줄당 한 라인 단위로 split).
    let body = match result {
        Ok(text) => text,
        Err(e) => format!("[AI 에러: {}]", e),
    };
    for line in body.lines() {
        current.push(format!("{}{}", prompt_prefix, line));
    }

    // 3) cap 적용 (handle_cli_outcome의 CLI_LINES_CAP과 일관).
    if current.len() > geulos_desktop_shell::handlers::CLI_LINES_CAP {
        let drop = current.len() - geulos_desktop_shell::handlers::CLI_LINES_CAP;
        current.drain(..drop);
    }

    let new_value = json!(current);
    if let Some(cli) = mounted_objects.iter_mut().find(|o| o.id == cli_target) {
        cli.state.insert("lines".into(), new_value.clone());
    }

    // 4) SetState broadcast (기존 send_state_sets 헬퍼 활용).
    send_state_sets(stream, req_seq, vec![(cli_target, "lines".to_string(), new_value)]).await;
}

/// State set 묶음을 wire에 직접 송신 (submit_input의 awaiting 분기 즉시 broadcast 용).
///
/// 일반 dispatch는 main loop의 broadcast 코드가 처리하지만, awaiting 분기는 *함수 안에서
/// 즉시 송신 후 continue* 패턴이라 별도 헬퍼. main loop와 같은 패턴 — 실패 시 eprintln
/// + 다음 항목 시도 (transient error tolerance).
async fn send_state_sets(
    stream: &mut TcpStream,
    req_seq: &mut u64,
    state_sets: Vec<(ObjectId, String, serde_json::Value)>,
) {
    for (oid, key, val) in state_sets {
        *req_seq += 1;
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
            break;
        }
    }
}
