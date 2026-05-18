# ADR-029 — 한글 IME 통합 (M7 T7.6)

**Status:** Accepted (2026-05-18)

## Context

ADR-023(CLI as shell)으로 데스크톱 셸 하단에 항상 보이는 CLI 패널을 도입했고, T7.5(ASCII v1)에서 영문/숫자/공백 입력을 처리했다. 그러나 사용자가 *한국 솔로 빌더이고 AI와 한국어로 대화*하는 GeulOS의 첫 도그푸딩 사용자임에도, 한글 자판을 눌러도 화면에 아무것도 나타나지 않는다.

원인: `compositor/src/main.rs::key_event_to_action`가 `KeyboardInput.text`가 *다중 char*인 경우(IME가 조합 결과를 한꺼번에 보내는 케이스 포함)를 무조건 무시한다. 한글 입력은 OS IME(Windows 11의 경우 TSF)가 별도 채널 — `WindowEvent::Ime` — 로 *조합 중 텍스트(Preedit)* 와 *조합 완료 텍스트(Commit)* 를 emit하는데, T7.5는 이 채널을 켜지도, 받지도 않는다.

옵션 검토:

- **자체 IME 엔진 구현 (직접 한글 조합 자동완성)** — 두벌식 자판 매핑 + 초성/중성/종성 조합 상태기계. *기각*: OS가 이미 제공하는 기능의 재발명. 다른 언어 IME(중국어 병음, 일본어 가나) 지원 시 폭발적 확장. OS IME 설정(키 매핑 커스터마이즈, 외부 IME 도구) 무시.
- **winit `WindowEvent::Ime` 위임** — winit 0.30이 Windows TSF / Linux IBus/Fcitx / macOS InputMethod를 자동으로 위임. *채택*. 자체 IME 코드 없이도 OS의 IME가 *그대로* 동작.

## Decision

winit `WindowEvent::Ime(Ime::Preedit / Ime::Commit / Ime::Enabled / Ime::Disabled)` 이벤트를 사용해 OS IME에 위임한다. 자체 IME 엔진은 만들지 않는다.

- **활성화:** `App::resumed`에서 `Window::set_ime_allowed(true)` 호출. Windows에서는 TSF가 winit를 통해 Preedit/Commit 이벤트 emit 시작.
- **컴포지터-로컬 상태:** `keyboard::CliLocalState`에 `preedit_text: String` 필드 추가. `input_buffer`와 같은 위치(*server tree와 분리 — ADR-023*) — 조합 중 텍스트는 *영영 server에 전송되지 않음*. Commit 시점에만 `input_buffer`로 들어가고, `submit_input` invoke는 *Enter 시점*에 발생 (T7.5 동작 유지).
- **라우팅:** `KeyboardFocus::Cli`일 때만 IME 이벤트를 cli_state에 반영한다. `KeyboardFocus::Window(_)` / `KeyboardFocus::None`이면 *완전 무시* — M8 read-only Window 본문은 키 입력을 받지 않으므로 일관됨. v2에서 TextArea/Editable Window가 도입되면 IME 라우팅도 그쪽으로 확장.
- **렌더:** `render_cli`에서 `input_buffer` 끝에 `preedit_text`를 *회색(`#888888`)* 으로 표시 — 사용자가 *조합 중* 임을 시각적으로 구분. cursor는 `input_buffer.cursor_pos` 위치에 기존 동작 그대로 (preedit는 cursor 뒤에 추가됨).
- **Enabled/Disabled:** 무시. winit 명세상 Disabled 도착 직전에 빈 Preedit가 emit되므로 `preedit_text`는 자연스럽게 비워진다.

## Consequences

- 한국 사용자가 *바로 한글 입력 가능* — GeulOS 첫 도그푸딩 차단 사항 해소.
- 자체 IME 코드 0줄 — 유지보수 부담 없음. 다른 언어 IME도 OS가 알아서 처리.
- **Preedit 위치 단순화 (v1):** preedit는 *cursor 위치와 무관*하게 `input_buffer` 끝에 그린다. 사용자가 cursor를 input 중간으로 옮긴 채 IME 입력하면 preedit가 끝에 표시 — UX 약점. v2에서 cursor 위치에 preedit 삽입 + cursor 자체를 preedit 내부 byte offset으로 이동.
- **플랫폼 차이:** winit IME는 Windows에서 안정적이지만 Linux(Wayland: IBus/Fcitx 필요)·macOS(InputMethod)는 환경에 따라 동작 차이. M7은 Windows 첫 도그푸딩 우선 — 비-Windows는 후속 마일스톤에서 검증.
- **Fallback:** 만일 사용자 환경에서 winit IME가 작동 안 하면 *clipboard paste (Ctrl+V)* 가 우회 경로 — known-issues에 명시. M9의 클립보드 API와 통합.
- T7.5의 `// TODO(T7.6): IME pre-edit 다중 문자 처리` 마커는 이 ADR로 제거. `key_event_to_action`의 multi-char 분기는 *KeyboardInput과 Ime 채널의 중복 방지* 의미로 남는다.

## 참고

- 관련 ADR: ADR-023 (CLI as shell — 본 ADR이 그 위에 IME 입력 layer 추가), ADR-009 (AI 가시성 — 한국어 prompt 가능)
- 관련 plan: `docs/plans/2026-05-18-geulos-m7-cli-extension.md` §T7.6
- 외부: winit 0.30 `WindowEvent::Ime` / `Window::set_ime_allowed`, Windows TSF (Text Services Framework)
