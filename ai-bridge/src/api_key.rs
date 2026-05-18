//! API key resolution chain + 검증 + 영구 저장 (M7 T7.9 / ADR-032).
//!
//! 우선순위:
//! 1. `ANTHROPIC_API_KEY` 환경 변수 (이미 set).
//! 2. `~/.geulos/api_key` 영속 파일 (T7.9 신규).
//! 3. CLI prompt — *호출자(desktop-shell)* 책임. ai-bridge는 헤드리스 layer만 담당.
//!
//! `.env` 파일은 dotenvy가 환경 변수로 load해주므로 1번 경로에 자연히 흡수된다 —
//! caller가 dotenvy::dotenv()를 먼저 호출해두면 충분 (ADR-030 이후 desktop-shell이 그렇게 함).

use std::path::PathBuf;
use std::time::Duration;

use crate::error::{BridgeError, BridgeResult};

/// 환경 변수 → 저장 파일 순으로 시도. 없으면 None.
///
/// 빈 문자열·whitespace-only는 *없음*으로 취급 — 사용자가 `set ANTHROPIC_API_KEY=`로
/// 비웠다거나 빈 파일이 남아있는 경우 자연히 무시되어 CLI prompt 흐름으로 넘어간다.
pub fn try_load() -> Option<String> {
    if let Ok(k) = std::env::var("ANTHROPIC_API_KEY") {
        let trimmed = k.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    load_from_file().ok().flatten()
}

/// `~/.geulos/api_key` 절대 경로. 디렉터리가 없으면 생성한다.
///
/// `USERPROFILE`(Windows) 또는 `HOME`(Unix). 두 변수가 모두 미설정인 비정상 환경에서만
/// `Config` 에러. chat_persist::sessions_dir과 동일 패턴.
fn key_file_path() -> BridgeResult<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .ok_or_else(|| BridgeError::Config("home dir unknown (USERPROFILE/HOME 미설정)".into()))?;
    let dir = PathBuf::from(home).join(".geulos");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("api_key"))
}

/// 저장 파일에서 key 한 줄을 읽는다. 파일 없음 → `Ok(None)`. trim 후 비어있어도 `None`.
fn load_from_file() -> BridgeResult<Option<String>> {
    let path = key_file_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(&path)?;
    let trimmed = s.trim().to_string();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}

/// `~/.geulos/api_key`에 key를 저장한다 (덮어쓰기). 호출 전에 [`validate`]로 검증된 key를
/// 넣는 것이 권장된다 — 잘못된 key가 영속되면 다음 실행마다 검증 실패가 반복된다.
///
/// **보안:** v1은 plain text. Windows ACL / Unix file mode 600은 v2 부채 (ADR-032).
pub fn save_to_file(key: &str) -> BridgeResult<()> {
    let path = key_file_path()?;
    std::fs::write(&path, key.trim())?;
    Ok(())
}

/// Anthropic `GET /v1/models`로 키만 검증한다. 응답 body는 무시.
///
/// - 200 OK → `Ok(())`
/// - 401 Unauthorized → `Err(Config("API key 무효 (401 Unauthorized)"))`
/// - 기타 → `Err(Config("validate 실패: HTTP {status}"))`
/// - 네트워크 오류 / 타임아웃 → `Err(Network(...))`
///
/// 타임아웃은 10초 — *프로세스가 무한 대기*하는 일이 없도록 명시. 사용자가 인터넷 끊긴
/// 환경에서도 안내 메시지를 보고 `/exit`로 빠질 수 있다.
pub async fn validate(key: &str) -> BridgeResult<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| BridgeError::Network(format!("client build: {}", e)))?;
    let resp = client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .map_err(|e| BridgeError::Network(format!("validate request: {}", e)))?;
    let status = resp.status();
    if status.is_success() {
        Ok(())
    } else if status.as_u16() == 401 {
        Err(BridgeError::Config("API key 무효 (401 Unauthorized)".into()))
    } else {
        Err(BridgeError::Config(format!("validate 실패: HTTP {}", status)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HOME / USERPROFILE을 임시 디렉터리로 잠시 바꾸고 ANTHROPIC_API_KEY도 unset.
    /// crate-wide `TEST_ENV_LOCK`을 chat_persist 모듈과 공유 — 두 모듈이 같은 env를
    /// 건드리므로 단일 자물쇠가 필수 (서로 다른 mutex면 parallel 실행 시 race).
    struct EnvGuard {
        _home: Option<std::ffi::OsString>,
        _user: Option<std::ffi::OsString>,
        _key: Option<std::ffi::OsString>,
        _tmp: tempfile::TempDir,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    fn override_home() -> EnvGuard {
        let lock = crate::TEST_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir");
        let prev_home = std::env::var_os("HOME");
        let prev_user = std::env::var_os("USERPROFILE");
        let prev_key = std::env::var_os("ANTHROPIC_API_KEY");
        std::env::set_var("HOME", tmp.path());
        std::env::set_var("USERPROFILE", tmp.path());
        std::env::remove_var("ANTHROPIC_API_KEY");
        EnvGuard { _home: prev_home, _user: prev_user, _key: prev_key, _tmp: tmp, _lock: lock }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self._home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match &self._user {
                Some(v) => std::env::set_var("USERPROFILE", v),
                None => std::env::remove_var("USERPROFILE"),
            }
            match &self._key {
                Some(v) => std::env::set_var("ANTHROPIC_API_KEY", v),
                None => std::env::remove_var("ANTHROPIC_API_KEY"),
            }
        }
    }

    #[test]
    fn try_load_returns_none_when_nothing_set() {
        let _g = override_home();
        // HOME=tmp(빈) + key env 없음 → None.
        assert!(try_load().is_none());
    }

    #[test]
    fn try_load_prefers_env_var_over_file() {
        let _g = override_home();
        save_to_file("file-key").unwrap();
        std::env::set_var("ANTHROPIC_API_KEY", "env-key");
        assert_eq!(try_load().as_deref(), Some("env-key"));
    }

    #[test]
    fn try_load_falls_back_to_file_when_env_empty() {
        let _g = override_home();
        save_to_file("file-key-only").unwrap();
        // env가 비어있으면(공백만) None으로 취급 → 파일로 fallback.
        std::env::set_var("ANTHROPIC_API_KEY", "   ");
        assert_eq!(try_load().as_deref(), Some("file-key-only"));
    }

    #[test]
    fn save_and_load_round_trip() {
        let _g = override_home();
        save_to_file("sk-ant-test-xyz").unwrap();
        // 직접 try_load — env가 unset이므로 파일 경로로 떨어진다.
        assert_eq!(try_load().as_deref(), Some("sk-ant-test-xyz"));
    }

    #[test]
    fn save_trims_whitespace() {
        let _g = override_home();
        save_to_file("  padded-key\n").unwrap();
        let p = key_file_path().unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert_eq!(s, "padded-key");
    }

    #[test]
    fn load_from_file_ignores_empty_file() {
        let _g = override_home();
        let p = key_file_path().unwrap();
        std::fs::write(&p, "   \n  ").unwrap();
        assert!(load_from_file().unwrap().is_none());
    }
}
