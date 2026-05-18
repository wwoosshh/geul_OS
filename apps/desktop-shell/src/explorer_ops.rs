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
}
