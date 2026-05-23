//! invoke handler 분리 — main.rs의 `match method` 큰 블록을 카테고리별 모듈로.
//!
//! 각 handler 함수는 *공통 state* (stream/mounted_objects/owner/ids/granted/pending/
//! fs_watcher/req_seq 등)를 매개변수로 받아 `InvokeOutcome`을 반환한다. main.rs는
//! 얇은 dispatch만 유지 — 분기·wire 흐름·SetState 송신 패턴은 *동작 동일*.
//!
//! 모듈 카테고리:
//! - [`fs_methods`] — save_to_file, save, create_file, create_folder, delete, rename, read, list
//! - [`explorer_methods`] — expand, collapse, navigate_to, navigate_up, open_file
//! - [`window_methods`] — move, resize, focus, close, close_confirm
//! - [`cli_methods`] — clear, append_line (submit_input은 ai_session/awaiting 등이 얽혀 main에 잔존)
//! - [`dialog_methods`] — respond
//! - [`external_methods`] — read_external, write_external
//!
//! 공통 helper (`add_wildcard_acl`, `parse_object_id`, `lookup_file_path`,
//! `lookup_folder_path`, `find_object_by_path`, `lazy_expand_if_needed`)는 본 모듈에
//! pub로 노출해 각 sub-module이 재사용.

use std::path::{Path, PathBuf};

use geulos_core::{AclEffect, AclEntry, ActorId, ActorPattern, MethodPattern, Object, ObjectId};
use geulos_proto::{encode_frame, EventKindFilterWire, MountMsg, SubscribeMsg};
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::cli_handler::SpecialAction;
use crate::fs_watcher::FsWatcher;
use crate::invoke_handler;
use crate::{explorer_ops, lazy_mount};

pub mod cli_methods;
pub mod dialog_methods;
pub mod explorer_methods;
pub mod external_methods;
pub mod fs_methods;
pub mod window_methods;

/// M8 동안 유지 — read-only로 자연 보호. M9 권한 다이얼로그 마일스톤에서
/// 매니페스트 기반 권한으로 교체 예정. 추적: KI-001 / KI-016 (`docs/known-issues.md`).
pub fn add_wildcard_acl(obj: &mut Object) {
    obj.acl.push(AclEntry {
        actor: ActorPattern::Wildcard,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
}

/// 문자열에서 ObjectId 파싱 (serde_json 경유 — core가 FromStr 미구현).
pub fn parse_object_id(s: &str) -> Option<ObjectId> {
    serde_json::from_str(&format!("\"{}\"", s)).ok()
}

/// 주어진 ID의 Folder 객체에서 `path` prop을 꺼낸다. 없으면 None.
///
/// lazy_expand_if_needed에서 폴더 디스크 경로를 알아낼 때 사용.
pub fn lookup_folder_path(objects: &[Object], id: ObjectId) -> Option<PathBuf> {
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
pub fn lookup_file_path(objects: &[Object], id: ObjectId) -> Option<PathBuf> {
    let obj = objects.iter().find(|o| o.id == id)?;
    if obj.type_uri.as_str() != "aios.std/File@1" {
        return None;
    }
    obj.props.get("path").and_then(|v| v.as_str()).map(PathBuf::from)
}

/// 주어진 path를 가진 mounted 객체 (File@1 또는 Folder@1)의 ObjectId + parent ObjectId를
/// 반환. M10 Phase 2 — fs watcher 이벤트의 path를 기존 객체에 매핑할 때 사용.
///
/// path 비교는 `Path::new`로 normalize한 직접 비교. Windows의 short/long path 차이는 v2에
/// canonicalize 검토 (v1은 lazy_mount가 입력한 path 그대로 보관해 *대부분* 일치).
pub fn find_object_by_path(
    objects: &[Object],
    target: &Path,
) -> Option<(ObjectId, Option<ObjectId>)> {
    objects.iter().find_map(|o| {
        let p = o.props.get("path").and_then(|v| v.as_str())?;
        if Path::new(p) == target {
            Some((o.id, o.parent))
        } else {
            None
        }
    })
}

/// CLI lines 히스토리 최대 보관 라인 수 (오래된 라인은 잘림).
pub const CLI_LINES_CAP: usize = 1000;

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
pub fn handle_cli_outcome(
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

/// 폴더 lazy expand — children이 비어있으면 lazy_mount + mount/subscribe 처리.
///
/// 부모 Folder.children도 갱신. 새 자식 id들의 mount/subscribe wire 메시지를 전송.
/// 호출 후 부모는 children 갱신, 새 자식 객체들이 `mounted_objects`에 추가됨.
///
/// Borrow 노트: stream/mounted_objects/req_seq 모두 mutable로 받지만 매개변수가 서로
/// 독립이라 borrow checker는 만족. mounted_objects를 push할 때 부모 갱신은 push 이후
/// 별도 `iter_mut().find` 로 분리되어 있어 동시 mutable borrow가 발생하지 않는다.
pub async fn lazy_expand_if_needed(
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    folder_id: ObjectId,
    req_seq: &mut u64,
    fs_watcher: Option<&mut FsWatcher>,
) -> Result<(), Box<dyn std::error::Error>> {
    // needs_expand=false (children 이미 mount됨)이어도 *watcher 등록은 보장* — 두 번째
    // navigate_to/expand 시점에 watcher 누락되면 외부 변경이 그 폴더에서 감지 안 됨
    // (사용자 보고 — 우측 navigate_to 후 빈 폴더가 *영원히* 빈 채로 남음).
    let folder_path = match lookup_folder_path(mounted_objects, folder_id) {
        Some(p) => p,
        None => return Ok(()),
    };
    if !explorer_ops::needs_expand(mounted_objects, folder_id) {
        // mount 흐름은 skip하되 watcher.watch는 *반드시* 한 번 더 시도. notify-rs는
        // 같은 path 중복 watch를 silent OK 처리.
        if let Some(watcher) = fs_watcher {
            let _ = watcher.watch(&folder_path);
        }
        return Ok(());
    }
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
