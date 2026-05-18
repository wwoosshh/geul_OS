# ADR-032 — API key 자동 입력/검증/저장 (M7 T7.9)

**Status:** Accepted (2026-05-18)

## Context

ADR-030(T7.7)·ADR-031(T7.8)로 CLI에서 AI 대화가 가능해졌으나 *AI 활성에 `ANTHROPIC_API_KEY` 환경 변수가 반드시 set*되어 있어야 한다. 미설정 시 `/ai start`/`load` 분기는 `[AI start 실패: config: ANTHROPIC_API_KEY not set]`을 한 줄 출력하고 끝 — 사용자가 *직접 환경 변수를 설정*하고 *desktop-shell을 재시작*해야 한다. 처음 사용자에게 friction이 크다.

사용자 요구(2026-05-18):
> AI 명령에서 key가 없으면 CLI 위에서 직접 입력 받고, 검증해서 저장하고, 원래 명령을 자동 이어 실행해줘. 다음부터는 저장된 key를 자동으로 쓰면 돼.

## Decision

### Key resolution chain (우선순위)

`geulos_ai_bridge::api_key::try_load()` 한 함수가 다음 순서로 시도:

1. `ANTHROPIC_API_KEY` 환경 변수 (이미 set) — 비어있지 않으면 채택.
2. `.env` 파일 (ADR-030 이후 dotenvy로 자동 load되어 1과 동일 경로).
3. `~/.geulos/api_key` (T7.9 신규 영속 파일, plain text 한 줄).
4. *CLI에서 사용자 직접 입력 프롬프트* — desktop-shell이 mode 전환으로 처리 (호출자 책임).

위 1~3은 ai-bridge가, 4는 desktop-shell이 담당한다 — *ai-bridge는 헤드리스 핵심 + desktop-shell이 UX* 책임 분리.

### 입력 받는 흐름 (Cli mode "awaiting_api_key")

`Cli@1.state.mode`에 신규 값 `"awaiting_api_key"` 추가 (ADR-031의 `"shell"`/`"ai"` 외). 흐름:

1. 사용자가 `/ai start [name]` 또는 `/ai load <name>` 입력.
2. desktop-shell이 `resolve_api_key()` 호출 → `None`이면 즉시 AI 세션 생성하지 *않고*:
   - `Cli.state.mode = "awaiting_api_key"`
   - `Cli.state.pending_action = "start"` / `"start NAME"` / `"load NAME"` (단순 string 인코딩 — main이 다시 parse)
   - CLI에 안내 메시지: `[ANTHROPIC_API_KEY 미설정] CLI에 키를 입력 후 Enter (취소: /exit)`
3. `mode = "awaiting_api_key"` 상태에서 `submit_input`은 *입력 텍스트를 key로 처리*:
   - `/exit` 또는 빈 문자열 → cancel, mode=shell 복귀, pending_action=null.
   - 그 외 → `api_key::validate(&key).await` 호출:
     - 성공 → `api_key::save_to_file(&key)` + mode=shell + pending_action=null SetState + pending 액션을 재실행 (즉 새 `AiStart`/`AiLoad`를 dispatch).
     - 실패 → 에러 메시지 한 줄 + mode 유지 → 사용자가 *재입력* 또는 `/exit` 가능.

### 검증 호출

Anthropic `GET https://api.anthropic.com/v1/models`:
- 헤더 `x-api-key: <key>` + `anthropic-version: 2023-06-01`.
- 200 OK → 유효.
- 401 Unauthorized → 무효 (사용자에게 친화적인 메시지).
- 그 외 status → 에러로 propagate.

타임아웃: `reqwest::Client::builder().timeout(Duration::from_secs(10))`. 무한 대기 방지.

### 영속 저장

위치: `~/.geulos/api_key` (Windows: `%USERPROFILE%\.geulos\api_key`). plain text, key 한 줄 (양끝 trim). chat_persist의 `sessions_dir` 패턴과 동일 — `USERPROFILE` 또는 `HOME` 환경 변수만 사용 (dirs crate 의존 회피).

**보안 노트:** plain text 저장은 v1 단순화. 사용자 단독 머신 가정 (M7). v2(M9+)에서 OS keychain(Windows Credential Manager / macOS Keychain / libsecret) 또는 sysconfig 단방향 hash 검토. 파일 권한(Unix 600)도 v2 부채.

### Cancel & 재입력

- `/exit`은 mode="awaiting_api_key"에서도 작동 — *cancel*로 의미 매핑. mode=shell + pending_action=null 복귀.
- 검증 실패 시 mode 유지 → 사용자가 *다시 입력* 가능. 재시도 횟수 제한 없음 (사용자가 `/exit`로 빠질 수 있으므로 무한 루프 위험 없음).

### Pending action 인코딩

`Cli.state.pending_action: String | null`. 단순 공백 분리 string:
- `"start"` — `/ai start` (이름 생략).
- `"start NAME"` — `/ai start NAME`.
- `"load NAME"` — `/ai load NAME`.

검증 성공 후 main이 `splitn(2, ' ')` 으로 parse해 원래 `SpecialAction::AiStart` / `AiLoad`를 *내부적으로 재구성*해 동일 분기 재실행. 세션 이름 규칙(`[A-Za-z0-9_-]+`)에 공백·메타문자 없으므로 단순 인코딩으로 충분.

### Masking

v1은 입력 plain text 그대로 표시. UX는 약하지만 mode 전환 안내가 명시적이라 사용자가 누가 보고있지 않은지 인지하고 입력. v2에서 input_buffer만 `*` 마스킹 (compositor render 분기 추가).

## Alternatives rejected

- **저장 안 함 (process-memory 만)** — 재시작마다 다시 입력. 사용자 의도 위배.
- **OS keychain v1** — 플랫폼별 의존성 + 초기 셋업 부담. M7 범위를 넘음. v2.
- **GUI 다이얼로그** — 컴포지터가 modal 다이얼로그를 그릴 인프라 없음 (Window는 file viewer만). CLI 입력이 가장 자연스러운 곳 (이미 mode 모델 존재).
- **HEAD 요청** — `/v1/models` GET이 더 명확한 동작. body 무시 → 부담 작음. 10초 타임아웃.
- **하나의 prompt 안에서 즉시 한 prompt에 key+자연어를 받아 처리** — UX 복잡. mode 분리가 깨끗.
- **검증 skip + 저장만** — 잘못된 키를 저장하면 다음 실행에서 같은 실패 반복. 검증이 한 번 추가 호출 비용 vs UX 안정성: 안정성 승.

## Consequences

- ai-bridge에 신규 `api_key` 모듈 — `try_load() -> Option<String>`, `validate(&str) -> BridgeResult<()>`, `save_to_file(&str) -> BridgeResult<()>`. reqwest로 호출 (이미 dep).
- desktop-shell의 `ai_session::api_key_from_env`는 deprecated alias로 보존하고 신규 `resolve_api_key()`가 chain을 사용.
- desktop-shell main의 `submit_input` 분기에 `mode == "awaiting_api_key"` 케이스 추가 — `dispatch_command`/`dispatch_chat` 호출 *대신* 입력 텍스트를 key로 처리.
- compositor `render_cli`는 mode에 `awaiting_api_key` 케이스 추가 — prompt `[API key 입력] > `.
- `Cli@1.state`에 `pending_action: Option<String>` 신규. 회귀 테스트 추가.
- `cli_handler::SpecialAction`은 변경 없음 — awaiting mode 처리는 main 분기 (dispatch 함수 외부).
- 영속 파일은 사용자 홈 — chat_persist와 동일 패턴. 한 머신 한 사용자 1 key 가정.
- ADR-030/031을 supersede *하지 않음* — 본 ADR은 그 위에 *친화적 입력 layer*를 더한다.

## 참고

- 관련 ADR: ADR-030 (chat session), ADR-031 (mode + 영속), ADR-023 (CLI as shell)
- 관련 plan: `docs/plans/2026-05-18-geulos-m7-cli-extension.md` §T7.9
- 관련 manual test: `docs/manual-tests/m7-cli-acceptance.md` 시나리오 D
- 관련 코드: `ai-bridge/src/api_key.rs` (신규), `apps/desktop-shell/src/{ai_session,main}.rs`, `compositor/src/render.rs::render_cli`, `core/src/object/std_types.rs::cli`
