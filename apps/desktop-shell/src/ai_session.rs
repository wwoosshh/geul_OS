//! ai-bridge ChatSession in-process 래퍼 — desktop-shell이 CLI에서 AI 호출 (M7 T7.7/T7.8).
//!
//! ADR-030 결정대로 desktop-shell이 `geulos-ai-bridge` crate에 직접 의존하고
//! ChatSession을 in-process로 owning한다. T7.8 / ADR-031에서 **명시적 mode + 영속 세션**
//! 으로 재설계되어 lifecycle이 명확해졌다:
//!
//! - `CliChatSession::start(api_key, wire, system, name)` — 새 세션 (history 빈 상태).
//! - `CliChatSession::load(api_key, wire, system, name)` — 디스크 세션 로드 (history 복원).
//! - `send(prompt)` — 한 user turn 처리 + *매 호출 직후 디스크에 dump* (crash safety).
//! - `list_sessions()` — 디렉터리 안 모든 세션 (name, message_count) 목록.
//!
//! ADR-009(AI 기본 불신)의 별 프로세스 격리 원칙은 M9+에 sandbox/process 분리
//! 마일스톤에서 다시 검토. M7 v1은 작동 시연 우선.

use std::path::PathBuf;

use chrono::Utc;
use geulos_ai_bridge::adapter::ClaudeAdapter;
use geulos_ai_bridge::chat_persist;
use geulos_ai_bridge::chat_session::ChatSession;
use geulos_ai_bridge::error::{BridgeError, BridgeResult};
use geulos_ai_bridge::wire::WireClient;

/// ai-bridge가 기본으로 쓰는 Claude 모델. `ai-bridge/src/main.rs::DEFAULT_MODEL`과 일관.
const DEFAULT_MODEL: &str = "claude-sonnet-4-6";

/// CLI에 통합된 AI 어시스턴트의 system prompt 기본값.
///
/// **단일 소스**: `ai-bridge/src/system_prompt.md`. 이전엔 desktop-shell이 별도 짧은
/// 한국어 한 줄 prompt를 hard-code해서, M9에 추가된 File@1.save / Window→file_id 흐름이
/// AI에게 전달되지 않아 AI가 PowerShell 명령을 fallback으로 제안하는 버그가 있었음.
pub const DEFAULT_CLI_SYSTEM_PROMPT: &str = geulos_ai_bridge::DEFAULT_SYSTEM_PROMPT;

/// CLI용 ChatSession 래퍼. *세션 이름·모델·생성 시각*을 보유하고 매 send 후 디스크 dump.
pub struct CliChatSession {
    inner: ChatSession<ClaudeAdapter>,
    name: String,
    model: String,
    /// ISO8601 UTC 문자열. 첫 생성(`start`) 시각 또는 디스크에서 로드한 값.
    created_at: String,
}

impl CliChatSession {
    /// 새 세션 생성 — history 빈 상태. `/ai start [name]` 분기에서 호출.
    pub fn start(api_key: String, wire: WireClient, system: String, name: String) -> Self {
        let model = DEFAULT_MODEL.to_string();
        let adapter = ClaudeAdapter::new(api_key, model.clone());
        let audit = audit_path_for_session(&name);
        ensure_audit_dir(&audit);
        if let Some(dir) = audit.parent() {
            rotate_audit_logs(dir);
        }
        let inner = ChatSession::new(adapter, wire, system).with_audit(audit);
        let created_at = Utc::now().to_rfc3339();
        Self { inner, name, model, created_at }
    }

    /// 디스크에서 세션 로드 — history·model·created_at 복원. `/ai load <name>` 분기에서 호출.
    ///
    /// 파일 없음·JSON 깨짐은 `BridgeError`로 propagate — caller가 사용자에게 안내한다.
    pub fn load(
        api_key: String,
        wire: WireClient,
        system: String,
        name: &str,
    ) -> BridgeResult<Self> {
        let persisted = chat_persist::load(name)?;
        let adapter = ClaudeAdapter::new(api_key, persisted.model.clone());
        let audit = audit_path_for_session(&persisted.name);
        ensure_audit_dir(&audit);
        if let Some(dir) = audit.parent() {
            rotate_audit_logs(dir);
        }
        let mut inner = ChatSession::new(adapter, wire, system).with_audit(audit);
        inner.load_history(persisted.history);
        Ok(Self {
            inner,
            name: persisted.name,
            model: persisted.model,
            created_at: persisted.created_at,
        })
    }

    /// 활성 세션 이름 (UI prompt 시각화·SetState에 사용).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 한 user prompt를 보내고 AI 응답 텍스트를 받는다. *매 send 직후 디스크에 dump* —
    /// 비정상 종료 시에도 마지막 send까지 보존된다 (ADR-031).
    ///
    /// dump 실패는 *log only* — AI 응답 자체는 사용자에게 보여주고 다음 send도 시도된다.
    /// 디스크 full 같은 환경 문제까지 AI 응답을 차단하면 오히려 UX 부담.
    pub async fn send(&mut self, prompt: &str) -> BridgeResult<String> {
        let reply = self.inner.send_message(prompt).await?;
        if let Err(e) =
            chat_persist::save(&self.name, &self.model, &self.created_at, self.inner.history())
        {
            eprintln!(
                "[desktop-shell] 세션 dump 실패 (응답은 정상 반환): name={} err={}",
                self.name, e
            );
        }
        Ok(reply)
    }

    /// `send`의 스트리밍 변종 — text_delta를 tx로 흘리며 최종 텍스트 반환. cancel로 중단.
    pub async fn send_streaming(
        &mut self,
        prompt: &str,
        tx: &tokio::sync::mpsc::Sender<geulos_ai_bridge::adapter::StreamEvent>,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> BridgeResult<String> {
        let reply = self.inner.send_message_streaming(prompt, tx, cancel).await?;
        if let Err(e) =
            chat_persist::save(&self.name, &self.model, &self.created_at, self.inner.history())
        {
            eprintln!("[desktop-shell] 세션 dump 실패 (응답은 정상): name={} err={}", self.name, e);
        }
        Ok(reply)
    }

    /// 디렉터리 안 모든 세션의 `(name, message_count)` 목록 — `/ai list` 분기에서 호출.
    /// 이 함수는 API key·wire 없이 작동 — `chat_session: None` 상태에서도 정상.
    pub fn list_sessions() -> BridgeResult<Vec<(String, usize)>> {
        chat_persist::list()
    }
}

/// `conv-YYYYMMDD-HHMMSS` 형식의 자동 세션 이름 (UTC). `/ai start` (name 생략) 분기에서 사용.
pub fn auto_name() -> String {
    let now = Utc::now();
    format!("conv-{}", now.format("%Y%m%d-%H%M%S"))
}

/// API key 환경 변수 (`ANTHROPIC_API_KEY`) 를 읽는다. `/ai start`/`load` 분기에서 호출.
///
/// 키가 없으면 `BridgeError::Config`.
///
/// **T7.9 (ADR-032)부터 deprecated** — 신규 코드는 [`resolve_api_key`]를 사용해 *저장 파일*
/// 까지 자동으로 시도하라. 기존 호출자 호환을 위해 보존만 한다.
#[deprecated(note = "use resolve_api_key() instead (T7.9 / ADR-032 chain)")]
pub fn api_key_from_env() -> BridgeResult<String> {
    let _ = dotenvy::dotenv();
    std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| BridgeError::Config("ANTHROPIC_API_KEY not set".to_string()))
}

/// **T7.9 (ADR-032)** API key resolution chain — env → 저장 파일 순.
///
/// 우선순위:
/// 1. `.env` 파일 (dotenvy로 자동 load — 다음 단계의 환경 변수에 자연히 흡수).
/// 2. `ANTHROPIC_API_KEY` 환경 변수.
/// 3. `~/.geulos/api_key` 저장 파일.
/// 4. (none) — caller가 CLI prompt 흐름으로 진입해야 함.
///
/// `None`이면 desktop-shell main이 mode를 `"awaiting_api_key"`로 전환해 CLI에서 직접 입력
/// 받는다. 검증 성공 시 `geulos_ai_bridge::api_key::save_to_file`로 저장 → 다음 실행부터는
/// 1~3 단계에서 잡힌다.
pub fn resolve_api_key() -> Option<String> {
    let _ = dotenvy::dotenv();
    geulos_ai_bridge::api_key::try_load()
}

/// `Role::Ai`로 server-host에 새 wire 연결. desktop-shell의 기존 wire와 분리된다 —
/// last_change_actor가 AI actor_id로 기록돼 T5 노란 점 시각화가 자연스럽게 동작.
pub async fn connect_wire(server_addr: &str) -> BridgeResult<WireClient> {
    Ok(WireClient::connect_as_ai(server_addr).await?)
}

/// 세션 이름 + 현재 UTC 시각으로 audit JSONL 파일 경로 생성.
/// `~/.geulos/logs/ai-chat/<session>-<YYYYMMDD-HHMMSS>.jsonl`.
///
/// 부모 디렉터리 생성은 caller (start/load) 책임 — 본 함수는 pure path build만.
pub fn audit_path_for_session(session_name: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let ts = Utc::now().format("%Y%m%d-%H%M%S");
    home.join(".geulos").join("logs").join("ai-chat").join(format!("{}-{}.jsonl", session_name, ts))
}

/// audit 파일의 부모 디렉터리를 생성. 실패는 log + 무시 (audit 자체가 best-effort).
fn ensure_audit_dir(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!(
                "[ai-session] audit 디렉터리 생성 실패 ({}): {} — JSONL 로그 비활성",
                parent.display(),
                e
            );
        }
    }
}

/// ai-chat audit JSONL 보관 상한. 초과분은 가장 오래된 것부터 삭제 (KI-031).
const MAX_AUDIT_FILES: usize = 500;

/// `dir` 안의 `*.jsonl`을 mtime 내림차순 정렬해 `MAX_AUDIT_FILES` 초과분(가장 오래된 것)
/// 삭제. best-effort — 읽기/삭제 실패는 log 후 무시 (audit retention이 세션 시작을 막으면 안 됨).
fn rotate_audit_logs(dir: &std::path::Path) {
    let mut files: Vec<(std::path::PathBuf, std::time::SystemTime)> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().map(|x| x == "jsonl").unwrap_or(false))
            .filter_map(|e| {
                let mtime = e.metadata().ok()?.modified().ok()?;
                Some((e.path(), mtime))
            })
            .collect(),
        Err(_) => return,
    };
    if files.len() <= MAX_AUDIT_FILES {
        return;
    }
    // 최신 우선 정렬 (mtime 내림차순) → 앞쪽 MAX개 유지, 나머지(가장 오래된 것) 삭제.
    files.sort_by_key(|f| std::cmp::Reverse(f.1));
    for (path, _) in files.into_iter().skip(MAX_AUDIT_FILES) {
        if let Err(e) = std::fs::remove_file(&path) {
            eprintln!("[ai-session] audit rotate 삭제 실패 ({}): {}", path.display(), e);
        }
    }
}

/// 스트리밍 delta를 flush(SetState broadcast)할지 — 적응형 max(80ms, 40자) (AI streaming v1).
/// 마지막 flush 후 80ms 경과 OR 40자 이상 누적 시 true.
pub fn should_flush(since_last_flush: std::time::Duration, pending_chars: usize) -> bool {
    since_last_flush >= std::time::Duration::from_millis(80) || pending_chars >= 40
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_path_for_session_returns_well_formed_path() {
        let p = audit_path_for_session("test-session-abc");
        let s = p.to_string_lossy();
        assert!(s.contains(".geulos"), "경로에 .geulos 포함: {}", s);
        assert!(s.contains("logs"), "경로에 logs 포함: {}", s);
        assert!(s.contains("ai-chat"), "경로에 ai-chat 포함: {}", s);
        assert!(s.contains("test-session-abc"), "세션 이름 포함: {}", s);
        assert!(s.ends_with(".jsonl"), "확장자 .jsonl: {}", s);
        // 타임스탬프 형식 (YYYYMMDD-HHMMSS) 검증 — 14자리 숫자 + dash = 15
        let stem = p.file_stem().unwrap().to_string_lossy();
        let after_session = stem.strip_prefix("test-session-abc-").unwrap_or("");
        assert_eq!(after_session.len(), 15, "ts 형식 길이: {}", after_session);
    }

    #[test]
    fn rotate_keeps_at_most_max_files() {
        let tmp = std::env::temp_dir().join(format!("geulos-rotate-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        // 깨끗한 시작 보장.
        for e in std::fs::read_dir(&tmp).unwrap().flatten() {
            let _ = std::fs::remove_file(e.path());
        }
        for i in 0..(MAX_AUDIT_FILES + 5) {
            let f = tmp.join(format!("sess-{:04}.jsonl", i));
            std::fs::write(&f, b"{}\n").unwrap();
        }
        rotate_audit_logs(&tmp);
        let count = std::fs::read_dir(&tmp)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().map(|x| x == "jsonl").unwrap_or(false))
            .count();
        assert_eq!(count, MAX_AUDIT_FILES, "rotate 후 {} 개 남아야 함", MAX_AUDIT_FILES);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn should_flush_on_time_or_length() {
        use std::time::Duration;
        assert!(!should_flush(Duration::from_millis(10), 5), "둘 다 미달 → false");
        assert!(should_flush(Duration::from_millis(90), 1), "80ms 경과 → true");
        assert!(should_flush(Duration::from_millis(10), 45), "40자 누적 → true");
        assert!(should_flush(Duration::from_millis(80), 40), "경계값 → true");
    }
}
