# M7 CLI Extension Acceptance — T7.5 / T7.6 / T7.7 통합

> 데스크톱 셸 하단 CLI 패널의 acceptance 절차. T7.5(기본 명령) + T7.6(한글 IME) + T7.7(AI chat session)이
> 한 흐름에서 검증된다. 통과 시 M7 보조 plan(`docs/plans/2026-05-18-geulos-m7-cli-extension.md`) 완료.

## 사전 조건

- 빌드:
  ```powershell
  cargo build -p geulos-server-host -p geulos-desktop-shell -p geulos-compositor
  ```
- 환경 변수 또는 workspace root의 `.env`에 `ANTHROPIC_API_KEY=sk-ant-...` 설정 (T7.7 시나리오 C용).
  키가 없으면 T7.7 분기는 *graceful degradation* 메시지로 동작하지만 AI 응답은 못 받음 → 시나리오 C 검증 불가.
- 한국어 IME 활성화 (Windows의 경우 [Win+Space]로 한/영 전환 가능).

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
   예상 로그(키 있을 때):
   ```
   [desktop-shell] connecting to 127.0.0.1:5550...
   [desktop-shell] HelloAck: actor=...
   [desktop-shell] mounted N objects
   [desktop-shell] subscribed to ... targets
   [desktop-shell] AI chat session 활성 (model=claude-sonnet-4-6)
   ```
   키 없을 때(graceful degradation):
   ```
   [desktop-shell] AI chat session 비활성: config: ANTHROPIC_API_KEY not set (echo/help/clear만 동작)
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

## 시나리오 C — AI 대화 (T7.7 / ADR-030)

> 키가 설정돼 있어야 함. 응답까지 수 초 ~ 수십 초 — 인터넷 + Anthropic API 응답 시간에 의존.
> 응답 대기 동안 CLI invoke 루프가 *blocking*된다 (M7 v1 trade-off — v2에 비동기).

| 단계 | 입력 | 기대 결과 |
|---|---|---|
| C1 | `현재 데스크톱에 어떤 객체가 있는지 알려줘` + Enter | desktop-shell 로그에 `AI prompt: 현재...`. 수 초 후 AI 응답이 CLI에 라인별로 출력. AI는 `list_objects_by_type`을 호출해 Desktop/FileTree/Explorer/Cli/Window/Folder 등을 본 뒤 종합해 답함 |
| C2 | `방금 본 객체 중에 Folder는 몇 개야?` + Enter | AI가 *이전 turn의 결과를 참고*해 답함 (history 누적 확인). 또는 추가 query 가능 |
| C3 | `unknown_xxx` + Enter | echo/help/clear가 아닌 입력은 자동으로 AI에 전달 (prefix-free routing). AI가 적절히 응대 |
| C4 | `clear` + Enter | CLI lines 비워짐. **단** AI session history는 *유지* — 다음 prompt에서 이전 컨텍스트 여전히 유효 (M7 v1; M8에 reset 옵션 검토) |

### 에러 케이스

| 케이스 | 기대 결과 |
|---|---|
| `ANTHROPIC_API_KEY` 미설정 상태에서 자연어 입력 | CLI에 `[AI 비활성 — ANTHROPIC_API_KEY 미설정]` 한 줄. echo/help/clear는 그대로 동작 |
| 네트워크 차단 상태에서 자연어 입력 | CLI에 `[AI 오류: network: ...]` 한 줄. 다음 prompt 다시 시도 가능 (session 보존) |
| AI가 매우 긴 응답 (수십 줄) | 모두 lines에 append. cap(1000라인) 초과 시 오래된 라인부터 잘림 |

## 검증 시각화 (T5)

- AI 응답으로 추가된 라인은 `last_change_actor = AI actor_id`로 기록 — compositor가 그 라인 옆에 *노란 점*을 그려야 한다. (T5의 ai 시각화 메커니즘 자동 적용.) 사람이 `echo`로 추가한 라인은 노란 점 없음.

## 통과 조건

- 모든 시나리오 A 단계 통과
- 시나리오 B의 IME 동작 — Windows 11 한국어 IME에서 preedit/commit 정상 (B1~B4)
- 시나리오 C 중 C1·C2·C4 통과 (AI key 있을 때) — multi-turn 컨텍스트 유지 확인
- key 없을 때 graceful degradation 메시지 확인
- 한 시간 이상 띄워둬도 desktop-shell이 crash하지 않음 (idle 상태에서 wire close 안 됨)

## 알려진 한계 (v1)

- AI 응답 대기 동안 CLI 외 다른 invoke (파일 트리 expand 등) blocking — v2에 tokio spawn으로 비동기.
- `clear`가 AI session history를 reset하지 않음 — UX 약점. v2에 명시적 `reset` 명령 또는 `clear --session`.
- 다중 CLI / 다중 세션 미지원 — v2.
- 응답 streaming(토큰 단위) 미지원 — 한 번에 전체 응답 도착. v2에 streaming.
