# M11.1 — Async AI 흐름 + AI 대화 JSONL 로그

**Date:** 2026-05-26
**Status:** Draft (사용자 review 대기)
**Parent:** M11 (보안 ACL) 완료 후속 minor
**해소 KI 후보:** *없음 직접 매칭. KI-007/008 (CPU rendering / single-thread runtime)과는 무관.*

## 동기

M11까지 시스템 *기능 완성도*는 OK이지만 사용자 보고 두 가지 UX/관측성 부족:

1. **AI 응답 대기 중 UI 완전 멈춤**
   - desktop-shell main loop가 *single tokio task*. `submit_input` 처리 분기에서 `chat_session.send(prompt).await`가 *수 초~수십 초* blocking → 그 동안 main loop가 다른 invoke frame을 read·dispatch 못함 → compositor 측 스크롤/클릭/키 invoke가 server에는 도착하나 desktop-shell이 처리 못 함 → 사용자 시각으론 *UI 얼어붙음*.
   - 추가로 *prompt 입력 즉시 echo도 없음* — 사용자가 Enter 누른 후 AI 응답 도착까지 lines에 아무 변화 없음 → "입력이 전송됐는지" 불확실.

2. **AI 흐름 외부 점검 수단 없음**
   - chat_persist는 *user/assistant 텍스트*만 JSON으로 dump. tool call/result, inner turn 단위, latency 정보 X.
   - `ChatSession`에 `with_audit(path)` hook이 *이미 존재*하지만 (1) text format이라 외부 parse 어렵고 (2) `CliChatSession::start/load`가 이 hook을 *호출하지 않아* 현재 항상 OFF.
   - 결과: AI가 *중복 fetch / 비효율 tool call 루프 / 과도한 token 사용*을 만들어도 사후 진단 불가.

## 범위

**핵심 목표 2개만**:
- (1) `submit_input` 흐름 비-blocking — 즉시 echo + spawned AI task + 응답 channel + main select!
- (2) AI 흐름 JSONL audit log — `~/.geulos/logs/ai-chat/<session>-<startup-ts>.jsonl` 자동 활성

**범위 외**:
- 성능 최적화 (KI-007/008)
- granted_dirs 영구화 / 매니페스트 권한 강제 (M12+)
- AI 응답 streaming (Anthropic API는 streaming 지원하나 본 마일스톤은 *blocking 호출 자체를 spawn으로 분리*만, 응답을 토큰 단위 progressive 표시는 v2)

## 비-목표

- chat_persist의 기존 JSON 형식 변경 (그대로 둠 — 두 file은 *다른 용도*: persist는 history 복원, audit는 사후 진단)
- 동시 다중 AI 세션 처리 (한 번에 한 `chat_session: Option<...>`만 active — 기존 모델 유지)
- 별 connection 사용 (현재 wire 한 개 그대로)

## Architecture

### Fix 1 — Async AI submit_input

**현재 흐름** (`apps/desktop-shell/src/main.rs:753-764`):
```
main loop (single task)
  └─ stream.read().await         ← 다음 frame 대기
  └─ match method:
       "submit_input":
         handle_submit_input(..., &mut chat_session, ...).await  ← 수 초~수십 초 blocking
         ↑ 이 동안 stream.read 다음 호출이 안 됨 → 다른 invoke 큐잉
```

**새 흐름**:
```
main loop
  └─ tokio::select! {
       frame = stream.read() => {
         match method:
           "submit_input":
             1. 사용자 입력 즉시 echo + status "(응답 대기 중...)" SetState broadcast
             2. spawn AI task:
                  let cs = chat_session.clone();  // Arc<tokio::sync::Mutex<Option<CliChatSession>>>
                  let tx = ai_response_tx.clone();
                  let prompt = ...;
                  tokio::spawn(async move {
                      let mut guard = cs.lock().await;
                      let result = match guard.as_mut() {
                          Some(c) => c.send(&prompt).await,
                          None => Err(...)
                      };
                      let _ = tx.send(AiResult { result, target_id: cli_id }).await;
                  });
             3. 즉시 return → main loop 다음 frame 처리 가능
           ...
       }
       Some(ai_result) = ai_response_rx.recv() => {
         // status 라인 제거 + 응답 append + SetState broadcast
         handle_ai_response(ai_result, ...).await;
       }
     }
```

**주요 구조 결정**:
- `chat_session: Option<CliChatSession>` → `Arc<tokio::sync::Mutex<Option<CliChatSession>>>`. tokio::sync::Mutex는 await across lock OK. 동시 multiple sends 시도해도 Mutex가 직렬화 (현재 사용 패턴상 한 번에 하나의 send이라 단순 직렬화로 충분).
- AI 응답 채널: `mpsc::channel::<AiResult>(16)`. AiResult struct는 `{ cli_target: ObjectId, result: BridgeResult<String>, session_name: String }`.
- main loop가 `tokio::select!`로 stream + ai_response_rx 둘 다 await. 어느 쪽이 먼저 ready되든 처리.

**상태 라인 처리**:
- prompt echo 즉시: `> {prompt_text}` line append + 마지막에 `[ai:{name}] 응답 대기 중...` sentinel line append.
- AI 응답 도착 시: sentinel 제거 (lines.retain(line != sentinel) 또는 마지막 라인이 sentinel이면 pop) → AI 응답 라인들 append → SetState broadcast.
- 동시에 *AI가 응답 받는 도중에도 사용자가 새 prompt 입력 가능*. 그 경우 Mutex가 lock 잡혀있으면 spawned task가 *대기*. UI는 *새 prompt가 echo*는 즉시, *AI 응답*은 직렬 처리. 한 prompt 후 다음 prompt는 안전.

### Fix 2 — AI 대화 JSONL 로그

**파일 위치**: `~/.geulos/logs/ai-chat/<session-name>-<YYYYMMDD-HHmmss>.jsonl`
- 디렉터리는 첫 사용 시 자동 생성
- 파일명에 startup timestamp 포함 → *같은 session name으로 여러 번 start/load*해도 각 실행 분리 보관
- *append* mode — 한 실행 동안 같은 파일에 누적

**기존 audit 메커니즘 활용**:
- `ChatSession::with_audit(path)`가 이미 있음 — 본 마일스톤에서 audit format을 **text → JSONL**로 변경.
- `audit(line: &str)` 메서드를 `audit_event(kind: &str, payload: Value)`로 교체.
- 각 호출 위치를 *semantic event*로 mapping:

| 현재 audit line | 새 JSONL event |
|---|---|
| `=== chat send ===` `prompt: ...` | `{kind: "user_prompt", text: "..."}` |
| `--- inner turn N ---` | (없음 — turn 정보는 각 event에 turn 필드로) |
| `text: ...` | `{kind: "ai_text", text: "...", turn: N}` |
| `tool_use: name(args)` | `{kind: "tool_call", name: "...", args: {...}, tool_use_id: "...", turn: N}` |
| `  -> output` | `{kind: "tool_result", tool_use_id: "...", result: ..., turn: N, latency_ms: M}` |
| `  -> error` | `{kind: "tool_error", tool_use_id: "...", error: "...", turn: N, latency_ms: M}` |
| `  -> report_done: summary` | `{kind: "report_done", summary: "...", turn: N}` |
| `=== end_turn ===` | `{kind: "end_turn", turn: N, reason: "no_tools" \| "max_inner_turns"}` |
| `=== chat done (Xs) ===` | `{kind: "send_done", total_ms: M, final_text_len: N}` |

**공통 필드**: 모든 event에 `{ts: "2026-05-26T...", kind: "...", session: "..."}` 자동 추가.

**파일 형식**: JSONL — *한 줄 한 JSON object*. tail/grep/jq로 분석 가능.

**활성화 자동화**:
- `CliChatSession::start(api_key, wire, system, name)` 안에서 audit path 결정 + `ChatSession::with_audit(path)` 호출.
- `CliChatSession::load(...)`도 동일.
- 사용자가 별도 설정 안 해도 *모든 AI 세션*이 자동 logging.

**chat_persist (기존 JSON dump)는 그대로** — 두 파일이 분리:
- `~/.geulos/sessions/<name>.json` — history 복원용 (재시작 시 load)
- `~/.geulos/logs/ai-chat/<name>-<ts>.jsonl` — 사후 진단용 (append 전용)

## Data flow (Fix 1 시각화)

```
User Enter "안녕 AI"
   │
   ▼
compositor → server.invoke(cli, submit_input, {text: "안녕 AI"})
   │
   ▼
server → desktop-shell.subscribe event
   │
   ▼ (main loop stream.read())
main loop tokio::select! → frame branch
   │
   ├─ (1) lines.push("> 안녕 AI") + lines.push("(응답 대기 중...)")  ← 즉시
   ├─ (2) SetState(cli.lines) broadcast                           ← 즉시
   ├─ (3) spawn task {
   │        chat_session.lock().await
   │        .send("안녕 AI").await                                ← 수 초~수십 초
   │        tx.send(AiResult).await
   │      }
   └─ (4) return → 다음 frame 처리 (loop 복귀)
                       │
   (그 동안 다른 invoke: scroll/click/key 처리됨)
                       │
                       ▼
              AI 응답 도착 → tx → ai_response_rx
                       │
   (main loop tokio::select! → ai_response_rx branch)
                       │
                       ├─ lines에서 "(응답 대기 중...)" sentinel 제거
                       ├─ lines.push(ai_response)
                       └─ SetState(cli.lines) broadcast
```

## Trade-offs

| 결정 | 채택 | 대안 | 이유 |
|---|---|---|---|
| chat_session ownership | `Arc<tokio::sync::Mutex<Option<...>>>` | channel pass back-and-forth | Mutex가 더 단순, await OK, 동시 send 없는 현 패턴엔 충분 |
| AI 응답 채널 | `mpsc::channel(16)` | broadcast | mpsc면 main loop만 consumer, 단순 |
| status 라인 | sentinel string `"(응답 대기 중...)"` | 별 state 필드 (cli.state.waiting) | sentinel이 단순, 기존 lines 모델 활용 |
| audit format | JSONL (event 기반) | text 유지 + 별 JSONL logger | format 변경이 변경 surface 적음, 기존 audit 호출 위치 그대로 |
| audit auto-enable | `CliChatSession::start/load`에서 자동 path 설정 | 사용자가 환경 변수로 opt-in | 자동이 *진단 수단 항상 가용* 보장. 디스크 사용량은 *세션당 수 MB 한계* |

## 회귀 위험

- **chat_session lock 잡힘 동안 새 prompt 시도** → spawned task가 *queueing*. 사용자 시각으론 "응답 늦음" 정도, *멈춤*은 아님 (UI 자체는 다른 invoke 처리 가능).
- **AI 응답 도착 vs 다른 SetState 동시** → SetState는 *server를 거치므로* 순서 보장. main loop가 single consumer라 race 없음.
- **JSONL 파일 디스크 부족** → audit 호출이 silent fail (기존 audit 패턴 그대로). AI 응답 자체는 정상 반환.
- **audit format 변경이 기존 의존자 깸** → audit_path 사용처 grep 결과: ChatSession::with_audit 호출 *0건* (CliChatSession 미연결). 안전.

## 검증 (Manual)

1. AI 세션 시작 후 prompt 입력 → *즉시* lines에 echo + 응답 대기 status 표시
2. AI 응답 도착 전에 사용자가 *스크롤/창 이동/CLI 클릭* → 모두 정상 반응
3. 응답 도착 → status 라인 제거 + AI 응답 표시
4. `~/.geulos/logs/ai-chat/`에 JSONL 파일 생성 + `cat` 으로 user_prompt/tool_call/tool_result/ai_text 라인 확인
5. JSONL 한 줄을 `jq` 로 parse 가능 (`tail -1 file.jsonl | jq`)
6. *중복 tool call 감지 시나리오*: AI에게 같은 question 두 번 → 두 번째 응답에 *cached info 활용*하는지 / 같은 tool call이 반복되는지 JSONL로 확인

## 측정 통과 기준

- `cargo test --workspace` 통과
- `clippy -D warnings` / `fmt --check` 통과
- Manual 검증 6개 항목 모두 통과
- AI 응답 *받는 도중* 다른 invoke (스크롤 등)의 시각 반응이 *< 100ms* 안에 보이는지 (체감)

## M11.1 범위 외 / 후속

- AI 응답 streaming (토큰 단위 progressive) — v2
- Audit log retention 정책 (파일 N개 보관 후 rotate) — v2
- AI 호출 hot-path metric (HashMap O(1) 확인 등 KI-001 ADR Negative) — 별 task
- KI-002 매니페스트 권한 강제 — M12
