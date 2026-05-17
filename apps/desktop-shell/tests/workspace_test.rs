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
fn resolve_treats_empty_env_as_unset() {
    // 빈 문자열은 *미설정*과 동일하게 취급되어 기본값으로 fallthrough해야 함.
    unsafe { env::set_var("GEULOS_WORKSPACE", "") };
    let resolved = workspace::resolve().expect("resolve");
    unsafe { env::remove_var("GEULOS_WORKSPACE") };
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

#[test]
fn ensure_exists_rejects_non_directory() {
    // 경로가 *파일*로 이미 존재하면 ensure_exists는 실패해야 함 (조용히 OK 금지).
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("not_a_dir");
    std::fs::write(&target, b"i am a file").expect("write");
    let result = workspace::ensure_exists(&target);
    assert!(result.is_err(), "expected error for non-directory path");
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
}
