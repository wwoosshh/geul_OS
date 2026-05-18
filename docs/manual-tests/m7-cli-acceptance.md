# M7 CLI Extension Acceptance — T7.5 / T7.6 / T7.7 / T7.8 / T7.9 통합

> 데스크톱 셸 하단 CLI 패널의 acceptance 절차. T7.5(기본 명령) + T7.6(한글 IME) +
> T7.7(AI chat session) + **T7.8 (명시적 mode + 영속 세션, ADR-031)** +
> **T7.9 (API key 자동 입력/검증/저장, ADR-032)** 이 한 흐름에서
> 검증된다. 통과 시 M7 보조 plan(`docs/plans/2026-05-18-geulos-m7-cli-extension.md`) 완료.

## 사전 조건

- 빌드:
  ```powershell
  cargo build -p geulos-server-host -p geulos-desktop-shell -p geulos-compositor
  ```
- 환경 변수 또는 workspace root의 `.env`에 `ANTHROPIC_API_KEY=sk-ant-...` 설정
  (시나리오 C의 `/ai start`/`load` + 실제 prompt 검증용). **시나리오 D(T7.9)는
  반대로 키가 *없는* 상태**가 필요 — 환경 변수 unset + `~/.geulos/api_key` 삭제 후
  desktop-shell 재시작. `/ai list`는 키 유무 무관하게 정상 작동 (graceful degradation).
- 한국어 IME 활성화 (Windows의 경우 [Win+Space]로 한/영 전환 가능).
- T7.8 영속 세션 파일 위치: `%USERPROFILE%\.geulos\ai-sessions\<name>.json` (Windows).

## 실행 순서 (3 터미널)

> 순서: server → desktop-shell → compositor. desktop-shell이 startup에 객체를 mount해야
> compositor의 초기 Query가 트리를 모두 본다 (M4 acceptance와 동일 패턴).

1. **Terminal 1 — server-host**:
   ```powershell
   .\target\debug\geulosd.exe
   ```
   예상 로그: `[server-host] listening on 127.0.0.1:5550`

2. **Terminal 2 — desktop-shell**:
   ```powershell
   .\target\debug\geulos-desktop-shell.exe
   ```
   예상 로그(키 유무 무관 — T7.8부터 session은 lazy):
   ```
   [desktop-shell] connecting to 127.0.0.1:5550...
   [desktop-shell] HelloAck: actor=...
   [desktop-shell] mounted N objects
   [desktop-shell] subscribed to ... targets
   [desktop-shell] CLI 시작 (shell 모드). /ai start | /ai load | /ai list | /exit 으로 AI 모드 진입/탈출.
   ```

3. **Terminal 3 — compositor**:
   ```powershell
   .\target\debug\geulos-compositor.exe
   ```
   창이 뜨고 좌측 FileTree, 우측 Canvas/Explorer, 하단 CLI 패널(검정 배경, `> _` 프롬프트)이 보여야 한다.

---

## 시나리오 A — 기본 명령 (T7.5)

| 단계 | 입력 | 기대 결과 |
|---|---|---|
| A1 | CLI 패널 클릭 | CLI에 키보드 focus (cursor 깜빡임) |
| A2 | `help` + Enter | `> help` echo 후 사용 가능 명령 목록 5줄 출력 (help/clear/echo/AI 안내) |
| A3 | `echo hello world` + Enter | `> echo hello world` echo 후 `hello world` 한 줄 출력 |
| A4 | `clear` + Enter | CLI lines 전체가 비워짐. 입력 echo도 사라짐 (POSIX `clear`와 일관) |
| A5 | `echo` (인자 없이) + Enter | `> echo` echo 후 빈 라인 한 줄 출력 |

## 시나리오 B — 한글 IME (T7.6 / ADR-029)

| 단계 | 입력 | 기대 결과 |
|---|---|---|
| B1 | OS IME를 한국어로 전환 (Win+Space) | OS의 IME 상태바가 "한"으로 표시 |
| B2 | `안녕하세요` 타이핑 | 조합 중인 글자는 `input_buffer` 끝에 *회색(#888888)* 으로 표시 (preedit) |
| B3 | Space 또는 Enter로 commit | 회색 글자가 *흰색*으로 바뀌어 input_buffer에 들어감 |
| B4 | (시나리오 C로 이어짐) `echo 안녕` + Enter | `> echo 안녕` + `안녕` 출력 — 한글이 깨지지 않고 정확히 표시 |

## 시나리오 C — AI 대화 mode + 영속 세션 (T7.8 / ADR-031)

> 응답까지 수 초 ~ 수십 초 — 인터넷 + Anthropic API 응답 시간에 의존. 응답 대기 동안
> CLI invoke 루프가 *blocking* (M7 v1 trade-off — v2에 비동기).

### C-1 새 세션 시작 + 자연어 prompt

| 단계 | 입력 | 기대 결과 |
|---|---|---|
| C1.1 | `현재 데스크톱에 어떤 객체가 있는지 알려줘` | shell 모드라 *AI 호출되지 않음* — `unknown command: 현재...` 한 줄 (T7.8 prefix-free routing 제거) |
| C1.2 | `/ai start` | prompt가 `> /ai start` → `(새 AI 세션 시작: conv-YYYYMMDD-HHMMSS)` 한 줄. prompt가 `[ai:conv-...] > `로 변경 |
| C1.3 | `현재 데스크톱에 어떤 객체가 있는지 알려줘` | desktop-shell 로그에 `AI prompt: ...`. 수 초 후 AI 응답이 CLI에 라인별로 출력 |
| C1.4 | `방금 본 객체 중에 Folder는 몇 개야?` | AI가 *이전 turn의 결과를 참고*해 답함 (history 누적) |
| C1.5 | `/exit` | `(셸 모드로 복귀)` 출력 + prompt가 `> `로 복원 |

### C-2 list + load — 이전 대화 재개

| 단계 | 입력 | 기대 결과 |
|---|---|---|
| C2.1 | `/ai list` | `저장된 세션 (1): conv-YYYYMMDD-HHMMSS  (4 메시지)` (C1에서 만든 세션) |
| C2.2 | `/ai load conv-YYYYMMDD-HHMMSS` (위 list에서 본 이름) | `(AI 세션 로드: conv-...)` + prompt가 `[ai:conv-...] > ` |
| C2.3 | `우리가 방금 무슨 얘기 했지?` | AI가 *디스크에서 복원한 history 기반으로 응답* — 이전 대화의 *지시대명사* 살아있음을 확인 |
| C2.4 | `/exit` | shell 모드 복귀 |

### C-3 명시적 이름 + 다중 세션

| 단계 | 입력 | 기대 결과 |
|---|---|---|
| C3.1 | `/ai start project_x` | `(새 AI 세션 시작: project_x)` + prompt `[ai:project_x] > ` |
| C3.2 | `안녕` | AI 인사 응답 |
| C3.3 | `/ai start project_y` (AI 모드 안에서 세션 *전환*) | `(새 AI 세션 시작: project_y)` + prompt `[ai:project_y] > ` (project_x 세션은 이미 디스크 commit) |
| C3.4 | `/exit` 후 `/ai list` | 최소 `project_x`, `project_y`, `conv-...` 세 세션이 보임 (이름 역순 정렬) |

### C-4 영속 검증 — 프로세스 재시작 후 로드

| 단계 | 입력 | 기대 결과 |
|---|---|---|
| C4.1 | desktop-shell + compositor 종료 후 재시작 | 정상 시작, shell 모드 |
| C4.2 | `/ai list` | C3에서 만든 `project_x` 등이 여전히 보임 (디스크 영속) |
| C4.3 | `/ai load project_x` 후 `방금 뭐라고 했지?` | 재시작 전 대화 컨텍스트가 살아있음 |

### 에러 케이스

| 케이스 | 기대 결과 |
|---|---|
| `ANTHROPIC_API_KEY` 미설정 + `~/.geulos/api_key` 없음 상태에서 `/ai start` | **T7.9 (ADR-032):** CLI mode가 `awaiting_api_key`로 전환, prompt가 `[API key 입력] > `로 바뀌고 안내 라인 `[ANTHROPIC_API_KEY 미설정] CLI에 키를 입력 후 Enter (취소: /exit)` 출력. 시나리오 D 참조 |
| 없는 세션 이름으로 `/ai load nosuch` | `[AI load 실패: io: ...No such file...]` 한 줄. mode는 shell 그대로 |
| 잘못된 세션 이름(`/ai load a/b`) | `[AI load 실패: config: invalid session name: ...]` |
| 네트워크 차단 상태에서 AI prompt | `[AI 오류: network: ...]`. session은 보존 — 다음 prompt 다시 시도 |
| AI가 매우 긴 응답 (수십 줄) | 모두 lines에 append. cap(1000라인) 초과 시 오래된 라인부터 잘림 |
| `clear` (AI 모드 안) | AI에게 *"clear"* 단어 prompt로 전달 (AI 모드는 slash 외 모든 입력이 AI prompt). 출력 히스토리는 비우지 않음 — 비우려면 `/exit` 후 `clear` |

## 시나리오 D — API key 자동 입력/검증/저장 (T7.9 / ADR-032)

> AI key 영속 파일: `%USERPROFILE%\.geulos\api_key` (Windows). plain text 한 줄.
> 이 시나리오 시작 전 *해당 파일이 없어야* 하고 `ANTHROPIC_API_KEY` 환경 변수도
> 미설정이어야 한다 (시나리오 C와 분리). 파일을 일시 삭제(또는 백업 이동)한 뒤
> desktop-shell을 재시작하면 깨끗한 상태가 된다.

### D-1 키 prompt + 잘못된 키 + 재입력 + 저장 + 자동 이어 실행

| 단계 | 입력 | 기대 결과 |
|---|---|---|
| D1.1 | `/ai start` | prompt `> /ai start` echo + 안내 `[ANTHROPIC_API_KEY 미설정] CLI에 키를 입력 후 Enter (취소: /exit)` + prompt가 `[API key 입력] > `로 전환. `Cli.state.mode = "awaiting_api_key"`, `pending_action = "start"` |
| D1.2 | (잘못된 key — `sk-fake-invalid`) | `[API key 입력] > sk-fake-invalid` echo + `[검증 실패: config: API key 무효 (401 Unauthorized)] 다시 입력하거나 /exit으로 취소.` + mode/prompt 유지 (재입력 가능) |
| D1.3 | (올바른 key) | `[API key 입력] > <key>` echo + `[저장됨 ~/.geulos/api_key]` + `(새 AI 세션 시작: conv-YYYYMMDD-HHMMSS)` + prompt가 `[ai:conv-...] > `로 전환 (자동으로 원래 `/ai start` 실행). `~/.geulos/api_key`에 key가 plain text로 저장됨 |
| D1.4 | `/exit` | shell 모드 복귀 |

### D-2 다음 실행에서 자동 로드 (재시작 시나리오)

| 단계 | 입력 | 기대 결과 |
|---|---|---|
| D2.1 | desktop-shell + compositor 종료 후 재시작 | 정상 시작, shell 모드. 환경 변수는 여전히 미설정이지만 D1.3에서 저장한 `~/.geulos/api_key`가 있음 |
| D2.2 | `/ai start` | *prompt를 보이지 않고* 곧장 `(새 AI 세션 시작: conv-...)` — chain의 *저장 파일* 단계에서 잡혀 awaiting 모드로 진입하지 않음 |
| D2.3 | `/exit` | shell 모드 복귀 |

### D-3 prompt 도중 cancel

| 단계 | 입력 | 기대 결과 |
|---|---|---|
| D3.1 | (`~/.geulos/api_key` 삭제 후) `/ai load conv-X` | prompt가 `[API key 입력] > `로 전환, `pending_action = "load conv-X"` |
| D3.2 | `/exit` | `(API key 입력 취소 — 셸 모드로 복귀)` + prompt `> ` 복귀. `pending_action = null`, mode = shell. 세션 로드는 *수행되지 않음* |

### 우선순위 chain 검증

| 순서 | 시도 | 기대 |
|---|---|---|
| 1 | `set ANTHROPIC_API_KEY=sk-env` + 파일 존재 | env가 승 (chain 1 > 3) |
| 2 | env 미설정 + 파일 존재 | 파일 키 사용 |
| 3 | env 미설정 + 파일 없음 | awaiting mode 진입 |
| 4 | env가 *공백만* (whitespace) + 파일 존재 | 공백은 *없음*으로 취급, 파일 키 사용 |

## 검증 시각화 (T5)

- AI 응답으로 추가된 라인은 `last_change_actor = AI actor_id`로 기록 — compositor가 그 라인 옆에 *노란 점*을 그려야 한다. (T5의 ai 시각화 메커니즘 자동 적용.) 사람이 `echo`로 추가한 라인은 노란 점 없음.

## 통과 조건

- 모든 시나리오 A 단계 통과
- 시나리오 B의 IME 동작 — Windows 11 한국어 IME에서 preedit/commit 정상 (B1~B4)
- 시나리오 C-1 ~ C-4 통과 (AI key 있을 때) — multi-turn 컨텍스트 유지 + 영속 + 재시작 후 로드 확인
- 시나리오 D-1 ~ D-3 통과 — key 없을 때 CLI 입력 prompt + 검증 + 저장 + 자동 이어 실행, cancel 동작 (T7.9)
- key 없을 때도 `/ai list`는 정상 작동 (그래픽 degradation)
- 한 시간 이상 띄워둬도 desktop-shell이 crash하지 않음 (idle 상태에서 wire close 안 됨)

## 알려진 한계 (v1)

- AI 응답 대기 동안 CLI 외 다른 invoke (파일 트리 expand 등) blocking — v2에 tokio spawn으로 비동기.
- 동시에 같은 세션을 두 프로세스에서 열면 *마지막 write가 승* (file lock 없음). v2 부채.
- 다중 동시 세션 미지원 — 한 시점 한 활성 세션. v2.
- 응답 streaming(토큰 단위) 미지원 — 한 번에 전체 응답 도착. v2에 streaming.
- AI 모드의 `clear`는 AI에 prompt로 전달됨 (lines 비움 X) — 비우려면 `/exit` 후 `clear`. v2에 `/clear` 같은 메타 명령 검토.
