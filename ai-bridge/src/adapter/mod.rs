//! LLM 어댑터 추상.
//!
//! 다중 백엔드 (Claude / OpenAI / Ollama) 지원을 위한 trait. 첫 구현은 Claude.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod claude;
pub mod mock;

pub use claude::ClaudeAdapter;
pub use mock::MockAdapter;

/// LLM의 한 response.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    /// 텍스트 출력 (있다면).
    pub text: Vec<String>,
    /// 도구 호출 요청 (있다면).
    pub tool_uses: Vec<ToolUse>,
    /// 모델이 왜 멈췄나.
    pub stop: LlmStop,
    /// 토큰 사용량 (input, output).
    pub tokens: (u64, u64),
}

/// 도구 호출 요청 한 건.
#[derive(Debug, Clone)]
pub struct ToolUse {
    /// LLM이 발급한 고유 ID (응답 매칭용).
    pub id: String,
    /// 도구 이름.
    pub name: String,
    /// 도구 인자 (JSON).
    pub input: Value,
}

/// 모델 종료 이유.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmStop {
    /// 모델이 자연 종료.
    EndTurn,
    /// 도구 호출이 응답에 포함됨.
    ToolUse,
    /// 토큰 제한 도달.
    MaxTokens,
    /// 기타.
    Other,
}

/// 대화 메시지 (LLM과 주고받는 한 단위).
///
/// `Serialize`/`Deserialize` 도출 — `chat_persist`가 세션 파일에 JSON으로 dump/load한다
/// (T7.8 / ADR-031). content는 `serde_json::Value`이므로 텍스트·tool_use·tool_result
/// 블록 모두 자연스럽게 round-trip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: LlmRole,
    /// 본문 — 텍스트 OR 도구 결과들 OR 모델의 도구 호출들.
    pub content: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LlmRole {
    User,
    Assistant,
}

/// 도구 정의 (Claude의 tool 형식).
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// LLM 어댑터 trait.
#[async_trait]
pub trait LlmAdapter: Send + Sync {
    /// 한 메시지 round-trip — system + history + tools → response.
    async fn complete(
        &self,
        system: &str,
        history: &[LlmMessage],
        tools: &[ToolDef],
    ) -> Result<LlmResponse, crate::BridgeError>;
}
