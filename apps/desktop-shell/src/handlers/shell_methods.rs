//! Desktop/TopBar/Dock/DesktopIcon 크롬 메서드 핸들러.
//! 모든 동작은 여기로 — 컴포지터 클릭과 AI Invoke가 동일하게 도달 (AI=사용자2).

use geulos_core::{std_types, ActorId, Object, ObjectId};
use geulos_proto::{encode_frame, EventKindFilterWire, MountMsg, SubscribeMsg};
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::applauncher::{resolve_app, AppKind};
use crate::drives;
use crate::handlers::{add_fs_object_acl, add_ui_object_acl};
use crate::invoke_handler::InvokeOutcome;
use crate::window_ops;

/// launch 결과 — 호출자(invoke 루프)가 mount/SetState/focus로 반영.
#[derive(Debug)]
pub enum LaunchOutcome {
    OpenFileManager,             // M2: FileManager 창 mount
    AlreadyOpenFocus(ObjectId),  // 이미 열린 창 focus
    Unknown(String),             // 알 수 없는 app — no-op + 로그
}

/// app_id로 무엇을 할지 결정. 이미 열린 FileManager 있으면 focus.
pub fn handle_launch(app_id: &str, mounted: &[Object]) -> LaunchOutcome {
    match resolve_app(app_id) {
        Some(AppKind::FileManager) => {
            if let Some(existing) =
                mounted.iter().find(|o| o.type_uri.as_str() == "aios.builtin/FileManager@1")
            {
                LaunchOutcome::AlreadyOpenFocus(existing.id)
            } else {
                LaunchOutcome::OpenFileManager
            }
        }
        None => LaunchOutcome::Unknown(app_id.to_string()),
    }
}

/// Desktop.launch(app) — app_id를 args에서 꺼낸다 (resolver).
pub fn desktop_launch_app_id(args: &serde_json::Value) -> String {
    args.get("app").and_then(|v| v.as_str()).unwrap_or("").to_string()
}

/// Dock.launch(item_id) — Dock.state.items에서 item_id에 해당하는 `app` 필드를 꺼낸다.
///
/// 배열에서 찾지 못하면 item_id를 app_id로 fallback (Dock items는 app id를 id로 사용).
pub fn dock_launch_app_id(target_id: ObjectId, args: &serde_json::Value, mounted: &[Object]) -> String {
    let item_id = args.get("item_id").and_then(|v| v.as_str()).unwrap_or("");
    mounted
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|dock| dock.state.get("items"))
        .and_then(|v| v.as_array())
        .and_then(|items| {
            items.iter().find(|item| item.get("id").and_then(|id| id.as_str()) == Some(item_id))
        })
        .and_then(|item| item.get("app").and_then(|a| a.as_str()))
        .map(|s| s.to_string())
        // fallback: item_id를 그대로 app_id로 사용.
        .unwrap_or_else(|| item_id.to_string())
}

/// DesktopIcon.open() — icon 객체의 props["app"]을 app_id로 사용.
pub fn desktop_icon_app_id(target_id: ObjectId, mounted: &[Object]) -> String {
    mounted
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|icon| icon.props.get("app"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// app launch dispatch — resolve된 `app_id`를 보고 FileManager 창을 mount하거나, 이미
/// 열린 창을 focus한다.
///
/// **runtime mount 메커니즘 (Explorer.open_file 패턴 미러):** InvokeOutcome는 state_sets
/// 만 운반하므로 *새 객체 mount는 핸들러가 직접 stream에 MountMsg+SubscribeMsg 프레임을
/// 쓰고 mounted_objects/Desktop.children을 in-place 갱신*한다. FileManager 창은 단일
/// Window와 달리 FileManager + FileTree + Explorer + 드라이브 Folder 여러 객체를 한 번에
/// mount해야 하므로 각 객체마다 frame을 보낸다.
///
/// 반환 InvokeOutcome는 focus batch만 — main loop의 broadcast 루프가 처리.
pub async fn handle_launch_app(
    app_id: &str,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    let outcome = handle_launch(app_id, mounted_objects);
    println!("[desktop-shell] launch app_id={:?} -> {:?}", app_id, outcome);
    match outcome {
        LaunchOutcome::OpenFileManager => {
            open_file_manager_window(stream, mounted_objects, owner, desktop_id, req_seq).await
        }
        LaunchOutcome::AlreadyOpenFocus(fm_id) => {
            // 이미 열린 FileManager를 focus + z 최상위. 다른 floating(Window/ConsoleWindow/
            // FileManager)은 focused=false batch.
            Ok(focus_file_manager(fm_id, mounted_objects))
        }
        LaunchOutcome::Unknown(_) => Ok(InvokeOutcome::empty()),
    }
}

/// FileManager 창 + FileTree + Explorer + 드라이브 Folder들을 런타임 mount.
///
/// 트리: Desktop > FileManager > [FileTree > [드라이브 Folder...], Explorer].
/// 각 객체를 MountMsg로 송신하고 *Invoke 가능한* 객체(FileManager/FileTree/Explorer/Folder)
/// 는 SubscribeMsg로 구독한다. (FileManager는 move/resize/focus/close, FileTree는 expand/
/// collapse, Explorer는 navigate/open_file, Folder는 fs 메서드.)
async fn open_file_manager_window(
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    let now_ms = chrono::Utc::now().timestamp_millis();

    // 위치/z/크기 — 기존 Window cascade 헬퍼 재사용.
    let pos = window_ops::next_window_position(mounted_objects, (120, 70));
    let new_z = window_ops::max_z(mounted_objects) + 1;
    let mut fm = std_types::file_manager(owner.clone(), pos.0, pos.1, 720, 480, new_z);
    fm.parent = Some(desktop_id);
    fm.set_state("focused", json!(true));

    let mut ft = std_types::file_tree(owner.clone(), "/");
    ft.parent = Some(fm.id);
    let mut ex = std_types::explorer(owner.clone());
    ex.parent = Some(fm.id);

    // 드라이브 Folder들 — FileTree 자식. children=[]로 lazy expand (startup에서 이동).
    let drive_paths = drives::list_drives();
    let mut drive_folders: Vec<Object> = drive_paths
        .iter()
        .map(|p| {
            let mut f = std_types::folder(
                owner.clone(),
                p.to_string_lossy().as_ref(),
                p.to_string_lossy().as_ref(),
                now_ms,
            );
            f.parent = Some(ft.id);
            f
        })
        .collect();
    ft.children = drive_folders.iter().map(|f| f.id).collect();
    fm.children = vec![ft.id, ex.id];

    // ACL — startup/ open_file과 동일 헬퍼. FileManager/FileTree/Explorer=UI, Folder=fs.
    add_ui_object_acl(&mut fm);
    add_ui_object_acl(&mut ft);
    add_ui_object_acl(&mut ex);
    for f in &mut drive_folders {
        add_fs_object_acl(f);
    }

    let fm_id = fm.id;

    // 기존 다른 모든 floating은 focused=false (open_file과 동일 batch 정책).
    let mut outs = vec![];
    for o in mounted_objects.iter_mut() {
        if matches!(
            o.type_uri.as_str(),
            "aios.builtin/Window@1" | "aios.builtin/ConsoleWindow@1" | "aios.builtin/FileManager@1"
        ) {
            o.state.insert("focused".into(), json!(false));
            outs.push((o.id, "focused".to_string(), json!(false)));
        }
    }

    // mount + subscribe — 트리 순서대로(부모 먼저). Folder만 fs 메서드 invoke가 흔하지만
    // FileTree/Explorer/FileManager도 모두 invoke 대상이므로 전부 구독.
    let mut to_mount: Vec<Object> = vec![fm, ft, ex];
    to_mount.extend(drive_folders);
    for obj in &to_mount {
        let mm = MountMsg {
            root_object_id: obj.id.to_string(),
            tree: serde_json::to_value(obj)?,
        };
        stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
        *req_seq += 1;
        let sub = SubscribeMsg {
            subscription_id: format!("sub-runtime-{}", req_seq),
            target: obj.id.to_string(),
            kinds: vec![EventKindFilterWire::Invoke],
            include_initial: false,
        };
        stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
    }

    // Desktop.children에 FileManager 추가 (open_file의 Window attach와 동일).
    if let Some(d) = mounted_objects.iter_mut().find(|o| o.id == desktop_id) {
        d.children.push(fm_id);
    }
    mounted_objects.extend(to_mount);

    Ok(InvokeOutcome { state_sets: outs })
}

/// 이미 열린 FileManager를 focus + z 최상위. 다른 floating은 focused=false batch.
///
/// window_methods::handle_focus와 동일 정책 — Window/ConsoleWindow/FileManager가 같은
/// z-space를 공유해 서로 앞으로 올라온다.
pub fn focus_file_manager(fm_id: ObjectId, mounted_objects: &mut [Object]) -> InvokeOutcome {
    let new_z = window_ops::max_z(mounted_objects) + 1;
    let mut outs = vec![];
    for o in mounted_objects.iter_mut() {
        let is_floating = matches!(
            o.type_uri.as_str(),
            "aios.builtin/Window@1" | "aios.builtin/ConsoleWindow@1" | "aios.builtin/FileManager@1"
        );
        if is_floating {
            let is_target = o.id == fm_id;
            o.state.insert("focused".into(), json!(is_target));
            outs.push((o.id, "focused".to_string(), json!(is_target)));
            if is_target {
                o.state.insert("z".into(), json!(new_z));
                outs.push((o.id, "z".to_string(), json!(new_z)));
            }
        }
    }
    InvokeOutcome { state_sets: outs }
}

/// TopBar.activate(item_id: String) — M1: item_id 로그만 (no-op).
pub fn handle_top_bar_activate(
    target_id: ObjectId,
    args: &serde_json::Value,
) -> InvokeOutcome {
    let item_id = args.get("item_id").and_then(|v| v.as_str()).unwrap_or("");
    println!(
        "[desktop-shell] TopBar({}) activate item_id={:?} (M1 no-op)",
        target_id, item_id
    );
    InvokeOutcome::empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_app_returns_unknown() {
        let out = handle_launch("nope", &[]);
        assert!(matches!(out, LaunchOutcome::Unknown(_)));
    }

    #[test]
    fn file_manager_when_none_open() {
        let out = handle_launch("file_manager", &[]);
        assert!(matches!(out, LaunchOutcome::OpenFileManager));
    }

    #[test]
    fn file_manager_dedup_when_already_open() {
        use geulos_core::std_types;
        let owner = ActorId::local_user();
        let fm = std_types::file_manager(owner, 0, 0, 700, 460, 1);
        let fm_id = fm.id;
        let mounted = vec![fm];
        let out = handle_launch("file_manager", &mounted);
        match out {
            LaunchOutcome::AlreadyOpenFocus(id) => assert_eq!(id, fm_id),
            other => panic!("expected AlreadyOpenFocus, got {:?}", other),
        }
    }

    #[test]
    fn desktop_icon_app_id_empty_when_no_app_prop() {
        use geulos_core::std_types;
        let owner = ActorId::local_user();
        // app="" → desktop_icon_app_id가 "" 반환 → resolve_app("") = None → Unknown.
        let icon = std_types::desktop_icon(owner, "", "라벨", "folder", 0, 0);
        let icon_id = icon.id;
        let mounted = vec![icon];
        let app_id = desktop_icon_app_id(icon_id, &mounted);
        assert_eq!(app_id, "");
        assert!(matches!(handle_launch(&app_id, &mounted), LaunchOutcome::Unknown(_)));
    }

    #[test]
    fn dock_launch_falls_back_to_item_id() {
        use geulos_core::std_types;
        let owner = ActorId::local_user();
        let dock = std_types::dock(owner);
        let dock_id = dock.id;
        let mounted = vec![dock];
        // items 비어있음 → item_id를 app_id로 fallback.
        let args = json!({ "item_id": "file_manager" });
        assert_eq!(dock_launch_app_id(dock_id, &args, &mounted), "file_manager");
    }

    #[test]
    fn dock_launch_reads_app_from_items() {
        use geulos_core::std_types;
        let owner = ActorId::local_user();
        let mut dock = std_types::dock(owner);
        dock.set_state(
            "items",
            json!([{ "id": "fm", "app": "file_manager", "label": "x", "icon": "folder" }]),
        );
        let dock_id = dock.id;
        let mounted = vec![dock];
        let args = json!({ "item_id": "fm" });
        assert_eq!(dock_launch_app_id(dock_id, &args, &mounted), "file_manager");
    }

    #[test]
    fn focus_file_manager_batches_focused() {
        use geulos_core::std_types;
        let owner = ActorId::local_user();
        let mut fm1 = std_types::file_manager(owner.clone(), 0, 0, 700, 460, 1);
        fm1.set_state("focused", json!(false));
        let fm1_id = fm1.id;
        let mut fm2 = std_types::file_manager(owner, 0, 0, 700, 460, 2);
        fm2.set_state("focused", json!(true));
        let fm2_id = fm2.id;
        let mut mounted = vec![fm1, fm2];
        let out = focus_file_manager(fm1_id, &mut mounted);
        // fm1 focused=true + z, fm2 focused=false.
        let fm1_focused = mounted.iter().find(|o| o.id == fm1_id).unwrap();
        let fm2_focused = mounted.iter().find(|o| o.id == fm2_id).unwrap();
        assert_eq!(fm1_focused.state.get("focused"), Some(&json!(true)));
        assert_eq!(fm2_focused.state.get("focused"), Some(&json!(false)));
        // out에 focused set이 최소 2개 (fm1 true, fm2 false) + fm1 z.
        assert!(out.state_sets.iter().any(|(id, k, v)| *id == fm1_id && k == "z" && v.is_i64()));
    }
}
