# GeulOS AI 응답 스트리밍 (Anthropic SSE) — 설계

> **Status:** designed (2026-06-02)

## 목표

AI 응답을 *완성 후 한 번에* 표시하던 모델을 Anthropic SSE 스트리밍으로 전환해 **첫 토큰까지의 체감 지연을 1~3초 단축**한다. 사용자는 텍스트가 토큰 단위로 점진 표시되는 것을 보며, 긴 응답을 *중단(interrupt)* 할 수 있다.

근거: `docs/known-issues.md` "정기 검토 시점 — AI 응답 streaming (Anthropic SSE): 응답 첫 토큰까지 1-3초 빨라짐 — 큰 UX 개선." 사용자 결정(2026-06-02): 견고성 하드닝 다음 작업으로 선택.

## 확정된 요구사항 (브레인스토밍 2026-06-02)

1. **스트리밍 범위 = 모든 turn 텍스트.** AI는 한 user prompt에 대해 여러 inner turn을 돌며 (도구 호출 사이에) 중간 "thinking" 텍스트도 생성한다. 그 *모든* assistant 텍스트를 점진 표시한다 (예: turn1 "README를 읽겠습니다" → 도구 진행 마커 → turn2 최종 답변). 도구 호출 자체는 진행 마커로 표시.
2. **flush = 적응형 `max(80ms, 40자)`.** 누적 delta를 80ms 경과 *또는* 40자 누적 중 먼저 도달 시 1회 flush(SetState broadcast). 초반 burst는 빠르게, 긴 텍스트는 부하 균등. 응답당 SetState ~15~30건 (토큰당 broadcast 시 50~100건 → 회피).
3. **중단(interrupt) v1 포함.** 사용자가 Esc로 스트리밍 중단 → SSE 연결 drop → *지금까지 받은 부분 텍스트는 보존*하고 "[중단됨]" 표시. OS "모든 동작 중단가능" 비전과 정합. 중단은 invoke 가능한 객체 메서드(`Cli@1.interrupt_ai`)로 — AI/외부 클라이언트도 호출 가능 (메모리 `feedback_ai_user_identical_command_surface`).

## 비목표 (YAGNI / 후속)

- 스트리밍 *옵션 토글* (사용자 설정으로 켜고 끄기) — v1은 항상 스트리밍. 설정화는 후속.
- `input_json_delta` (도구 인자 스트리밍 표시) — 도구 인자는 완성 후 한 번에 dispatch. 인자 자체를 점진 표시하지 않음.
- thinking/extended-thinking 블록 — 현재 모델 호출에 thinking 미사용. 도입 시 별 작업.
- SSE 재연결/재시도 — 연결 drop은 부분 commit + 에러 표시로 graceful 종료, 자동 재시도 안 함.

## 아키텍처 — 4층 데이터 흐름

```
① ai-bridge/src/adapter/claude.rs
   complete_streaming(system, history, tools, tx: Sender<StreamEvent>, cancel: CancellationToken)
     → reqwest POST (stream:true) → resp.bytes_stream()
     → adapter/sse.rs::SseParser 로 청크 누적·파싱
     → 매 text_delta 마다 tx.send(StreamEvent::TextDelta { turn, text })
     → 종료 시 누적된 text/tool_uses/stop/usage 로 LlmResponse 재구성 반환
     → cancel 발동 시 stream drop + 부분 LlmResponse 반환 (stop=Cancelled)

② ai-bridge/src/chat_session.rs
   send_message_streaming(user_prompt, tx, cancel) -- 기존 send_message의 스트리밍 변종
     turn loop 각 turn에서 complete_streaming 호출, tx로 델타 forward,
     turn 경계/도구 호출 시 tx.send(StreamEvent::ToolStart { name }) 등 마커 발행

③ apps/desktop-shell/src/ai_session.rs
   CliChatSession.send_streaming(prompt, tx, cancel) -- inner.send_message_streaming wrap
     매 send 직후 디스크 dump (기존 동작 유지)

④ apps/desktop-shell main loop
   기존 AI dispatch(tokio::spawn)를 스트리밍 버전으로:
     - stream_rx: mpsc::Receiver<StreamEvent> 신규 select! arm
     - 적응형 throttle 누적 → SetState(Cli.streaming_text = 누적본)
     - StreamEvent::Done/Cancelled → streaming_text를 lines로 commit + streaming_text 비움 + streaming_active=false
     - CancellationToken은 desktop-shell이 보유; Cli.interrupt_ai invoke 시 cancel()

⑤ compositor (bin/geulos-vm-compositor + host winit)
   render_cli: 기존 lines(확정) 아래에 streaming_text(라이브) + 끝에 커서 █
   Esc 키 (focus=Cli, streaming_active일 때) → Cli@1.interrupt_ai invoke 송신
```

## 컴포넌트 (단위별 책임·인터페이스)

### C1. SSE 파서 — `ai-bridge/src/adapter/sse.rs` (신규)

순수 증분 파서. 네트워크/async 무관 — 바이트 청크를 받아 완성된 SSE 이벤트를 뱉는다.

```rust
/// Anthropic messages SSE 이벤트 (필요한 것만).
pub enum SseEvent {
    MessageStart { input_tokens: u64, cache_read: u64, cache_creation: u64 },
    ContentBlockStart { index: usize, block: BlockStart }, // Text | ToolUse{id,name}
    TextDelta { index: usize, text: String },
    MessageDelta { stop_reason: Option<String>, output_tokens: u64 },
    MessageStop,
    Ping,            // 무시
    Other,           // 무시 (input_json_delta 등)
}

pub struct SseParser { buf: Vec<u8> }
impl SseParser {
    pub fn new() -> Self;
    /// 바이트 청크 push 후, 지금까지 완성된 (event,data) 블록을 파싱해 반환.
    /// 미완 블록은 내부 buf에 보존. SSE 프레임 경계는 `\n\n`.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent>;
}
```

- **테스트:** canned SSE 바이트열(여러 청크로 쪼갠 경우 포함)을 push → 기대 이벤트 시퀀스. 프레임이 청크 경계에 걸쳐도 정확히 재조립.

### C2. `LlmAdapter` trait 확장 — `ai-bridge/src/adapter/mod.rs`

```rust
/// 스트리밍 이벤트 — adapter → 상위로 흐르는 점진 신호.
pub enum StreamEvent {
    TextDelta { turn: usize, text: String },
    ToolStart { turn: usize, name: String },
    Done,                 // 한 user-turn 정상 종료
    Cancelled,            // 사용자/외부 중단
    Error { message: String },
}

#[async_trait]
pub trait LlmAdapter: Send + Sync {
    async fn complete(...) -> Result<LlmResponse, BridgeError>;  // 기존 유지

    /// 스트리밍 1회 round-trip. text_delta를 tx로 즉시 흘리고, 종료 시 full LlmResponse 반환.
    /// 기본 구현: complete() 호출 후 전체 텍스트를 한 번에 tx로 emit (비스트리밍 어댑터 호환).
    async fn complete_streaming(
        &self, system: &str, history: &[LlmMessage], tools: &[ToolDef],
        turn: usize, tx: &mpsc::Sender<StreamEvent>, cancel: &CancellationToken,
    ) -> Result<LlmResponse, BridgeError> { /* default: complete()→emit once */ }
}
```

- `LlmStop`에 `Cancelled` variant 추가.
- ClaudeAdapter는 `complete_streaming`을 override (실제 SSE). MockAdapter는 텍스트를 몇 조각으로 쪼개 emit (테스트용).
- **기존 `complete` 호출자(standalone scenario 등) 무영향.**

### C3. ClaudeAdapter 스트리밍 구현 — `ai-bridge/src/adapter/claude.rs`

- body에 `"stream": true` 추가 (system/tools cache_control는 그대로 — 스트리밍도 usage 보고).
- `resp.bytes_stream()` (reqwest `stream` feature) 을 `tokio::select!`로 cancel과 함께 polling.
- 각 청크를 `SseParser::push` → 이벤트별:
  - `TextDelta` → 누적 text 블록에 append + `tx.send(StreamEvent::TextDelta)`.
  - `ContentBlockStart{ToolUse}` → tool_use 골격 시작 (인자는 후속 input_json_delta 누적, 표시는 안 함).
  - `MessageDelta` → stop_reason/output_tokens 기록.
  - `MessageStop` → 루프 종료.
- cancel 발동: select!에서 cancel 분기 → stream drop(연결 종료) → 부분 LlmResponse(stop=Cancelled) 반환.
- 종료 시 `[claude-usage]` eprintln 유지 (캐시 동작 관측).

### C4. 적응형 throttle — `apps/desktop-shell/src/ai_session.rs` (헬퍼)

```rust
/// 누적 delta를 flush할지. 마지막 flush 후 80ms 경과 OR 40자 누적 시 true.
pub fn should_flush(since_last_flush: Duration, pending_chars: usize) -> bool {
    since_last_flush >= Duration::from_millis(80) || pending_chars >= 40
}
```

- **테스트:** (10ms, 5자)→false / (90ms, 1자)→true / (10ms, 45자)→true.
- main loop가 이 헬퍼로 stream_rx 델타를 배치, true일 때만 SetState.

### C5. `Cli@1` 객체 — desktop-shell std_types + handler

- state 2개 추가: `streaming_text: String` (라이브 누적), `streaming_active: bool`.
- **생애주기:** 한 `send_streaming`(=한 user-turn) 동안 `streaming_text`는 *모든 inner turn의 텍스트 + 도구 마커*를 가로질러 누적된다 (turn 경계마다 reset 안 함). `StreamEvent::Done`(user-turn 종료) 도착 시 누적본 전체를 `lines`로 *1회* commit하고 `streaming_text=""`. 즉 비스트리밍 `send_message`의 `final_text`와 동일한 최종본이 lines에 남는다 (등가성).
- 메서드 `interrupt_ai()` — desktop-shell이 보유한 CancellationToken.cancel() 호출. ACL: `system:compositor`(Esc 경로) + `ai:*`/사용자 invoke 허용 (동일 명령표면). 활성 스트림 없으면 noop.
- 완료/중단 시 desktop-shell이 streaming_text를 lines로 commit("[중단됨]" suffix는 Cancelled일 때), streaming_text="", streaming_active=false → SetState 3건.

### C6. 컴포지터 라이브 렌더 + Esc — compositor

- `render_cli`: 기존 `lines` 렌더 후 `streaming_active`면 `streaming_text`를 이어서 렌더 + 마지막에 커서 `█` (회색 점멸 불필요, 정적 블록).
- Esc 키: `keyboard_focus==Cli && streaming_active` 일 때 `Cli@1.interrupt_ai` invoke 송신. (그 외 Esc는 기존 동작 유지 — rename 취소 등과 충돌 없게 조건 분리.)
- VM 컴포지터(`bin/geulos-vm-compositor`)와 host winit `main.rs` 양쪽 적용 (parity).

## 에러 처리

- **SSE 네트워크 drop** (bytes_stream Err): 지금까지 누적 텍스트로 부분 LlmResponse + `StreamEvent::Error` → desktop-shell이 부분 commit + "[연결 끊김]" 표시. 자동 재시도 안 함 (사용자가 재입력).
- **사용자 Esc**: `StreamEvent::Cancelled` → 부분 commit + "[중단됨]".
- **빈 응답**: 기존처럼 명시 피드백 (M11.1 I-1 fix 유지).
- **prompt caching**: 스트리밍 응답도 `message_start`/`message_delta` usage에 cache_read 보고 — 기존 효율 측정 유지.
- 이 경로는 Anthropic HTTP SSE 전용 — 객체서버 wire timeout(KI-032)과 무관한 별 채널.

## 테스트 전략

- **C1 SSE 파서** (단위, 핵심): canned 바이트 → 이벤트 시퀀스; 청크 경계 분할 케이스.
- **C2/C3 어댑터**: MockAdapter 스트리밍이 N개 델타 + 최종 응답 emit 검증. ClaudeAdapter는 SSE 파서 경유라 파서 테스트가 핵심 커버.
- **C4 throttle**: `should_flush` 경계값 테스트.
- **chat_session**: `send_message_streaming`이 MockAdapter로 텍스트 델타를 tx에 forward + 최종 텍스트가 비스트리밍 `send_message`와 동일한지 (등가성).
- **중단**: cancel token 발동 → 부분 텍스트 보존 + stop=Cancelled.
- **수동/VM**: `boot/build.ps1` + `launch.ps1 -Graphics`로 실제 스트리밍 점진 표시 + Esc 중단 육안 확인 (메모리 `project_vm_build_run_invocation`의 정상부팅 신호 + serial.log).

## 구현 단계 (한 spec, 단계적 plan)

- **Phase 1 — 어댑터 SSE 스트리밍.** C1(sse.rs) + C2(trait 확장) + C3(ClaudeAdapter stream). 첫 소비자 = standalone `ai-bridge/src/main.rs` CLI가 델타를 stdout에 출력. SSE 파싱 de-risk + 즉시 가치 (dev CLI 스트리밍).
- **Phase 2 — desktop-shell plumbing.** C4(throttle) + C5의 state 부분 + main loop stream_rx arm + SetState(streaming_text). 컴포지터는 아직 라이브 미렌더라도 SetState 누적 → lines commit은 동작.
- **Phase 3 — 라이브 렌더 + 중단.** C5 메서드(interrupt_ai) + C6(컴포지터 커서 렌더 + Esc invoke). VM 부팅 end-to-end 검증.

각 Phase는 빌드·테스트·커밋 독립. Phase 2는 Phase 1에, Phase 3은 Phase 2에 의존.

## 영향받는 파일

| 파일 | 변경 |
|---|---|
| `ai-bridge/src/adapter/sse.rs` | **신규** — SSE 증분 파서 |
| `ai-bridge/src/adapter/mod.rs` | `StreamEvent`/`LlmStop::Cancelled` + trait `complete_streaming` 기본구현 |
| `ai-bridge/src/adapter/claude.rs` | `complete_streaming` override (stream:true + bytes_stream + SSE) |
| `ai-bridge/src/adapter/mock.rs` | MockAdapter 스트리밍 (델타 emit) |
| `ai-bridge/src/chat_session.rs` | `send_message_streaming` turn loop |
| `ai-bridge/src/main.rs` | standalone CLI 델타 stdout 출력 (Phase 1 소비자) |
| `ai-bridge/Cargo.toml` | reqwest `stream` feature, `tokio-util`(CancellationToken), `futures-util`(StreamExt) |
| `apps/desktop-shell/src/ai_session.rs` | `send_streaming` + `should_flush` 헬퍼 |
| `apps/desktop-shell/src/handlers/*` (cli) | `Cli.interrupt_ai` 메서드 + ACL + streaming_text commit |
| `apps/desktop-shell/src/main.rs` (또는 ai dispatch) | stream_rx select! arm + 적응형 throttle + CancellationToken 보유 |
| `core` std_types `cli` | `streaming_text`/`streaming_active` state 기본값 |
| `compositor/src/render*.rs` + `bin/geulos-vm-compositor` | 라이브 streaming_text 렌더 + 커서 |
| `compositor` 입력 핸들러 (양 백엔드) | Esc → interrupt_ai invoke |

## 후속 / 연계

- ADR 신설 예정 (구현 시 ADR-042 AI streaming).
- 후속 known-issue 후보: 스트리밍 옵션 토글(사용자 설정), input_json_delta 표시, SSE 재연결.
- 관련 메모리: [[project_vm_build_run_invocation]] (VM 검증), `feedback_ai_user_identical_command_surface` (interrupt를 메서드로).
