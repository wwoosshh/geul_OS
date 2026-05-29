//! Desktop/TopBar/Dock/DesktopIcon 크롬 메서드 핸들러.
//! 모든 동작은 여기로 — 컴포지터 클릭과 AI Invoke가 동일하게 도달 (AI=사용자2).

use geulos_core::{Object, ObjectId};

use crate::applauncher::{resolve_app, AppKind};
use crate::invoke_handler::InvokeOutcome;

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

/// Desktop.launch(app: String) — app_id로 앱 실행 (M1: resolve + log, M2: mount).
pub fn handle_desktop_launch(
    target_id: ObjectId,
    args: &serde_json::Value,
    mounted: &[Object],
) -> InvokeOutcome {
    let app_id = args.get("app").and_then(|v| v.as_str()).unwrap_or("");
    let outcome = handle_launch(app_id, mounted);
    println!(
        "[desktop-shell] Desktop({}) launch {:?} -> {:?}",
        target_id, app_id, outcome
    );
    // M1: 로그만. M2에서 mount/focus 반영.
    InvokeOutcome::empty()
}

/// Dock.launch(item_id: String) — Dock item의 app을 실행.
///
/// item_id를 Dock.state.items 배열에서 찾아 그 `app` 필드를 app_id로 사용.
/// 배열에서 찾지 못하면 item_id를 app_id로 fallback (Dock items는 app id를 id로 사용).
pub fn handle_dock_launch(
    target_id: ObjectId,
    args: &serde_json::Value,
    mounted: &[Object],
) -> InvokeOutcome {
    let item_id = args.get("item_id").and_then(|v| v.as_str()).unwrap_or("");

    // Dock 객체에서 state.items를 읽어 item_id가 일치하는 항목의 app 필드 꺼냄.
    let app_id: String = mounted
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|dock| dock.state.get("items"))
        .and_then(|v| v.as_array())
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("id").and_then(|id| id.as_str()) == Some(item_id)
            })
        })
        .and_then(|item| item.get("app").and_then(|a| a.as_str()))
        .map(|s| s.to_string())
        // fallback: item_id를 그대로 app_id로 사용 (Dock items는 app id를 id로 사용하는 컨벤션).
        .unwrap_or_else(|| item_id.to_string());

    let outcome = handle_launch(&app_id, mounted);
    println!(
        "[desktop-shell] Dock({}) launch item_id={:?} app_id={:?} -> {:?}",
        target_id, item_id, app_id, outcome
    );
    // M1: 로그만.
    InvokeOutcome::empty()
}

/// DesktopIcon.open() — 아이콘에 연결된 앱 실행.
///
/// app_id는 icon 객체의 props["app"]에서 읽는다.
pub fn handle_desktop_icon_open(
    target_id: ObjectId,
    mounted: &[Object],
) -> InvokeOutcome {
    let app_id: String = mounted
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|icon| icon.props.get("app"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();

    let outcome = handle_launch(&app_id, mounted);
    println!(
        "[desktop-shell] DesktopIcon({}) open app_id={:?} -> {:?}",
        target_id, app_id, outcome
    );
    // M1: 로그만.
    InvokeOutcome::empty()
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
        use geulos_core::{ActorId, std_types};
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
    fn desktop_icon_open_empty_when_no_app_prop() {
        use geulos_core::{ActorId, std_types};
        let owner = ActorId::local_user();
        // app="" → resolve_app("") = None → Unknown("")
        let icon = std_types::desktop_icon(owner, "", "라벨", "folder", 0, 0);
        let icon_id = icon.id;
        let mounted = vec![icon];
        let out = handle_launch(
            mounted.iter().find(|o| o.id == icon_id)
                .and_then(|o| o.props.get("app"))
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            &mounted,
        );
        assert!(matches!(out, LaunchOutcome::Unknown(_)));
    }
}
