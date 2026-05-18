# ADR-023 — CLI as shell first-class (M7 T7.5)

**Status:** Accepted (2026-05-18)

## Context

M7 T7까지 데스크톱 셸은 좌측 FileTree + 우측 Canvas 두 패널만으로 구성됐다. 사용자가
직접 결정(2026-05-18):

> "내 글os는 ai가 직접 os를 조작할수 있다는점을 고려해서 현재의 화면좌측파일구조처럼
> ai와 대화를 진행할 cli를 구현해야해"
> "이건 단순히 ai기능의 추가라기보다 바탕화면에서 항상 접근 가능한 cli라고 보는게 맞아"
> "cli가 있어야 ai를 api형태로 호출하는데 용이하고 그래야 ai와 대화를 통해 작업을 진행할수 있어"

CLI를 어디에 둘 지의 옵션 검토:
- **Toggle 패널** (Ctrl+\` 같은 단축키로 표시/숨김): 컴팩트하지만 *셸의 본질*이라는 정체성
  약화. 매번 toggle 필요.
- **별 앱** (Canvas에 mount): 캔버스 점유. 다른 앱과 동시 사용 불가.
- **하단 고정 패널** (FileTree·Canvas와 동급): 항상 가용. 셸의 일급 구성요소로 위상 명확.

## Decision

CLI는 데스크톱 셸의 *4번째 builtin* — Desktop의 자식 [FileTree, Canvas, **Cli**] 순서로
mount한다. 화면 *하단*에 항상 보임 (toggle 아님). 일반 명령 dispatch와 (T7.7부터) AI
호출이 모두 여기서 시작된다.

- **네임스페이스:** `aios.builtin/Cli@1` — ADR-020의 `aios.builtin/*` (셸 빌트인) 정책과 일관.
- **레이아웃:** 상단 70% 높이에 FileTree(좌 30%) + Canvas(우 70%), 하단 30% 높이에 CLI 풀폭.
- **객체 모델:** `state.lines`(출력 히스토리)·`state.history`(입력 히스토리)·
  `state.session_id`(T7.7 AI 세션). 메서드 `submit_input(text)`·`clear()`·`append_line(text)`.
- **입력 버퍼 위치:** `input_buffer`/`cursor_pos`는 *컴포지터 local state* (server tree와
  분리). 매 키 입력마다 invoke를 보내면 latency가 크기 때문. Enter commit 시점에만
  `submit_input` invoke. Cli 객체는 commit된 lines만 server에 보관.
- **명령 dispatch:** desktop-shell이 `submit_input` invoke를 받아 `cli_handler::dispatch_command`로
  파싱·실행, 결과를 lines에 append + StateSetMsg broadcast.
- **T7.5 범위:** ASCII 입력 + `help`/`clear`/`echo`/unknown 4종 명령. 한글 IME는 T7.6,
  AI 호출은 T7.7.

## Consequences

- CLI가 셸의 *본질*로 명시 — bash/PowerShell의 위상과 동등.
- AI는 CLI 위의 *한 명령*. CLI 없이 AI 호출 안 함.
- input_buffer를 컴포지터 local state로 두므로 단일 라이터 이벤트 루프(ADR-003) 위배 X —
  Cli 객체의 mutate는 여전히 desktop-shell이 단일 라이터.
- ACL: 컴포지터(외부 actor)가 `submit_input` invoke 가능해야 함. T8(KI-001 정리) 시
  매니페스트 기반 권한으로 교체 — T7.5는 일단 wildcard ACL.
- focused 객체 개념 부재. T7.5는 *Cli만 키보드 입력 받음* 가정 — 후속에 TextArea 등 추가
  시 focus 시스템 필요.
- 한 CLI = 한 AI 세션 (T7.7). 다중 세션/탭은 v2.
