//! Folder@1의 create_file/create_folder/delete/rename 핸들러 (M10 Phase 1 / ADR-036).
//!
//! 각 함수는 *순수 fs operation* + 결과의 새 객체 (또는 destroyed marker). main.rs invoke
//! 분기가 permission 판정 + Dialog 흐름 + state broadcast를 wrap.

use std::path::{Path, PathBuf};

use geulos_core::{std_types, ActorId, Object};

/// 폴더 안에 새 빈 파일 생성. 결과는 mount할 File@1 객체 (parent=None).
///
/// path 충돌 (이미 존재)이면 Err. fs::write가 *replace*하지 않도록 사전 check.
pub fn create_file_in(
    owner: &ActorId,
    folder_path: &Path,
    name: &str,
    now_ms: i64,
) -> Result<Object, String> {
    let new_path = folder_path.join(name);
    if new_path.exists() {
        return Err(format!("이미 존재: {}", new_path.display()));
    }
    std::fs::write(&new_path, "").map_err(|e| format!("파일 생성 실패: {}", e))?;
    let mime = crate::lazy_mount::guess_mime(name);
    let mut obj =
        std_types::file(owner.clone(), new_path.to_string_lossy().as_ref(), name, mime, now_ms);
    obj.set_state("last_change_actor", serde_json::json!("ai"));
    obj.set_state("last_change_ms", serde_json::json!(now_ms));
    Ok(obj)
}

/// 폴더 안에 새 빈 폴더 생성.
pub fn create_folder_in(
    owner: &ActorId,
    folder_path: &Path,
    name: &str,
    now_ms: i64,
) -> Result<Object, String> {
    let new_path = folder_path.join(name);
    if new_path.exists() {
        return Err(format!("이미 존재: {}", new_path.display()));
    }
    std::fs::create_dir(&new_path).map_err(|e| format!("폴더 생성 실패: {}", e))?;
    let mut obj =
        std_types::folder(owner.clone(), new_path.to_string_lossy().as_ref(), name, now_ms);
    obj.set_state("last_change_actor", serde_json::json!("ai"));
    obj.set_state("last_change_ms", serde_json::json!(now_ms));
    Ok(obj)
}

/// 폴더 자체 삭제. recursive=true면 자식 포함.
pub fn delete_folder(path: &Path, recursive: bool) -> Result<(), String> {
    if recursive {
        std::fs::remove_dir_all(path).map_err(|e| format!("폴더 재귀 삭제 실패: {}", e))
    } else {
        std::fs::remove_dir(path).map_err(|e| format!("폴더 삭제 실패: {}", e))
    }
}

/// 폴더 이름 변경. 결과는 새 PathBuf.
pub fn rename_folder(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let parent = path.parent().ok_or_else(|| "부모 디렉터리 없음".to_string())?;
    let new_path = parent.join(new_name);
    if new_path.exists() {
        return Err(format!("이미 존재: {}", new_path.display()));
    }
    std::fs::rename(path, &new_path).map_err(|e| format!("폴더 이름 변경 실패: {}", e))?;
    Ok(new_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_file_in_empty_folder() {
        let dir = tempdir().unwrap();
        let owner = ActorId::local_user();
        let obj = create_file_in(&owner, dir.path(), "x.txt", 0).expect("ok");
        assert_eq!(obj.props.get("name").and_then(|v| v.as_str()), Some("x.txt"));
        assert!(dir.path().join("x.txt").exists());
    }

    #[test]
    fn create_file_conflict_errors() {
        let dir = tempdir().unwrap();
        let owner = ActorId::local_user();
        create_file_in(&owner, dir.path(), "x.txt", 0).expect("ok");
        let err = create_file_in(&owner, dir.path(), "x.txt", 0).unwrap_err();
        assert!(err.contains("이미 존재"));
    }

    #[test]
    fn create_folder_in_empty_folder() {
        let dir = tempdir().unwrap();
        let owner = ActorId::local_user();
        let obj = create_folder_in(&owner, dir.path(), "sub", 0).expect("ok");
        assert_eq!(obj.props.get("name").and_then(|v| v.as_str()), Some("sub"));
        assert!(dir.path().join("sub").is_dir());
    }

    #[test]
    fn delete_folder_recursive() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("a");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("x.txt"), "x").unwrap();
        // 비-recursive는 실패해야 (not empty).
        assert!(delete_folder(&sub, false).is_err());
        // recursive는 성공.
        delete_folder(&sub, true).expect("ok");
        assert!(!sub.exists());
    }

    #[test]
    fn rename_folder_returns_new_path() {
        let dir = tempdir().unwrap();
        let old = dir.path().join("old");
        std::fs::create_dir(&old).unwrap();
        let new = rename_folder(&old, "new").expect("ok");
        assert!(!old.exists());
        assert!(new.is_dir());
        assert_eq!(new.file_name().unwrap(), "new");
    }

    #[test]
    fn rename_folder_conflict_errors() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let err = rename_folder(&a, "b").unwrap_err();
        assert!(err.contains("이미 존재"));
    }
}
