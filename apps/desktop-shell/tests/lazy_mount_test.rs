use geulos_core::ActorId;
use geulos_desktop_shell::lazy_mount;
use std::fs;
use tempfile::tempdir;

#[test]
fn expand_folder_returns_direct_children_only() {
    let tmp = tempdir().unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    fs::write(tmp.path().join("a.txt"), b"hello").unwrap();
    fs::write(tmp.path().join("subdir").join("nested.txt"), b"x").unwrap();

    let owner = ActorId::local_user();
    let objs = lazy_mount::expand_folder(&owner, tmp.path(), 0).unwrap();
    let names: Vec<String> = objs
        .iter()
        .map(|o| o.props.get("name").and_then(|v| v.as_str()).unwrap_or("?").to_string())
        .collect();
    assert!(names.contains(&"subdir".to_string()));
    assert!(names.contains(&"a.txt".to_string()));
    assert!(!names.iter().any(|n| n == "nested.txt"), "재귀 mount 안 됨");
}

#[test]
fn expand_folder_returns_empty_on_permission_denied() {
    // 존재하지 않는 경로는 io::Error — 빈 vec 반환 (silent).
    let owner = ActorId::local_user();
    let result = lazy_mount::expand_folder(&owner, std::path::Path::new("/no/such/path"), 0);
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[test]
fn expand_folder_sets_parent_none_for_caller_to_fill() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("x.txt"), b"y").unwrap();
    let owner = ActorId::local_user();
    let objs = lazy_mount::expand_folder(&owner, tmp.path(), 0).unwrap();
    for o in &objs {
        assert!(o.parent.is_none(), "호출자가 parent 채움");
    }
}
