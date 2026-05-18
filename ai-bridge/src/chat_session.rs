//! 대화식 multi-prompt 세션 (M7 T7.7, ADR-030).
//!
//! 기존 `Session::run_task`는 task 모델 — 한 user prompt를 받아 `report_done`까지
//! 자동 multi-turn 후 outcome 반환. CLI에서 사용자가 한국어로 여러 prompt를 *대화*하는
//! 패턴에는 맞지 않다 (history 단절). ChatSession은 chat 모델 — `send_message`를 반복
//! 호출하고, 매 호출이 한 user turn에 해당하며 history는 struct field로 누적된다.
//!
//! 한 `send_message` 내부에서는 *inner tool-use loop*가 일어난다 — AI가 query/get/invoke
//! 등의 도구를 자기 책임으로 호출하다가 `EndTurn` 또는 `max_inner_turns` 도달 시 종료.
//! 마지막 텍스트 응답(또는 `report_done` 요약)이 caller에 반환된다.
//!
//! **에러 시 history 보존:** send_message가 실패하면 원본 history가 그대로 — 다음 prompt
//! 시점에서 깨진 상태가 아니다. tool dispatch 자체는 `tools::dispatch_tool`을 재활용.

use std::path::PathBuf;
use std::time::Instant;

use chrono::Utc;
use serde_json::{json, Value};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::adapter::{LlmAdapter, LlmMessage, LlmResponse, LlmRole, LlmStop, ToolDef};
use crate::error::BridgeResult;
use crate::tools::{dispatch_tool, standard_tools, DispatchResult};
use crate::wire::WireClient;

/// 한 chat session = (adapter, wire, system, tools, history, audit).
///
/// `send_message`를 반복 호출하며 history가 누적된다. budget은 단순 `max_inner_turns`만
/// — 매 user turn 내부의 tool-use loop가 무한 루프에 빠지지 않도록 보호하는 안전장치.
pub struct ChatSession<A: LlmAdapter> {
    adapter: A,
    wire: WireClient,
    system: String,
    tools: Vec<ToolDef>,
    history: Vec<LlmMessage>,
    audit_path: Option<PathBuf>,
    /// 한 send_message 안에서 model ↔ tool round-trip 최대 횟수.
    max_inner_turns: usize,
}

impl<A: LlmAdapter> ChatSession<A> {
    /// 새 chat session (표준 도구 + max_inner_turns=8).
    pub fn new(adapter: A, wire: WireClient, system: String) -> Self {
        Self {
            adapter,
            wire,
            system,
            tools: standard_tools(),
            history: Vec::new(),
            audit_path: None,
            max_inner_turns: 8,
        }
    }

    /// 감사 로그 파일 경로 (append).
    pub fn with_audit(mut self, path: impl Into<PathBuf>) -> Self {
        self.audit_path = Some(path.into());
        self
    }

    /// 한 send_message 안의 최대 tool round-trip 횟수 변경.
    pub fn with_max_inner_turns(mut self, n: usize) -> Self {
        self.max_inner_turns = n;
        self
    }

    /// 현재 history 길이 (테스트·디버그용).
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// 한 user prompt에 대해 모델 응답을 받는다. tool use가 있으면 dispatch 후 model에
    /// 결과를 다시 넘기는 루프를 돌고, 최종 text(또는 report_done 요약)를 합쳐 반환한다.
    ///
    /// **history는 성공 시에만 commit.** 도중에 에러가 발생하면 self.history는 호출 이전
    /// 상태로 남는다 — caller가 같은 ChatSession에 다음 prompt를 보낼 때 깨진 history가
    /// 아니다.
    pub async fn send_message(&mut self, user_prompt: &str) -> BridgeResult<String> {
        let started = Instant::now();

        // 작업용 복사본. 성공 시 self.history로 commit.
        let mut history = self.history.clone();
        history.push(LlmMessage {
            role: LlmRole::User,
            content: Value::String(user_prompt.to_string()),
        });

        self.audit(&format!("\n=== chat send ===\n prompt: {}", user_prompt)).await;

        let mut final_text = String::new();
        let mut turn = 0usize;
        loop {
            turn += 1;
            if turn > self.max_inner_turns {
                self.audit(&format!("=== max_inner_turns ({}) reached ===", self.max_inner_turns))
                    .await;
                break;
            }

            self.audit(&format!("--- inner turn {} ---", turn)).await;

            let resp: LlmResponse =
                self.adapter.complete(&self.system, &history, &self.tools).await?;

            for t in &resp.text {
                self.audit(&format!("text: {}", t)).await;
                if !final_text.is_empty() {
                    final_text.push('\n');
                }
                final_text.push_str(t);
            }
            for tu in &resp.tool_uses {
                self.audit(&format!(
                    "tool_use: {}({})",
                    tu.name,
                    serde_json::to_string(&tu.input).unwrap_or_default()
                ))
                .await;
            }

            history.push(LlmMessage {
                role: LlmRole::Assistant,
                content: response_to_assistant_content(&resp),
            });

            // tool use 없이 EndTurn이면 한 user turn 종료.
            if resp.stop == LlmStop::EndTurn && resp.tool_uses.is_empty() {
                self.audit("=== end_turn (no tools) ===").await;
                break;
            }

            // tool dispatch — `report_done`은 chat 모델에선 *그냥 요약 텍스트 합치기*.
            let mut tool_results: Vec<Value> = Vec::new();
            let mut done = false;
            for tu in &resp.tool_uses {
                let r = dispatch_tool(&mut self.wire, &tu.name, &tu.input).await;
                match r {
                    Ok(DispatchResult::Output(v)) => {
                        self.audit(&format!("  -> {}", trim_value(&v))).await;
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tu.id,
                            "content": serde_json::to_string(&v).unwrap_or_default(),
                        }));
                    }
                    Ok(DispatchResult::Done { summary }) => {
                        self.audit(&format!("  -> report_done: {}", summary)).await;
                        if !final_text.is_empty() {
                            final_text.push('\n');
                        }
                        final_text.push_str(&summary);
                        done = true;
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tu.id,
                            "content": "ok",
                        }));
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        self.audit(&format!("  -> error: {}", msg)).await;
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tu.id,
                            "content": format!("error: {}", msg),
                            "is_error": true,
                        }));
                    }
                }
            }

            history.push(LlmMessage { role: LlmRole::User, content: Value::Array(tool_results) });

            if done {
                break;
            }
        }

        self.audit(&format!("=== chat done ({:.2}s) ===", started.elapsed().as_secs_f64())).await;

        // 성공 — history commit.
        self.history = history;
        Ok(final_text)
    }

    async fn audit(&self, line: &str) {
        if let Some(path) = &self.audit_path {
            let stamped = format!("{} {}\n", Utc::now().to_rfc3339(), line);
            if let Ok(mut f) = File::options().create(true).append(true).open(path).await {
                let _ = f.write_all(stamped.as_bytes()).await;
            }
        }
    }
}

/// LlmResponse → assistant content (Claude의 content blocks).
///
/// `session.rs`에도 동일 함수 존재 — DRY 위반이지만 둘 다 5줄짜리 trivial 변환.
/// M9 정리(공용 helper 모듈로 추출) 메모. T7.7 v1은 중복 유지로 단순화 우선.
fn response_to_assistant_content(resp: &LlmResponse) -> Value {
    let mut blocks = Vec::new();
    for t in &resp.text {
        blocks.push(json!({ "type": "text", "text": t }));
    }
    for tu in &resp.tool_uses {
        blocks.push(json!({
            "type": "tool_use",
            "id": tu.id,
            "name": tu.name,
            "input": tu.input,
        }));
    }
    Value::Array(blocks)
}

/// JSON value를 200자로 자른 디버그 문자열 (audit 로그에 raw dump 방지).
fn trim_value(v: &Value) -> String {
    let s = serde_json::to_string(v).unwrap_or_default();
    if s.len() > 200 {
        format!("{}...", &s[..200])
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{LlmResponse, LlmStop, MockAdapter, ToolUse};
    use geulos_server_host::run_listener;

    /// 테스트용 — 빈 LlmResponse (EndTurn, tool 없음).
    fn end_turn_response(text: &str, tokens_in: u64, tokens_out: u64) -> LlmResponse {
        LlmResponse {
            text: vec![text.to_string()],
            tool_uses: vec![],
            stop: LlmStop::EndTurn,
            tokens: (tokens_in, tokens_out),
        }
    }

    /// 테스트용 wire — 임시 server-host listener에 연결.
    async fn make_wire() -> WireClient {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(run_listener(listener));
        WireClient::connect_as_ai(&addr.to_string()).await.unwrap()
    }

    #[tokio::test]
    async fn chat_history_accumulates_across_sends() {
        let wire = make_wire().await;
        let mock = MockAdapter::new(vec![
            end_turn_response("첫 응답", 10, 5),
            end_turn_response("두 번째 응답", 20, 8),
        ]);
        let mut chat = ChatSession::new(mock, wire, "테스트 system prompt".to_string());

        let r1 = chat.send_message("첫 prompt").await.unwrap();
        assert_eq!(r1, "첫 응답");
        // 한 user + 한 assistant 추가됐어야.
        assert_eq!(chat.history_len(), 2, "1회 send 후 history는 [user, assistant]");

        let r2 = chat.send_message("두 번째 prompt").await.unwrap();
        assert_eq!(r2, "두 번째 응답");
        // 누적 — 두 번째 send 전까지 첫 user/assistant 보존 + 신규 user/assistant.
        assert_eq!(chat.history_len(), 4, "2회 send 후 history는 4 messages");

        // 두 번째 user 메시지 content가 실제 prompt인지 확인.
        if let Value::String(s) = &chat.history[2].content {
            assert_eq!(s, "두 번째 prompt");
        } else {
            panic!("third history entry should be user text message");
        }
    }

    #[tokio::test]
    async fn chat_send_failure_preserves_history() {
        let wire = make_wire().await;
        // mock에 응답 1개만 — 두 번째 send는 "mock exhausted" 에러.
        let mock = MockAdapter::new(vec![end_turn_response("ok", 5, 5)]);
        let mut chat = ChatSession::new(mock, wire, "sys".to_string());

        chat.send_message("p1").await.unwrap();
        assert_eq!(chat.history_len(), 2);

        let err = chat.send_message("p2-fails").await;
        assert!(err.is_err(), "두 번째 send는 mock exhausted로 실패해야 함");
        // history는 첫 send까지만 — 실패한 user prompt는 commit되지 않음.
        assert_eq!(
            chat.history_len(),
            2,
            "에러 시 history는 호출 이전 상태 유지 (실패한 user prompt 미commit)"
        );
    }

    #[tokio::test]
    async fn chat_send_includes_report_done_summary() {
        let wire = make_wire().await;
        // 첫 응답: report_done 호출. 두 번째 호출은 발생하지 않아야 (done이면 즉시 break).
        let mock = MockAdapter::new(vec![LlmResponse {
            text: vec!["작업 시작".to_string()],
            tool_uses: vec![ToolUse {
                id: "tu-1".to_string(),
                name: "report_done".to_string(),
                input: json!({"summary": "전부 끝났습니다"}),
            }],
            stop: LlmStop::ToolUse,
            tokens: (10, 5),
        }]);
        let mut chat = ChatSession::new(mock, wire, "sys".to_string());

        let r = chat.send_message("해줘").await.unwrap();
        // text + report_done summary가 합쳐져 반환.
        assert!(r.contains("작업 시작"), "최종 응답에 model text 포함");
        assert!(r.contains("전부 끝났습니다"), "최종 응답에 report_done summary 포함");
    }
}
