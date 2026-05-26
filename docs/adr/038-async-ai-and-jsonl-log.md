# ADR-038 — Async AI 흐름 + JSONL 대화 로그

- **상태:** Accepted
- **결정일:** 2026-05-26
- **부모 spec:** `docs/specs/2026-05-26-geulos-m11_1-async-ai-and-log.md`
- **부모 plan:** `docs/plans/2026-05-26-geulos-m11_1-async-ai-and-log.md`

## Context

M11까지 기능 완성도는 OK이나 두 UX/관측성 문제:
1. desktop-shell main loop가 single tokio task — AI send.await 동안 다른
   invoke 처리 차단 (스크롤·클릭·키 모두 지연). 사용자 시각으론 UI 멈춤.
2. AI 흐름 외부 점검 수단 부재 — chat_persist는 user/assistant 텍스트만
   저장, tool call/result/latency 정보 X.

## Decision

1. **submit_input AI dispatch → spawn:** chat_session을 Arc<tokio::sync::
   Mutex<Option<...>>>로 wrap. AI mode 분기에서 즉시 echo + sentinel
   "(응답 대기 중...)" SetState broadcast 후 tokio::spawn으로 AI send 분리.
   응답은 mpsc::channel<AiResult>(16) → main loop tokio::select!에 새 arm.
2. **Audit JSONL:** 기존 ChatSession::audit (text format) → audit_event(kind,
   payload)로 교체. 8 event 종류 (user_prompt/ai_text/tool_call/tool_result/
   tool_error/report_done/end_turn/send_done). 공통 ts/kind 필드 자동
   prepend. tool_call/result에 latency_ms 포함.
3. **자동 활성화:** CliChatSession::start/load가 ~/.geulos/logs/ai-chat/
   <session>-<YYYYMMDD-HHmmss>.jsonl 경로를 자동 결정하고 ensure_dir +
   with_audit. 사용자 설정 불필요.

## 대안

- (A) chat_session ownership을 channel로 pass back-and-forth: Mutex 회피
  되나 main loop의 chat_session 즉시 접근 (예: /ai list 등 빠른 read-only)
  이 직렬화. 기각.
- (B) AI 응답 streaming (Anthropic API 토큰 단위): 본 ADR 범위 외. v2.
- (C) 별 wire connection for AI processing: 복잡도 ↑. 현재 단일 connection
  으로 충분.
- (D) Audit format을 그대로 text 유지: 외부 parse 어려움. 기각.

## Consequences

**Positive:**
- AI 응답 대기 동안 UI 일반 동작 (스크롤/클릭/키) 차단 없음.
- JSONL audit가 jq/grep/tail로 분석 가능 — 중복 tool call, 비효율 호출 패턴,
  과도한 token 사용 등 사후 진단 base.
- tool_call latency 측정으로 wire round-trip 성능 추적.
- 빈 AI 응답도 `[AI: (빈 응답)]`으로 명시 피드백 (sentinel 제거 후 silent
  blank 회귀 방지 — code review I-1).

**Negative:**
- Arc<Mutex> 도입으로 chat_session 접근에 lock overhead (uncontended 시
  ns 단위, 무시 가능).
- JSONL 파일이 세션당 수 MB 누적 가능 — log rotation은 v2 (현재 사용자가
  필요 시 수동 삭제).

**Neutral:**
- main loop는 이미 tokio::select! (stream + watcher_tick) — ai_response_rx
  arm 추가가 자연. 새 인프라 아님.
- chat_persist 기존 JSON 형식 무변경 — 두 file 분리 (persist=복원, audit=
  진단). 호환성 100% 유지.
