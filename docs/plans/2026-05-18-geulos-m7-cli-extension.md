> **Status:** completed (2026-05-18)
> **Note:** M7 하단 CLI 패널 정식 마감 — chat session 유지 + Claude multi-turn. 한글 IME는 후속 VM compositor 한글 IME (Tab 토글) 작업에서 추가.

# GeulOS M7 보조 plan — 하단 CLI 패널 (셸 일급 구성요소)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> **NEVER push** — controller batches push at end of milestone.

> **ADR 번호 갱신 (2026-05-18, M8 spec 작성 시):** ADR-024/025 시드(M7 T7.7 AI chat session / M7 T7.6 한글 IME)는 M8 ADR-026~028이 먼저 작성됨에 따라 029(AI chat session)/030(한글 IME)로 재번호 예정. M7 T7.6/T7.7 재개 시점에 본문 작성 시 새 번호 사용. (본문 내 "ADR-024"/"ADR-025" 언급은 재개 implementer가 갱신할 때까지 *원문 보존*.)

**Parent plan:** `docs/plans/2026-05-18-geulos-m7-desktop-shell.md` (T1~T9 + Final). 본 보조 plan은 T7 완료 후, T7.5/T7.6/T7.7 세 task를 끼워넣는다 — T8(KI-001 정리)·T9(acceptance) **이전**에 들어와야 한다 (acceptance는 CLI 포함된 셸을 검증해야 도그푸딩 가치가 있음).

**Goal:** 데스크톱 셸 화면 *하단*에 항상 보이는 CLI 패널 추가. CLI는 *셸의 일급 구성요소* (좌측 트리·우측 캔버스와 동급) — bash/PowerShell처럼 모든 동적 명령 접근의 진입점. AI 호출은 그 위의 *한 명령*일 뿐.

**Why this extension (사용자 결정 — 2026-05-18):**

사용자 인용:
> "내 글os는 ai가 직접 os를 조작할수 있다는점을 고려해서 현재의 화면좌측파일구조처럼 ai와 대화를 진행할 cli를 구현해야해"
> "이건 단순히 ai기능의 추가라기보다 바탕화면에서 항상 접근 가능한 cli라고 보는게 맞아"
> "cli가 있어야 ai를 api형태로 호출하는데 용이하고 그래야 ai와 대화를 통해 작업을 진행할수 있어"
> "지금은 cli를 만드는게 먼저야. cli에서 ai를 호출하는게 추가기능으로 들어가게 되는거지 cli를 통해 좀더 다이나믹한 명령어접근이 가능해"

해석:
1. CLI는 셸의 *본질*. AI는 한 명령.
2. 위치: 화면 *하단* 고정 (toggle 아님).
3. 일반 명령(help, clear, ls 같은 다이나믹 접근) + AI 호출 둘 다.
4. AI 호출은 *대화 세션* (history 유지) — Claude API의 multi-turn.
5. 한글 입력 필수 (사용자가 한국인, AI와 한국어 대화).

**Scope choice — 사용자 디자인 결정:**

| 결정 | 답 |
|---|---|
| ① CLI 명령 범위 | CLI 자체가 우선. *다이나믹 명령 접근* (basic dispatch + 점진 확장). AI는 한 명령. |
| ② 한글 IME | M7에 포함 (winit IME 우선 + 실패 시 직접 구현). |
| ③ AI 연동 방식 | **chat session 유지** (대화 이어짐). Claude API의 multi-turn 대화 모델. |
| ④ 위치 | 화면 하단 고정 (상시 가용). |

**Architecture (확장):**

```
┌──────────────────────────────────────────────────┐
│ GeulOS Compositor                                │
│ ┌────────────┬──────────────────────────────┐    │
│ │ FileTree   │ Canvas                       │ 70%│
│ │ (좌 30%)   │ (우 70%)                     │ 높이│
│ │            │                              │    │
│ └────────────┴──────────────────────────────┘    │
│ ┌──────────────────────────────────────────┐     │
│ │ CLI                                       │ 30%│
│ │ > _                                       │ 높이│
│ │ (출력 라인 누적)                          │    │
│ └──────────────────────────────────────────┘     │
└──────────────────────────────────────────────────┘
```

데스크톱 트리 구조:
```
aios.builtin/Desktop@1
├── aios.builtin/FileTree@1   (좌측 상단)
├── aios.builtin/Canvas@1     (우측 상단)
└── aios.builtin/Cli@1        ← 신규 (하단)
```

CLI 객체 모델:
- `props`: 없음 (또는 `prompt_text: String` 기본 프롬프트 문자)
- `state`:
  - `input_buffer: String` — 현재 편집 중인 입력 라인
  - `cursor_pos: usize` — byte index
  - `lines: [String]` — 출력 히스토리 (oldest first; cap at ~1000 lines)
  - `history: [String]` — 입력 히스토리 (↑/↓ 키 네비게이션용; M7 v1 deferred 가능)
  - `session_id: Option<String>` — AI 채팅 세션 ID (T7.7부터)
- `methods`:
  - `submit_input()` — 현재 input_buffer를 라인으로 commit + 명령 dispatch
  - `clear()` — lines + input_buffer 비움
  - `append_line(text: String)` — 외부에서 출력 라인 추가 (AI 응답 등)

**Tech Stack (추가):**
- 신규 객체 타입: `aios.builtin/Cli@1`
- 컴포지터: 키보드 입력 처리 (winit `WindowEvent::KeyboardInput`, `WindowEvent::Ime`)
- IME: winit Ime 이벤트 활용 (Preedit + Commit) — Windows TSF는 winit이 자동 위임
- CLI 명령 dispatch: desktop-shell이 `submit_input` invoke 받아 명령 파싱 → 결과를 `append_line`으로 다시 push
- AI session: ai-bridge에 chat session API 추가 (메시지 history 유지, claude-sonnet-4-6 또는 claude-opus-4-7)

**Selection criteria (M7 완료 조건에 추가):**
- 컴포지터 창 하단에 CLI 패널 표시 (높이 30%)
- 키보드 입력으로 영문 + 한글 둘 다 입력 가능
- `help` 입력 + Enter → 사용 가능 명령 목록 출력
- `clear` 입력 + Enter → 출력 라인 비워짐
- `echo <text>` → text 그대로 출력
- `/ai <prompt>` 또는 prefix-free 자연어 → AI 응답이 누적 출력 (chat session)
- AI 응답 동안 CLI에 *AI 작업 표시* (T5의 노란 점 메커니즘 재사용 — last_change_actor=="ai"인 라인 강조)
- 한글 typing 시 preedit 표시(조합 중) + commit (조합 완료) 정상

**Scope estimate:** +3주 추가 (T7.5 1주 + T7.6 1주 + T7.7 1주). 전체 M7 7주 → **10주**.

---

## ADR 시드 (T7.5에서 본문 작성)

- **ADR-023 — CLI as shell first-class.** CLI는 데스크톱 셸의 *4번째 builtin* (Desktop의 자식 [FileTree, Canvas, Cli]). 항상 보임. 일반 명령 dispatch + AI 호출 모두 여기서. `aios.builtin/Cli@1` 네임스페이스는 ADR-020의 `aios.builtin/*`와 일관.
- **ADR-024 — AI chat session model.** AI 호출은 *대화 세션*. Claude API multi-turn — Cli 객체의 `state.session_id`가 ai-bridge의 세션 핸들. CLI에 `clear` 시 세션도 reset. M7은 *단일 세션* (한 CLI = 한 세션). 메시지 history는 ai-bridge가 보관, CLI는 ID만.
- **ADR-025 — 한글 IME 전략.** winit `Ime::Preedit`/`Ime::Commit` 이벤트 우선 활용 (Windows TSF는 winit이 위임). Preedit는 input_buffer에 *조합 중 마커*로 보이게 렌더, Commit 시 실제 byte 삽입. 실패 시 fallback은 clipboard paste(Ctrl+V) 한정 + KI-013으로 잔여 작업 추적.

---

## 파일 구조 (사전 매핑)

```
core/src/object/std_types.rs              # 수정: cli() 팩토리 추가
core/tests/std_types_test.rs              # 수정: Cli 라운드트립 + 시각화 필드

apps/desktop-shell/src/main.rs            # 수정: Cli mount + submit_input 핸들러 + clear/echo dispatch
apps/desktop-shell/src/cli_handler.rs     # 신규: 명령 파싱 + dispatch (handle_help/handle_clear/handle_echo)
apps/desktop-shell/tests/cli_handler_test.rs  # 신규: 단위 테스트

compositor/src/layout.rs                  # 수정: Desktop 3분할 — 상단(좌30%/우70%, 높이70%) + 하단 Cli(높이30%)
compositor/src/render.rs                  # 수정: Cli 렌더 (prompt, input_buffer, cursor, lines history)
compositor/src/main.rs                    # 수정: WindowEvent::KeyboardInput + Ime 핸들러 → 활성 CLI에 입력 전달
compositor/src/keyboard.rs                # 신규: 키보드 → invoke/state update 변환 (T7.5 v1: ASCII; T7.6에서 IME)
compositor/tests/layout_test.rs           # 수정: 3분할 검증 테스트 추가

ai-bridge/src/session.rs                  # 신규 (T7.7): chat session API
ai-bridge/src/main.rs 또는 lib.rs         # 수정 (T7.7): session create/send/close 명령 추가
apps/desktop-shell/src/ai_session.rs      # 신규 (T7.7): ai-bridge와 RPC 또는 직접 호출

docs/adr/023-cli-as-shell.md              # 신규
docs/adr/024-ai-chat-session.md           # 신규
docs/adr/025-hangul-ime.md                # 신규
docs/plans/2026-05-18-geulos-m7-cli-extension.md  # 이 문서
docs/manual-tests/m7-cli-acceptance.md    # 신규 (T7.7 마지막)
```

---

## Task T7.5 — 하단 CLI 패널 (기본 입력/출력 + 명령 dispatch, AI 없음)

**Estimated:** 1주

**Files:**
- Modify: `core/src/object/std_types.rs` (add `cli()` factory)
- Modify: `core/tests/std_types_test.rs` (round-trip)
- Create: `docs/adr/023-cli-as-shell.md`
- Modify: `compositor/src/layout.rs` (3분할)
- Modify: `compositor/tests/layout_test.rs` (3분할 테스트 추가)
- Modify: `compositor/src/render.rs` (Cli 렌더)
- Modify: `compositor/src/main.rs` (키보드 이벤트 디스패치, T7.5는 ASCII만)
- Create: `compositor/src/keyboard.rs` (키 → state update 변환, ASCII v1)
- Create: `apps/desktop-shell/src/cli_handler.rs` (help/clear/echo dispatch)
- Modify: `apps/desktop-shell/src/lib.rs` (`pub mod cli_handler;`)
- Modify: `apps/desktop-shell/src/main.rs` (Cli mount + submit_input invoke 처리 + append_line 처리)
- Create: `apps/desktop-shell/tests/cli_handler_test.rs` (4 tests)

### 핵심 단계 (다음 세션에서 implementer가 펼쳐 씀)

1. ADR-023 작성
2. `aios.builtin/Cli@1` 팩토리 + 라운드트립 테스트
3. cli_handler.rs — `dispatch_command(input: &str) -> CommandOutcome` (Vec<String> output_lines + special action enum like Clear/Exit)
4. cli_handler_test.rs — help/clear/echo/unknown 4가지
5. layout.rs `layout_desktop` 수정 — 상단 70% 높이에 좌(30%) + 우(70%), 하단 30% 높이에 CLI 풀폭
6. render.rs Cli 렌더 — 검정 배경, 흰 텍스트, 마지막 N라인 표시 + 입력 라인 (`> {input_buffer}`) + 깜빡이는 cursor
7. compositor keyboard.rs — KeyboardInput → 활성 Cli의 `input_buffer` UiAction::Invoke 또는 클라이언트-사이드 state update (디자인 결정 필요: 매 키 invoke vs 로컬 state)
8. desktop-shell main.rs — `submit_input` invoke 받으면 cli_handler::dispatch_command → output_lines를 `append_line`으로 SetState
9. 수동 시연: CLI에 `help` 타이핑 + Enter → 명령 목록 출력. `echo 안녕` 시도(영문은 OK, 한글은 T7.6까지 □)

### 주의 사항

- 키 입력 라우팅: focused 객체 개념 없음 — 일단 *Cli만 키보드 입력 받음* 가정. 미래에 TextArea 등 추가 시 focus 시스템 필요.
- 매 키마다 server invoke는 latency 큼 → desktop-shell이 local state 유지하고 *commit(Enter)* 시점에만 invoke. 또는 cursor/buffer만 client-side state, lines는 server에. 디자인 결정: **client-side input_buffer, commit 시 invoke** (단순 + 빠름).
- 그러나 이 결정은 input_buffer가 server tree에 없게 만듦 — render는 어디서 가져옴? **답: compositor의 *별도 local state* (tree와 분리)**. Cli 객체는 lines + history만 server에 보관, input_buffer는 컴포지터 local. T7.5에선 이대로.

---

## Task T7.6 — 한글 IME 통합

**Estimated:** 1주

**Files:**
- Create: `docs/adr/025-hangul-ime.md`
- Modify: `compositor/src/main.rs` (winit Ime 이벤트 활성화 + 처리)
- Modify: `compositor/src/keyboard.rs` (Ime::Preedit, Ime::Commit 핸들러)
- Modify: `compositor/src/render.rs` (preedit 영역 표시 — 조합 중 underline 또는 다른 색)
- Modify: `docs/known-issues.md` (KI-013 진척 또는 closed)

### 핵심 단계

1. ADR-025 작성 (winit Ime 전략 + Windows TSF 위임 + fallback)
2. winit Window 생성 시 `set_ime_allowed(true)`
3. `WindowEvent::Ime(Ime::Preedit(text, cursor_range))` 처리 — 컴포지터에 preedit_text local state 추가
4. `WindowEvent::Ime(Ime::Commit(text))` 처리 — input_buffer에 text 삽입 + preedit_text 비움
5. render.rs — preedit_text를 input_buffer 뒤에 *조합 중 마커*로 그림 (예: 다른 색 + underline)
6. 수동 시연: 한국어 자판으로 "안녕" 타이핑 → 조합 중 표시 → Enter로 commit
7. KI-013 update 또는 closed

### 위험

- winit Windows IME 동작 확인 필요 — 일부 winit 버전은 IME 부분 지원. 0.30 기준 OK.
- Korean IME 사용자 설정(MSIME / 새 한국어) 모두 OK 확인.
- 입력 자판 전환 시 (한/영 키) 컴포지터 응답 — winit이 알아서.

---

## Task T7.7 — AI session 통합 (CLI에서 AI 호출)

**Estimated:** 1주

**Files:**
- Create: `docs/adr/024-ai-chat-session.md`
- Create: `ai-bridge/src/session.rs` (chat session API)
- Modify: `ai-bridge/src/lib.rs` 또는 `main.rs` (session create/send/close 명령)
- Create: `apps/desktop-shell/src/ai_session.rs` (ai-bridge 호출 래퍼)
- Modify: `apps/desktop-shell/src/cli_handler.rs` (slash command `/ai` 또는 prefix-free → ai_session 호출)
- Create: `docs/manual-tests/m7-cli-acceptance.md`

### 핵심 단계

1. ADR-024 작성 (chat session model + ID 관리)
2. ai-bridge에 `pub fn create_session() -> SessionId`, `pub fn send(session_id, prompt) -> Result<String>`, `pub fn close_session(session_id)` 추가
3. SessionId → Vec<Message> 매핑 in-memory (M7은 process-local; persistence는 후속)
4. desktop-shell ai_session.rs — ai-bridge를 직접 lib로 import 또는 별도 process로 spawn (디자인 결정)
5. cli_handler.rs — 명령 라우팅에 `ai_query(input_text)` 추가. 첫 단어가 등록된 명령이면 그 명령, 아니면 AI prompt로 처리 (또는 `/ai` 명시 prefix — 더 안전)
6. AI 응답 도착 시 `append_line` invoke로 CLI에 출력 (last_change_actor="ai" 자동 노란 점)
7. 시연: CLI에 "오늘 워크스페이스에 어떤 파일이 있나요?" 입력 → Claude가 list_objects_by_type으로 확인 → 응답 출력
8. m7-cli-acceptance.md 작성

### 디자인 결정 (T7.7 implementer 시점)

- ai-bridge 호출: in-process lib import vs 별 process spawn?
  - in-process: 간단, 그러나 desktop-shell이 ai-bridge 의존성 직접
  - separate: 격리, RPC overhead, 권한 분리 가능 (ADR-009 일관)
  - **추천: in-process (M7)** → 후속 격리 가능
- prefix-free vs `/ai`:
  - prefix-free: 등록된 명령(help/clear/echo/...) 아니면 AI로. 자연스러움.
  - `/ai`: 명시적, 실수 방지. 단 매번 prefix 부담.
  - **추천: prefix-free, `/help`로 명령 list 알림**

---

## 자체 점검 (보조 plan)

**스펙 커버리지:**
- CLI 일급 구성 — T7.5 ✓
- 한글 IME — T7.6 ✓
- AI chat session — T7.7 ✓
- 위치 하단 — T7.5 layout ✓

**스코프 위험:**

| 위험 | 완충 |
|---|---|
| winit Ime 이벤트가 Windows에서 기대대로 안 옴 | T7.6에 직접 구현 fallback 명시. 한 주 추가 가능 |
| ai-bridge가 chat session API 추가가 큰 변경 | T7.7 1주 estimate에 큰 변경 포함. session 자체는 in-memory Vec<Message>면 충분 |
| Claude API 호출이 동기적으로 desktop-shell 블락 | ai-bridge 호출을 tokio spawn으로 비동기, 응답 도착 시 append_line invoke. CLI에 "thinking..." 표시 가능 |
| keyboard.rs가 컴포지터의 단일 라이터 가정 깨뜨림 | input_buffer는 *컴포지터 local* state, server tree와 분리. Cli 객체는 commit된 lines만 보관 |

**알려진 한계 (보조 plan 범위 밖):**
- CLI 명령 자동완성, 히스토리 네비게이션 (↑/↓) — v2
- CLI 색상 토큰 (ANSI 같은) — v2
- 다중 CLI 세션 / 탭 — v2
- AI 응답 streaming (token-by-token) — v2 (M7은 한 번에 응답)

---

## 다음 세션 시작 가이드

본 보조 plan은 *T7 완료 후* 진입한다. T7 review가 아직 안 됐다면 다음 두 가지 옵션:

**옵션 A (권장):** T7 spec/quality review를 *건너뛰고* T10 final review에 묶음. T7.5~T7.7 + T8 + T9 끝나면 일괄 final review가 본 plan + 보조 plan 전체를 cover.

**옵션 B:** T7만 따로 spec/quality review 진행 후 T7.5 시작.

다음 세션 추천 시작:
```
1. 이전대화 컨텍스트 복원 (memory + 본 plan 참고)
2. T7.5 implementer 디스패치 (위 "핵심 단계" 사용)
3. T7.6 → T7.7 순차 진행
4. T8 (KI-001 wildcard ACL 정리 + CLI 객체 ACL도 정리)
5. T9 (CLI 포함된 셸의 acceptance + 도그푸딩)
6. T10 Final review (T1~T9 + T7.5/T7.6/T7.7 모두 cover)
```
