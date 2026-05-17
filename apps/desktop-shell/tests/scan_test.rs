use geulos_core::ActorId;
use geulos_desktop_shell::scan;
use std::fs;

fn owner() -> ActorId {
    ActorId::local_user()
}

#[test]
fn scan_empty_directory_returns_empty_children() {
    let dir = tempfile::tempdir().unwrap();
    let result = scan::scan_tree(&owner(), dir.path()).expect("scan");
    assert_eq!(result.objects.len(), 0);
}

#[test]
fn scan_flat_directory_returns_file_objects() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "hello").unwrap();
    fs::write(dir.path().join("b.md"), "# md").unwrap();
    let result = scan::scan_tree(&owner(), dir.path()).expect("scan");
    assert_eq!(result.objects.len(), 2);
    let mimes: Vec<&str> = result
        .objects
        .iter()
        .filter_map(|o| o.props.get("mime").and_then(|v| v.as_str()))
        .collect();
    assert!(mimes.contains(&"text/plain"));
    assert!(mimes.contains(&"text/markdown"));
}

#[test]
fn scan_nested_directory_returns_folder_and_files() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub").join("c.txt"), "nested").unwrap();
    let result = scan::scan_tree(&owner(), dir.path()).expect("scan");
    assert_eq!(result.objects.len(), 2);
    let has_folder = result.objects.iter().any(|o| o.type_uri.as_str() == "aios.std/Folder@1");
    let has_file = result.objects.iter().any(|o| o.type_uri.as_str() == "aios.std/File@1");
    assert!(has_folder && has_file);
}

#[test]
fn scan_skips_hidden_and_noisy_dirs() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join(".git").join("HEAD"), "x").unwrap();
    fs::create_dir(dir.path().join("node_modules")).unwrap();
    fs::create_dir(dir.path().join("target")).unwrap();
    fs::write(dir.path().join(".hidden"), "x").unwrap();
    fs::write(dir.path().join("visible.txt"), "x").unwrap();
    let result = scan::scan_tree(&owner(), dir.path()).expect("scan");
    assert_eq!(result.objects.len(), 1, "only visible.txt should remain");
}

#[test]
fn scan_attaches_preview_to_text_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("note.md"), "hello world").unwrap();
    let result = scan::scan_tree(&owner(), dir.path()).expect("scan");
    let file = result.objects.iter().find(|o| o.type_uri.as_str() == "aios.std/File@1").unwrap();
    assert_eq!(file.state.get("preview").and_then(|v| v.as_str()), Some("hello world"));
}

#[test]
fn scan_preview_preserves_korean_text() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("hangul.md"), "안녕").unwrap();
    let result = scan::scan_tree(&owner(), dir.path()).expect("scan");
    let file = result.objects.iter().find(|o| o.type_uri.as_str() == "aios.std/File@1").unwrap();
    assert_eq!(
        file.state.get("preview").and_then(|v| v.as_str()),
        Some("안녕"),
        "preview must preserve trailing multi-byte char when buffer ended cleanly"
    );
}
