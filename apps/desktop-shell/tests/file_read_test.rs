use geulos_desktop_shell::file_read::read_file_for_window;
use std::fs;
use tempfile::tempdir;

#[test]
fn read_small_text_file_returns_content() {
    let tmp = tempdir().unwrap();
    let p = tmp.path().join("hello.txt");
    fs::write(&p, "Hello\nWorld\n한글\n").unwrap();
    let result = read_file_for_window(&p, "text/plain");
    assert!(!result.too_large);
    assert_eq!(result.text, "Hello\nWorld\n한글\n");
}

#[test]
fn read_non_text_mime_returns_unsupported_message() {
    let tmp = tempdir().unwrap();
    let p = tmp.path().join("img.png");
    fs::write(&p, [0x89, 0x50, 0x4E, 0x47]).unwrap();
    let result = read_file_for_window(&p, "image/png");
    assert!(result.text.contains("viewer 미지원"));
    assert!(!result.too_large);
}

#[test]
fn read_invalid_utf8_returns_error_message() {
    let tmp = tempdir().unwrap();
    let p = tmp.path().join("bin.txt");
    fs::write(&p, [0xFF, 0xFE, 0xFD]).unwrap();
    let result = read_file_for_window(&p, "text/plain");
    assert!(result.text.contains("텍스트 파일 아님"));
    assert!(!result.too_large);
}

#[test]
fn read_oversized_file_truncates_to_1mb() {
    let tmp = tempdir().unwrap();
    let p = tmp.path().join("big.txt");
    let big = "a".repeat(2 * 1024 * 1024); // 2MB
    fs::write(&p, &big).unwrap();
    let result = read_file_for_window(&p, "text/plain");
    assert!(result.too_large);
    assert_eq!(result.text.len(), 1024 * 1024);
}

#[test]
fn read_missing_file_returns_error_message() {
    let result = read_file_for_window(std::path::Path::new("/no/such/file"), "text/plain");
    assert!(result.text.contains("읽기 실패"));
}
