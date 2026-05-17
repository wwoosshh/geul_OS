//! 결정론 MockAdapter — 테스트 전용.

use async_trait::async_trait;
use std::sync::Mutex;

use super::{LlmAdapter, LlmMessage, LlmResponse, ToolDef};
use crate::error::{BridgeError, BridgeResult};

/// 미리 정해진 응답을 순서대로 반환하는 어댑터.
///
/// 테스트에서 LLM의 비결정성을 제거. 응답이 소진되면 에러 반환.
pub struct MockAdapter {
    responses: Mutex<std::collections::VecDeque<LlmResponse>>,
}

impl MockAdapter {
    /// 미리 준비된 응답 목록으로 어댑터 생성.
    pub fn new(responses: Vec<LlmResponse>) -> Self {
        Self { responses: Mutex::new(responses.into_iter().collect()) }
    }
}

#[async_trait]
impl LlmAdapter for MockAdapter {
    async fn complete(
        &self,
        _system: &str,
        _history: &[LlmMessage],
        _tools: &[ToolDef],
    ) -> BridgeResult<LlmResponse> {
        let mut q = self.responses.lock().unwrap();
        q.pop_front().ok_or_else(|| BridgeError::Config("mock exhausted".to_string()))
    }
}
