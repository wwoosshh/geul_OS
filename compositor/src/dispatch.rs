//! 클릭 dispatch — Folder/File/Window 등 타입별로 UiAction 생성.
//! main.rs(winit)와 VM 컴포지터가 공유.

use geulos_core::{Object, ObjectId};

use crate::layout::HitRole;
use crate::messages::UiAction;
use crate::tree_model::TreeModel;

/// 클릭 dispatch — 타입별 UiAction 생성.
///
/// - `aios.std/Folder@1`: ExpandToggle → FileTree expand/collapse, Body → Explorer.navigate_to.
/// - `aios.std/File@1`: Explorer.open_file.
/// - 그 외 (echo-app 호환): 첫 메서드를 args=null로 호출.
pub fn dispatch_click(
    tree: &TreeModel,
    target: ObjectId,
    obj: &Object,
    role: HitRole,
) -> Vec<UiAction> {
    match obj.type_uri.as_str() {
        "aios.std/Folder@1" => {
            let mut actions = Vec::new();
            if role == HitRole::ExpandToggle {
                if let Some(ft) = find_file_tree(tree) {
                    let is_expanded =
                        ft.state.get("expanded").and_then(|v| v.as_array()).is_some_and(|arr| {
                            arr.iter().any(|v| v.as_str() == Some(&target.to_string()))
                        });
                    actions.push(UiAction::Invoke {
                        target: ft.id,
                        method: if is_expanded { "collapse" } else { "expand" }.to_string(),
                        args: serde_json::json!({ "id": target.to_string() }),
                    });
                }
            } else if let Some(explorer) = find_explorer(tree) {
                actions.push(UiAction::Invoke {
                    target: explorer.id,
                    method: "navigate_to".to_string(),
                    args: serde_json::json!({ "folder_id": target.to_string() }),
                });
            }
            actions
        }
        "aios.std/File@1" => {
            if let Some(explorer) = find_explorer(tree) {
                vec![UiAction::Invoke {
                    target: explorer.id,
                    method: "open_file".to_string(),
                    args: serde_json::json!({ "file_id": target.to_string() }),
                }]
            } else {
                vec![]
            }
        }
        _ => {
            if let Some(m) = obj.methods.first() {
                vec![UiAction::Invoke {
                    target,
                    method: m.name().to_string(),
                    args: serde_json::Value::Null,
                }]
            } else {
                vec![]
            }
        }
    }
}

/// 첫 번째 *destroyed가 아닌* FileTree 반환. FM close+reopen 시 트리에 옛 destroyed FT가
/// 남아있을 수 있어 필터링 필수 — 안 그러면 dispatch가 stale 타겟으로 invoke 보내 무동작.
pub fn find_file_tree(tree: &TreeModel) -> Option<&Object> {
    for id in tree.ids() {
        if let Some(o) = tree.get(id) {
            if o.type_uri.as_str() == "aios.builtin/FileTree@1"
                && !o.state.get("destroyed").and_then(|v| v.as_bool()).unwrap_or(false)
            {
                return Some(o);
            }
        }
    }
    None
}

/// 첫 번째 *destroyed가 아닌* Explorer 반환 (find_file_tree와 동일 이유).
pub fn find_explorer(tree: &TreeModel) -> Option<&Object> {
    for id in tree.ids() {
        if let Some(o) = tree.get(id) {
            if o.type_uri.as_str() == "aios.builtin/Explorer@1"
                && !o.state.get("destroyed").and_then(|v| v.as_bool()).unwrap_or(false)
            {
                return Some(o);
            }
        }
    }
    None
}
