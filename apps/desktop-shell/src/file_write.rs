//! 파일 저장 — `save(path, content)` (M9 / ADR-035).
//!
//! UTF-8 1MB cap. v1은 직접 `fs::write` (atomic 아님 — crash 시 원본 손상 가능, v2에서
//! temp+rename 검토). 모든 실패는 `Err(String)` — 호출자가 invoke 응답이나 CLI 안내에 사용.

use std::path::Path;

const MAX_BYTES: usize = 1024 * 1024;

/// content를 path에 UTF-8로 저장. 1MB 초과면 에러.
pub fn save(path: &Path, content: &str) -> Result<(), String> {
    if content.len() > MAX_BYTES {
        return Err(format!("1MB 초과 ({}B > {}B) — v1 미지원", content.len(), MAX_BYTES));
    }
    std::fs::write(path, content).map_err(|e| format!("저장 실패: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn save_writes_content_to_path() {
        let mut tmp = tempfile::NamedTempFile::new().expect("tempfile");
        writeln!(tmp, "old").unwrap();
        let path = tmp.path().to_path_buf();
        save(&path, "new content\n").expect("save ok");
        let read = std::fs::read_to_string(&path).expect("read");
        assert_eq!(read, "new content\n");
    }

    #[test]
    fn save_rejects_over_1mb() {
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let big: String = "a".repeat(MAX_BYTES + 1);
        let err = save(tmp.path(), &big).unwrap_err();
        assert!(err.contains("1MB 초과"), "got: {}", err);
    }

    #[test]
    fn save_to_nonexistent_dir_returns_err() {
        // 부모 디렉터리가 *존재하지 않는* 절대 경로 — fs::write가 실패해야 함.
        let abs = std::env::temp_dir().join("geulos-m9-nonexistent-path-test-XYZ/x.txt");
        let err = save(&abs, "x").unwrap_err();
        assert!(err.contains("저장 실패"), "got: {}", err);
    }
}
