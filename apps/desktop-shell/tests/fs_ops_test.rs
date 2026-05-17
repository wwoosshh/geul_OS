//! fs_ops 모듈 단위 테스트 — 디스크 기록 + path traversal 가드.
//!
//! 모든 테스트는 tempfile crate로 격리된 디렉터리 안에서만 실행 → CI/로컬 영향 0.

use geulos_desktop_shell::fs_ops;
use std::fs;

#[test]
fn create_file_writes_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.md");
    fs_ops::create_empty_file(&path).expect("create_empty_file 성공");
    assert!(path.exists(), "파일이 생성되어야 함");
    let bytes = fs::read(&path).unwrap();
    assert!(bytes.is_empty(), "빈 파일이어야 함");
}

#[test]
fn write_file_replaces_content_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note.txt");
    fs::write(&path, b"old").unwrap();
    fs_ops::atomic_write(&path, b"new content").expect("atomic_write 성공");
    let bytes = fs::read(&path).unwrap();
    assert_eq!(bytes, b"new content");
    // tmp 파일이 남아있으면 안 됨.
    let tmp = path.with_extension("tmp.geulos");
    assert!(!tmp.exists(), "tmp 파일은 rename 후 사라져야 함");
}

#[test]
fn write_file_creates_if_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("created.md");
    assert!(!path.exists());
    fs_ops::atomic_write(&path, b"hello").expect("부모 dir까지 만들고 write 성공");
    let bytes = fs::read(&path).unwrap();
    assert_eq!(bytes, b"hello");
}

#[test]
fn delete_removes_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doomed.md");
    fs::write(&path, b"bye").unwrap();
    assert!(path.exists());
    fs_ops::delete_file(&path).expect("delete_file 성공");
    assert!(!path.exists(), "삭제 후 파일이 사라져야 함");
}

#[test]
fn safe_join_rejects_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();

    // 정상 join.
    let ok = fs_ops::safe_join(base, "ok.txt").expect("정상 파일명은 통과");
    assert!(ok.starts_with(base));

    // 단순 .. — 거부.
    let escape = fs_ops::safe_join(base, "../escape.txt");
    assert!(escape.is_err(), "단순 ../ 은 거부되어야 함: {:?}", escape);

    // 하위 디렉터리 — 통과.
    let sub_ok = fs_ops::safe_join(base, "sub/ok.txt").expect("sub/ok.txt 는 통과");
    assert!(sub_ok.starts_with(base));

    // sub 들어갔다가 두 단계 위로 — base 밖으로 탈출. 거부.
    let sneaky = fs_ops::safe_join(base, "sub/../../escape");
    assert!(sneaky.is_err(), "탈출 시도는 거부되어야 함: {:?}", sneaky);
}
