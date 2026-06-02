# ADR-042 — AI 응답 스트리밍 (Anthropic SSE)

- **상태:** 채택 (2026-06-02)
- **맥락 spec:** `docs/specs/2026-05-... ` → `docs/specs/2026-06-02-geulos-ai-response-streaming.md`
- **구현 계획:** `docs/plans/2026-06-02-geulos-ai-response-streaming.md`

## 맥락

AI 응답을 *완성 후 한 번에* 표시하던 모델(`ChatSession::send_message` → 최종 문자열 반환)은 첫 토큰까지 사용자가 빈 화면을 응시하게 했다. 응답이 길수록 체감 지연이 컸다. Anthropic messages API는 `stream:true`로 SSE 토큰 스트리밍을 지원하므로, 이를 4층 파이프라인 끝(컴포지터)까지 흘려 점진 표시한다.

## 결정

1. **어댑터 레이어 스트리밍.** `LlmAdapter`에 `complete_streaming(..., turn, tx: &mpsc::Sender<StreamEvent>, cancel: &CancellationToken)` 추가 (기본 구현은 비스트리밍 `complete` 1회 emit으로 호환). `ClaudeAdapter`가 `stream:true` + `bytes_stream()` + 직접 SSE 파싱(`adapter/sse.rs`, SDK 없음)으로 override. prompt caching(system+tools cache_control)은 스트리밍에서도 유지.

2. **모든 turn 텍스트 스트리밍 + 도구 마커.** AI의 inner turn마다의 텍스트를 `StreamEvent::TextDelta`로 흘리고, 도구 호출은 `ToolStart` 마커로 표시. **스트리밍은 텍스트 표시 전용** — `tool_use` 인자(input_json_delta)는 누적하지 않으므로, 도구 호출 turn은 chat_session이 *비스트리밍 `complete`로 재요청*해 완전한 tool_use를 얻는다. (input_json_delta 표시는 후속.)

3. **적응형 throttle `max(80ms, 40자)`.** 토큰당 SetState broadcast(응답당 50~100건)는 와이어/컴포지터 부하 과다 + 과거 race(KI-018/026) 우려. desktop-shell main loop가 델타를 누적하다 80ms 경과 *또는* 40자 누적 시 1회 `Cli.streaming_text` SetState (응답당 ~15~30건).

4. **전용 `Cli.streaming_text`/`streaming_active` state.** 확정 `lines`와 분리된 라이브 영역. 컴포지터가 그 아래에 커서 `█`와 함께 렌더, `Done`/`Cancelled`/`Error` 시 `lines`로 1회 commit. 비스트리밍 `send_message`의 `final_text`와 등가.

5. **중단 = invoke 메서드.** `Cli@1.interrupt_ai()` — desktop-shell이 보유한 `CancellationToken`을 cancel. ACL은 compositor(Esc 경로) + **AiSession `Exact("interrupt_ai")`**(동일 명령표면) — AI는 interrupt_ai만 호출 가능, `submit_input`은 불가(입력 주입 차단). Esc 키(host winit + VM evdev)는 `streaming_active`일 때만 interrupt_ai invoke, 그 외 기존 Esc(VM rename-cancel) 보존.

## 구현 패턴

desktop-shell의 기존 `ConsoleEvent → apply_console_line` 채널 패턴을 그대로 미러링 — `stream_rx` select! arm + `set_cli_streaming`/`commit_cli_streaming`(StateSetMsg + KI-018 local 동기). 성공/취소는 stream arm이 단일 표시(adapter가 Ok 반환), 에러만 `ai_response_tx`로 surface.

## 결과

- **검증:** SSE 파서 단위 테스트(델타순서/청크경계 재조립/tool_use 이름), `send_message_streaming` 등가성 테스트(델타 합 == 최종 텍스트), interrupt ACL 테스트(AI는 interrupt_ai만), VM end-to-end 부팅(musl 컴파일 + 4층 기동 + 패닉 0). 토큰 점진 표시·Esc 중단 육안은 GUI 창에서.
- **v1 한계 (후속):** 스트리밍 텍스트 전용(도구 인자 미점진), 스트리밍 옵션 토글 없음(항상 on), SSE 재연결 없음(drop 시 부분 commit + 에러), 네트워크 drop 시 에러 마커 2개(내용 중복 X).

## 대안 (기각)

- **콜백 클로저** delta 전달 — 결국 채널로 보내야 해 우회. 채널이 desktop-shell mpsc 모델과 자연 연결.
- **`lines` 직접 append** — 확정/진행중 혼재, 부분 줄 잔존. 전용 streaming_text가 깔끔 + 커서 표현 자연.
