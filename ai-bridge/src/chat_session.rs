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

    /// 현재 history 참조 (T7.8 / ADR-031 — chat_persist가 디스크에 dump).
    pub fn history(&self) -> &[LlmMessage] {
        &self.history
    }

    /// 디스크에서 로드한 history로 *덮어쓴다* (T7.8 / ADR-031). `CliChatSession::load`가
    /// 새 ChatSession을 만든 직후 한 번 호출 — 이후 `send_message`가 이 history에 누적된다.
    pub fn load_history(&mut self, history: Vec<LlmMessage>) {
        self.history = history;
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

        self.audit_event("user_prompt", json!({ "text": user_prompt })).await;

        let mut final_text = String::new();
        let mut turn = 0usize;
        loop {
            turn += 1;
            if turn > self.max_inner_turns {
                self.audit_event(
                    "end_turn",
                    json!({ "turn": turn - 1, "reason": "max_inner_turns" }),
                )
                .await;
                break;
            }

            let resp: LlmResponse =
                self.adapter.complete(&self.system, &history, &self.tools).await?;

            for t in &resp.text {
                self.audit_event("ai_text", json!({ "turn": turn, "text": t })).await;
                if !final_text.is_empty() {
                    final_text.push('\n');
                }
                final_text.push_str(t);
            }
            for tu in &resp.tool_uses {
                self.audit_event(
                    "tool_call",
                    json!({
                        "turn": turn,
                        "tool_use_id": tu.id,
                        "name": tu.name,
                        "args": tu.input,
                    }),
                )
                .await;
            }

            history.push(LlmMessage {
                role: LlmRole::Assistant,
                content: response_to_assistant_content(&resp),
            });

            // tool use 없이 EndTurn이면 한 user turn 종료.
            if resp.stop == LlmStop::EndTurn && resp.tool_uses.is_empty() {
                self.audit_event("end_turn", json!({ "turn": turn, "reason": "no_tools" })).await;
                break;
            }

            // tool dispatch — `report_done`은 chat 모델에선 *그냥 요약 텍스트 합치기*.
            let mut tool_results: Vec<Value> = Vec::new();
            let mut done = false;
            for tu in &resp.tool_uses {
                let tool_started = Instant::now();
                let r = dispatch_tool(&mut self.wire, &tu.name, &tu.input).await;
                let latency_ms = tool_started.elapsed().as_millis() as u64;
                match r {
                    Ok(DispatchResult::Output(v)) => {
                        self.audit_event(
                            "tool_result",
                            json!({
                                "turn": turn,
                                "tool_use_id": tu.id,
                                "latency_ms": latency_ms,
                                "result": v,
                            }),
                        )
                        .await;
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tu.id,
                            "content": serde_json::to_string(&v).unwrap_or_default(),
                        }));
                    }
                    Ok(DispatchResult::Done { summary }) => {
                        self.audit_event(
                            "report_done",
                            json!({
                                "turn": turn,
                                "tool_use_id": tu.id,
                                "latency_ms": latency_ms,
                                "summary": summary,
                            }),
                        )
                        .await;
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
                        self.audit_event(
                            "tool_error",
                            json!({
                                "turn": turn,
                                "tool_use_id": tu.id,
                                "latency_ms": latency_ms,
                                "error": msg.clone(),
                            }),
                        )
                        .await;
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

        self.audit_event(
            "send_done",
            json!({
                "total_ms": started.elapsed().as_millis() as u64,
                "final_text_len": final_text.len(),
            }),
        )
        .await;

        // 성공 — history commit.
        self.history = history;
        Ok(final_text)
    }

    /// JSONL event 한 줄 append. `kind`는 이벤트 종류, `payload`는 그 외 필드.
    /// 공통 필드 `{ts, kind}`가 자동 prepend. payload 객체의 키와 충돌하면 payload 우선.
    ///
    /// M11.1 신규: audit_path가 설정된 경우 외부 진단용 JSONL 파일에 append. 실패는
    /// silent (디스크 full 등이 AI 응답을 차단하면 안 됨).
    async fn audit_event(&self, kind: &str, mut payload: Value) {
        // 공통 필드 주입 (audit_path 유무와 무관 — stderr mirror에도 ts/kind 필요).
        if let Value::Object(map) = &mut payload {
            map.entry("ts".to_string()).or_insert_with(|| Value::String(Utc::now().to_rfc3339()));
            map.entry("kind".to_string()).or_insert_with(|| Value::String(kind.to_string()));
        } else {
            payload = json!({
                "ts": Utc::now().to_rfc3339(),
                "kind": kind,
                "value": payload,
            });
        }

        let line = match serde_json::to_string(&payload) {
            Ok(s) => format!("{}\n", s),
            Err(_) => return,
        };

        // VM 시리얼 콘솔로 mirror — VM 내부 audit JSONL은 호스트에서 추출 어려움.
        // 페이로드에 user prompt / AI 응답 / 파일 본문이 들어가 시리얼 로그가 외부에
        // 노출되는 환경에서 정보 누출 우려 — 두 단계 gate:
        // - 기본: metadata만 (ts, kind, turn 등 — text/args/result는 길이로만 요약).
        // - GEULOS_AI_AUDIT_STDERR=1: 전체 payload (진단/측정 시).
        let full_mirror = std::env::var("GEULOS_AI_AUDIT_STDERR").as_deref() == Ok("1");
        if full_mirror {
            eprint!("[ai-chat-audit] {}", line);
        } else if let Value::Object(map) = &payload {
            // metadata + 본문 *길이* 요약. tool_call args, tool_result.result, ai_text.text
            // user_prompt.text 등 길이만 size 필드로 노출.
            let mut meta = serde_json::Map::new();
            for key in ["ts", "kind", "turn", "tool_use_id", "name", "latency_ms"] {
                if let Some(v) = map.get(key) {
                    meta.insert(key.to_string(), v.clone());
                }
            }
            for key in ["text", "args", "result", "summary"] {
                if let Some(v) = map.get(key) {
                    let size = serde_json::to_string(v).map(|s| s.len()).unwrap_or(0);
                    meta.insert(format!("{}_size", key), json!(size));
                }
            }
            if let Ok(s) = serde_json::to_string(&Value::Object(meta)) {
                eprintln!("[ai-chat-audit] {}", s);
            }
        }

        // 파일 audit (호스트 빌드/VM 모두 ~/.geulos/logs/ai-chat/<session>.jsonl에 append).
        let Some(path) = &self.audit_path else { return };
        if let Ok(mut f) = File::options().create(true).append(true).open(path).await {
            let _ = f.write_all(line.as_bytes()).await;
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

    #[tokio::test]
    async fn audit_writes_jsonl_events_for_user_prompt_and_ai_text() {
        let wire = make_wire().await;
        let mock = MockAdapter::new(vec![end_turn_response("응답입니다", 5, 5)]);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let mut chat = ChatSession::new(mock, wire, "sys".to_string()).with_audit(path.clone());

        chat.send_message("안녕").await.unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert!(!lines.is_empty(), "JSONL 라인이 하나 이상");

        // 각 줄이 valid JSON object
        for l in &lines {
            let v: serde_json::Value = serde_json::from_str(l)
                .unwrap_or_else(|e| panic!("JSONL parse 실패: {} on line: {}", e, l));
            assert!(v.get("ts").is_some(), "ts 필드 필수: {}", l);
            assert!(v.get("kind").is_some(), "kind 필드 필수: {}", l);
        }

        // user_prompt + ai_text + end_turn + send_done 존재
        let kinds: Vec<String> = lines
            .iter()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|v| v.get("kind").and_then(|k| k.as_str()).map(String::from))
            .collect();
        assert!(kinds.contains(&"user_prompt".to_string()), "user_prompt 이벤트 누락: {:?}", kinds);
        assert!(kinds.contains(&"ai_text".to_string()), "ai_text 이벤트 누락: {:?}", kinds);
        assert!(kinds.contains(&"end_turn".to_string()), "end_turn 이벤트 누락: {:?}", kinds);
        assert!(kinds.contains(&"send_done".to_string()), "send_done 이벤트 누락: {:?}", kinds);
    }
}
