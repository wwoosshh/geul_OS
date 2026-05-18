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
/// 짧고 명확하게: 한국어 답변 + GeulOS CLI에서 호출됨을 명시 + 도구 사용 안내.
/// 향후 system_prompt.md 같은 별 파일로 빼면 prompt iteration 용이 — v2.
pub const DEFAULT_CLI_SYSTEM_PROMPT: &str = "당신은 GeulOS의 CLI에서 호출된 AI 어시스턴트입니다. \
한국어로 간결하게 답하세요. \
필요시 list_objects_by_type / get_object / invoke_method / subscribe / drain 도구를 사용해 \
데스크톱의 객체 상태를 직접 조회·조작할 수 있습니다. \
작업이 명확하면 한 줄 요약으로 답하고, 도구 호출 결과는 사용자에게 핵심만 전달하세요.";

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
        let inner = ChatSession::new(adapter, wire, system);
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
        let mut inner = ChatSession::new(adapter, wire, system);
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
/// 키가 없으면 `BridgeError::Config` — caller(`main.rs`)는 그것을 잡아 graceful degradation
/// 메시지를 CLI에 출력한다.
pub fn api_key_from_env() -> BridgeResult<String> {
    let _ = dotenvy::dotenv();
    std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| BridgeError::Config("ANTHROPIC_API_KEY not set".to_string()))
}

/// `Role::Ai`로 server-host에 새 wire 연결. desktop-shell의 기존 wire와 분리된다 —
/// last_change_actor가 AI actor_id로 기록돼 T5 노란 점 시각화가 자연스럽게 동작.
pub async fn connect_wire(server_addr: &str) -> BridgeResult<WireClient> {
    Ok(WireClient::connect_as_ai(server_addr).await?)
}
