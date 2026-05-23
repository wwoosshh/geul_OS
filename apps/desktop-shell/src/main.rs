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

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use geulos_core::{
    std_types, AclEffect, AclEntry, ActorId, ActorPattern, MethodPattern, Object, ObjectId,
};
use geulos_desktop_shell::ai_session::{self, CliChatSession};
use geulos_desktop_shell::cli_handler::{self, SpecialAction};
use geulos_desktop_shell::fs_watcher::{FsChange, FsWatcher};
use geulos_desktop_shell::{
    dialog_ops, drives, explorer_ops, file_ops, file_read, file_write, folder_ops, granted_dirs,
    invoke_handler, lazy_mount, permission, window_ops,
};
use geulos_proto::{
    decode_frame, encode_frame, EventKindFilterWire, EventMsg, Hello, HelloAck, MountAck, MountMsg,
    Role, StateSetMsg, SubscribeAck, SubscribeMsg,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SERVER_ADDR: &str = "127.0.0.1:5550";

/// M8 동안 유지 — read-only로 자연 보호. M9 권한 다이얼로그 마일스톤에서
/// 매니페스트 기반 권한으로 교체 예정. 추적: KI-001 / KI-016 (`docs/known-issues.md`).
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
/// lazy_expand_if_needed에서 폴더 디스크 경로를 알아낼 때 사용.
fn lookup_folder_path(objects: &[Object], id: ObjectId) -> Option<PathBuf> {
    let obj = objects.iter().find(|o| o.id == id)?;
    if obj.type_uri.as_str() != "aios.std/Folder@1" {
        return None;
    }
    obj.props.get("path").and_then(|v| v.as_str()).map(PathBuf::from)
}

/// 주어진 ID의 File 객체에서 `path` prop을 꺼낸다. 없으면 None (M9 T8 재도입).
///
/// `save_to_file` / `save` 분기에서 디스크에 write할 경로 lookup. M7-M8 동안 read-only로
/// dead였다가 M9 권한/쓰기 도입과 함께 재활성. lookup_folder_path와 대칭 — File 타입만 매칭.
fn lookup_file_path(objects: &[Object], id: ObjectId) -> Option<PathBuf> {
    let obj = objects.iter().find(|o| o.id == id)?;
    if obj.type_uri.as_str() != "aios.std/File@1" {
        return None;
    }
    obj.props.get("path").and_then(|v| v.as_str()).map(PathBuf::from)
}

/// CLI lines 히스토리 최대 보관 라인 수 (오래된 라인은 잘림).
const CLI_LINES_CAP: usize = 1000;

/// CLI 입력 dispatch 결과를 Cli.state.lines에 반영하고 StateSet 출력 생성.
///
/// `prompt_prefix`는 입력 echo에 prepend할 prompt 문자열 — shell 모드는 `"> "`,
/// AI 모드는 `"[ai:<name>] > "` (T7.8). `input_echo`가 비어있지 않으면 첫 라인으로
/// `{prompt_prefix}{input_echo}`를 추가해 사용자 입력 자체도 출력 히스토리에 남김
/// (전형적 셸 동작). special이 Clear면 기존 라인 다 비우고 echo·output_lines도 무시 —
/// clear 명령은 깨끗한 상태가 목적. 사용자 입력 `clear`의 input echo도 의도적으로
/// drop — POSIX `clear`와 일관.
///
/// mounted_objects의 Cli 객체에서 현재 lines를 읽고 capped된 새 배열을 만들어
/// state_sets로 반환. mounted_objects도 동기화 갱신.
fn handle_cli_outcome(
    mounted_objects: &mut [Object],
    cli_target: ObjectId,
    prompt_prefix: &str,
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
        current.push(format!("{}{}", prompt_prefix, input_echo));
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
    chat_session: &mut Option<CliChatSession>,
) -> Result<String, String> {
    let wire = ai_session::connect_wire(server_addr).await.map_err(|e| e.to_string())?;
    let system = ai_session::DEFAULT_CLI_SYSTEM_PROMPT.to_string();
    if is_start {
        let session = CliChatSession::start(key, wire, system, name.to_string());
        *chat_session = Some(session);
        Ok(format!("(새 AI 세션 시작: {})", name))
    } else {
        let session = CliChatSession::load(key, wire, system, name).map_err(|e| e.to_string())?;
        *chat_session = Some(session);
        Ok(format!("(AI 세션 로드: {})", name))
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
    fs_watcher: Option<&mut FsWatcher>,
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
        child_ids.push(child_id);
        let mm =
            MountMsg { root_object_id: child_id.to_string(), tree: serde_json::to_value(&child)? };
        stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
        // **Folder + File 모두 subscribe** — M9 T10: AI가 File.save invoke 호출하면
        // desktop-shell이 받아 Dialog mount해야 한다. 이전엔 File subscribe 누락으로
        // invoke가 server에서 도착하지 않아 Dialog가 안 떴음 (사용자 보고).
        *req_seq += 1;
        let sub = SubscribeMsg {
            subscription_id: format!("sub-runtime-{}", req_seq),
            target: child_id.to_string(),
            kinds: vec![EventKindFilterWire::Invoke],
            include_initial: false,
        };
        stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
        mounted_objects.push(child);
    }
    if let Some(parent) = mounted_objects.iter_mut().find(|o| o.id == folder_id) {
        parent.children = child_ids;
        // child_count state도 갱신.
        let len = parent.children.len();
        parent.state.insert("child_count".to_string(), serde_json::json!(len));
    }
    // M10 Phase 2: expand된 폴더를 watcher에 등록 — 외부에서 이 폴더 안 파일을 만들거나
    // 삭제하면 100ms 폴링 사이클에서 감지되어 main이 mount/destroy로 반영.
    if let Some(watcher) = fs_watcher {
        if let Err(e) = watcher.watch(&folder_path) {
            eprintln!(
                "[desktop-shell] fs_watcher watch 등록 실패 {}: {}",
                folder_path.display(),
                e
            );
        }
    }
    Ok(())
}

/// 주어진 path를 가진 mounted 객체 (File@1 또는 Folder@1)의 ObjectId + parent ObjectId를
/// 반환. M10 Phase 2 — fs watcher 이벤트의 path를 기존 객체에 매핑할 때 사용.
///
/// path 비교는 `Path::new`로 normalize한 직접 비교. Windows의 short/long path 차이는 v2에
/// canonicalize 검토 (v1은 lazy_mount가 입력한 path 그대로 보관해 *대부분* 일치).
fn find_object_by_path(objects: &[Object], target: &Path) -> Option<(ObjectId, Option<ObjectId>)> {
    objects.iter().find_map(|o| {
        let p = o.props.get("path").and_then(|v| v.as_str())?;
        if Path::new(p) == target {
            Some((o.id, o.parent))
        } else {
            None
        }
    })
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
            add_wildcard_acl(&mut new_obj);
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

    // 추적: KI-001 / KI-016 — M9 권한 다이얼로그 도착 시 일괄 제거.
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
    let mut chat_session: Option<CliChatSession> = None;
    println!(
        "[desktop-shell] CLI 시작 (shell 모드). /ai start | /ai load | /ai list | /exit 으로 AI 모드 진입/탈출."
    );

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
                                fs_watcher.as_mut(),
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
                                fs_watcher.as_mut(),
                            )
                            .await?;
                            explorer_ops::handle_navigate_to(target_id, fid)
                        }
                        None => invoke_handler::InvokeOutcome::empty(),
                    }
                }
                "navigate_up" => {
                    // Explorer 상단 "/" 행 클릭 — 현재 active_folder의 parent로 이동.
                    // parent 없으면 빈 string으로 reset → 드라이브 일람 화면.
                    let current_active = mounted_objects
                        .iter()
                        .find(|o| o.id == target_id)
                        .and_then(|ex| ex.state.get("active_folder").and_then(|v| v.as_str()))
                        .and_then(parse_object_id);
                    explorer_ops::handle_navigate_up(target_id, &mounted_objects, current_active)
                }
                // ─────────────────────── T8.7: Explorer.open_file ───────────────────────
                // 같은 파일을 이미 연 Window가 있으면 *그것만 focus + z 최상위*. 없으면
                // 새 Window를 Desktop 자식으로 mount하고 그 Window에 invoke subscribe.
                // 어느 분기든 *focused 갱신은 모든 Window를 대상으로* batch — 정확히
                // 한 Window만 focused=true가 되도록.
                // ─────────────────── T8.10: Window move/resize/focus/close ───────────────────
                // 컴포지터가 마우스 드래그/클릭/[x]를 invoke로 변환해 보냄. desktop-shell이
                // mounted_objects의 Window 상태를 갱신하고 StateSet으로 broadcast → 컴포지터가
                // 다음 프레임에 반영. close는 정식 DestroyMsg/emit_destroyed 와이어 경로가
                // proto에 *없으므로* SetState destroyed=true 우회 (KI-011 tombstone과 형식 일치).
                // 컴포지터 layout/render는 state.destroyed=true Window를 skip — 자연스럽게 사라짐.
                "move" => {
                    let x = args.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let y = args.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    if let Some(w) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
                        w.state.insert("x".into(), json!(x));
                        w.state.insert("y".into(), json!(y));
                    }
                    invoke_handler::InvokeOutcome {
                        state_sets: vec![
                            (target_id, "x".to_string(), json!(x)),
                            (target_id, "y".to_string(), json!(y)),
                        ],
                    }
                }
                "resize" => {
                    // 최소 크기 — 너무 작아 title bar/[x]/resize handle이 사라지는 걸 방지.
                    let w_val =
                        (args.get("w").and_then(|v| v.as_i64()).unwrap_or(600) as i32).max(200);
                    let h_val =
                        (args.get("h").and_then(|v| v.as_i64()).unwrap_or(400) as i32).max(120);
                    if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
                        o.state.insert("w".into(), json!(w_val));
                        o.state.insert("h".into(), json!(h_val));
                    }
                    invoke_handler::InvokeOutcome {
                        state_sets: vec![
                            (target_id, "w".to_string(), json!(w_val)),
                            (target_id, "h".to_string(), json!(h_val)),
                        ],
                    }
                }
                "focus" => {
                    // open_file의 중복 분기와 동일 패턴 — 모든 Window batch update.
                    let new_z = window_ops::max_z(&mounted_objects) + 1;
                    let mut outs = vec![];
                    for o in &mut mounted_objects {
                        if o.type_uri.as_str() == "aios.builtin/Window@1" {
                            let is_target = o.id == target_id;
                            o.state.insert("focused".into(), json!(is_target));
                            outs.push((o.id, "focused".to_string(), json!(is_target)));
                            if is_target {
                                o.state.insert("z".into(), json!(new_z));
                                outs.push((o.id, "z".to_string(), json!(new_z)));
                            }
                        }
                    }
                    invoke_handler::InvokeOutcome { state_sets: outs }
                }
                "close" => {
                    // proto에 DestroyMsg / emit_destroyed 와이어 trigger가 없어 (확인 완료
                    // — server-host/src/dispatch.rs는 Mount/Invoke/Query/StateSet/Get만 처리.
                    // emit_destroyed는 DisconnectActor 시 server 내부에서만 호출), SetState
                    // destroyed=true로 tombstone 플래그. desktop-shell 측 mounted_objects와
                    // Desktop.children에서도 즉시 제거 — 같은 파일 재open 시 새 Window가 정상 생성.
                    // 컴포지터의 layout_desktop이 state.destroyed=true Window를 skip하므로
                    // 다음 프레임에서 시각적으로 사라진다.
                    let close_id = target_id;
                    mounted_objects.retain(|o| o.id != close_id);
                    if let Some(d) = mounted_objects.iter_mut().find(|o| o.id == desktop_id) {
                        d.children.retain(|c| *c != close_id);
                    }
                    invoke_handler::InvokeOutcome {
                        state_sets: vec![(close_id, "destroyed".to_string(), json!(true))],
                    }
                }
                "open_file" => {
                    let fid_str = args.get("file_id").and_then(|v| v.as_str()).unwrap_or("");
                    match parse_object_id(fid_str) {
                        Some(file_id) => {
                            if let Some(existing_window_id) =
                                window_ops::find_window_for_file(&mounted_objects, file_id)
                            {
                                // 중복 — 새 mount 없이 focus + z 최상위만.
                                let new_z = window_ops::max_z(&mounted_objects) + 1;
                                let mut outs = vec![];
                                for o in &mut mounted_objects {
                                    if o.type_uri.as_str() == "aios.builtin/Window@1" {
                                        let is_target = o.id == existing_window_id;
                                        o.state.insert("focused".into(), json!(is_target));
                                        outs.push((o.id, "focused".to_string(), json!(is_target)));
                                        if is_target {
                                            o.state.insert("z".into(), json!(new_z));
                                            outs.push((o.id, "z".to_string(), json!(new_z)));
                                        }
                                    }
                                }
                                invoke_handler::InvokeOutcome { state_sets: outs }
                            } else {
                                // 새 Window mount.
                                let title = mounted_objects
                                    .iter()
                                    .find(|o| o.id == file_id)
                                    .and_then(|f| f.props.get("name").and_then(|v| v.as_str()))
                                    .unwrap_or("(파일)")
                                    .to_string();
                                let pos =
                                    window_ops::next_window_position(&mounted_objects, (300, 200));
                                let new_z = window_ops::max_z(&mounted_objects) + 1;
                                let mut new_window = window_ops::build_new_window(
                                    &owner,
                                    desktop_id,
                                    file_id,
                                    &title,
                                    pos,
                                    (600, 400),
                                    new_z,
                                );
                                add_wildcard_acl(&mut new_window);

                                // M8 part 2 (ADR-033): Window mount 시점에 file 본문 read.
                                // File 객체의 props.path / props.mime를 lookup해 file_read에
                                // 위임. 결과를 Window.state.content / content_too_large에 채움.
                                // File 객체가 없거나 path/mime이 비어있어도 file_read가 graceful
                                // 안내 메시지 반환 — panic X.
                                let (file_path, mime) = {
                                    let f = mounted_objects.iter().find(|o| o.id == file_id);
                                    match f {
                                        Some(file) => {
                                            let p = file
                                                .props
                                                .get("path")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let m = file
                                                .props
                                                .get("mime")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("application/octet-stream");
                                            (std::path::PathBuf::from(p), m.to_string())
                                        }
                                        None => (
                                            std::path::PathBuf::new(),
                                            "application/octet-stream".to_string(),
                                        ),
                                    }
                                };
                                let fc = file_read::read_file_for_window(&file_path, &mime);
                                new_window
                                    .state
                                    .insert("content".into(), serde_json::json!(fc.text));
                                new_window.state.insert(
                                    "content_too_large".into(),
                                    serde_json::json!(fc.too_large),
                                );

                                let new_id = new_window.id;
                                // 기존 다른 모든 Window는 focused=false.
                                let mut outs = vec![];
                                for o in &mut mounted_objects {
                                    if o.type_uri.as_str() == "aios.builtin/Window@1" {
                                        o.state.insert("focused".into(), json!(false));
                                        outs.push((o.id, "focused".to_string(), json!(false)));
                                    }
                                }
                                // Window mount 송신.
                                let mm = MountMsg {
                                    root_object_id: new_id.to_string(),
                                    tree: serde_json::to_value(&new_window)?,
                                };
                                stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
                                // Window 자체에 invoke subscribe — move/resize/focus/close (T8.10).
                                req_seq += 1;
                                let sub = SubscribeMsg {
                                    subscription_id: format!("sub-runtime-{}", req_seq),
                                    target: new_id.to_string(),
                                    kinds: vec![EventKindFilterWire::Invoke],
                                    include_initial: false,
                                };
                                stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
                                // mounted_objects + desktop.children 갱신.
                                if let Some(d) =
                                    mounted_objects.iter_mut().find(|o| o.id == desktop_id)
                                {
                                    d.children.push(new_id);
                                }
                                mounted_objects.push(new_window);
                                invoke_handler::InvokeOutcome { state_sets: outs }
                            }
                        }
                        None => invoke_handler::InvokeOutcome::empty(),
                    }
                }
                // ─────────────────────── T7.5/T7.8: 하단 CLI 패널 ───────────────────────
                // T7.8 (ADR-031): 명시적 mode + 영속 세션. 현재 Cli.state.mode를 보고
                // dispatch_command (shell) 또는 dispatch_chat (ai)로 분기. SpecialAction은
                // AiStart/Load/List/Exit/Send/Clear — 각각 chat_session 갱신 + Cli.state.mode/
                // session_name 갱신 + state_sets에 추가.
                "submit_input" => {
                    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let current_mode = mounted_objects
                        .iter()
                        .find(|o| o.id == target_id)
                        .and_then(|o| o.state.get("mode").and_then(|v| v.as_str()))
                        .unwrap_or("shell")
                        .to_string();
                    let prompt_prefix = prompt_prefix_for(&mounted_objects, target_id);

                    // T7.9 (ADR-032): awaiting_api_key 모드는 *dispatch 외부에서* 처리.
                    // 입력을 key로 취급해 검증·저장·resume. dispatch_command/dispatch_chat에는
                    // 진입하지 않는다.
                    if current_mode == "awaiting_api_key" {
                        let trimmed = text.trim();
                        // /exit 또는 빈 입력 → cancel.
                        if trimmed.is_empty() || trimmed == "/exit" {
                            let extra = exit_awaiting_mode(&mut mounted_objects, target_id);
                            // T7.10: input_echo는 *항상 빈 문자열* — awaiting 모드의 입력은
                            // 잠재적 API key. /exit 같은 무해한 토큰도 lines에 안 남겨야 *AI tool로
                            // get_object(cli) 호출 시 키 유출 위험을 구조적으로 0으로* 만든다
                            // (분기 분석을 의존하지 않고 *항상 skip*하는 게 안전).
                            let mut combined = handle_cli_outcome(
                                &mut mounted_objects,
                                target_id,
                                &prompt_prefix,
                                "",
                                vec!["(API key 입력 취소 — 셸 모드로 복귀)".to_string()],
                                None,
                            );
                            combined.state_sets.extend(extra);
                            for (oid, key, val) in combined.state_sets {
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
                                    break;
                                }
                            }
                            continue;
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
                                let pending = mounted_objects
                                    .iter()
                                    .find(|o| o.id == target_id)
                                    .and_then(|o| {
                                        o.state.get("pending_action").and_then(|v| v.as_str())
                                    })
                                    .unwrap_or("")
                                    .to_string();
                                // mode/pending_action SetState — shell로 일단 복귀.
                                let mut extra = exit_awaiting_mode(&mut mounted_objects, target_id);
                                // pending 실행.
                                let (is_start, name): (bool, Option<String>) = {
                                    let mut parts = pending.splitn(2, ' ');
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
                                    lines.push(
                                        "[pending 액션이 비어있어 셸 모드로 복귀]".to_string(),
                                    );
                                } else {
                                    match start_or_load_session(
                                        &addr,
                                        key.clone(),
                                        &session_name,
                                        is_start,
                                        &mut chat_session,
                                    )
                                    .await
                                    {
                                        Ok(msg) => {
                                            lines.push(msg);
                                            if chat_session.is_some() {
                                                if let Some(cli) = mounted_objects
                                                    .iter_mut()
                                                    .find(|o| o.id == target_id)
                                                {
                                                    cli.state.insert("mode".into(), json!("ai"));
                                                    cli.state.insert(
                                                        "session_name".into(),
                                                        json!(&session_name),
                                                    );
                                                }
                                                // 기 push된 mode=shell을 ai로 덮어쓰기 — 같은 key의
                                                // 마지막 set이 승. session_name도 갱신.
                                                extra.push((
                                                    target_id,
                                                    "mode".to_string(),
                                                    json!("ai"),
                                                ));
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
                                let mut combined = handle_cli_outcome(
                                    &mut mounted_objects,
                                    target_id,
                                    &prompt_prefix,
                                    "",
                                    lines,
                                    None,
                                );
                                combined.state_sets.extend(extra);
                                for (oid, k, val) in combined.state_sets {
                                    req_seq += 1;
                                    let ss = StateSetMsg {
                                        request_id: format!("r-{}", req_seq),
                                        target: oid.to_string(),
                                        key: k,
                                        value: val,
                                    };
                                    let bytes = match serde_json::to_vec(&ss) {
                                        Ok(b) => b,
                                        Err(e) => {
                                            eprintln!(
                                                "[desktop-shell] StateSet 직렬화 실패: {}",
                                                e
                                            );
                                            continue;
                                        }
                                    };
                                    if let Err(e) = stream.write_all(&encode_frame(&bytes)).await {
                                        eprintln!("[desktop-shell] StateSet 송신 실패: {}", e);
                                        break;
                                    }
                                }
                                continue;
                            }
                            Err(e) => {
                                // 검증 실패 — mode 유지, 안내 출력.
                                // T7.10: 입력 echo 빈 문자열 (key 본문 lines 노출 방지).
                                let combined = handle_cli_outcome(
                                    &mut mounted_objects,
                                    target_id,
                                    &prompt_prefix,
                                    "",
                                    vec![format!(
                                        "[검증 실패: {}] 다시 입력하거나 /exit으로 취소.",
                                        e
                                    )],
                                    None,
                                );
                                for (oid, k, val) in combined.state_sets {
                                    req_seq += 1;
                                    let ss = StateSetMsg {
                                        request_id: format!("r-{}", req_seq),
                                        target: oid.to_string(),
                                        key: k,
                                        value: val,
                                    };
                                    let bytes = match serde_json::to_vec(&ss) {
                                        Ok(b) => b,
                                        Err(e) => {
                                            eprintln!(
                                                "[desktop-shell] StateSet 직렬화 실패: {}",
                                                e
                                            );
                                            continue;
                                        }
                                    };
                                    if let Err(e) = stream.write_all(&encode_frame(&bytes)).await {
                                        eprintln!("[desktop-shell] StateSet 송신 실패: {}", e);
                                        break;
                                    }
                                }
                                continue;
                            }
                        }
                    }

                    let outcome = if current_mode == "ai" {
                        cli_handler::dispatch_chat(&text)
                    } else {
                        cli_handler::dispatch_command(&text)
                    };

                    // mode/session_name SetState를 별도로 누적해 outcome에 합친다.
                    let mut extra_sets: Vec<(ObjectId, String, serde_json::Value)> = Vec::new();
                    let outcome = match outcome.special {
                        Some(SpecialAction::AiStart(name_opt)) => {
                            // 이전 세션은 매 send 후 dump되므로 그대로 drop으로 OK.
                            let name = name_opt.unwrap_or_else(ai_session::auto_name);
                            // T7.9 (ADR-032): key chain — 없으면 awaiting_api_key 모드 진입.
                            match ai_session::resolve_api_key() {
                                Some(key) => {
                                    let lines = match start_or_load_session(
                                        &addr,
                                        key,
                                        &name,
                                        true,
                                        &mut chat_session,
                                    )
                                    .await
                                    {
                                        Ok(msg) => vec![msg],
                                        Err(e) => vec![format!("[AI start 실패: {}]", e)],
                                    };
                                    // 성공 시 mode=ai + session_name=name SetState.
                                    if chat_session.is_some() {
                                        if let Some(cli) =
                                            mounted_objects.iter_mut().find(|o| o.id == target_id)
                                        {
                                            cli.state.insert("mode".into(), json!("ai"));
                                            cli.state.insert("session_name".into(), json!(&name));
                                        }
                                        extra_sets.push((
                                            target_id,
                                            "mode".to_string(),
                                            json!("ai"),
                                        ));
                                        extra_sets.push((
                                            target_id,
                                            "session_name".to_string(),
                                            json!(name),
                                        ));
                                    }
                                    handle_cli_outcome(
                                        &mut mounted_objects,
                                        target_id,
                                        &prompt_prefix,
                                        &text,
                                        lines,
                                        None,
                                    )
                                }
                                None => {
                                    let pending = format!("start {}", name);
                                    let extra = enter_awaiting_mode(
                                        &mut mounted_objects,
                                        target_id,
                                        pending,
                                    );
                                    extra_sets.extend(extra);
                                    handle_cli_outcome(
                                        &mut mounted_objects,
                                        target_id,
                                        &prompt_prefix,
                                        &text,
                                        vec![
                                            "[ANTHROPIC_API_KEY 미설정] CLI에 키를 입력 후 Enter (취소: /exit)".to_string(),
                                        ],
                                        None,
                                    )
                                }
                            }
                        }
                        Some(SpecialAction::AiLoad(name)) => {
                            // T7.9 (ADR-032): key chain — 없으면 awaiting_api_key 모드 진입.
                            match ai_session::resolve_api_key() {
                                Some(key) => {
                                    let lines = match start_or_load_session(
                                        &addr,
                                        key,
                                        &name,
                                        false,
                                        &mut chat_session,
                                    )
                                    .await
                                    {
                                        Ok(msg) => vec![msg],
                                        Err(e) => vec![format!("[AI load 실패: {}]", e)],
                                    };
                                    if chat_session.is_some() {
                                        if let Some(cli) =
                                            mounted_objects.iter_mut().find(|o| o.id == target_id)
                                        {
                                            cli.state.insert("mode".into(), json!("ai"));
                                            cli.state.insert("session_name".into(), json!(&name));
                                        }
                                        extra_sets.push((
                                            target_id,
                                            "mode".to_string(),
                                            json!("ai"),
                                        ));
                                        extra_sets.push((
                                            target_id,
                                            "session_name".to_string(),
                                            json!(name),
                                        ));
                                    }
                                    handle_cli_outcome(
                                        &mut mounted_objects,
                                        target_id,
                                        &prompt_prefix,
                                        &text,
                                        lines,
                                        None,
                                    )
                                }
                                None => {
                                    let pending = format!("load {}", name);
                                    let extra = enter_awaiting_mode(
                                        &mut mounted_objects,
                                        target_id,
                                        pending,
                                    );
                                    extra_sets.extend(extra);
                                    handle_cli_outcome(
                                        &mut mounted_objects,
                                        target_id,
                                        &prompt_prefix,
                                        &text,
                                        vec![
                                            "[ANTHROPIC_API_KEY 미설정] CLI에 키를 입력 후 Enter (취소: /exit)".to_string(),
                                        ],
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
                            handle_cli_outcome(
                                &mut mounted_objects,
                                target_id,
                                &prompt_prefix,
                                &text,
                                lines,
                                None,
                            )
                        }
                        Some(SpecialAction::AiExit) => {
                            // 세션은 매 send 후 dump됨 — drop으로 OK.
                            chat_session = None;
                            // T7.9 (ADR-032): pending_action도 항상 null로 리셋 — awaiting에서
                            // /ai start로 들어왔다가 다시 /exit하는 등 잔재 방지.
                            if let Some(cli) =
                                mounted_objects.iter_mut().find(|o| o.id == target_id)
                            {
                                cli.state.insert("mode".into(), json!("shell"));
                                cli.state.insert("session_name".into(), json!(null));
                                cli.state.insert("pending_action".into(), json!(null));
                            }
                            extra_sets.push((target_id, "mode".to_string(), json!("shell")));
                            extra_sets.push((target_id, "session_name".to_string(), json!(null)));
                            extra_sets.push((target_id, "pending_action".to_string(), json!(null)));
                            handle_cli_outcome(
                                &mut mounted_objects,
                                target_id,
                                &prompt_prefix,
                                &text,
                                vec!["(셸 모드로 복귀)".to_string()],
                                None,
                            )
                        }
                        Some(SpecialAction::AiSend(prompt)) => {
                            let lines = if let Some(session) = chat_session.as_mut() {
                                eprintln!("[desktop-shell] AI prompt: {}", prompt);
                                match session.send(&prompt).await {
                                    Ok(reply) if reply.is_empty() => {
                                        vec!["[AI: (빈 응답)]".to_string()]
                                    }
                                    Ok(reply) => reply.lines().map(String::from).collect(),
                                    Err(e) => vec![format!("[AI 오류: {}]", e)],
                                }
                            } else {
                                // 이론상 mode=ai인데 chat_session=None은 발생 안 함.
                                // 방어적으로 안내.
                                vec!["[AI 세션 없음 — /ai start로 시작]".to_string()]
                            };
                            handle_cli_outcome(
                                &mut mounted_objects,
                                target_id,
                                &prompt_prefix,
                                &text,
                                lines,
                                None,
                            )
                        }
                        Some(SpecialAction::Clear) | None => handle_cli_outcome(
                            &mut mounted_objects,
                            target_id,
                            &prompt_prefix,
                            &text,
                            outcome.output_lines,
                            outcome.special,
                        ),
                    };
                    // mode/session_name SetState를 outcome 뒤에 합쳐 한 번에 broadcast.
                    let mut combined = outcome;
                    combined.state_sets.extend(extra_sets);
                    combined
                }
                "clear" => {
                    // 외부에서 직접 clear 호출 — lines 비움.
                    handle_cli_outcome(
                        &mut mounted_objects,
                        target_id,
                        "",
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
                        "",
                        vec![text.to_string()],
                        None,
                    )
                }
                // ─────────────────── M9 T8: 편집/저장/권한 흐름 ───────────────────
                // Window.save_to_file(content) — 사용자 Ctrl+S. compositor가 *editor local content*
                // 를 args.content로 실어 보냄 (Window.state.content는 *읽지 않음*).
                //
                // 이유 (사용자 보고 freeze fix): 이전 v1은 매 키 입력마다 SetState(content)를 wire에
                // push해서 큰 텍스트 파일에서 wire backpressure로 입력 freeze 발생. 이제 content는
                // 컴포지터가 master, save 시점에만 args로 한 번 전달. desktop-shell이 args.content를
                // 직접 디스크에 commit + Window.state.content도 같이 갱신해서 다음 viewer load 일관.
                //
                // 사용자 직접 액션이므로 permission::judge(local-user, Save) = Allow.
                "save_to_file" => {
                    let content =
                        args.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    eprintln!(
                        "[desktop-shell] save_to_file invoke 수신 target={} content_len={}",
                        target_id,
                        content.len()
                    );
                    // Window.save_to_file은 *compositor의 Ctrl+S가 발송*하는 UI 직접 액션이므로
                    // 권한 검사 없이 항상 허용. AI 등 외부 actor가 File 자체에 write할 때만
                    // (`File.save` invoke) permission::judge로 Dialog confirm을 띄운다.
                    let file_id_opt = mounted_objects
                        .iter()
                        .find(|o| o.id == target_id)
                        .and_then(|w| w.props.get("file_id").and_then(|v| v.as_str()))
                        .and_then(parse_object_id);
                    match file_id_opt {
                        Some(file_id) => match lookup_file_path(&mounted_objects, file_id) {
                            Some(path) => match file_write::save(&path, &content) {
                                Ok(()) => {
                                    eprintln!(
                                        "[desktop-shell] save_to_file OK → {}",
                                        path.display()
                                    );
                                    if let Some(w) =
                                        mounted_objects.iter_mut().find(|o| o.id == target_id)
                                    {
                                        w.state.insert("dirty".into(), json!(false));
                                        w.state.insert("content".into(), json!(&content));
                                    }
                                    invoke_handler::InvokeOutcome {
                                        state_sets: vec![
                                            (target_id, "dirty".to_string(), json!(false)),
                                            (target_id, "content".to_string(), json!(content)),
                                        ],
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[desktop-shell] save_to_file 실패: {}", e);
                                    invoke_handler::InvokeOutcome::empty()
                                }
                            },
                            None => {
                                eprintln!(
                                    "[desktop-shell] save_to_file: file_id={}의 path 조회 실패",
                                    file_id
                                );
                                invoke_handler::InvokeOutcome::empty()
                            }
                        },
                        None => {
                            eprintln!(
                                "[desktop-shell] save_to_file: Window.props.file_id 누락 또는 파싱 실패 (target={})",
                                target_id
                            );
                            invoke_handler::InvokeOutcome::empty()
                        }
                    }
                }
                // M10 T7: Folder@1.create_file(name) — 폴더 안에 새 빈 파일 생성.
                // permission::judge_with_path로 dir 단위 grant 판정 → Allow면 즉시 fs +
                // mount/subscribe/parent.children 갱신, ConfirmRequired면 Dialog + Pending에
                // CreateFile variant 보관 (respond 분기가 take → 실제 실행).
                "create_file" => {
                    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let folder_path_opt = mounted_objects
                        .iter()
                        .find(|o| o.id == target_id)
                        .and_then(|f| f.props.get("path").and_then(|v| v.as_str()))
                        .map(PathBuf::from);
                    match folder_path_opt {
                        Some(folder_path) => {
                            let verdict = permission::judge_with_path(
                                &sender_actor,
                                permission::Op::CreateFile,
                                &folder_path,
                                &granted,
                            );
                            match verdict {
                                permission::Verdict::Allow => {
                                    let now = chrono::Utc::now().timestamp_millis();
                                    // M10 Phase 2: 우리가 막 만들 파일을 watcher echo 캐시에
                                    // 미리 등록 — fs::write 직후 도착할 notify 이벤트는 무시.
                                    if let Some(w) = fs_watcher.as_ref() {
                                        w.mark_self_op(folder_path.join(&name));
                                    }
                                    match folder_ops::create_file_in(
                                        &owner,
                                        &folder_path,
                                        &name,
                                        now,
                                    ) {
                                        Ok(mut new_obj) => {
                                            new_obj.parent = Some(target_id);
                                            add_wildcard_acl(&mut new_obj);
                                            let new_id = new_obj.id;
                                            let mm = MountMsg {
                                                root_object_id: new_id.to_string(),
                                                tree: serde_json::to_value(&new_obj)?,
                                            };
                                            stream
                                                .write_all(&encode_frame(&serde_json::to_vec(&mm)?))
                                                .await?;
                                            req_seq += 1;
                                            let sub = SubscribeMsg {
                                                subscription_id: format!("sub-runtime-{}", req_seq),
                                                target: new_id.to_string(),
                                                kinds: vec![EventKindFilterWire::Invoke],
                                                include_initial: false,
                                            };
                                            stream
                                                .write_all(&encode_frame(&serde_json::to_vec(
                                                    &sub,
                                                )?))
                                                .await?;
                                            if let Some(p) = mounted_objects
                                                .iter_mut()
                                                .find(|o| o.id == target_id)
                                            {
                                                p.children.push(new_id);
                                            }
                                            mounted_objects.push(new_obj);
                                            eprintln!(
                                                "[desktop-shell] create_file OK → {}/{}",
                                                folder_path.display(),
                                                name
                                            );
                                            invoke_handler::InvokeOutcome::empty()
                                        }
                                        Err(e) => {
                                            eprintln!("[desktop-shell] create_file 실패: {}", e);
                                            invoke_handler::InvokeOutcome::empty()
                                        }
                                    }
                                }
                                permission::Verdict::ConfirmRequired => {
                                    let mut dialog = std_types::dialog(
                                        owner.clone(),
                                        "AI 파일 생성 확인",
                                        &format!(
                                            "AI가 {} 안에 '{}'를 생성하려고 합니다 — 허용?",
                                            folder_path.display(),
                                            name
                                        ),
                                        "confirm",
                                        vec!["허용".to_string(), "거부".to_string()],
                                    );
                                    dialog.parent = Some(desktop_id);
                                    add_wildcard_acl(&mut dialog);
                                    let dialog_id = dialog.id;
                                    let mm = MountMsg {
                                        root_object_id: dialog_id.to_string(),
                                        tree: serde_json::to_value(&dialog)?,
                                    };
                                    stream
                                        .write_all(&encode_frame(&serde_json::to_vec(&mm)?))
                                        .await?;
                                    req_seq += 1;
                                    let sub = SubscribeMsg {
                                        subscription_id: format!("sub-runtime-{}", req_seq),
                                        target: dialog_id.to_string(),
                                        kinds: vec![EventKindFilterWire::Invoke],
                                        include_initial: false,
                                    };
                                    stream
                                        .write_all(&encode_frame(&serde_json::to_vec(&sub)?))
                                        .await?;
                                    mounted_objects.push(dialog);
                                    let (tx, _rx) = tokio::sync::oneshot::channel::<String>();
                                    pending.insert(
                                        dialog_id,
                                        dialog_ops::PendingEntry {
                                            op: dialog_ops::PendingFs::CreateFile {
                                                folder_id: target_id,
                                                folder_path,
                                                name,
                                            },
                                            tx,
                                        },
                                    );
                                    eprintln!(
                                        "[desktop-shell] AI create_file Dialog mount (folder {}): 사용자 응답 대기",
                                        target_id
                                    );
                                    invoke_handler::InvokeOutcome::empty()
                                }
                            }
                        }
                        None => {
                            eprintln!(
                                "[desktop-shell] create_file: folder path 누락 target={}",
                                target_id
                            );
                            invoke_handler::InvokeOutcome::empty()
                        }
                    }
                }
                // M10 T7: Folder@1.create_folder(name) — create_file과 동일 패턴, fs는
                // create_dir, Dialog/Pending은 CreateFolder variant.
                "create_folder" => {
                    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let folder_path_opt = mounted_objects
                        .iter()
                        .find(|o| o.id == target_id)
                        .and_then(|f| f.props.get("path").and_then(|v| v.as_str()))
                        .map(PathBuf::from);
                    match folder_path_opt {
                        Some(folder_path) => {
                            let verdict = permission::judge_with_path(
                                &sender_actor,
                                permission::Op::CreateFolder,
                                &folder_path,
                                &granted,
                            );
                            match verdict {
                                permission::Verdict::Allow => {
                                    let now = chrono::Utc::now().timestamp_millis();
                                    // M10 Phase 2: echo 캐시 — 새로 만들 폴더 path 등록.
                                    if let Some(w) = fs_watcher.as_ref() {
                                        w.mark_self_op(folder_path.join(&name));
                                    }
                                    match folder_ops::create_folder_in(
                                        &owner,
                                        &folder_path,
                                        &name,
                                        now,
                                    ) {
                                        Ok(mut new_obj) => {
                                            new_obj.parent = Some(target_id);
                                            add_wildcard_acl(&mut new_obj);
                                            let new_id = new_obj.id;
                                            let mm = MountMsg {
                                                root_object_id: new_id.to_string(),
                                                tree: serde_json::to_value(&new_obj)?,
                                            };
                                            stream
                                                .write_all(&encode_frame(&serde_json::to_vec(&mm)?))
                                                .await?;
                                            req_seq += 1;
                                            let sub = SubscribeMsg {
                                                subscription_id: format!("sub-runtime-{}", req_seq),
                                                target: new_id.to_string(),
                                                kinds: vec![EventKindFilterWire::Invoke],
                                                include_initial: false,
                                            };
                                            stream
                                                .write_all(&encode_frame(&serde_json::to_vec(
                                                    &sub,
                                                )?))
                                                .await?;
                                            if let Some(p) = mounted_objects
                                                .iter_mut()
                                                .find(|o| o.id == target_id)
                                            {
                                                p.children.push(new_id);
                                            }
                                            mounted_objects.push(new_obj);
                                            eprintln!(
                                                "[desktop-shell] create_folder OK → {}/{}",
                                                folder_path.display(),
                                                name
                                            );
                                            invoke_handler::InvokeOutcome::empty()
                                        }
                                        Err(e) => {
                                            eprintln!("[desktop-shell] create_folder 실패: {}", e);
                                            invoke_handler::InvokeOutcome::empty()
                                        }
                                    }
                                }
                                permission::Verdict::ConfirmRequired => {
                                    let mut dialog = std_types::dialog(
                                        owner.clone(),
                                        "AI 폴더 생성 확인",
                                        &format!(
                                            "AI가 {} 안에 '{}' 폴더를 생성하려고 합니다 — 허용?",
                                            folder_path.display(),
                                            name
                                        ),
                                        "confirm",
                                        vec!["허용".to_string(), "거부".to_string()],
                                    );
                                    dialog.parent = Some(desktop_id);
                                    add_wildcard_acl(&mut dialog);
                                    let dialog_id = dialog.id;
                                    let mm = MountMsg {
                                        root_object_id: dialog_id.to_string(),
                                        tree: serde_json::to_value(&dialog)?,
                                    };
                                    stream
                                        .write_all(&encode_frame(&serde_json::to_vec(&mm)?))
                                        .await?;
                                    req_seq += 1;
                                    let sub = SubscribeMsg {
                                        subscription_id: format!("sub-runtime-{}", req_seq),
                                        target: dialog_id.to_string(),
                                        kinds: vec![EventKindFilterWire::Invoke],
                                        include_initial: false,
                                    };
                                    stream
                                        .write_all(&encode_frame(&serde_json::to_vec(&sub)?))
                                        .await?;
                                    mounted_objects.push(dialog);
                                    let (tx, _rx) = tokio::sync::oneshot::channel::<String>();
                                    pending.insert(
                                        dialog_id,
                                        dialog_ops::PendingEntry {
                                            op: dialog_ops::PendingFs::CreateFolder {
                                                folder_id: target_id,
                                                folder_path,
                                                name,
                                            },
                                            tx,
                                        },
                                    );
                                    eprintln!(
                                        "[desktop-shell] AI create_folder Dialog mount (folder {}): 사용자 응답 대기",
                                        target_id
                                    );
                                    invoke_handler::InvokeOutcome::empty()
                                }
                            }
                        }
                        None => {
                            eprintln!(
                                "[desktop-shell] create_folder: folder path 누락 target={}",
                                target_id
                            );
                            invoke_handler::InvokeOutcome::empty()
                        }
                    }
                }
                // M10 T7: File@1.delete() or Folder@1.delete(recursive). target type_uri로
                // File/Folder 분기. Delete는 *항상 ConfirmRequired* (granted 무관 — permission
                // 정책 보장). Dialog kind="warn" + 해당 PendingFs::Delete{File,Folder}.
                "delete" => {
                    let target_obj_kind = mounted_objects
                        .iter()
                        .find(|o| o.id == target_id)
                        .map(|o| o.type_uri.as_str().to_string());
                    let path_opt = mounted_objects
                        .iter()
                        .find(|o| o.id == target_id)
                        .and_then(|o| o.props.get("path").and_then(|v| v.as_str()))
                        .map(PathBuf::from);
                    let recursive =
                        args.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false);
                    match (target_obj_kind.as_deref(), path_opt) {
                        (Some("aios.std/File@1"), Some(path)) => {
                            let mut dialog = std_types::dialog(
                                owner.clone(),
                                "AI 파일 삭제 확인",
                                &format!("AI가 {}를 삭제하려고 합니다 — 허용?", path.display()),
                                "warn",
                                vec!["허용".to_string(), "거부".to_string()],
                            );
                            dialog.parent = Some(desktop_id);
                            add_wildcard_acl(&mut dialog);
                            let dialog_id = dialog.id;
                            let mm = MountMsg {
                                root_object_id: dialog_id.to_string(),
                                tree: serde_json::to_value(&dialog)?,
                            };
                            stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
                            req_seq += 1;
                            let sub = SubscribeMsg {
                                subscription_id: format!("sub-runtime-{}", req_seq),
                                target: dialog_id.to_string(),
                                kinds: vec![EventKindFilterWire::Invoke],
                                include_initial: false,
                            };
                            stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
                            mounted_objects.push(dialog);
                            let (tx, _rx) = tokio::sync::oneshot::channel::<String>();
                            pending.insert(
                                dialog_id,
                                dialog_ops::PendingEntry {
                                    op: dialog_ops::PendingFs::DeleteFile {
                                        file_id: target_id,
                                        path,
                                    },
                                    tx,
                                },
                            );
                            eprintln!(
                                "[desktop-shell] AI delete_file Dialog mount (file {}): 사용자 응답 대기",
                                target_id
                            );
                            invoke_handler::InvokeOutcome::empty()
                        }
                        (Some("aios.std/Folder@1"), Some(path)) => {
                            let mut dialog = std_types::dialog(
                                owner.clone(),
                                "AI 폴더 삭제 확인",
                                &format!(
                                    "AI가 {}를 {}삭제하려고 합니다 — 허용?",
                                    path.display(),
                                    if recursive { "재귀 " } else { "" }
                                ),
                                "warn",
                                vec!["허용".to_string(), "거부".to_string()],
                            );
                            dialog.parent = Some(desktop_id);
                            add_wildcard_acl(&mut dialog);
                            let dialog_id = dialog.id;
                            let mm = MountMsg {
                                root_object_id: dialog_id.to_string(),
                                tree: serde_json::to_value(&dialog)?,
                            };
                            stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
                            req_seq += 1;
                            let sub = SubscribeMsg {
                                subscription_id: format!("sub-runtime-{}", req_seq),
                                target: dialog_id.to_string(),
                                kinds: vec![EventKindFilterWire::Invoke],
                                include_initial: false,
                            };
                            stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
                            mounted_objects.push(dialog);
                            let (tx, _rx) = tokio::sync::oneshot::channel::<String>();
                            pending.insert(
                                dialog_id,
                                dialog_ops::PendingEntry {
                                    op: dialog_ops::PendingFs::DeleteFolder {
                                        folder_id: target_id,
                                        path,
                                        recursive,
                                    },
                                    tx,
                                },
                            );
                            eprintln!(
                                "[desktop-shell] AI delete_folder Dialog mount (folder {}): 사용자 응답 대기",
                                target_id
                            );
                            invoke_handler::InvokeOutcome::empty()
                        }
                        _ => {
                            eprintln!(
                                "[desktop-shell] delete: unknown type 또는 path 누락 target={}",
                                target_id
                            );
                            invoke_handler::InvokeOutcome::empty()
                        }
                    }
                }
                // M10 T7: File@1 / Folder@1 .rename(new_name). target type 판정 → parent_dir에
                // 대한 permission::judge_with_path(Rename) → Allow면 즉시 fs::rename + props 갱신,
                // ConfirmRequired면 Dialog + Pending::Rename. respond 분기가 take + grant 추가.
                "rename" => {
                    let new_name =
                        args.get("new_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let target_obj = mounted_objects.iter().find(|o| o.id == target_id);
                    let target_obj_kind = target_obj.map(|o| o.type_uri.as_str().to_string());
                    let path_opt = target_obj
                        .and_then(|o| o.props.get("path").and_then(|v| v.as_str()))
                        .map(PathBuf::from);
                    let is_folder = matches!(target_obj_kind.as_deref(), Some("aios.std/Folder@1"));
                    match (target_obj_kind.as_deref(), path_opt) {
                        (Some("aios.std/File@1"), Some(path))
                        | (Some("aios.std/Folder@1"), Some(path)) => {
                            let parent_dir =
                                path.parent().unwrap_or(std::path::Path::new("/")).to_path_buf();
                            let verdict = permission::judge_with_path(
                                &sender_actor,
                                permission::Op::Rename,
                                &parent_dir,
                                &granted,
                            );
                            match verdict {
                                permission::Verdict::Allow => {
                                    // M10 Phase 2: echo — rename은 old path Remove + new path
                                    // Create 두 이벤트가 발생. 둘 다 mark.
                                    if let Some(w) = fs_watcher.as_ref() {
                                        w.mark_self_op(path.clone());
                                        w.mark_self_op(parent_dir.join(&new_name));
                                    }
                                    let result = if is_folder {
                                        folder_ops::rename_folder(&path, &new_name)
                                    } else {
                                        file_ops::rename_file(&path, &new_name)
                                    };
                                    match result {
                                        Ok(new_path) => {
                                            if let Some(o) = mounted_objects
                                                .iter_mut()
                                                .find(|o| o.id == target_id)
                                            {
                                                o.props.insert("name".into(), json!(&new_name));
                                                o.props.insert(
                                                    "path".into(),
                                                    json!(new_path.to_string_lossy()),
                                                );
                                            }
                                            eprintln!(
                                                "[desktop-shell] rename OK → {}",
                                                new_path.display()
                                            );
                                            invoke_handler::InvokeOutcome {
                                                state_sets: vec![(
                                                    target_id,
                                                    "name".to_string(),
                                                    json!(&new_name),
                                                )],
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("[desktop-shell] rename 실패: {}", e);
                                            invoke_handler::InvokeOutcome::empty()
                                        }
                                    }
                                }
                                permission::Verdict::ConfirmRequired => {
                                    let mut dialog = std_types::dialog(
                                        owner.clone(),
                                        "AI 이름 변경 확인",
                                        &format!(
                                            "AI가 {}를 '{}'(으)로 이름 변경하려고 합니다 — 허용?",
                                            path.display(),
                                            new_name
                                        ),
                                        "confirm",
                                        vec!["허용".to_string(), "거부".to_string()],
                                    );
                                    dialog.parent = Some(desktop_id);
                                    add_wildcard_acl(&mut dialog);
                                    let dialog_id = dialog.id;
                                    let mm = MountMsg {
                                        root_object_id: dialog_id.to_string(),
                                        tree: serde_json::to_value(&dialog)?,
                                    };
                                    stream
                                        .write_all(&encode_frame(&serde_json::to_vec(&mm)?))
                                        .await?;
                                    req_seq += 1;
                                    let sub = SubscribeMsg {
                                        subscription_id: format!("sub-runtime-{}", req_seq),
                                        target: dialog_id.to_string(),
                                        kinds: vec![EventKindFilterWire::Invoke],
                                        include_initial: false,
                                    };
                                    stream
                                        .write_all(&encode_frame(&serde_json::to_vec(&sub)?))
                                        .await?;
                                    mounted_objects.push(dialog);
                                    let (tx, _rx) = tokio::sync::oneshot::channel::<String>();
                                    pending.insert(
                                        dialog_id,
                                        dialog_ops::PendingEntry {
                                            op: dialog_ops::PendingFs::Rename {
                                                target_id,
                                                path,
                                                new_name,
                                                is_folder,
                                            },
                                            tx,
                                        },
                                    );
                                    eprintln!(
                                        "[desktop-shell] AI rename Dialog mount (target {}): 사용자 응답 대기",
                                        target_id
                                    );
                                    invoke_handler::InvokeOutcome::empty()
                                }
                            }
                        }
                        _ => {
                            eprintln!(
                                "[desktop-shell] rename: unknown type 또는 path 누락 target={}",
                                target_id
                            );
                            invoke_handler::InvokeOutcome::empty()
                        }
                    }
                }
                // File.save — AI/외부 actor가 직접 호출. args.content를 받아 디스크에 write.
                // sender_actor가 local-user면 Allow, AI(또는 그 외)면 ConfirmRequired →
                // Dialog mount + PendingMap.insert. 사용자가 Dialog 클릭하면 respond 분기가
                // PendingMap.take → save 실행 + Dialog destroy.
                "save" => {
                    let content =
                        args.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    match lookup_file_path(&mounted_objects, target_id) {
                        Some(p) => {
                            let verdict = permission::judge(&sender_actor, permission::Op::Save);
                            match verdict {
                                permission::Verdict::Allow => {
                                    // M10 Phase 2: echo — save 직후 watcher가 Modified를 보고함.
                                    if let Some(w) = fs_watcher.as_ref() {
                                        w.mark_self_op(p.clone());
                                    }
                                    match file_write::save(&p, &content) {
                                        Ok(()) => invoke_handler::InvokeOutcome {
                                            state_sets: vec![(
                                                target_id,
                                                "dirty".to_string(),
                                                json!(false),
                                            )],
                                        },
                                        Err(e) => {
                                            eprintln!("[desktop-shell] save 실패: {}", e);
                                            invoke_handler::InvokeOutcome::empty()
                                        }
                                    }
                                }
                                permission::Verdict::ConfirmRequired => {
                                    // Dialog mount — desktop 자식, modal.
                                    let mut dialog = std_types::dialog(
                                        owner.clone(),
                                        "AI 저장 확인",
                                        &format!(
                                            "AI가 {}를 저장하려고 합니다 — 허용?",
                                            p.display()
                                        ),
                                        "confirm",
                                        vec!["허용".to_string(), "거부".to_string()],
                                    );
                                    dialog.parent = Some(desktop_id);
                                    add_wildcard_acl(&mut dialog);
                                    let dialog_id = dialog.id;

                                    // wire 송신 — MountMsg + Invoke SubscribeMsg.
                                    let mm = MountMsg {
                                        root_object_id: dialog_id.to_string(),
                                        tree: serde_json::to_value(&dialog)?,
                                    };
                                    stream
                                        .write_all(&encode_frame(&serde_json::to_vec(&mm)?))
                                        .await?;
                                    req_seq += 1;
                                    let sub = SubscribeMsg {
                                        subscription_id: format!("sub-runtime-{}", req_seq),
                                        target: dialog_id.to_string(),
                                        kinds: vec![EventKindFilterWire::Invoke],
                                        include_initial: false,
                                    };
                                    stream
                                        .write_all(&encode_frame(&serde_json::to_vec(&sub)?))
                                        .await?;
                                    mounted_objects.push(dialog);

                                    // PendingMap에 보관 — respond 분기가 take + save 실행.
                                    // oneshot tx는 v1에서 사용 X (동기 처리). 인프라 보존.
                                    let (tx, _rx) = tokio::sync::oneshot::channel::<String>();
                                    pending.insert(
                                        dialog_id,
                                        dialog_ops::PendingEntry {
                                            op: dialog_ops::PendingFs::Save {
                                                file_id: target_id,
                                                path: p.clone(),
                                                content,
                                            },
                                            tx,
                                        },
                                    );
                                    eprintln!(
                                        "[desktop-shell] AI save Dialog mount (file {}): 사용자 응답 대기",
                                        target_id
                                    );
                                    invoke_handler::InvokeOutcome::empty()
                                }
                            }
                        }
                        None => invoke_handler::InvokeOutcome::empty(),
                    }
                }
                // Dialog.respond — 사용자가 [허용]/[거부] 클릭. PendingMap.take → 분기에 따라
                // 적절한 fs operation 실행 + 객체 mount/destroy/state 갱신 + (Create/Rename은)
                // granted_dirs 추가 + Dialog destroy (KI-011 tombstone 패턴).
                //
                // M10 T7: PendingFs 모든 variant 처리 (Save/CreateFile/CreateFolder/DeleteFile/
                // DeleteFolder/Rename). Create*/Rename 승인 시 부모 dir grant 추가 → 같은 dir
                // 안 후속 동일 actor 작업은 confirm 생략 (per-dir TOFU).
                "respond" => {
                    let action =
                        args.get("action").and_then(|v| v.as_str()).unwrap_or("거부").to_string();
                    let pending_entry = pending.take(target_id);
                    if let Some(entry) = pending_entry {
                        if action == "허용" {
                            let now = chrono::Utc::now().timestamp_millis();
                            match entry.op {
                                dialog_ops::PendingFs::Save { path, content, .. } => {
                                    // M10 Phase 2: echo 표시 — Dialog 승인 후 fs op 직전.
                                    if let Some(w) = fs_watcher.as_ref() {
                                        w.mark_self_op(path.clone());
                                    }
                                    match file_write::save(&path, &content) {
                                        Ok(()) => {
                                            eprintln!(
                                                "[desktop-shell] AI save 승인 → {} 저장 완료",
                                                path.display()
                                            );
                                            // Save도 dir grant — 같은 dir 후속 write 자유 (ADR-036
                                            // 모델 일관). M9는 path-blind judge였어서 매번 confirm.
                                            if let Some(parent) = path.parent() {
                                                granted.insert(parent.to_path_buf());
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!(
                                                "[desktop-shell] AI save (응답 후) 실패: {}",
                                                e
                                            );
                                        }
                                    }
                                }
                                dialog_ops::PendingFs::CreateFile {
                                    folder_id,
                                    folder_path,
                                    name,
                                } => {
                                    if let Some(w) = fs_watcher.as_ref() {
                                        w.mark_self_op(folder_path.join(&name));
                                    }
                                    match folder_ops::create_file_in(
                                        &owner,
                                        &folder_path,
                                        &name,
                                        now,
                                    ) {
                                        Ok(mut new_obj) => {
                                            new_obj.parent = Some(folder_id);
                                            add_wildcard_acl(&mut new_obj);
                                            let new_id = new_obj.id;
                                            let mm = MountMsg {
                                                root_object_id: new_id.to_string(),
                                                tree: serde_json::to_value(&new_obj)?,
                                            };
                                            stream
                                                .write_all(&encode_frame(&serde_json::to_vec(&mm)?))
                                                .await?;
                                            req_seq += 1;
                                            let sub = SubscribeMsg {
                                                subscription_id: format!("sub-runtime-{}", req_seq),
                                                target: new_id.to_string(),
                                                kinds: vec![EventKindFilterWire::Invoke],
                                                include_initial: false,
                                            };
                                            stream
                                                .write_all(&encode_frame(&serde_json::to_vec(
                                                    &sub,
                                                )?))
                                                .await?;
                                            if let Some(p) = mounted_objects
                                                .iter_mut()
                                                .find(|o| o.id == folder_id)
                                            {
                                                p.children.push(new_id);
                                            }
                                            mounted_objects.push(new_obj);
                                            eprintln!(
                                                "[desktop-shell] AI create_file 승인 → {}/{}",
                                                folder_path.display(),
                                                name
                                            );
                                        }
                                        Err(e) => {
                                            eprintln!(
                                                "[desktop-shell] AI create_file (응답 후) 실패: {}",
                                                e
                                            );
                                        }
                                    }
                                    granted.insert(folder_path);
                                }
                                dialog_ops::PendingFs::CreateFolder {
                                    folder_id,
                                    folder_path,
                                    name,
                                } => {
                                    if let Some(w) = fs_watcher.as_ref() {
                                        w.mark_self_op(folder_path.join(&name));
                                    }
                                    match folder_ops::create_folder_in(
                                        &owner,
                                        &folder_path,
                                        &name,
                                        now,
                                    ) {
                                        Ok(mut new_obj) => {
                                            new_obj.parent = Some(folder_id);
                                            add_wildcard_acl(&mut new_obj);
                                            let new_id = new_obj.id;
                                            let mm = MountMsg {
                                                root_object_id: new_id.to_string(),
                                                tree: serde_json::to_value(&new_obj)?,
                                            };
                                            stream
                                                .write_all(&encode_frame(&serde_json::to_vec(&mm)?))
                                                .await?;
                                            req_seq += 1;
                                            let sub = SubscribeMsg {
                                                subscription_id: format!("sub-runtime-{}", req_seq),
                                                target: new_id.to_string(),
                                                kinds: vec![EventKindFilterWire::Invoke],
                                                include_initial: false,
                                            };
                                            stream
                                                .write_all(&encode_frame(&serde_json::to_vec(
                                                    &sub,
                                                )?))
                                                .await?;
                                            if let Some(p) = mounted_objects
                                                .iter_mut()
                                                .find(|o| o.id == folder_id)
                                            {
                                                p.children.push(new_id);
                                            }
                                            mounted_objects.push(new_obj);
                                            eprintln!(
                                                "[desktop-shell] AI create_folder 승인 → {}/{}",
                                                folder_path.display(),
                                                name
                                            );
                                        }
                                        Err(e) => {
                                            eprintln!(
                                                "[desktop-shell] AI create_folder (응답 후) 실패: {}",
                                                e
                                            );
                                        }
                                    }
                                    granted.insert(folder_path);
                                }
                                dialog_ops::PendingFs::DeleteFile { file_id, path } => {
                                    if let Some(w) = fs_watcher.as_ref() {
                                        w.mark_self_op(path.clone());
                                    }
                                    match file_ops::delete_file(&path) {
                                        Ok(()) => {
                                            if let Some(o) =
                                                mounted_objects.iter_mut().find(|o| o.id == file_id)
                                            {
                                                o.state.insert("destroyed".into(), json!(true));
                                            }
                                            eprintln!(
                                                "[desktop-shell] AI delete_file 승인 → {}",
                                                path.display()
                                            );
                                        }
                                        Err(e) => {
                                            eprintln!(
                                                "[desktop-shell] AI delete_file (응답 후) 실패: {}",
                                                e
                                            );
                                        }
                                    }
                                    // delete는 grant 안 함 — 다음 delete도 항상 confirm 정책.
                                }
                                dialog_ops::PendingFs::DeleteFolder {
                                    folder_id,
                                    path,
                                    recursive,
                                } => {
                                    if let Some(w) = fs_watcher.as_ref() {
                                        w.mark_self_op(path.clone());
                                    }
                                    match folder_ops::delete_folder(&path, recursive) {
                                        Ok(()) => {
                                            if let Some(o) = mounted_objects
                                                .iter_mut()
                                                .find(|o| o.id == folder_id)
                                            {
                                                o.state.insert("destroyed".into(), json!(true));
                                            }
                                            eprintln!(
                                                "[desktop-shell] AI delete_folder 승인 → {}",
                                                path.display()
                                            );
                                        }
                                        Err(e) => {
                                            eprintln!(
                                                "[desktop-shell] AI delete_folder (응답 후) 실패: {}",
                                                e
                                            );
                                        }
                                    }
                                }
                                dialog_ops::PendingFs::Rename {
                                    target_id: tid,
                                    path,
                                    new_name,
                                    is_folder,
                                } => {
                                    // M10 Phase 2: rename = Remove(old) + Create(new) 두 이벤트.
                                    if let Some(w) = fs_watcher.as_ref() {
                                        w.mark_self_op(path.clone());
                                        if let Some(parent) = path.parent() {
                                            w.mark_self_op(parent.join(&new_name));
                                        }
                                    }
                                    let result = if is_folder {
                                        folder_ops::rename_folder(&path, &new_name)
                                    } else {
                                        file_ops::rename_file(&path, &new_name)
                                    };
                                    match result {
                                        Ok(new_path) => {
                                            if let Some(o) =
                                                mounted_objects.iter_mut().find(|o| o.id == tid)
                                            {
                                                o.props.insert("name".into(), json!(&new_name));
                                                o.props.insert(
                                                    "path".into(),
                                                    json!(new_path.to_string_lossy()),
                                                );
                                            }
                                            if let Some(parent) = new_path.parent() {
                                                granted.insert(parent.to_path_buf());
                                            }
                                            eprintln!(
                                                "[desktop-shell] AI rename 승인 → {}",
                                                new_path.display()
                                            );
                                        }
                                        Err(e) => {
                                            eprintln!(
                                                "[desktop-shell] AI rename (응답 후) 실패: {}",
                                                e
                                            );
                                        }
                                    }
                                }
                            }
                        } else {
                            eprintln!("[desktop-shell] AI 요청 거부됨 (action={})", action);
                        }
                        // 인프라 보존 — tx는 사용 X (동기 처리), 명시적 drop으로 의도 표시.
                        drop(entry.tx);
                    }
                    // Dialog destroy — mounted_objects에서 제거 + SetState destroyed=true.
                    // (close 분기와 같은 KI-011 우회 — proto에 DestroyMsg 없음.)
                    let dialog_id = target_id;
                    mounted_objects.retain(|o| o.id != dialog_id);
                    if let Some(d) = mounted_objects.iter_mut().find(|o| o.id == desktop_id) {
                        d.children.retain(|c| *c != dialog_id);
                    }
                    invoke_handler::InvokeOutcome {
                        state_sets: vec![(dialog_id, "destroyed".to_string(), json!(true))],
                    }
                }
                // Window.close_confirm — close button 클릭. dirty=false면 즉시 destroy (기존
                // close와 동일). dirty=true면 v1 단순화: close 거부 + eprintln 안내. 사용자는
                // Ctrl+S로 저장 후 다시 [x] 클릭 필요. 3-버튼 Dialog 흐름은 v2 (spec 시나리오 B).
                "close_confirm" => {
                    let dirty = mounted_objects
                        .iter()
                        .find(|o| o.id == target_id)
                        .and_then(|w| w.state.get("dirty").and_then(|v| v.as_bool()))
                        .unwrap_or(false);
                    if !dirty {
                        let close_id = target_id;
                        mounted_objects.retain(|o| o.id != close_id);
                        if let Some(d) = mounted_objects.iter_mut().find(|o| o.id == desktop_id) {
                            d.children.retain(|c| *c != close_id);
                        }
                        invoke_handler::InvokeOutcome {
                            state_sets: vec![(close_id, "destroyed".to_string(), json!(true))],
                        }
                    } else {
                        // v1: 3-버튼 Dialog 흐름은 v2 — PendingFs::Save variant가 (file_id,
                        // content) 전용이라 close 정보 보관이 어색. 일단 close 거부 + 안내.
                        eprintln!(
                            "[desktop-shell] dirty Window {} 닫기 거부 — Ctrl+S 후 다시 [x] 클릭",
                            target_id
                        );
                        invoke_handler::InvokeOutcome::empty()
                    }
                }
                // File.read — AI가 *fresh content + size*를 동적으로 조회. lazy_mount 시점
                // 의 stale state 대신 fs::read를 새로 호출해 SetState로 broadcast. AI는
                // invoke 후 subscribe + drain 또는 get_object로 fresh state 인지.
                "read" => {
                    let path_opt = mounted_objects
                        .iter()
                        .find(|o| o.id == target_id)
                        .filter(|o| o.type_uri.as_str() == "aios.std/File@1")
                        .and_then(|o| o.props.get("path").and_then(|v| v.as_str()))
                        .map(std::path::PathBuf::from);
                    match path_opt {
                        Some(path) => match std::fs::read_to_string(&path) {
                            Ok(content) => {
                                let size = content.len() as i64;
                                if let Some(o) =
                                    mounted_objects.iter_mut().find(|o| o.id == target_id)
                                {
                                    o.state.insert("content".into(), json!(&content));
                                    o.state.insert("size".into(), json!(size));
                                }
                                eprintln!(
                                    "[desktop-shell] File.read OK ({} bytes) → {}",
                                    size,
                                    path.display()
                                );
                                invoke_handler::InvokeOutcome {
                                    state_sets: vec![
                                        (target_id, "content".to_string(), json!(content)),
                                        (target_id, "size".to_string(), json!(size)),
                                    ],
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "[desktop-shell] File.read 실패 {}: {}",
                                    path.display(),
                                    e
                                );
                                invoke_handler::InvokeOutcome::empty()
                            }
                        },
                        None => invoke_handler::InvokeOutcome::empty(),
                    }
                }
                // Folder.list — AI가 *expand되지 않은* 폴더의 children을 동적으로 mount + 인지.
                // 사용자가 FileTree로 안 열어둬도 AI는 list 호출로 즉시 자식 트리 접근.
                "list" => {
                    if mounted_objects
                        .iter()
                        .find(|o| o.id == target_id)
                        .map(|o| o.type_uri.as_str() == "aios.std/Folder@1")
                        .unwrap_or(false)
                    {
                        // 기존 lazy_expand 흐름 재사용 — 직계 children mount + subscribe.
                        lazy_expand_if_needed(
                            &mut stream,
                            &mut mounted_objects,
                            &owner,
                            target_id,
                            &mut req_seq,
                            fs_watcher.as_mut(),
                        )
                        .await?;
                        let count = mounted_objects
                            .iter()
                            .find(|o| o.id == target_id)
                            .map(|o| o.children.len())
                            .unwrap_or(0);
                        eprintln!("[desktop-shell] Folder.list → {} children", count);
                        invoke_handler::InvokeOutcome {
                            state_sets: vec![(target_id, "child_count".to_string(), json!(count))],
                        }
                    } else {
                        invoke_handler::InvokeOutcome::empty()
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
