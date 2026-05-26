//! Explorer/FileTree/Window mount 관련 method handler — expand/collapse/navigate_to/
//! navigate_up/open_file (T6/T8.7/T8.10).
//!
//! - `expand`/`collapse` — FileTree 트리의 펼침 상태. lazy_expand_if_needed로 자식 mount.
//! - `navigate_to`/`navigate_up` — Explorer 컬럼 뷰의 active_folder 갱신.
//! - `open_file` — Explorer 더블클릭. 같은 파일 Window 있으면 focus, 없으면 새 Window
//!   mount + file 본문 read + Window 자체에 invoke subscribe.

use geulos_core::{ActorId, Object, ObjectId};
use geulos_proto::{encode_frame, EventKindFilterWire, MountMsg, SubscribeMsg};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::fs_watcher::FsWatcher;
use crate::handlers::{add_ui_object_acl, lazy_expand_if_needed, parse_object_id};
use crate::invoke_handler::{
    self, handle_file_tree_collapse, handle_file_tree_expand, InvokeOutcome,
};
use crate::{explorer_ops, file_read, window_ops};

/// FileTree.expand(id) — 자식 lazy mount + tracked_expanded 갱신.
#[allow(clippy::too_many_arguments)]
pub async fn handle_expand(
    target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    tracked_expanded: &mut Vec<ObjectId>,
    fs_watcher: Option<&mut FsWatcher>,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    let fid_str = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
    Ok(match parse_object_id(fid_str) {
        Some(fid) => {
            lazy_expand_if_needed(stream, mounted_objects, owner, fid, req_seq, fs_watcher).await?;
            let outcome = handle_file_tree_expand(target_id, tracked_expanded, fid);
            if !tracked_expanded.contains(&fid) {
                tracked_expanded.push(fid);
            }
            outcome
        }
        None => InvokeOutcome::empty(),
    })
}

/// FileTree.collapse(id) — tracked_expanded 제거.
pub fn handle_collapse(
    target_id: ObjectId,
    args: &Value,
    tracked_expanded: &mut Vec<ObjectId>,
) -> InvokeOutcome {
    let fid_str = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
    match parse_object_id(fid_str) {
        Some(fid) => {
            let outcome = handle_file_tree_collapse(target_id, tracked_expanded, fid);
            tracked_expanded.retain(|x| *x != fid);
            outcome
        }
        None => InvokeOutcome::empty(),
    }
}

/// Explorer.navigate_to(folder_id) — 폴더 lazy expand 후 active_folder 갱신.
///
/// 새 폴더 진입 시 *Explorer.state.scroll_y = 0* 도 함께 broadcast — 이전 폴더의 스크롤
/// 위치가 유지되면 새 폴더의 첫 자식이 화면 위로 사라지는 버그 (사용자 보고: "스크롤이
/// 내려가서 파일이 화면 상단 너머에 존재해 보이지 않음") 방지.
#[allow(clippy::too_many_arguments)]
pub async fn handle_navigate_to(
    target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    fs_watcher: Option<&mut FsWatcher>,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    let fid_str = args.get("folder_id").and_then(|v| v.as_str()).unwrap_or("");
    Ok(match parse_object_id(fid_str) {
        Some(fid) => {
            lazy_expand_if_needed(stream, mounted_objects, owner, fid, req_seq, fs_watcher).await?;
            // local mounted_objects 동기 갱신 — 다음 navigate_up이 *server StateSet
            // 왕복 전에* 호출되면 stale active_folder를 읽어 잘못된 parent로 이동하는
            // race 방지. scroll_y도 함께 reset (이전 폴더 스크롤 위치 잔존 버그).
            if let Some(ex) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
                ex.state.insert("scroll_y".to_string(), json!(0));
                ex.state.insert("active_folder".to_string(), json!(fid.to_string()));
            }
            let mut outcome = explorer_ops::handle_navigate_to(target_id, fid);
            outcome.state_sets.push((target_id, "scroll_y".to_string(), json!(0)));
            outcome
        }
        None => InvokeOutcome::empty(),
    })
}

/// Explorer.navigate_up — 상단 "/" 행 클릭. 현재 active_folder의 parent로 이동.
/// parent 없으면 빈 string으로 reset → 드라이브 일람 화면.
/// scroll_y=0도 함께 reset (handle_navigate_to와 동일 정책).
pub fn handle_navigate_up(target_id: ObjectId, mounted_objects: &mut [Object]) -> InvokeOutcome {
    let current_active = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|ex| ex.state.get("active_folder").and_then(|v| v.as_str()))
        .and_then(parse_object_id);
    let mut outcome = explorer_ops::handle_navigate_up(target_id, mounted_objects, current_active);
    // 새 active_folder도 local 동기 갱신 (navigate_to와 동일 race 방어). 빠른 연속 호출 시
    // server StateSet 왕복 전이라도 stale 값을 읽지 않도록.
    if let Some(new_active_val) = outcome
        .state_sets
        .iter()
        .find_map(|(id, key, v)| (*id == target_id && key == "active_folder").then(|| v.clone()))
    {
        if let Some(ex) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
            ex.state.insert("scroll_y".to_string(), json!(0));
            ex.state.insert("active_folder".to_string(), new_active_val);
        }
    }
    outcome.state_sets.push((target_id, "scroll_y".to_string(), json!(0)));
    outcome
}

/// Explorer.open_file(file_id) — 같은 파일을 이미 연 Window가 있으면 *그것만 focus +
/// z 최상위*. 없으면 새 Window를 Desktop 자식으로 mount하고 그 Window에 invoke
/// subscribe. 어느 분기든 *focused 갱신은 모든 Window를 대상으로* batch — 정확히
/// 한 Window만 focused=true가 되도록.
#[allow(clippy::too_many_arguments)]
pub async fn handle_open_file(
    _target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    let fid_str = args.get("file_id").and_then(|v| v.as_str()).unwrap_or("");
    let file_id = match parse_object_id(fid_str) {
        Some(id) => id,
        None => return Ok(invoke_handler::InvokeOutcome::empty()),
    };
    if let Some(existing_window_id) = window_ops::find_window_for_file(mounted_objects, file_id) {
        // 중복 — 새 mount 없이 focus + z 최상위만.
        let new_z = window_ops::max_z(mounted_objects) + 1;
        let mut outs = vec![];
        for o in mounted_objects.iter_mut() {
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
        return Ok(InvokeOutcome { state_sets: outs });
    }
    // 새 Window mount.
    let title = mounted_objects
        .iter()
        .find(|o| o.id == file_id)
        .and_then(|f| f.props.get("name").and_then(|v| v.as_str()))
        .unwrap_or("(파일)")
        .to_string();
    let pos = window_ops::next_window_position(mounted_objects, (300, 200));
    let new_z = window_ops::max_z(mounted_objects) + 1;
    let mut new_window =
        window_ops::build_new_window(owner, desktop_id, file_id, &title, pos, (600, 400), new_z);
    add_ui_object_acl(&mut new_window);

    // M8 part 2 (ADR-033): Window mount 시점에 file 본문 read.
    // File 객체의 props.path / props.mime를 lookup해 file_read에
    // 위임. 결과를 Window.state.content / content_too_large에 채움.
    // File 객체가 없거나 path/mime이 비어있어도 file_read가 graceful
    // 안내 메시지 반환 — panic X.
    let (file_path, mime) = {
        let f = mounted_objects.iter().find(|o| o.id == file_id);
        match f {
            Some(file) => {
                let p = file.props.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let m = file
                    .props
                    .get("mime")
                    .and_then(|v| v.as_str())
                    .unwrap_or("application/octet-stream");
                (std::path::PathBuf::from(p), m.to_string())
            }
            None => (std::path::PathBuf::new(), "application/octet-stream".to_string()),
        }
    };
    let fc = file_read::read_file_for_window(&file_path, &mime);
    new_window.state.insert("content".into(), serde_json::json!(fc.text));
    new_window.state.insert("content_too_large".into(), serde_json::json!(fc.too_large));

    let new_id = new_window.id;
    // 기존 다른 모든 Window는 focused=false.
    let mut outs = vec![];
    for o in mounted_objects.iter_mut() {
        if o.type_uri.as_str() == "aios.builtin/Window@1" {
            o.state.insert("focused".into(), json!(false));
            outs.push((o.id, "focused".to_string(), json!(false)));
        }
    }
    // Window mount 송신.
    let mm =
        MountMsg { root_object_id: new_id.to_string(), tree: serde_json::to_value(&new_window)? };
    stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
    // Window 자체에 invoke subscribe — move/resize/focus/close (T8.10).
    *req_seq += 1;
    let sub = SubscribeMsg {
        subscription_id: format!("sub-runtime-{}", req_seq),
        target: new_id.to_string(),
        kinds: vec![EventKindFilterWire::Invoke],
        include_initial: false,
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
    // mounted_objects + desktop.children 갱신.
    if let Some(d) = mounted_objects.iter_mut().find(|o| o.id == desktop_id) {
        d.children.push(new_id);
    }
    mounted_objects.push(new_window);
    Ok(InvokeOutcome { state_sets: outs })
}
