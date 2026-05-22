//! Explorer 객체의 navigate_to/open_file 핸들러 (M8).

use geulos_core::{Object, ObjectId};
use serde_json::json;

use crate::invoke_handler::InvokeOutcome;

/// `navigate_to(folder_id)` — Explorer.state.active_folder 갱신.
pub fn handle_navigate_to(explorer_id: ObjectId, folder_id: ObjectId) -> InvokeOutcome {
    InvokeOutcome {
        state_sets: vec![(explorer_id, "active_folder".to_string(), json!(folder_id.to_string()))],
    }
}

/// `navigate_up()` — active_folder의 *부모*로 이동. (Explorer 상단 "/" 행 클릭 시.)
///
/// - 현재 active_folder가 None 또는 mount 안 됨 → "" (드라이브 일람) 리셋.
/// - 현재 폴더의 parent가 Some(p) → active_folder = p.to_string().
/// - 현재 폴더의 parent가 None (드라이브 자체) → "" (드라이브 일람) 리셋.
pub fn handle_navigate_up(
    explorer_id: ObjectId,
    mounted_objects: &[Object],
    current_active_folder: Option<ObjectId>,
) -> InvokeOutcome {
    let new_active = match current_active_folder {
        Some(id) => mounted_objects
            .iter()
            .find(|o| o.id == id)
            .and_then(|o| o.parent)
            .map(|p| p.to_string())
            .unwrap_or_default(),
        None => String::new(),
    };
    InvokeOutcome {
        state_sets: vec![(explorer_id, "active_folder".to_string(), json!(new_active))],
    }
}

/// 활성 폴더가 비어있으면 (children=[]) lazy expand 필요한지 판정.
pub fn needs_expand(mounted_objects: &[Object], folder_id: ObjectId) -> bool {
    mounted_objects
        .iter()
        .find(|o| o.id == folder_id)
        .map(|f| f.children.is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use geulos_core::{std_types, ActorId};

    #[test]
    fn navigate_to_sets_active_folder_state() {
        let explorer_id = ObjectId::new();
        let folder_id = ObjectId::new();
        let outcome = handle_navigate_to(explorer_id, folder_id);
        assert_eq!(outcome.state_sets.len(), 1);
        let (id, key, val) = &outcome.state_sets[0];
        assert_eq!(*id, explorer_id);
        assert_eq!(key, "active_folder");
        assert_eq!(val.as_str(), Some(folder_id.to_string().as_str()));
    }

    #[test]
    fn needs_expand_returns_true_for_empty_folder() {
        let owner = ActorId::local_user();
        let f = std_types::folder(owner, "/x", "x", 0);
        let fid = f.id;
        assert!(needs_expand(&[f], fid));
    }

    #[test]
    fn needs_expand_returns_false_for_populated_folder() {
        let owner = ActorId::local_user();
        let mut f = std_types::folder(owner, "/x", "x", 0);
        f.children.push(ObjectId::new());
        let fid = f.id;
        assert!(!needs_expand(&[f], fid));
    }

    #[test]
    fn needs_expand_returns_false_for_missing_folder() {
        assert!(!needs_expand(&[], ObjectId::new()));
    }

    #[test]
    fn navigate_up_to_parent_when_folder_has_parent() {
        let owner = ActorId::local_user();
        let parent = std_types::folder(owner.clone(), "/p", "p", 0);
        let parent_id = parent.id;
        let mut child = std_types::folder(owner, "/p/c", "c", 0);
        child.parent = Some(parent_id);
        let child_id = child.id;
        let explorer_id = ObjectId::new();

        let outcome = handle_navigate_up(explorer_id, &[parent, child], Some(child_id));
        assert_eq!(outcome.state_sets.len(), 1);
        let (id, key, val) = &outcome.state_sets[0];
        assert_eq!(*id, explorer_id);
        assert_eq!(key, "active_folder");
        assert_eq!(val.as_str(), Some(parent_id.to_string().as_str()));
    }

    #[test]
    fn navigate_up_resets_to_empty_when_at_drive_root() {
        let owner = ActorId::local_user();
        let drive = std_types::folder(owner, "/C", "C", 0); // parent=None
        let drive_id = drive.id;
        let explorer_id = ObjectId::new();

        let outcome = handle_navigate_up(explorer_id, &[drive], Some(drive_id));
        let (_, _, val) = &outcome.state_sets[0];
        assert_eq!(val.as_str(), Some(""), "드라이브 root (parent=None) → 빈 string");
    }

    #[test]
    fn navigate_up_resets_to_empty_when_active_folder_none() {
        let explorer_id = ObjectId::new();
        let outcome = handle_navigate_up(explorer_id, &[], None);
        let (_, _, val) = &outcome.state_sets[0];
        assert_eq!(val.as_str(), Some(""));
    }
}
