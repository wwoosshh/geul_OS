//! invoke_handler 순수 함수 단위 테스트.

use geulos_core::ObjectId;
use geulos_desktop_shell::invoke_handler::*;

#[test]
fn expand_adds_folder_to_list() {
    let target = ObjectId::new();
    let folder = ObjectId::new();
    let outcome = handle_file_tree_expand(target, &[], folder);
    assert_eq!(outcome.state_sets.len(), 1);
    let (oid, key, val) = &outcome.state_sets[0];
    assert_eq!(*oid, target);
    assert_eq!(key, "expanded");
    let arr = val.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0].as_str(), Some(folder.to_string().as_str()));
}

#[test]
fn collapse_removes_folder() {
    let target = ObjectId::new();
    let folder = ObjectId::new();
    let outcome = handle_file_tree_collapse(target, &[folder], folder);
    let (_, _, val) = &outcome.state_sets[0];
    assert_eq!(val.as_array().unwrap().len(), 0);
}

#[test]
fn select_sets_node_id() {
    let target = ObjectId::new();
    let node = ObjectId::new();
    let outcome = handle_file_tree_select(target, node);
    let (_, key, val) = &outcome.state_sets[0];
    assert_eq!(key, "selected");
    assert_eq!(val.as_str(), Some(node.to_string().as_str()));
}

#[test]
fn canvas_set_file_updates_active_file() {
    let target = ObjectId::new();
    let file = ObjectId::new();
    let outcome = handle_canvas_set_file(target, file);
    let (_, key, val) = &outcome.state_sets[0];
    assert_eq!(key, "active_file");
    assert_eq!(val.as_str(), Some(file.to_string().as_str()));
}
