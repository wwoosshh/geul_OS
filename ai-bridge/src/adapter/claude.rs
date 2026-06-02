//! Claude REST 어댑터.
//!
//! 공식 Rust SDK가 없으므로 reqwest로 직접 호출.
//! API: https://docs.anthropic.com/en/api/messages

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::sse::{SseEvent, SseParser};
use super::StreamEvent;
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

    /// 스트리밍 응답 — text_delta를 tx로 즉시 흘리고, 종료 시 full LlmResponse 반환.
    /// tool_use 인자(input_json_delta)를 content_block index별로 누적해 *완전한* tool_use를
    /// 구성하므로 caller는 재요청 없이 바로 dispatch 가능 — 도구 turn의 텍스트도 정상 스트리밍.
    /// cancel 발동 시 stream drop + 부분 텍스트 응답(미완 도구 인자는 버림).
    #[allow(clippy::too_many_arguments)]
    async fn complete_streaming(
        &self,
        system: &str,
        history: &[LlmMessage],
        tools: &[ToolDef],
        turn: usize,
        tx: &mpsc::Sender<StreamEvent>,
        cancel: &CancellationToken,
    ) -> BridgeResult<LlmResponse> {
        // body 구성 — complete()와 동일 (cache_control 포함) + stream:true.
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
        let mut tools_json: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({ "name": t.name, "description": t.description, "input_schema": t.input_schema })
            })
            .collect();
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
            "stream": true,
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

        // SSE 스트림 소비 — 텍스트 누적 + text_delta 즉시 emit.
        // tool_use는 content_block index별로 (id, name, 인자 JSON 버퍼)를 누적 —
        // input_json_delta 토막이 모두 도착하면 완전한 인자가 되어 *재요청 없이* dispatch 가능.
        let mut parser = SseParser::new();
        let mut stream = resp.bytes_stream();
        let mut texts: Vec<String> = Vec::new();
        let mut acc_text = String::new();
        // index → (tool_use_id, tool_name, partial_json 누적 버퍼)
        let mut tool_blocks: std::collections::BTreeMap<usize, (String, String, String)> =
            std::collections::BTreeMap::new();
        let mut stop = LlmStop::Other;
        let mut out_tokens = 0u64;

        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    // 사용자 중단 — stream drop(연결 종료) + 부분 응답 (도구 인자 미완은 버림).
                    if !acc_text.is_empty() { texts.push(std::mem::take(&mut acc_text)); }
                    let _ = tx.send(StreamEvent::Cancelled).await;
                    return Ok(LlmResponse { text: texts, tool_uses: Vec::new(), stop: LlmStop::Cancelled, tokens: (0, out_tokens) });
                }
                chunk = stream.next() => {
                    match chunk {
                        Some(Ok(bytes)) => {
                            for ev in parser.push(&bytes) {
                                match ev {
                                    SseEvent::TextDelta { text, .. } => {
                                        acc_text.push_str(&text);
                                        let _ = tx.send(StreamEvent::TextDelta { turn, text }).await;
                                    }
                                    SseEvent::ContentBlockStart { index, tool_name: Some(name), tool_id } => {
                                        let _ = tx.send(StreamEvent::ToolStart { turn, name: name.clone() }).await;
                                        tool_blocks.insert(index, (tool_id.unwrap_or_default(), name, String::new()));
                                    }
                                    SseEvent::InputJsonDelta { index, partial_json } => {
                                        if let Some(b) = tool_blocks.get_mut(&index) {
                                            b.2.push_str(&partial_json);
                                        }
                                    }
                                    SseEvent::MessageDelta { stop_reason, output_tokens } => {
                                        out_tokens = output_tokens;
                                        stop = match stop_reason.as_deref() {
                                            Some("end_turn") => LlmStop::EndTurn,
                                            Some("tool_use") => LlmStop::ToolUse,
                                            Some("max_tokens") => LlmStop::MaxTokens,
                                            _ => LlmStop::Other,
                                        };
                                    }
                                    SseEvent::MessageStop => {}
                                    _ => {}
                                }
                            }
                        }
                        Some(Err(e)) => {
                            if !acc_text.is_empty() { texts.push(std::mem::take(&mut acc_text)); }
                            let _ = tx.send(StreamEvent::Error { message: e.to_string() }).await;
                            return Err(BridgeError::Network(e.to_string()));
                        }
                        None => break,
                    }
                }
            }
        }
        if !acc_text.is_empty() {
            texts.push(acc_text);
        }
        // 누적 버퍼를 완전한 ToolUse로 — 빈 버퍼는 인자 없는 도구로 간주({}).
        let tool_uses: Vec<ToolUse> = tool_blocks
            .into_values()
            .map(|(id, name, buf)| {
                let input = if buf.trim().is_empty() {
                    json!({})
                } else {
                    match serde_json::from_str(&buf) {
                        Ok(v) => v,
                        Err(e) => {
                            // 진단(2026-06-02): 도구 인자 누적 파싱 실패 시 무엇이 깨졌는지.
                            eprintln!(
                                "[claude-stream] tool input 파싱 실패 name={} err={} buf_len={} buf_head={:.160}",
                                name, e, buf.len(), buf
                            );
                            json!({})
                        }
                    }
                };
                let keys: Vec<&String> =
                    input.as_object().map(|o| o.keys().collect()).unwrap_or_default();
                eprintln!(
                    "[claude-stream] tool_use name={} id_empty={} input_keys={:?}",
                    name,
                    id.is_empty(),
                    keys
                );
                ToolUse { id, name, input }
            })
            .collect();
        eprintln!("[claude-usage] streaming out_tokens={} tool_uses={}", out_tokens, tool_uses.len());
        Ok(LlmResponse { text: texts, tool_uses, stop, tokens: (0, out_tokens) })
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
    let cache_read =
        usage.and_then(|u| u.get("cache_read_input_tokens")).and_then(|v| v.as_u64()).unwrap_or(0);
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
