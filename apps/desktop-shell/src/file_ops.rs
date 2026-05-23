//! File@1의 delete/rename 핸들러 (M10 Phase 1 / ADR-036).
//! file_write::save와 분리 — save는 *content 갱신*, file_ops는 *fs 객체 자체 조작*.

use std::path::{Path, PathBuf};

pub fn delete_file(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| format!("파일 삭제 실패: {}", e))
}

pub fn rename_file(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let parent = path.parent().ok_or_else(|| "부모 디렉터리 없음".to_string())?;
    let new_path = parent.join(new_name);
    if new_path.exists() {
        return Err(format!("이미 존재: {}", new_path.display()));
    }
    std::fs::rename(path, &new_path).map_err(|e| format!("파일 이름 변경 실패: {}", e))?;
    Ok(new_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn delete_existing_file() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("x.txt");
        std::fs::write(&p, "hi").unwrap();
        delete_file(&p).expect("ok");
        assert!(!p.exists());
    }

    #[test]
    fn delete_missing_returns_err() {
        let err = delete_file(Path::new("/nope/never")).unwrap_err();
        assert!(err.contains("파일 삭제 실패"));
    }

    #[test]
    fn rename_file_to_new_name() {
        let dir = tempdir().unwrap();
        let old = dir.path().join("old.txt");
        std::fs::write(&old, "x").unwrap();
        let new = rename_file(&old, "new.txt").expect("ok");
        assert!(!old.exists());
        assert!(new.is_file());
    }

    #[test]
    fn rename_conflict_errors() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "x").unwrap();
        std::fs::write(&b, "y").unwrap();
        let err = rename_file(&a, "b.txt").unwrap_err();
        assert!(err.contains("이미 존재"));
    }
}
