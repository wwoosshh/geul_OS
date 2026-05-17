//! 워크스페이스 루트 결정 + 생성.

use std::path::{Path, PathBuf};

/// 워크스페이스 루트 경로 결정.
///
/// 우선순위:
/// 1. 환경변수 `GEULOS_WORKSPACE` — 명시적 override
/// 2. `%USERPROFILE%\GeulOS\workspace` (Windows) / `$HOME/GeulOS/workspace` (그 외)
///
/// 둘 다 못 찾으면 에러 — 호스트가 사용자 디렉터리 환경변수를 갖고 있지 않은
/// 극단적 환경이므로 명시적 실패가 적절.
pub fn resolve() -> Result<PathBuf, String> {
    if let Ok(s) = std::env::var("GEULOS_WORKSPACE") {
        if !s.is_empty() {
            return Ok(PathBuf::from(s));
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| "USERPROFILE/HOME 환경변수 없음".to_string())?;
    Ok(PathBuf::from(home).join("GeulOS").join("workspace"))
}

/// 디렉터리가 없으면 *재귀적*으로 생성.
///
/// 경로가 이미 *파일*(또는 그 외 비-디렉터리)로 존재하면 `AlreadyExists` 에러로 거부.
/// 그렇지 않으면 후속 fs 작업이 알 수 없는 경로에서 실패하게 됨.
pub fn ensure_exists(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        if !path.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "GEULOS_WORKSPACE points at a non-directory",
            ));
        }
        return Ok(());
    }
    std::fs::create_dir_all(path)?;
    Ok(())
}
