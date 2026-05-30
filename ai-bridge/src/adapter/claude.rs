//! Claude REST 어댑터.
//!
//! 공식 Rust SDK가 없으므로 reqwest로 직접 호출.
//! API: https://docs.anthropic.com/en/api/messages

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use super::{LlmAdapter, LlmMessage, LlmResponse, LlmRole, LlmStop, ToolDef, ToolUse};
use crate::error::{BridgeError, BridgeResult};

const CLAUDE_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic Claude REST 어댑터.
pub struct ClaudeAdapter {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: u32,
}

impl ClaudeAdapter {
    /// API 키와 모델 ID로 어댑터 생성.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
            model: model.into(),
            max_tokens: 2048,
        }
    }

    /// `ANTHROPIC_API_KEY` 환경 변수로부터 어댑터 생성.
    pub fn from_env(model: impl Into<String>) -> BridgeResult<Self> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| BridgeError::Config("ANTHROPIC_API_KEY not set".to_string()))?;
        Ok(Self::new(key, model))
    }

    /// 최대 출력 토큰 변경.
    pub fn with_max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = n;
        self
    }
}

#[async_trait]
impl LlmAdapter for ClaudeAdapter {
    async fn complete(
        &self,
        system: &str,
        history: &[LlmMessage],
        tools: &[ToolDef],
    ) -> BridgeResult<LlmResponse> {
        let messages_json: Vec<Value> = history
            .iter()
            .map(|m| {
                let role = match m.role {
                    LlmRole::User => "user",
                    LlmRole::Assistant => "assistant",
                };
                json!({ "role": role, "content": m.content })
            })
            .collect();

        // D: prompt caching — system + tools에 cache_control 마커. 매 turn 같은 prefix
        // (~5KB system + tool 정의)를 캐시 hit해 input token 90% 절감 (5분 TTL).
        // system은 array form으로 변경해야 cache_control 가능 (string은 cache 안 됨).
        let mut tools_json: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();
        // 마지막 tool에만 cache_control — 그 시점까지의 모든 tool 정의가 한 캐시 단위.
        if let Some(Value::Object(map)) = tools_json.last_mut() {
            map.insert("cache_control".to_string(), json!({"type": "ephemeral"}));
        }
        let system_blocks = json!([{
            "type": "text",
            "text": system,
            "cache_control": {"type": "ephemeral"},
        }]);

        let body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": system_blocks,
            "messages": messages_json,
            "tools": tools_json,
        });

        let resp = self
            .client
            .post(CLAUDE_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| BridgeError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let txt = resp.text().await.unwrap_or_default();
            return Err(BridgeError::ApiError { status: status.as_u16(), detail: txt });
        }

        let json: Value = resp.json().await.map_err(|e| BridgeError::Network(e.to_string()))?;
        parse_claude_response(json)
    }
}

fn parse_claude_response(json: Value) -> BridgeResult<LlmResponse> {
    let stop_str = json.get("stop_reason").and_then(|v| v.as_str()).unwrap_or("");
    let stop = match stop_str {
        "end_turn" => LlmStop::EndTurn,
        "tool_use" => LlmStop::ToolUse,
        "max_tokens" => LlmStop::MaxTokens,
        _ => LlmStop::Other,
    };

    let usage = json.get("usage");
    let in_tokens = usage.and_then(|u| u.get("input_tokens")).and_then(|v| v.as_u64()).unwrap_or(0);
    let out_tokens =
        usage.and_then(|u| u.get("output_tokens")).and_then(|v| v.as_u64()).unwrap_or(0);
    // D 검증용: prompt caching 동작 여부 시리얼 출력.
    let cache_read = usage
        .and_then(|u| u.get("cache_read_input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_creation = usage
        .and_then(|u| u.get("cache_creation_input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    eprintln!(
        "[claude-usage] in={} out={} cache_read={} cache_creation={}",
        in_tokens, out_tokens, cache_read, cache_creation
    );

    let mut text = Vec::new();
    let mut tool_uses = Vec::new();
    if let Some(content) = json.get("content").and_then(|v| v.as_array()) {
        for block in content {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                        text.push(t.to_string());
                    }
                }
                "tool_use" => {
                    tool_uses.push(ToolUse {
                        id: block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        name: block.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        input: block.get("input").cloned().unwrap_or(json!({})),
                    });
                }
                _ => {}
            }
        }
    }

    Ok(LlmResponse { text, tool_uses, stop, tokens: (in_tokens, out_tokens) })
}
