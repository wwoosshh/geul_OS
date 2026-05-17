use geulos_desktop_shell::workspace;
use std::env;

#[test]
fn resolve_uses_env_when_set() {
    let dir = tempfile::tempdir().expect("tempdir");
    let p = dir.path().to_string_lossy().to_string();
    unsafe { env::set_var("GEULOS_WORKSPACE", &p) };
    let resolved = workspace::resolve().expect("resolve");
    unsafe { env::remove_var("GEULOS_WORKSPACE") };
    assert_eq!(resolved, std::path::PathBuf::from(p));
}

#[test]
fn resolve_falls_back_to_userprofile_default() {
    unsafe { env::remove_var("GEULOS_WORKSPACE") };
    let resolved = workspace::resolve().expect("resolve");
    let s = resolved.to_string_lossy();
    assert!(s.contains("GeulOS"), "expected GeulOS in path, got {}", s);
    assert!(s.ends_with("workspace"), "expected ends with workspace, got {}", s);
}

#[test]
fn ensure_exists_creates_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("inner").join("workspace");
    workspace::ensure_exists(&target).expect("ensure");
    assert!(target.exists() && target.is_dir());
}
