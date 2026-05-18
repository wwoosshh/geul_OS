# ADR-030 — AI chat session in CLI (M7 T7.7)

**Status:** Accepted (2026-05-18)

## Context

ADR-023(CLI as shell)으로 데스크톱 셸 하단 CLI 패널이 일급 구성요소가 되었고, T7.5(명령 dispatch v1) + T7.6/ADR-029(한글 IME 위임)으로 *사용자가 한국어로 입력 가능*한 상태가 됐다. GeulOS 비전의 *핵심 작동 시연* — "AI에게 객체 모델 + 자연어 prompt를 주면 AI가 데스크톱을 조회·조작" — 이 남아있는데, 이는 *대화 컨텍스트가 유지되어야* 자연스럽다. 한 prompt당 새 세션이면 "방금 그 객체"를 가리킬 수 없다.

ai-bridge에는 M5 산물 `Session<A: LlmAdapter>`가 이미 있다. `run_task(prompt) -> SessionOutcome`은 `report_done`까지 자동 multi-turn — *task 모델* (한 prompt = 한 lifecycle). 시나리오 러너용으로 만들어졌다. 그러나 CLI 사용 패턴은 *chat 모델* — `send_message`를 반복 호출, history가 누적되어야 한다. 두 모델의 핵심 차이는 한 lifecycle 안에서 *몇 번의 user turn*이 있느냐:

| 기존 `Session::run_task` (task) | 신규 `ChatSession::send_message` (chat) |
|---|---|
| user turn 1회 → assistant가 `report_done`까지 자동 진행 | user turn N회 — 매 user prompt마다 한 응답 반환 |
| budget 소진 = outcome 종료 | budget 소진 = error, session 객체 재사용 가능 |
| history는 함수 local | history는 struct field, 영속 |

## Decision

ai-bridge에 신규 `ChatSession<A: LlmAdapter>` struct를 추가하고 desktop-shell에서 in-process로 import해 CLI에 통합한다. 기존 `Session`은 *시나리오 task 러너*로서 의미 유지 — 책임 분리.

### ChatSession API

```rust
pub struct ChatSession<A: LlmAdapter> { /* adapter, wire, system, tools, history, audit, max_inner_turns */ }
impl<A: LlmAdapter> ChatSession<A> {
    pub fn new(adapter: A, wire: WireClient, system: String) -> Self;
    pub fn with_audit(self, path: impl Into<PathBuf>) -> Self;
    pub async fn send_message(&mut self, user_prompt: &str) -> BridgeResult<String>;
}
```

- 한 `send_message` 호출 안에서 *tool use auto-loop* 발생 — AI가 `list_objects_by_type` / `get_object` / `invoke_method` / `subscribe` / `drain` 등을 자기 책임으로 호출하다가 `EndTurn` 또는 `max_inner_turns` 도달 시 종료. `report_done` 호출 시 그 요약을 최종 응답에 합쳐 반환.
- *성공 시에만* history commit — 도중에 에러가 나면 원본 history 보존. 사용자가 다음 prompt를 보낼 때 깨진 상태가 아님.
- 도구 dispatch는 기존 `tools::dispatch_tool`을 재활용 (DRY). `response_to_assistant_content` helper만 기존 `session.rs`에 중복(둘 다 5줄짜리 trivial 변환) — M9에 정리 메모.

### CLI 통합

- **prefix-free routing:** `cli_handler::dispatch_command`가 등록된 명령(`help` / `echo` / `clear`)만 인식, 그 외 입력은 `SpecialAction::AiPrompt(text)` 반환. `main.rs`의 `submit_input` 핸들러가 해당 분기에서 `ChatSession::send_message`를 await하고 응답 텍스트를 라인별로 `lines`에 append. 사용자가 "오늘 워크스페이스에 어떤 파일이 있나요?" 같은 자연어를 *그대로* 입력하면 AI에게 위임된다.
- **단일 세션:** desktop-shell 프로세스 생애 동안 한 `ChatSession` 인스턴스. `Cli` 객체의 `state.session_id`는 M7 v1에서 *형식상 보관*만 (process-local UUID 가능). 다중 세션 / 사용자 명시적 reset은 M9+.
- **graceful degradation:** `ANTHROPIC_API_KEY`가 미설정이면 `ChatSession`을 생성하지 않고 AI prompt 분기에서 `[AI 비활성 — ANTHROPIC_API_KEY 미설정]` 라인을 출력. echo/help/clear는 그대로 작동.
- **에러 처리:** 네트워크 실패 / API 키 오류 / budget 류 에러는 `[AI 오류: {detail}]` 한 줄로 CLI에 출력하고 session은 살아있게 둔다 (history는 보존 — 위 *성공 시에만 commit* 정책).
- **claude-sonnet-4-6 default** — `ai-bridge/src/main.rs::DEFAULT_MODEL`과 일관. Opus는 비용이 높아 v2.

### in-process import

desktop-shell이 `geulos-ai-bridge` crate에 *직접* 의존 (workspace path). ai-bridge는 desktop-shell에 의존하지 않으므로 순환 없음. ADR-009(AI 기본 불신)의 *별도 프로세스 격리* 원칙은 M9+에 separate process / sandbox 마일스톤에서 다시 검토. M7 v1은 *작동 시연*이 우선.

## Alternatives rejected

- **기존 `Session::run_task`를 매 prompt마다 새로 호출** — history가 *prompt마다 끊김*. "방금 그 윈도우" 같은 지시대명사 불가. 도그푸딩의 자연스러움 손실.
- **`Session`에 `send_message` 메서드 추가** — task/chat 모델이 *한 struct에서 공존*하면 budget·완료 의미가 혼란. (예: budget 소진 후 chat은 재사용 가능, task는 끝.) 책임 분리로 별 struct가 명확.
- **AI 호출용 slash prefix(`/ai <prompt>`)** — 매번 prefix 입력 부담. *등록 명령만 명시*하는 prefix-free routing이 자연스러움 (사용자가 명령을 외울 필요 없음 — 모르는 단어는 AI에게).
- **AI 호출을 비동기 spawn + 응답 도착 시 append_line** — desktop-shell의 이벤트 루프가 응답 대기 동안 *다른 invoke를 처리 가능*. M7 v1은 *blocking await* — 사용자가 기다리는 동안 다른 입력 안 옴 (CLI에 집중). 비동기 보완은 v2 (KI-014 후보로 known-issues 추가 가능).

## Consequences

- GeulOS 비전 *작동 시연 가능* — CLI에 한국어로 prompt → AI가 query/invoke로 데스크톱 조사 → 답변. 도그푸딩 차단 해소.
- desktop-shell이 ai-bridge crate에 직접 의존 → 빌드 그래프 확장. clippy/test 모두 함께 돌아간다.
- AI 호출 동안 *전체 invoke 루프 정지* — 사용자가 그 사이 다른 객체(파일 트리 등)를 클릭하면 응답 지연 후에 처리. UX 약점, v2 비동기로 해소.
- ai-bridge가 server-host에 *별도 TCP connection*을 연다 (desktop-shell의 connection과 분리). server-host는 multi-actor 지원하므로 OK. 두 connection이 같은 객체 트리를 share — ai-bridge의 `Role::Ai`로 발급된 actor_id가 invoke의 last_change_actor가 되어 *T5 노란 점 시각화*가 자연스럽게 켜진다 (CLI에 AI 응답이 도착할 때 last_change_actor=ai).

## 참고

- 관련 ADR: ADR-009 (AI untrusted-default — in-process는 v1 trade-off), ADR-023 (CLI as shell), ADR-029 (한글 IME — prompt 입력 layer)
- 관련 plan: `docs/plans/2026-05-18-geulos-m7-cli-extension.md` §T7.7
- 관련 manual test: `docs/manual-tests/m7-cli-acceptance.md`
- 기존 코드: `ai-bridge/src/session.rs` (task 모델 — 의미 유지), `ai-bridge/src/tools.rs::dispatch_tool` (재활용)
