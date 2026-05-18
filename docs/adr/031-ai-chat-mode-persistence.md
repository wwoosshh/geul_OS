# ADR-031 — AI chat mode 진입/탈출 + 세션 영속/로드 (M7 T7.8)

**Status:** Accepted (2026-05-18)

## Context

ADR-030 (T7.7)으로 CLI에 `ChatSession`을 통합했지만 *routing 모델이 사용자 의도와 다름*. T7.7은 **prefix-free routing** — `help`/`echo`/`clear` 외 *모든 입력*이 자동으로 AI에게 전달. 짧게는 편하지만 다음 두 가지 문제가 있다:

1. **모드 경계 부재** — 사용자가 "지금 일반 셸인지 AI와 대화 중인지" 시각적/의미적으로 구분할 수 없다. `echo` 한 줄이 *우연히* AI로 안 가는 것은 명령 이름이 등록되어 있기 때문일 뿐이다. CLI의 "prompt가 prompt이어야 한다"는 셸 본질이 흐려진다.
2. **세션 lifecycle 불투명** — process 생애에 한 `ChatSession` 인스턴스만 존재. 다중 대화 / 이전 대화 재개 / 새 대화 시작이 모두 *암묵적* (재시작이 곧 reset). 사용자가 직접 인식·제어할 수 없다.

사용자 요구(2026-05-18):
> "ai와 대화를 시작할때 cli에서 시작명령어를 쳐야 cli에서 대화가 가능하고 종료명령어를 통해 대화를 종료하면 cli로 돌아오도록 구현해줘. 그리고 대화내용은 영구저장해서 cli에서 ai와 대화를 시작할때 이전 대화를 로드하거나 새 대화를 시작하는등의 조작이 가능하도록 구현해줘"

즉 **명시적 mode 진입/탈출** + **영속화** + **세션 로드**가 필요.

## Decision

### CLI mode 모델

`Cli@1` 객체의 `state`에 두 필드를 추가한다:

- `mode: String` — `"shell"`(기본) | `"ai"`
- `session_name: Option<String>` — AI 모드에서만 set. shell 모드에서는 null.

(T7.5의 placeholder `session_id`는 *의미 모호*했으므로 제거하고 위 두 필드로 대체.)

`mode`는 *server tree에 broadcast*되어 컴포지터·AI·다른 클라이언트 모두 같은 진실을 본다. 모드 전환은 desktop-shell이 `submit_input` invoke 처리 결과로 `SetState`를 보낸다.

### Slash 명령 dispatch

- `/ai start [name]` — 새 세션 생성. name 생략 시 `conv-YYYYMMDD-HHMMSS` auto-name. AI 모드 진입.
- `/ai load <name>` — `~/.geulos/ai-sessions/<name>.json` 로드. history 복원. AI 모드 진입.
- `/ai list` — 디렉터리 안의 모든 세션 + 각 메시지 수 표시 (shell 모드든 ai 모드든 작동).
- `/exit` — *AI 모드 안에서만* 의미. 일반 CLI 모드로 복귀.
- 그 외 입력:
  - **shell 모드** — 기존 `help`/`echo`/`clear` 그대로. 등록 외 명령은 `unknown command` 한 줄 (T7.7의 prefix-free routing은 제거 — `/ai` 명시가 필요).
  - **ai 모드** — `/exit` / `/ai ...` 외 모든 입력은 AI에게 전달 (`SpecialAction::AiSend`).

### 영속화 형식

위치: `~/.geulos/ai-sessions/<name>.json`. (Windows: `%USERPROFILE%\.geulos\ai-sessions\`.) 디렉터리는 *세션 dir 접근 시 자동 생성*.

JSON schema:
```json
{
  "name": "conv-20260518-180000",
  "created_at": "2026-05-18T18:00:00Z",
  "model": "claude-sonnet-4-6",
  "history": [
    {"role": "User", "content": "..."},
    {"role": "Assistant", "content": [...]}
  ]
}
```

`LlmMessage`/`LlmRole`에 `#[derive(Serialize, Deserialize)]` 추가 — `content: Value`는 이미 serde-friendly.

**저장 시점:** *매 `send` 직후 즉시* (안전성 우선). 종료 시 별도 flush 불필요. 종료가 비정상(crash)이어도 마지막 send까지는 보존된다.

### 파일명 safety

`session_path(name)`은 `[A-Za-z0-9_-]+`만 허용. `../etc/passwd` 같은 path traversal·구분자 포함은 reject. 빈 문자열도 reject. 사용자 입력 / auto-name 모두 이 규칙 안에 있다.

### Auto-name

`conv-{YYYYMMDD}-{HHMMSS}` (UTC). `/ai list`가 이름 *역순* 정렬(`b.0.cmp(&a.0)`)하면 timestamp 기반 auto-name은 *자연히 최신 우선* 정렬된다.

### CliChatSession 재설계

- `new_from_env`는 *유지하지 않음*. 대신:
  - `CliChatSession::start(api_key, wire, system, name)` — 새 세션 (history 빈 상태).
  - `CliChatSession::load(api_key, wire, system, name)` — 디스크 세션 로드 (history 복원).
- 매 `send` 직후 `chat_persist::save(...)` 호출.
- desktop-shell main이 *시작 시* 세션을 만들지 않는다 — `chat_session: Option<CliChatSession>` 시작값은 `None`. `/ai start`/`load`에서 lazy 생성. API key 미설정이면 `/ai start`/`load`만 안내 메시지 — `/ai list`는 디렉터리 read이므로 정상 작동.

### 모드 전환 invariants

- shell 모드에서 자연어 입력 → AI로 가지 *않음*. `unknown command` 출력.
- AI 모드에서 `/ai start`/`/ai load` → 현재 세션 자연 종료(이미 disk에 commit됨) + 새 세션으로 *전환*.
- `/exit`은 AI 모드에서만 의미. shell 모드의 `/exit`은 `이미 셸 모드입니다.` 안내.

### 시각화

컴포지터 `render_cli`가 `Cli.state.mode` + `session_name` 보고 prompt를 결정:
- shell 모드 → `> `
- AI 모드 → `[ai:<session_name>] > `

사용자 입력 echo도 같은 prompt prefix로 출력(handle_cli_outcome에 prompt prefix 매개 추가).

## Alternatives rejected

- **prefix-free routing 유지 (T7.7 그대로)** — *사용자가 명시적으로 반대*. 모드 경계 명시·세션 lifecycle 명시가 본 task의 핵심 요구.
- **세션을 process-memory에만 두기** — 재시작이 곧 reset. 사용자 요구의 "이전 대화 로드" 직접 위배.
- **하나의 거대 JSONL 파일에 모든 세션 append** — 검색·삭제가 어려움. 한 세션 = 한 파일이 *디렉터리만 봐도* 일람 가능 + 사용자가 외부 도구(notepad/vim)로 직접 확인·편집 가능.
- **`/ai start`에서 별도 ChatSession new + name만 메모리에 보관, 종료 시 dump** — crash 시 손실. *매 send 직후 즉시 save*가 작은 부담으로 큰 안전성.
- **`SQLite` 등 DB로 저장** — 의존성 증가 + M7 v1 범위를 넘음. JSON 파일이면 인간이 읽고 grep도 가능.
- **시작·종료 명령 prefix 없이 별 키(`Ctrl+T`)로 모드 토글** — 슬래시 명령 패턴이 *명시·로그·재현 가능*. UI 키 binding은 향후 보조 단축키로 확장 가능 (M8+ v2).

## Consequences

- ADR-030의 *prefix-free routing 결정 부분만* superseded. *task 모델 vs chat 모델 분리*(=`Session::run_task` vs `ChatSession::send_message` 책임 분리)는 그대로 유지.
- desktop-shell이 `chat_session: Option<CliChatSession>`을 lazy 관리 — start/load/exit 시 mutate. 각 분기에서 `Cli.state.mode` + `session_name`을 SetState로 broadcast.
- 컴포지터 `render_cli`가 prompt 텍스트를 동적으로 만든다 — `[ai:<name>] > `는 한글 글리프 없음, 기존 measure_text_width로 정확한 cursor 좌표 보장.
- 영속 파일은 사용자 홈 아래 `.geulos/ai-sessions/` — Windows: `%USERPROFILE%\.geulos\ai-sessions\`, Linux/macOS: `$HOME/.geulos/ai-sessions/`. `std::env::var("USERPROFILE" | "HOME")`만 사용 — `dirs` crate 의존 추가 없음.
- 동시에 같은 세션을 두 프로세스에서 열면 마지막 write가 승리 (file lock 없음). 알려진 한계, v2 부채.
- 큰 history (수백 메시지)는 매 send마다 전체 dump = O(N) 디스크. M7 v1은 OK. v2에서 append-only journal 또는 SQLite 검토.

## 참고

- 관련 ADR: ADR-030 (chat session — 본 ADR이 routing 부분만 supersede), ADR-023 (CLI as shell), ADR-029 (한글 IME — prompt 입력 layer), ADR-009 (AI untrusted — in-process trade-off 유지)
- 관련 plan: `docs/plans/2026-05-18-geulos-m7-cli-extension.md` §T7.8
- 관련 manual test: `docs/manual-tests/m7-cli-acceptance.md`
- 기존 코드: `ai-bridge/src/chat_session.rs` (history accessor 추가), `apps/desktop-shell/src/{ai_session,cli_handler,main}.rs` (mode/세션 lifecycle), `compositor/src/render.rs::render_cli` (prompt 시각화)
