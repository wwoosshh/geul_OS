# AI 응답 스트리밍 (Anthropic SSE) Implementation Plan

> **Status:** planned (2026-06-02)
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** AI 응답을 Anthropic SSE로 토큰 단위 점진 표시(첫 토큰까지 1~3초 단축)하고, Esc로 중단 가능하게 한다.

**Architecture:** 4층 흐름 — ① ClaudeAdapter가 `stream:true` + `bytes_stream()`를 SSE 파서로 읽어 `StreamEvent`를 mpsc로 흘리고, ② ChatSession turn loop가 forward, ③ desktop-shell이 적응형 `max(80ms,40자)` throttle로 `Cli.streaming_text` SetState, ④ 컴포지터가 라이브 렌더 + 커서. 중단은 `CancellationToken` + invoke 메서드 `Cli@1.interrupt_ai`. desktop-shell의 기존 `ConsoleEvent → apply_console_line` 채널 패턴을 그대로 미러링.

**Tech Stack:** Rust, reqwest(`stream` feature) + `futures-util`(StreamExt) + `tokio-util`(CancellationToken), tokio mpsc, 직접 SSE 파싱(SDK 없음).

**Spec:** `docs/specs/2026-06-02-geulos-ai-response-streaming.md` (브레인스토밍 확정: 모든 turn 텍스트 스트리밍 / 적응형 flush / v1 중단).

---

## File Structure

| 파일 | 책임 | Phase |
|---|---|---|
| `ai-bridge/Cargo.toml` | reqwest `stream` + `futures-util` + `tokio-util` 의존 | 1 |
| `ai-bridge/src/adapter/mod.rs` | `StreamEvent` enum, `LlmStop::Cancelled`, trait `complete_streaming` 기본구현 | 1 |
| `ai-bridge/src/adapter/sse.rs` | **신규** — SSE 증분 파서 (순수, 테스트 핵심) | 1 |
| `ai-bridge/src/adapter/claude.rs` | `complete_streaming` override (stream:true + bytes_stream + cancel) | 1 |
| `ai-bridge/src/adapter/mock.rs` | MockAdapter 스트리밍 (델타 emit) | 1 |
| `ai-bridge/src/chat_session.rs` | `send_message_streaming` turn loop | 1 |
| `apps/desktop-shell/src/ai_session.rs` | `should_flush` 헬퍼 + `send_streaming` | 2 |
| `core/src/object/std_types.rs` | `Cli@1`에 `streaming_text`/`streaming_active` state + `interrupt_ai` method | 2/3 |
| `apps/desktop-shell/src/main.rs` | stream_rx select! arm + 적응형 throttle + CancellationToken 보유 | 2 |
| `apps/desktop-shell/src/handlers/` (cli) | `interrupt_ai` 핸들러 + ACL | 3 |
| `compositor/src/render.rs` | `render_cli` 라이브 streaming_text + 커서 | 3 |
| `compositor` 입력 핸들러(host `main.rs` + `bin/geulos-vm-compositor`) | Esc → `interrupt_ai` invoke | 3 |

**Spec 대비 1건 정정:** Spec은 Phase 1 첫 소비자를 "standalone ai-bridge CLI stdout"으로 적었으나, `ai-bridge/src/main.rs`는 `ChatSession`이 아닌 `Session::run_task`(scenario 모델)을 사용한다. Phase 1은 **테스트로 검증**(SSE 파서 canned 바이트 + MockAdapter 스트리밍 등가성)하고, 실제 육안 소비자는 Phase 2(desktop-shell)로 한다. standalone CLI 스트리밍은 후속(Session 모델 별도).

---

# Phase 1 — 어댑터 SSE 스트리밍

### Task 1.1: 의존성 + `StreamEvent`/`LlmStop::Cancelled` + trait 기본구현

**Files:**
- Modify: `ai-bridge/Cargo.toml`
- Modify: `ai-bridge/src/adapter/mod.rs`

- [ ] **Step 1: reqwest `stream` feature + 신규 deps**

`ai-bridge/Cargo.toml`의 `[dependencies]`에서 reqwest 라인을 features 추가로 교체 + 2줄 추가:

```toml
reqwest = { version = "0.12", features = ["json", "rustls-tls", "stream"], default-features = false }
futures-util = "0.3"
tokio-util = "0.7"
```

> 주의: 워크스페이스 `reqwest = { workspace = true }`는 `stream`이 없다. ai-bridge에서만 features를 덮어쓰려면 위처럼 *명시 버전*으로 풀어 적는다(현재 workspace 정의와 동일 버전 0.12 + 기존 features 유지 + stream 추가).

- [ ] **Step 2: 빌드로 deps 확인**

Run: `cargo build -p geulos-ai-bridge`
Expected: 성공 (새 crate fetch).

- [ ] **Step 3: `StreamEvent` + `LlmStop::Cancelled` + trait 메서드 추가**

`ai-bridge/src/adapter/mod.rs`. 상단 import에 `use tokio::sync::mpsc;` 와 `use tokio_util::sync::CancellationToken;` 추가. `LlmStop` enum에 variant 추가:

```rust
pub enum LlmStop {
    EndTurn,
    ToolUse,
    MaxTokens,
    /// 사용자/외부 중단 (KI: AI streaming v1).
    Cancelled,
    Other,
}
```

`LlmResponse` 정의 아래에 추가:

```rust
/// 스트리밍 중 adapter → 상위로 흐르는 점진 신호.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// assistant 텍스트 토막 (turn = 현재 inner turn 번호).
    TextDelta { turn: usize, text: String },
    /// 한 inner turn에서 도구 호출 블록 시작 (진행 마커용).
    ToolStart { turn: usize, name: String },
    /// 한 user-turn 전체 정상 종료.
    Done,
    /// 사용자/외부 중단.
    Cancelled,
    /// 스트리밍 중 에러 (네트워크 drop 등).
    Error { message: String },
}
```

`LlmAdapter` trait에 기본구현 메서드 추가 (기존 `complete` 아래):

```rust
    /// 스트리밍 1회 round-trip. text_delta를 tx로 즉시 흘리고, 종료 시 full LlmResponse 반환.
    /// 기본 구현: 비스트리밍 complete()를 호출하고 결과 텍스트를 한 번에 emit (스트리밍
    /// 미지원 어댑터 호환). ClaudeAdapter가 override해 실제 SSE.
    async fn complete_streaming(
        &self,
        system: &str,
        history: &[LlmMessage],
        tools: &[ToolDef],
        turn: usize,
        tx: &mpsc::Sender<StreamEvent>,
        _cancel: &CancellationToken,
    ) -> Result<LlmResponse, crate::BridgeError> {
        let resp = self.complete(system, history, tools).await?;
        for t in &resp.text {
            let _ = tx.send(StreamEvent::TextDelta { turn, text: t.clone() }).await;
        }
        Ok(resp)
    }
```

- [ ] **Step 4: 빌드 + 커밋**

Run: `cargo build -p geulos-ai-bridge && cargo clippy -p geulos-ai-bridge --lib -- -D warnings`
Expected: 클린

```bash
git add ai-bridge/Cargo.toml ai-bridge/src/adapter/mod.rs Cargo.lock
git commit -m "feat(ai-bridge): StreamEvent + complete_streaming trait 기본구현 + reqwest stream dep"
```

---

### Task 1.2: SSE 증분 파서 (`sse.rs`)

**Files:**
- Create: `ai-bridge/src/adapter/sse.rs`
- Modify: `ai-bridge/src/adapter/mod.rs` (`pub mod sse;` 선언)

- [ ] **Step 1: 모듈 선언**

`ai-bridge/src/adapter/mod.rs` 상단 `pub mod claude;` 옆에 `pub mod sse;` 추가.

- [ ] **Step 2: 실패 테스트 먼저 작성**

`ai-bridge/src/adapter/sse.rs` 생성, 하단에 테스트부터:

```rust
//! Anthropic messages SSE 증분 파서.
//!
//! 네트워크/async 무관 — 바이트 청크를 push하면 완성된 SSE 이벤트를 뱉는다.
//! SSE 프레임 경계는 빈 줄(`\n\n`). 한 프레임은 `event: <name>` + `data: <json>` 라인.

use serde_json::Value;

/// 파싱된 SSE 이벤트 (필요한 것만; 나머지는 Other/Ping).
#[derive(Debug, Clone, PartialEq)]
pub enum SseEvent {
    MessageStart,
    /// content block 시작 — Text 또는 ToolUse.
    ContentBlockStart { index: usize, tool_name: Option<String> },
    /// text_delta 토막.
    TextDelta { index: usize, text: String },
    /// message_delta — stop_reason + output_tokens.
    MessageDelta { stop_reason: Option<String>, output_tokens: u64 },
    MessageStop,
    /// ping / input_json_delta / content_block_stop 등 무시 대상.
    Other,
}

/// 증분 SSE 파서 — push로 청크를 먹이고 완성된 이벤트를 받는다.
#[derive(Default)]
pub struct SseParser {
    buf: String,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// 바이트 청크를 누적하고, 지금까지 완성된 프레임(`\n\n` 종결)을 파싱해 반환.
    /// 미완 프레임은 내부 buf에 보존.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.push_str(&String::from_utf8_lossy(chunk));
        let mut events = Vec::new();
        // 완성된 프레임만 처리 — 마지막 미완 조각은 buf에 남긴다.
        while let Some(idx) = self.buf.find("\n\n") {
            let frame: String = self.buf.drain(..idx + 2).collect();
            if let Some(ev) = parse_frame(&frame) {
                events.push(ev);
            }
        }
        events
    }
}

/// 한 SSE 프레임("event: ...\ndata: ...\n\n")을 SseEvent로. data JSON의 type으로 분기.
fn parse_frame(frame: &str) -> Option<SseEvent> {
    let mut data_json: Option<Value> = None;
    for line in frame.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            data_json = serde_json::from_str(rest.trim()).ok();
        }
    }
    let data = data_json?;
    let ty = data.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match ty {
        "message_start" => Some(SseEvent::MessageStart),
        "content_block_start" => {
            let index = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let tool_name = data
                .get("content_block")
                .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("tool_use"))
                .and_then(|b| b.get("name").and_then(|v| v.as_str()))
                .map(String::from);
            Some(SseEvent::ContentBlockStart { index, tool_name })
        }
        "content_block_delta" => {
            let delta = data.get("delta")?;
            if delta.get("type").and_then(|v| v.as_str()) == Some("text_delta") {
                let index = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let text = delta.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                Some(SseEvent::TextDelta { index, text })
            } else {
                Some(SseEvent::Other) // input_json_delta 등
            }
        }
        "message_delta" => {
            let stop_reason = data
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(|v| v.as_str())
                .map(String::from);
            let output_tokens =
                data.get("usage").and_then(|u| u.get("output_tokens")).and_then(|v| v.as_u64()).unwrap_or(0);
            Some(SseEvent::MessageDelta { stop_reason, output_tokens })
        }
        "message_stop" => Some(SseEvent::MessageStop),
        _ => Some(SseEvent::Other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_deltas_in_order() {
        let mut p = SseParser::new();
        let sse = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"안녕\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" 세계\"}}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":7}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );
        let evs = p.push(sse.as_bytes());
        assert_eq!(evs[0], SseEvent::MessageStart);
        assert!(matches!(evs[1], SseEvent::ContentBlockStart { index: 0, tool_name: None }));
        assert_eq!(evs[2], SseEvent::TextDelta { index: 0, text: "안녕".into() });
        assert_eq!(evs[3], SseEvent::TextDelta { index: 0, text: " 세계".into() });
        assert_eq!(evs[4], SseEvent::MessageDelta { stop_reason: Some("end_turn".into()), output_tokens: 7 });
        assert_eq!(evs[5], SseEvent::MessageStop);
    }

    #[test]
    fn reassembles_frame_split_across_chunks() {
        let mut p = SseParser::new();
        // 프레임이 청크 경계에 걸쳐도 정확히 재조립.
        let e1 = p.push(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,");
        assert!(e1.is_empty(), "미완 프레임은 이벤트 0");
        let e2 = p.push(b"\"delta\":{\"type\":\"text_delta\",\"text\":\"x\"}}\n\n");
        assert_eq!(e2, vec![SseEvent::TextDelta { index: 0, text: "x".into() }]);
    }

    #[test]
    fn tool_use_block_start_carries_name() {
        let mut p = SseParser::new();
        let f = "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"get_object\"}}\n\n";
        let evs = p.push(f.as_bytes());
        assert_eq!(evs, vec![SseEvent::ContentBlockStart { index: 1, tool_name: Some("get_object".into()) }]);
    }
}
```

- [ ] **Step 3: 테스트 실패 확인 (모듈 미선언/미존재면 컴파일 에러 → 선언 후)**

Run: `cargo test -p geulos-ai-bridge --lib adapter::sse`
Expected: Step 2의 구현이 같은 파일에 있으므로 바로 PASS여야 한다. 만약 FAIL이면 파서 로직 수정.

- [ ] **Step 4: 통과 확인 + 커밋**

Run: `cargo test -p geulos-ai-bridge --lib adapter::sse && cargo clippy -p geulos-ai-bridge --lib -- -D warnings`
Expected: 3 tests PASS, 클린

```bash
git add ai-bridge/src/adapter/sse.rs ai-bridge/src/adapter/mod.rs
git commit -m "feat(ai-bridge): SSE 증분 파서 (sse.rs) + 청크 경계 재조립 테스트"
```

---

### Task 1.3: ClaudeAdapter `complete_streaming` override

**Files:**
- Modify: `ai-bridge/src/adapter/claude.rs`

- [ ] **Step 1: import + 스트리밍 메서드 구현**

`ai-bridge/src/adapter/claude.rs` 상단 import에 추가:

```rust
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use super::sse::{SseEvent, SseParser};
use super::StreamEvent;
```

`impl LlmAdapter for ClaudeAdapter` 안, `complete` 메서드 아래에 `complete_streaming` 추가. body 구성은 기존 `complete`와 동일하되 `"stream": true` 추가:

```rust
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
            .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.input_schema }))
            .collect();
        if let Some(Value::Object(map)) = tools_json.last_mut() {
            map.insert("cache_control".to_string(), json!({"type": "ephemeral"}));
        }
        let system_blocks = json!([{ "type": "text", "text": system, "cache_control": {"type": "ephemeral"} }]);
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

        // SSE 스트림 소비 — 텍스트/도구 블록 누적 + text_delta 즉시 emit.
        let mut parser = SseParser::new();
        let mut stream = resp.bytes_stream();
        let mut texts: Vec<String> = Vec::new(); // index→누적 텍스트는 단일 text block 가정 합치기
        let mut acc_text = String::new();
        let mut tool_uses: Vec<ToolUse> = Vec::new();
        let mut stop = LlmStop::Other;
        let mut out_tokens = 0u64;

        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    // 사용자 중단 — stream drop(연결 종료) + 부분 응답.
                    if !acc_text.is_empty() { texts.push(std::mem::take(&mut acc_text)); }
                    let _ = tx.send(StreamEvent::Cancelled).await;
                    return Ok(LlmResponse { text: texts, tool_uses, stop: LlmStop::Cancelled, tokens: (0, out_tokens) });
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
                                    SseEvent::ContentBlockStart { tool_name: Some(name), .. } => {
                                        let _ = tx.send(StreamEvent::ToolStart { turn, name: name.clone() }).await;
                                        tool_uses.push(ToolUse { id: String::new(), name, input: json!({}) });
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
                        None => break, // 스트림 종료
                    }
                }
            }
        }
        if !acc_text.is_empty() { texts.push(acc_text); }
        eprintln!("[claude-usage] streaming out_tokens={}", out_tokens);
        Ok(LlmResponse { text: texts, tool_uses, stop, tokens: (0, out_tokens) })
    }
```

> **한계 메모 (코드 주석으로 남길 것):** 이 v1은 `tool_use`의 *인자(input_json_delta)*를 누적하지 않는다 — `ToolUse.input`은 `{}`, `id`는 빈 문자열. 따라서 **스트리밍 경로는 텍스트-only 응답에 적합**하고, *도구 호출이 있는 turn*은 인자가 비어 dispatch가 깨진다. 이를 chat_session(Task 1.4)에서 처리: 도구 호출이 감지되면(stop==ToolUse 또는 tool_uses 비어있지 않음) 그 turn은 **비스트리밍 `complete`로 재요청**해 완전한 tool_use를 얻는다. 즉 스트리밍은 *텍스트 표시 전용*, 도구 dispatch는 기존 `complete` 경로 유지. (input_json_delta 누적은 후속 — spec 비목표.)

- [ ] **Step 2: 빌드 + 클린**

Run: `cargo build -p geulos-ai-bridge && cargo clippy -p geulos-ai-bridge --lib -- -D warnings`
Expected: 클린. (실제 SSE 호출 테스트는 네트워크 필요 → 단위 테스트 X, Task 1.2 파서 테스트가 핵심 커버. 수동 검증은 Phase 3.)

- [ ] **Step 3: 커밋**

```bash
git add ai-bridge/src/adapter/claude.rs
git commit -m "feat(ai-bridge): ClaudeAdapter complete_streaming — stream:true + SSE + cancel select"
```

---

### Task 1.4: MockAdapter 스트리밍 + `send_message_streaming` (등가성 TDD)

**Files:**
- Modify: `ai-bridge/src/adapter/mock.rs`
- Modify: `ai-bridge/src/chat_session.rs`

- [ ] **Step 1: MockAdapter 스트리밍 override**

먼저 `ai-bridge/src/adapter/mock.rs`를 읽어 MockAdapter가 `complete`를 어떻게 구현했는지 확인(보통 미리 설정한 `LlmResponse`를 반환). `complete_streaming`을 override해 텍스트를 두 조각으로 쪼개 emit:

```rust
    async fn complete_streaming(
        &self,
        system: &str,
        history: &[LlmMessage],
        tools: &[ToolDef],
        turn: usize,
        tx: &tokio::sync::mpsc::Sender<crate::adapter::StreamEvent>,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> crate::error::BridgeResult<LlmResponse> {
        let resp = self.complete(system, history, tools).await?;
        // 각 text 블록을 절반씩 두 델타로 쪼개 emit (스트리밍 흉내).
        for t in &resp.text {
            let mid = t.chars().count() / 2;
            let head: String = t.chars().take(mid).collect();
            let tail: String = t.chars().skip(mid).collect();
            let _ = tx.send(crate::adapter::StreamEvent::TextDelta { turn, text: head }).await;
            let _ = tx.send(crate::adapter::StreamEvent::TextDelta { turn, text: tail }).await;
        }
        Ok(resp)
    }
```

(mock.rs 상단에 필요한 use 추가: `use crate::adapter::{LlmMessage, LlmResponse, ToolDef};` 등 기존 것 재사용.)

- [ ] **Step 2: `send_message_streaming` 실패 테스트 작성**

`ai-bridge/src/chat_session.rs`의 `#[cfg(test)] mod tests`에 추가. MockAdapter가 단순 텍스트 응답(EndTurn, no tools)을 주도록 구성하고, 스트리밍 변종이 (a) tx로 델타를 보내고 (b) 최종 반환 텍스트가 비스트리밍 `send_message`와 동일한지 검증:

```rust
    #[tokio::test]
    async fn send_message_streaming_emits_deltas_and_equals_nonstreaming() {
        use crate::adapter::StreamEvent;
        // MockAdapter가 "안녕하세요 GeulOS" 텍스트 + EndTurn 반환하도록 구성.
        // (mock 구성 API는 mock.rs 참고 — with_response 등.)
        let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
        let cancel = tokio_util::sync::CancellationToken::new();
        let mut session = /* 테스트용 ChatSession<MockAdapter> 구성 */;
        let final_text = session.send_message_streaming("요약해줘", &tx, &cancel).await.unwrap();
        drop(tx);
        // 델타 수집.
        let mut streamed = String::new();
        let mut got_done = false;
        while let Some(ev) = rx.recv().await {
            match ev {
                StreamEvent::TextDelta { text, .. } => streamed.push_str(&text),
                StreamEvent::Done => got_done = true,
                _ => {}
            }
        }
        assert!(got_done, "Done 이벤트 발행");
        assert_eq!(streamed, final_text, "델타 합 == 최종 텍스트");
        assert!(final_text.contains("GeulOS"));
    }
```

> 테스트의 ChatSession 구성은 기존 `mod tests`의 다른 테스트(예: send_message 테스트)가 MockAdapter + WireClient를 어떻게 만드는지 그대로 따른다. 없으면 기존 send_message 테스트 패턴을 복제.

- [ ] **Step 3: 테스트 실패 확인**

Run: `cargo test -p geulos-ai-bridge --lib chat_session::tests::send_message_streaming`
Expected: FAIL — `send_message_streaming` 미정의.

- [ ] **Step 4: `send_message_streaming` 구현**

`chat_session.rs`에 `send_message`(현 ~88~238) 옆에 스트리밍 변종 추가. 기존 `send_message`를 거의 그대로 복제하되: (a) `adapter.complete` → `adapter.complete_streaming(..., turn, tx, cancel)`, (b) **도구 호출 turn은 비스트리밍 재요청**(아래 주석대로), (c) turn 종료/완료 시 `tx.send(StreamEvent::Done/Cancelled)`. 핵심 차이만 발췌:

```rust
    /// `send_message`의 스트리밍 변종. text_delta를 tx로 흘리며, 최종 텍스트는 동일하게 반환.
    /// 도구 호출이 있는 turn은 스트리밍이 tool_use 인자를 누적하지 않으므로(Task 1.3 한계)
    /// *비스트리밍 complete로 재요청*해 완전한 tool_use를 얻는다 — 스트리밍은 텍스트 표시 전용.
    pub async fn send_message_streaming(
        &mut self,
        user_prompt: &str,
        tx: &mpsc::Sender<StreamEvent>,
        cancel: &CancellationToken,
    ) -> BridgeResult<String> {
        let started = Instant::now();
        let mut history = self.history.clone();
        history.push(LlmMessage { role: LlmRole::User, content: Value::String(user_prompt.to_string()) });
        self.audit_event("user_prompt", json!({ "text": user_prompt })).await;

        let mut final_text = String::new();
        let mut turn = 0usize;
        loop {
            turn += 1;
            if turn > self.max_inner_turns {
                self.audit_event("end_turn", json!({ "turn": turn - 1, "reason": "max_inner_turns" })).await;
                break;
            }
            if cancel.is_cancelled() {
                let _ = tx.send(StreamEvent::Cancelled).await;
                self.audit_event("end_turn", json!({ "turn": turn - 1, "reason": "cancelled" })).await;
                // history는 commit 안 함 — 부분 응답은 caller가 final_text로 표시.
                return Ok(final_text);
            }

            // 1) 스트리밍으로 텍스트 표시 — text_delta가 tx로 흐른다.
            let resp = self.adapter
                .complete_streaming(&self.system, &history, &self.tools, turn, tx, cancel)
                .await?;

            if resp.stop == LlmStop::Cancelled {
                // adapter가 중간 cancel — 부분 텍스트 commit하고 종료.
                for t in &resp.text { if !final_text.is_empty() { final_text.push('\n'); } final_text.push_str(t); }
                history.push(LlmMessage { role: LlmRole::Assistant, content: response_to_assistant_content(&resp) });
                self.history = history;
                return Ok(final_text);
            }

            // 2) 도구 호출 turn이면 비스트리밍 complete로 재요청해 완전한 tool_use 확보.
            let resp = if !resp.tool_uses.is_empty() || resp.stop == LlmStop::ToolUse {
                self.adapter.complete(&self.system, &history, &self.tools).await?
            } else {
                resp
            };

            for t in &resp.text {
                self.audit_event("ai_text", json!({ "turn": turn, "text": t })).await;
                if !final_text.is_empty() { final_text.push('\n'); }
                final_text.push_str(t);
            }
            for tu in &resp.tool_uses {
                let _ = tx.send(StreamEvent::ToolStart { turn, name: tu.name.clone() }).await;
            }
            history.push(LlmMessage { role: LlmRole::Assistant, content: response_to_assistant_content(&resp) });

            if resp.stop == LlmStop::EndTurn && resp.tool_uses.is_empty() {
                self.audit_event("end_turn", json!({ "turn": turn, "reason": "no_tools" })).await;
                break;
            }

            // 3) 도구 dispatch — 기존 send_message의 tool loop를 그대로 사용.
            //    (send_message 본문 219줄까지의 tool_results 수집 + report_done 처리 블록을 복제.
            //     done 시 final_text에 summary 합치고 break.)
            // ↓↓↓ 기존 send_message의 tool dispatch 블록(라인 ~178-224)을 여기에 복제 ↓↓↓
            // (DRY: 가능하면 private async fn dispatch_turn_tools(&mut self, resp, turn, &mut final_text)
            //  -> bool(done) 로 추출해 send_message/streaming 양쪽이 호출. 추출 시 양쪽 수정.)
        }

        let _ = tx.send(StreamEvent::Done).await;
        self.audit_event("send_done", json!({ "total_ms": started.elapsed().as_millis() as u64, "final_text_len": final_text.len() })).await;
        self.history = history;
        Ok(final_text)
    }
```

> **DRY 권장:** 도구 dispatch 루프(`send_message`의 ~178-224)를 `async fn dispatch_turn_tools(&mut self, resp: &LlmResponse, turn: usize, final_text: &mut String) -> BridgeResult<bool>`로 추출하고 `send_message`도 그것을 호출하도록 리팩터. 추출이 부담되면 v1은 블록 복제 + "DRY: M-후속 추출" 주석. 어느 쪽이든 *동작은 동일*해야 한다.

상단 import에 추가: `use crate::adapter::StreamEvent; use tokio::sync::mpsc; use tokio_util::sync::CancellationToken;`.

- [ ] **Step 5: 테스트 통과 + 클린 + 커밋**

Run: `cargo test -p geulos-ai-bridge --lib && cargo clippy -p geulos-ai-bridge --all-targets -- -D warnings`
Expected: 신규 테스트 포함 전체 PASS, 클린

```bash
git add ai-bridge/src/adapter/mock.rs ai-bridge/src/chat_session.rs
git commit -m "feat(ai-bridge): chat_session.send_message_streaming + MockAdapter 스트리밍 (등가성 테스트)"
```

---

# Phase 2 — desktop-shell plumbing

### Task 2.1: 적응형 throttle 헬퍼 `should_flush`

**Files:**
- Modify: `apps/desktop-shell/src/ai_session.rs`

- [ ] **Step 1: 실패 테스트 작성**

`ai_session.rs`의 `#[cfg(test)] mod tests`에 추가:

```rust
    #[test]
    fn should_flush_on_time_or_length() {
        use std::time::Duration;
        assert!(!should_flush(Duration::from_millis(10), 5), "둘 다 미달 → false");
        assert!(should_flush(Duration::from_millis(90), 1), "80ms 경과 → true");
        assert!(should_flush(Duration::from_millis(10), 45), "40자 누적 → true");
        assert!(should_flush(Duration::from_millis(80), 40), "경계값 → true");
    }
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test -p geulos-desktop-shell should_flush_on_time_or_length`
Expected: FAIL — 미정의.

- [ ] **Step 3: 구현**

`ai_session.rs`에 추가 (헬퍼 영역):

```rust
/// 스트리밍 delta를 flush(SetState broadcast)할지 — 적응형 max(80ms, 40자) (KI: AI streaming v1).
/// 마지막 flush 후 80ms 경과 OR 40자 이상 누적 시 true.
pub fn should_flush(since_last_flush: std::time::Duration, pending_chars: usize) -> bool {
    since_last_flush >= std::time::Duration::from_millis(80) || pending_chars >= 40
}
```

- [ ] **Step 4: 통과 + 커밋**

Run: `cargo test -p geulos-desktop-shell should_flush_on_time_or_length`
Expected: PASS

```bash
git add apps/desktop-shell/src/ai_session.rs
git commit -m "feat(desktop-shell): 적응형 스트리밍 throttle should_flush(80ms/40자)"
```

---

### Task 2.2: `Cli@1` streaming state

**Files:**
- Modify: `core/src/object/std_types.rs:326-340` (`cli` fn)

- [ ] **Step 1: state 2개 추가**

`std_types.rs`의 `cli()` 함수에서 `obj.set_state("pending_action", ...)` 다음 줄에 추가:

```rust
    // AI streaming v1: 라이브 누적 텍스트 + 활성 플래그. Done/Cancelled 시 lines로 commit.
    obj.set_state("streaming_text", json!(""));
    obj.set_state("streaming_active", json!(false));
```

- [ ] **Step 2: 빌드 (core 영향 범위 확인)**

Run: `cargo build -p geulos-core && cargo test -p geulos-core --lib`
Expected: 성공 (state 추가는 하위호환).

- [ ] **Step 3: 커밋**

```bash
git add core/src/object/std_types.rs
git commit -m "feat(core): Cli@1에 streaming_text/streaming_active state (AI streaming v1)"
```

---

### Task 2.3: `CliChatSession.send_streaming` + 채널 타입

**Files:**
- Modify: `apps/desktop-shell/src/ai_session.rs`

- [ ] **Step 1: `send_streaming` 추가**

`ai_session.rs`의 `CliChatSession`에 `send`(현 ~88-99) 옆에 추가. inner의 streaming 변종을 호출하고 매 send 직후 디스크 dump(기존 동작 유지):

```rust
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
```

> `StreamEvent`가 `geulos_ai_bridge::adapter`에서 `pub` 재export되는지 확인. 안 되어 있으면 `ai-bridge/src/lib.rs` 또는 `adapter/mod.rs`에서 `pub use` 노출 (Task 1.1에서 이미 pub enum이면 경로만 맞추면 됨).

- [ ] **Step 2: 빌드 + 커밋**

Run: `cargo build -p geulos-desktop-shell`
Expected: 성공

```bash
git add apps/desktop-shell/src/ai_session.rs
git commit -m "feat(desktop-shell): CliChatSession.send_streaming (StreamEvent 채널 + cancel)"
```

---

### Task 2.4: main loop stream arm + 적응형 throttle + SetState

**Files:**
- Modify: `apps/desktop-shell/src/main.rs` (AiResult 영역 ~44, 채널 선언 ~619, select! arm ~679, dispatch 호출부 ~954-964)

**배경:** 기존 AI dispatch는 `submit_input`이 `tokio::spawn`으로 `session.send()`를 돌리고 결과 1건을 `ai_response_tx`로 보낸 뒤 `handle_ai_response`가 sentinel 교체. 스트리밍은 **`ConsoleEvent → apply_console_line` 패턴(main.rs ~700)을 미러링** — 별 채널 `stream_rx`로 `StreamEvent`를 받아 적응형 throttle 후 `Cli.streaming_text` SetState.

- [ ] **Step 1: 스트리밍 채널 + CancellationToken 선언**

`main.rs` ~619의 `ai_response_tx/rx` 선언 근처에 추가:

```rust
    // AI streaming v1: 델타 채널 (spawned AI task → main loop) + 활성 스트림 cancel 토큰.
    let (stream_tx, mut stream_rx) =
        tokio::sync::mpsc::channel::<geulos_ai_bridge::adapter::StreamEvent>(256);
    let mut ai_cancel: Option<tokio_util::sync::CancellationToken> = None;
    // throttle 상태: (누적 streaming_text, 마지막 flush 시각, 활성 Cli target).
    let mut stream_accum = String::new();
    let mut stream_last_flush = std::time::Instant::now();
    let mut stream_target: Option<ObjectId> = None;
```

- [ ] **Step 2: dispatch를 스트리밍으로 — submit_input의 AI spawn 교체**

`main.rs` ~954-964의 AI dispatch(현재 `session.send()` spawn → `ai_response_tx`)를 읽고, 스트리밍 변종으로 교체. spawn 안에서 `send_streaming(prompt, &stream_tx, &cancel)`를 호출하고, 완료 시 최종 결과를 기존 `ai_response_tx`로도 보낸다(에러/최종 commit 신호). cancel 토큰은 spawn 전에 생성해 `ai_cancel`에 저장:

```rust
    // 새 토큰 발급 — 이전 스트림이 있으면 먼저 cancel(보통 없음, 직렬 입력).
    let cancel = tokio_util::sync::CancellationToken::new();
    ai_cancel = Some(cancel.clone());
    stream_target = Some(cli_target);
    stream_accum.clear();
    stream_last_flush = std::time::Instant::now();
    // streaming_active=true SetState (sentinel 대체).
    // (기존 sentinel echo 라인은 유지하거나 streaming_text 라이브로 대체 — 아래 arm에서 처리.)
    let stream_tx2 = stream_tx.clone();
    let chat = std::sync::Arc::clone(&chat_session);
    let resp_tx = ai_response_tx.clone();
    tokio::spawn(async move {
        let mut guard = chat.lock().await;
        if let Some(session) = guard.as_mut() {
            let r = session.send_streaming(&prompt, &stream_tx2, &cancel).await
                .map_err(|e| e.to_string());
            let _ = resp_tx.send(AiResult {
                cli_target,
                result: r,
                sentinel: String::new(),    // 스트리밍 모드: sentinel 미사용
                prompt_prefix,
            }).await;
        }
    });
```

> 기존 dispatch와의 차이를 최소화하라 — `prompt`, `cli_target`, `prompt_prefix` 추출은 기존 코드 그대로. sentinel 기반 "(응답 대기 중...)"은 스트리밍에선 streaming_active로 대체하므로 빈 문자열. 기존 `handle_ai_response`는 sentinel="" 일 때 단순히 최종 결과를 처리하도록 동작 확인(또는 분기 추가).

- [ ] **Step 3: `stream_rx` select! arm 추가 (ConsoleEvent arm 미러링)**

`main.rs` ~679의 `ai_response_rx` arm 근처(같은 select! 블록)에 추가:

```rust
            // AI streaming v1: 델타 수신 → 적응형 throttle → Cli.streaming_text SetState.
            Some(ev) = stream_rx.recv() => {
                use geulos_ai_bridge::adapter::StreamEvent;
                match ev {
                    StreamEvent::TextDelta { text, .. } => {
                        stream_accum.push_str(&text);
                        let pending = stream_accum.chars().count();
                        if ai_session::should_flush(stream_last_flush.elapsed(), pending) {
                            if let Some(t) = stream_target {
                                set_cli_streaming(&mut stream, &mut mounted_objects, &mut req_seq, t, &stream_accum, true).await;
                            }
                            stream_last_flush = std::time::Instant::now();
                        }
                    }
                    StreamEvent::ToolStart { name, .. } => {
                        stream_accum.push_str(&format!("\n(도구 실행: {})\n", name));
                        if let Some(t) = stream_target {
                            set_cli_streaming(&mut stream, &mut mounted_objects, &mut req_seq, t, &stream_accum, true).await;
                        }
                        stream_last_flush = std::time::Instant::now();
                    }
                    StreamEvent::Done | StreamEvent::Cancelled | StreamEvent::Error { .. } => {
                        // 누적 텍스트를 lines로 commit + streaming 비활성.
                        let suffix = match ev {
                            StreamEvent::Cancelled => "\n[중단됨]",
                            StreamEvent::Error { .. } => "\n[연결 끊김]",
                            _ => "",
                        };
                        if let Some(t) = stream_target.take() {
                            commit_cli_streaming(&mut stream, &mut mounted_objects, &mut req_seq, t, &stream_accum, suffix).await;
                        }
                        stream_accum.clear();
                        ai_cancel = None;
                    }
                }
                continue;
            }
```

- [ ] **Step 4: `set_cli_streaming` / `commit_cli_streaming` 헬퍼 작성**

`main.rs` 하단(또는 ai_session)의 헬퍼 영역에 두 함수 추가. `apply_console_line`(shellrunner_methods)의 SetState 패턴을 그대로 따른다:

```rust
/// 라이브 streaming_text + streaming_active SetState (broadcast + local 동기 갱신).
async fn set_cli_streaming(
    stream: &mut tokio::net::TcpStream,
    mounted_objects: &mut [Object],
    req_seq: &mut u64,
    target: ObjectId,
    text: &str,
    active: bool,
) {
    for (key, val) in [("streaming_text", serde_json::json!(text)), ("streaming_active", serde_json::json!(active))] {
        if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target) {
            o.state.insert(key.to_string(), val.clone()); // KI-018 local 동기 갱신
        }
        *req_seq += 1;
        let ss = geulos_proto::StateSetMsg {
            request_id: format!("r-cli-stream-{}", req_seq),
            target: target.to_string(), key: key.to_string(), value: val,
        };
        use tokio::io::AsyncWriteExt;
        let _ = stream.write_all(&geulos_proto::encode_frame(&serde_json::to_vec(&ss).unwrap_or_default())).await;
    }
}

/// 누적 streaming_text를 lines에 1줄 append + streaming_text 비움 + streaming_active=false.
async fn commit_cli_streaming(
    stream: &mut tokio::net::TcpStream,
    mounted_objects: &mut [Object],
    req_seq: &mut u64,
    target: ObjectId,
    text: &str,
    suffix: &str,
) {
    let full = format!("{}{}", text, suffix);
    // lines 읽어 append.
    let mut lines: Vec<String> = mounted_objects.iter().find(|o| o.id == target)
        .and_then(|o| o.state.get("lines")).and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if !full.trim().is_empty() { lines.push(full); }
    let lines_val = serde_json::json!(lines);
    // 3 SetState: lines, streaming_text="", streaming_active=false.
    for (key, val) in [
        ("lines", lines_val),
        ("streaming_text", serde_json::json!("")),
        ("streaming_active", serde_json::json!(false)),
    ] {
        if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target) {
            o.state.insert(key.to_string(), val.clone());
        }
        *req_seq += 1;
        let ss = geulos_proto::StateSetMsg {
            request_id: format!("r-cli-commit-{}", req_seq),
            target: target.to_string(), key: key.to_string(), value: val,
        };
        use tokio::io::AsyncWriteExt;
        let _ = stream.write_all(&geulos_proto::encode_frame(&serde_json::to_vec(&ss).unwrap_or_default())).await;
    }
}
```

> 실제 `StateSetMsg`/`encode_frame` 사용법은 `shellrunner_methods::apply_console_line`(main.rs ~700이 호출)에서 그대로 복사. import/시그니처를 그 함수에 맞춰 조정.

- [ ] **Step 5: 빌드 + 클린 + 커밋**

Run: `cargo build -p geulos-desktop-shell && cargo clippy -p geulos-desktop-shell --lib -- -D warnings`
Expected: 클린 (새 경고 0)

```bash
git add apps/desktop-shell/src/main.rs apps/desktop-shell/src/ai_session.rs
git commit -m "feat(desktop-shell): AI 스트리밍 plumbing — stream_rx arm + 적응형 throttle + Cli.streaming_text SetState"
```

---

# Phase 3 — 라이브 렌더 + 중단

### Task 3.1: `Cli@1.interrupt_ai` 메서드 + 핸들러 + ACL

**Files:**
- Modify: `core/src/object/std_types.rs` (`cli` fn — method 등록)
- Modify: `apps/desktop-shell/src/main.rs` (interrupt 처리 — ai_cancel.cancel())
- Modify: cli invoke 라우팅 + ACL (handlers)

- [ ] **Step 1: 메서드 시그니처 등록**

`std_types.rs::cli()`의 methods push 영역에 추가:

```rust
    obj.methods.push(MethodSig::new("interrupt_ai"));
```

- [ ] **Step 2: invoke 라우팅 + cancel**

desktop-shell의 Cli invoke 라우팅을 읽고(`submit_input`/`clear`/`append_line`을 어디서 매칭하는지 grep `"submit_input"`), `"interrupt_ai"` arm 추가. main loop의 `ai_cancel`에 접근 가능한 위치라면 직접 `if let Some(c) = &ai_cancel { c.cancel(); }`. 라우팅이 핸들러 함수로 분리돼 있으면 cancel 토큰을 거기로 전달하거나, invoke를 main loop arm에서 가로채 처리.

> **핵심:** `ai_cancel`은 main loop local. invoke가 wire로 들어오면 stream.read arm에서 처리되므로, 그 처리부에서 method=="interrupt_ai"이면 `ai_cancel`을 cancel하면 된다. 별 핸들러로 빼지 말고 main loop의 invoke 처리 지점에서 직접 cancel하는 게 가장 단순(상태 근접).

- [ ] **Step 3: ACL — system:compositor + ai + user 허용**

cli 객체 ACL 설정부(grep `add_ui_object_acl` 또는 cli mount 지점)를 읽고, `interrupt_ai`를 invoke 허용 목록에 추가. "동일 명령표면" 원칙(memory `feedback_ai_user_identical_command_surface`)대로 AI/사용자/compositor 모두 호출 가능. (Dialog.respond처럼 compositor 전용으로 막지 말 것 — 중단은 누구나 가능해야.)

- [ ] **Step 4: 빌드 + 커밋**

Run: `cargo build -p geulos-core -p geulos-desktop-shell && cargo clippy -p geulos-desktop-shell --lib -- -D warnings`
Expected: 클린

```bash
git add core/src/object/std_types.rs apps/desktop-shell/src/
git commit -m "feat: Cli@1.interrupt_ai 메서드 + cancel 라우팅 + ACL (동일 명령표면)"
```

---

### Task 3.2: 컴포지터 라이브 렌더 (`render_cli`)

**Files:**
- Modify: `compositor/src/render.rs:728+` (`render_cli`)

- [ ] **Step 1: streaming_text를 visible lines 뒤에 렌더 + 커서**

`render_cli`(현 728~782의 visible 렌더 루프 직후, 입력 라인 렌더 전)에 추가. `streaming_active`면 `streaming_text`를 wrap해 이어 그리고 끝에 커서 `█`:

```rust
    // AI streaming v1: 라이브 누적 텍스트 (확정 lines 아래, 입력 라인 위) + 커서.
    let streaming_active = obj.state.get("streaming_active").and_then(|v| v.as_bool()).unwrap_or(false);
    if streaming_active {
        let stream_text = obj.state.get("streaming_text").and_then(|v| v.as_str()).unwrap_or("");
        let cursor_text = format!("{}█", stream_text);
        for vline in crate::editor::wrap_by_pixel_width(&cursor_text, cli_wrap_w) {
            if y + CLI_LINE_HEIGHT > text_bottom { break; }
            draw_text(buffer, w, h, &vline.text, text_x, y, theme::TERMINAL_TEXT);
            y += CLI_LINE_HEIGHT;
        }
    }
```

> 실제 wrap/draw API(`wrap_by_pixel_width`, `draw_text`, `CLI_LINE_HEIGHT`, `text_bottom`, `cli_wrap_w`)는 render_cli 내 기존 변수 그대로. 입력 라인 렌더가 streaming 뒤에 오도록 위치 조정(스트리밍 중엔 입력 라인이 비어있어도 무방).

- [ ] **Step 2: 빌드 (host + VM compositor)**

Run: `cargo build -p geulos-compositor`
Expected: 성공

- [ ] **Step 3: 커밋**

```bash
git add compositor/src/render.rs
git commit -m "feat(compositor): render_cli 라이브 streaming_text + 커서 █"
```

---

### Task 3.3: Esc → `interrupt_ai` invoke (양 백엔드)

**Files:**
- Modify: `compositor/src/main.rs` (host winit 입력)
- Modify: `compositor/src/bin/geulos-vm-compositor*` (VM evdev 입력)

- [ ] **Step 1: host winit Esc 처리**

compositor host `main.rs`의 키 처리(grep `NamedKey::Escape` 또는 기존 Esc 분기)를 읽고, `keyboard_focus==Cli && streaming_active`일 때 `Cli@1.interrupt_ai` invoke 송신. 기존 Esc 용도(rename 취소 등)와 충돌 않도록 *streaming_active 조건 우선* 분기:

```rust
    // AI 스트리밍 중 Esc → interrupt_ai invoke (rename 취소 등 다른 Esc보다 우선).
    if is_escape && cli_streaming_active {
        send_invoke(cli_id, "interrupt_ai", json!({}));
        // 그 외 Esc 처리는 else로.
    }
```

> `cli_streaming_active`는 컴포지터가 보유한 Cli 객체 state에서 읽음(이미 streaming_text 렌더로 접근 중). `send_invoke` 헬퍼는 기존 invoke 송신 패턴 사용(예: 버튼 press invoke).

- [ ] **Step 2: VM compositor Esc 처리**

`bin/geulos-vm-compositor`의 evdev 키 처리에서 동형 분기 추가(Esc keycode → 동일 invoke). 두 백엔드 parity 유지(메모리: VM compositor parity).

- [ ] **Step 3: 빌드 + 클린 + 커밋**

Run: `cargo build -p geulos-compositor && cargo clippy -p geulos-compositor --all-targets -- -D warnings`
Expected: 클린

```bash
git add compositor/src/
git commit -m "feat(compositor): Esc → Cli.interrupt_ai invoke (host + VM 백엔드)"
```

---

### Task 3.4: VM end-to-end 검증

**Files:** (없음 — 검증 전용)

- [ ] **Step 1: workspace 전체 그린**

Run: `cargo test --workspace`
Expected: 신규 테스트 포함 PASS (compositor layout_test의 폰트 의존 실패 10건은 KI-033 기존 환경부채 — 무관).

Run: `cargo clippy -p geulos-ai-bridge -p geulos-desktop-shell -p geulos-core -p geulos-compositor --all-targets -- -D warnings`
Expected: 변경 crate 클린.

- [ ] **Step 2: VM 빌드 + 부팅**

(메모리 `project_vm_build_run_invocation` 절차 — bash에서 powershell.exe + bash 리다이렉트.)

```bash
powershell.exe -NoProfile -File boot/build.ps1 -Release > boot/build-stream.log 2>&1; tail -5 boot/build-stream.log
rm -f boot/serial.log
powershell.exe -NoProfile -File boot/qemu/launch.ps1 -Graphics > boot/launch-stream.log 2>&1 &
sleep 25; grep -iE "geulosd listening|HelloAck|vm-compositor" boot/serial.log | tr -d '\r'
```
Expected: 정상 부팅 신호. GUI 창에서 `/ai start` → 프롬프트 입력 → **텍스트가 토큰 단위로 점진 표시** + Esc로 중단 시 "[중단됨]" 육안 확인. (육안 확인은 사용자가 수행 — 자동 불가.)

- [ ] **Step 3: 정리 + known-issues + ADR**

VM 종료(`taskkill //F //IM qemu-system-x86_64.exe`), verify 로그 삭제. `docs/known-issues.md` "우선 검토"에서 AI streaming 제거 + 후속(옵션 토글/input_json_delta/SSE 재연결) 기록. ADR-042 신설(AI streaming 결정).

```bash
rm -f boot/build-stream.log boot/launch-stream.log
git add docs/known-issues.md docs/adr/042-ai-response-streaming.md
git commit -m "docs: AI 응답 스트리밍 마감 — known-issues 갱신 + ADR-042"
```

---

## Self-Review 메모

- **Spec 커버리지:** SSE 파서(T1.2)/trait+StreamEvent(T1.1)/ClaudeAdapter stream(T1.3)/등가성+Mock(T1.4)/throttle(T2.1)/Cli state(T2.2)/send_streaming(T2.3)/main plumbing(T2.4)/interrupt 메서드(T3.1)/라이브 렌더(T3.2)/Esc(T3.3)/검증(T3.4) — spec C1~C6 + 3 Phase 전부 매핑.
- **타입 일관성:** `StreamEvent{TextDelta{turn,text},ToolStart{turn,name},Done,Cancelled,Error{message}}` (T1.1 정의 → T1.3/1.4/2.4 사용 일치). `LlmStop::Cancelled`(T1.1→T1.3/1.4). `should_flush(Duration,usize)->bool`(T2.1→T2.4). `streaming_text`/`streaming_active` state 이름(T2.2→T2.4/T3.2). `interrupt_ai` 메서드명(T3.1→T3.3). `set_cli_streaming`/`commit_cli_streaming`(T2.4 정의·사용).
- **명시한 미해결 통합점(실행 시 read 필요):** mock.rs의 complete 구성 API, chat_session test의 ChatSession 생성 패턴, send_message tool dispatch 블록(DRY 추출 권장), main.rs의 정확한 AI dispatch 지점(~954-964)·invoke 라우팅·cli ACL 지점, StateSetMsg/encode_frame 사용법(apply_console_line 참조), compositor 입력 핸들러의 Esc/invoke 송신 패턴. 각 Task에 anchor + 미러 대상 명시.
- **알려진 v1 한계(의도적, spec 비목표):** 스트리밍은 텍스트 표시 전용 — 도구 호출 turn은 비스트리밍 `complete`로 재요청(tool_use 인자 누적 X). input_json_delta·옵션 토글·SSE 재연결은 후속.
- **범위:** 한 spec, 3 Phase 순차(2→1, 3→2 의존). 독립 subsystem 아님 → 분할 불요.
