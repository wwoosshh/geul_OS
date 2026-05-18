//! ai-bridge ChatSession in-process 래퍼 — desktop-shell이 CLI에서 AI 호출 (M7 T7.7).
//!
//! ADR-030 결정대로 desktop-shell이 `geulos-ai-bridge` crate에 직접 의존하고
//! ChatSession을 in-process로 owning한다. `ChatSession`은 한 ChatAdapter + 한
//! WireClient + history를 갖는데, 본 래퍼는 *desktop-shell 입장*에서 필요한
//! API만 좁게 노출 — `new_with_env`(환경변수에서 키 + 임시 wire connection)와
//! `send`(한 user prompt → 응답).
//!
//! ADR-009(AI 기본 불신)의 별 프로세스 격리 원칙은 M9+에 sandbox/process 분리
//! 마일스톤에서 다시 검토. M7 v1은 작동 시연 우선.

use geulos_ai_bridge::adapter::ClaudeAdapter;
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

/// CLI용 ChatSession 래퍼.
pub struct CliChatSession {
    inner: ChatSession<ClaudeAdapter>,
}

impl CliChatSession {
    /// 명시적 (api_key, wire, system)으로 새 세션.
    pub fn new(api_key: String, wire: WireClient, system: String) -> Self {
        let adapter = ClaudeAdapter::new(api_key, DEFAULT_MODEL.to_string());
        Self { inner: ChatSession::new(adapter, wire, system) }
    }

    /// 표준 진입점 — `ANTHROPIC_API_KEY` 환경 변수에서 키 읽고, 주어진 server 주소에
    /// `Role::Ai`로 wire 연결, DEFAULT_CLI_SYSTEM_PROMPT로 세션 생성.
    ///
    /// 키가 없으면 `BridgeError::Config`. 호출자는 이 에러를 잡아 *graceful degradation*
    /// (echo/help/clear만 동작, AI prompt 분기에선 안내 메시지)로 진행한다.
    pub async fn new_from_env(server_addr: &str) -> BridgeResult<Self> {
        // .env 자동 로드 (이미 다른 곳에서 로드됐어도 멱등). ai-bridge/main.rs와 동등 UX.
        let _ = dotenvy::dotenv();
        let key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| BridgeError::Config("ANTHROPIC_API_KEY not set".to_string()))?;
        let wire = WireClient::connect_as_ai(server_addr).await?;
        Ok(Self::new(key, wire, DEFAULT_CLI_SYSTEM_PROMPT.to_string()))
    }

    /// 한 user prompt를 보내고 AI 응답 텍스트를 받는다. history는 내부에 누적.
    pub async fn send(&mut self, user_prompt: &str) -> BridgeResult<String> {
        self.inner.send_message(user_prompt).await
    }
}
