//! AI chat 세션 영구 저장/로드 (M7 T7.8 / ADR-031).
//!
//! 파일 위치: `~/.geulos/ai-sessions/<name>.json`. (Windows: `%USERPROFILE%\.geulos\ai-sessions\<name>.json`,
//! Linux/macOS: `$HOME/.geulos/ai-sessions/<name>.json`.) desktop-shell의 `CliChatSession`이
//! 매 `send` 직후 dump하고, `/ai list` / `/ai load`로 사용자가 관리한다.
//!
//! 한 세션 = 한 JSON 파일 = 검색·삭제·외부 도구 편집이 쉬움 (vs. 거대 단일 JSONL).
//! 파일명은 `[A-Za-z0-9_-]+`만 허용 — path traversal 등 공격 방어.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::adapter::LlmMessage;
use crate::error::{BridgeError, BridgeResult};

/// 디스크에 저장되는 한 세션의 전체 상태.
///
/// `created_at`은 ISO8601 UTC 문자열, `model`은 어댑터가 사용한 model id
/// (`claude-sonnet-4-6` 등). history는 `LlmMessage`의 vec — Serialize/Deserialize는
/// `adapter::mod`에서 derived.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    pub name: String,
    /// ISO8601 (예: `2026-05-18T18:00:00Z`).
    pub created_at: String,
    /// 어댑터가 사용한 모델 id.
    pub model: String,
    pub history: Vec<LlmMessage>,
}

/// 사용자 홈 아래 `.geulos/ai-sessions/` 경로. 디렉터리가 없으면 생성한다.
///
/// `USERPROFILE` (Windows) 또는 `HOME` (Unix) 환경 변수를 사용 — `dirs` crate 의존 회피.
/// 두 변수가 모두 미설정인 비정상 환경에서는 `BridgeError::Config` 반환.
pub fn sessions_dir() -> BridgeResult<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .ok_or_else(|| BridgeError::Config("home dir unknown (USERPROFILE/HOME 미설정)".into()))?;
    let dir = PathBuf::from(home).join(".geulos").join("ai-sessions");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// `<name>.json` 파일 경로. name 검증 — `[A-Za-z0-9_-]+`만 허용.
///
/// 빈 문자열·구분자·점·공백 포함은 모두 reject — path traversal(`../etc/passwd`) 등을
/// 원천 차단한다. 사용자 입력의 `/ai start mysess` / `/ai load conv-20260518-180000`
/// 모두 이 규칙 안에 자연히 들어간다.
pub fn session_path(name: &str) -> BridgeResult<PathBuf> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(BridgeError::Config(format!(
            "invalid session name: {:?} (영문/숫자/하이픈/언더스코어만 허용)",
            name
        )));
    }
    Ok(sessions_dir()?.join(format!("{}.json", name)))
}

/// 세션을 디스크에 저장한다 (덮어쓰기). `chat_persist::save`는 매 send 직후 호출되므로
/// 마지막 send까지의 history가 항상 디스크에 commit 되어 있다 (crash safety).
pub fn save(name: &str, model: &str, created_at: &str, history: &[LlmMessage]) -> BridgeResult<()> {
    let path = session_path(name)?;
    let p = PersistedSession {
        name: name.to_string(),
        created_at: created_at.to_string(),
        model: model.to_string(),
        history: history.to_vec(),
    };
    let json = serde_json::to_string_pretty(&p)?;
    std::fs::write(&path, json)?;
    Ok(())
}

/// 세션 파일을 로드한다. 파일 없음·JSON 깨짐은 모두 에러 — caller는 사용자에게 안내한다.
pub fn load(name: &str) -> BridgeResult<PersistedSession> {
    let path = session_path(name)?;
    let bytes = std::fs::read(&path)?;
    let parsed: PersistedSession = serde_json::from_slice(&bytes)?;
    Ok(parsed)
}

/// 디렉터리 안 모든 `.json` 세션의 `(name, message_count)` 목록.
///
/// 깨진 파일은 *조용히 skip* — `/ai list`가 한 깨진 파일 때문에 전체가 실패하는 건
/// 사용자 입장에서 부담. (디버그 로그는 caller가 별도로 emit 가능.) 정렬: 이름 역순 —
/// auto-name이 `conv-YYYYMMDD-HHMMSS` 형식이라 *자연히 최신 세션이 위*에 노출된다.
pub fn list() -> BridgeResult<Vec<(String, usize)>> {
    let dir = sessions_dir()?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let s: PersistedSession = match serde_json::from_slice(&bytes) {
            Ok(x) => x,
            Err(_) => continue,
        };
        out.push((s.name, s.history.len()));
    }
    // 이름 역순 — auto-name이 timestamp 기반이면 최신이 위에 옴.
    out.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::LlmRole;
    use serde_json::Value;

    /// HOME / USERPROFILE을 임시 디렉터리로 잠시 바꾼다. RAII로 원복.
    ///
    /// 한 process 안 여러 테스트가 *환경 변수를 공유*하므로 동시에 실행되면 서로의
    /// HOME을 덮어써 race. Mutex로 직렬화한다.
    struct EnvGuard {
        _home: Option<std::ffi::OsString>,
        _user: Option<std::ffi::OsString>,
        _tmp: tempfile::TempDir,
        _lock: std::sync::MutexGuard<'static, ()>,
    }
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn override_home() -> EnvGuard {
        let lock = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir");
        let prev_home = std::env::var_os("HOME");
        let prev_user = std::env::var_os("USERPROFILE");
        std::env::set_var("HOME", tmp.path());
        std::env::set_var("USERPROFILE", tmp.path());
        EnvGuard { _home: prev_home, _user: prev_user, _tmp: tmp, _lock: lock }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // tempdir은 drop 시 자동 삭제. HOME/USERPROFILE 원복.
            match &self._home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match &self._user {
                Some(v) => std::env::set_var("USERPROFILE", v),
                None => std::env::remove_var("USERPROFILE"),
            }
        }
    }

    #[test]
    fn session_path_rejects_invalid_chars() {
        let _g = override_home();
        assert!(session_path("").is_err(), "빈 이름");
        assert!(session_path("../etc/passwd").is_err(), "path traversal");
        assert!(session_path("a/b").is_err(), "구분자");
        assert!(session_path("a.b").is_err(), "점");
        assert!(session_path("a b").is_err(), "공백");
        assert!(session_path("한글").is_err(), "한글");
        // 허용
        assert!(session_path("conv-20260518-180000").is_ok());
        assert!(session_path("my_conv").is_ok());
        assert!(session_path("CamelCase123").is_ok());
    }

    #[test]
    fn save_load_round_trip_preserves_history() {
        let _g = override_home();
        let history = vec![
            LlmMessage { role: LlmRole::User, content: Value::String("안녕".to_string()) },
            LlmMessage {
                role: LlmRole::Assistant,
                content: Value::String("반갑습니다".to_string()),
            },
        ];
        save("test_sess", "claude-sonnet-4-6", "2026-05-18T10:00:00Z", &history).unwrap();
        let p = load("test_sess").unwrap();
        assert_eq!(p.name, "test_sess");
        assert_eq!(p.model, "claude-sonnet-4-6");
        assert_eq!(p.created_at, "2026-05-18T10:00:00Z");
        assert_eq!(p.history.len(), 2);
        assert!(matches!(p.history[0].role, LlmRole::User));
        assert!(matches!(p.history[1].role, LlmRole::Assistant));
        if let Value::String(s) = &p.history[0].content {
            assert_eq!(s, "안녕");
        } else {
            panic!("first message content should be a string");
        }
    }

    #[test]
    fn list_returns_sessions_sorted_by_name_desc() {
        let _g = override_home();
        save("conv-20260518-100000", "m", "2026-05-18T10:00:00Z", &[]).unwrap();
        save("conv-20260518-180000", "m", "2026-05-18T18:00:00Z", &[]).unwrap();
        save("conv-20260518-090000", "m", "2026-05-18T09:00:00Z", &[]).unwrap();
        let entries = list().unwrap();
        // 이름 역순 — 최신 timestamp가 위.
        assert_eq!(entries[0].0, "conv-20260518-180000");
        assert_eq!(entries[1].0, "conv-20260518-100000");
        assert_eq!(entries[2].0, "conv-20260518-090000");
        for (_, count) in &entries {
            assert_eq!(*count, 0);
        }
    }

    #[test]
    fn list_skips_non_json_and_broken_files() {
        let _g = override_home();
        save("ok_sess", "m", "2026-05-18T00:00:00Z", &[]).unwrap();
        // 디렉터리 안에 깨진 파일 + 비-json 파일 직접 작성.
        let dir = sessions_dir().unwrap();
        std::fs::write(dir.join("broken.json"), b"{ not json").unwrap();
        std::fs::write(dir.join("ignore.txt"), b"hello").unwrap();
        let entries = list().unwrap();
        // 깨진 / 비-json은 skip되어 ok_sess만 남음.
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "ok_sess");
    }
}
