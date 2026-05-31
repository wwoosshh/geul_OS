> **Status:** adopted (2026-05-31)
> **Note:** SP1 채택 — Desktop@1 + cli_height + applauncher + 윈도잉 환경 정착 (`apps/desktop-shell/src/applauncher.rs` + handlers/shell_methods.rs). 후속 SP2~SP4 별도.

# SP1 — 데스크톱 셸 환경 (윈도잉 데스크톱으로 재설계)

**Date:** 2026-05-29
**Status:** Draft (사용자 review 대기)
**Parent:** UI 재설계 분해의 첫 sub-project. 후속: SP2 파일관리자 심화, SP3 메모장, SP4 한글 IME.

## 동기

현재 GeulOS 데스크톱은 **고정 3패널**(좌 FileTree 25% / 우 Explorer 75% / 하단 CLI)이다. 사용자는 이를 일반 OS(Windows/macOS)처럼 **윈도잉 데스크톱 환경**으로 바꾸려 한다: 바탕화면 + 실행 아이콘 + 상단 네비게이션 바 + 우측 퀵런치 독 + 그 위에 떠 있는 창 앱(파일관리자 등) + 상하 리사이즈 가능한 CLI.

**관통 원칙 (GeulOS 비전):** *AI가 사용할 수 있는 명령과 사용자가 할 수 있는 명령이 동일해야 한다.* AI는 OS를 쓰는 "사용자 2"다. 프로그램이 화면에 출력만 되는 게 아니라, **버튼·기능 하나하나가 AI에게 접근 가능한 선택지(invoke 가능한 객체 메서드)로 노출**되어야 한다. 이미 AI가 컴포지터 경유 파일 생성/수정/리네임/삭제 + 쉘 명령 실행으로 부분 입증됨. SP1의 모든 새 요소는 이 불변식을 지킨다.

## 핵심 발견 (설계 근거)

탐색 결과(파일·라인 인용):

- **떠있는 창은 이미 동작한다.** `core/src/object/std_types.rs`의 `Window@1`(L323-365)은 `x/y/w/h/z/focused/content` state + `move/resize/focus/close` 메서드를 갖고, 다중 창 공존·z-order·focus가 `apps/desktop-shell/src/handlers/window_methods.rs`(L56-124)·`window_ops.rs`에 구현됨. 윈도잉 엔진의 절반이 이미 있음.
- **레이아웃은 고정이다.** `compositor/src/layout.rs`의 `layout_desktop`(L191-342)이 좌25%/우75%/하단CLI를 하드코딩하고, 떠있는 `Window@1`/`ConsoleWindow@1`만 state의 x/y/w/h로 자유 배치. → 바탕화면/네비바/독/아이콘을 위해 이 함수를 확장해야 함.
- **본문 동작은 이미 AI-invocable.** `FileTree`(expand/select/refresh), `Explorer`(navigate_to/navigate_up/open_file)는 이미 메서드로 노출 → 파일관리자 창은 "창 틀 + 기존 FileTree/Explorer"로 재구성하면 명령 표면이 자동 보장.
- **재사용 자산**: `compositor/src/theme.rs`(색/간격 토큰), `window_geom.rs`(타이틀바·리사이즈·최소크기 상수), `hit_test.rs`(역순 z hit-test + 모달 우선), `icons.rs`(아이콘 렌더), `render.rs`(`render_window` 등).
- **클릭 = Invoke 경로 확립됨.** 컴포지터 `dispatch_click`→`UiAction::Invoke`→서버, AI도 동일 서버로 Invoke. 단일 명령 표면이 이미 구조적으로 존재.
- 한글 IME는 미배선(`vm_input.rs` US-QWERTY만) — **SP4, 이번 범위 밖.**

## 결정된 선택 (브레인스토밍 2026-05-29)

| 항목 | 선택 | 이유 |
|---|---|---|
| 구현 접근 | **접근 A — 객체 모델 데스크톱 환경 (재사용 중심)** | 크롬을 객체로 → AI-검사/조작 가능(원칙 일치), 기존 layout/render에 통합. 컴포지터 전용 크롬(접근 B)은 원칙 위배라 기각. 전면 WM 재작성(접근 C)은 YAGNI. |
| 데스크톱 구성 | **클린 바탕화면 + 앱=창** | 파일관리자를 창으로, 부팅 시 바탕화면+아이콘. 비전 일치. |
| 파일관리자 | **기존 FileTree+Explorer를 창으로 래핑** | 트리/탐색기 렌더·메서드 그대로 재사용 → 최소 churn + 명령 표면 자동 보장. |
| CLI | **하단 고정 + 상하 드래그 리사이즈** | 사용자 요구. 높이는 `Desktop.cli_height`. |
| 리사이즈/이동도 메서드 | **창 이동·리사이즈·CLI 높이 전부 Invoke 메서드** | 원칙 — AI도 동일하게 조작 가능. |

## 비-목표 (이번 범위 밖)

- **메모장 앱**(SP3), **한글 IME**(SP4).
- **macOS식 per-app 메뉴바**(File/Edit/View가 활성 앱 따라 바뀌는 풀 메뉴) — v1 네비바는 최소(GeulOS 메뉴 + 시계 + 표시기).
- 창 스냅/타일링, alt-tab 스위처, 워크스페이스, 멀티모니터.
- **바탕화면 이미지** — v1은 단색/그라데이션(`Desktop.wallpaper`=색). 이미지는 후속.
- AI가 자율적으로 데스크톱을 조작하는 *데모/에이전트* — 이번엔 "AI도 동일 Invoke 가능"한 구조만 보장(파리티 테스트로 입증), 자율 시나리오는 별도.

## 성공 기준

1. 부팅 시 **바탕화면 + 상단 네비바 + 우측 독 + 바탕화면 아이콘**이 뜬다(고정 3패널 아님).
2. 바탕화면 아이콘 또는 독을 클릭하면 **파일관리자 창**이 떠서 기존 트리/탐색기처럼 폴더 탐색·파일 열기가 된다(창 이동/리사이즈/닫기 동작).
3. **CLI 하단 패널을 상단 모서리로 드래그**해 높이를 조절할 수 있다.
4. **AI 패리티**: 서버로 `Desktop.launch("file_manager")`(또는 `Dock.launch`/`DesktopIcon.open`)를 Invoke하면 사용자가 클릭한 것과 **동일한 결과**(파일관리자 창 등장)가 난다. `Query`로 트리에서 실행 가능 항목·메서드를 발견할 수 있다.
5. 영속 디스크 루트에서 동작(기존 부팅 체인 회귀 없음).

**합격 판정**: (1)~(3)은 사용자 시각 확인(QEMU 창). (4)는 AI(또는 테스트 클라이언트)가 서버 Invoke로 동일 창 생성 확인.

## Architecture

### 명령 표면 불변식

SP1의 모든 affordance = **트리 객체의 메서드**. 컴포지터 클릭/드래그 → 해당 메서드 `Invoke`. AI는 객체 서버로 동일 Invoke/Query. **컴포지터 전용 동작 금지.**

### 새/확장 객체 타입 (`core/src/object/std_types.rs`)

| 객체 | state | methods (= 명령 표면) |
|---|---|---|
| `Desktop@1` (확장) | `wallpaper: String`(색 hex/그라데이션 토큰), `cli_height: i32`(px) | `launch(app: String)`, `set_wallpaper(v)`, `set_cli_height(px)` |
| `TopBar@1` (신규) | `items: [{id,label}]`, `clock: String`(컴포지터 자동갱신) | `activate(item_id)` |
| `Dock@1` (신규) | `items: [{app,label,icon}]` | `launch(item_id)` |
| `DesktopIcon@1` (신규, 다중) | `app: String`, `label`, `icon`, `x: i32`, `y: i32` | `open()` (=해당 app 실행) |
| `FileManager@1` (신규, 창) | `x/y/w/h/z/focused`(Window 동형), 자식=`[FileTree, Explorer]` | `move/resize/focus/close` |

- `launch(app)`/`open()`/`Dock.launch(id)`는 모두 desktop-shell의 **앱 레지스트리**(app_id → 창 구성 함수)로 수렴. 이미 열린 앱은 기존 창 focus(dedup).
- `TopBar.clock`은 컴포지터가 표시(자동). 메뉴 항목 동작만 `activate`로 invoke.

### 컴포넌트 변경 (파일 단위)

| 파일 | 변경 |
|---|---|
| `core/src/object/std_types.rs` | `TopBar@1`/`Dock@1`/`DesktopIcon@1`/`FileManager@1` 타입·factory 추가, `Desktop@1`에 `wallpaper`·`cli_height` state 추가. |
| `apps/desktop-shell/src/handlers/` | launch 핸들러(앱 레지스트리), TopBar/Dock/DesktopIcon/FileManager 메서드 dispatch. `set_cli_height`/`set_wallpaper`. |
| `apps/desktop-shell/src/main.rs` | 시작 시 mount를 **클린 데스크톱**으로: Desktop(wallpaper) + TopBar + Dock + DesktopIcon들 + Cli. FileTree/Explorer는 더 이상 고정 mount 아님 — 파일관리자 실행 시 창 자식으로 mount(M2). |
| `compositor/src/layout.rs` | `layout_desktop` 확장: 상단 TopBar 스트립 + 우측 Dock 스트립 + 하단 Cli(`cli_height`) + 가운데 바탕화면(아이콘 x/y 배치 + 떠있는 창). `FileManager` 창 본문에 FileTree(좌)/Explorer(우) 배치. 새 `HitRole`(DockItem, TopBarItem, CliResizeHandle, DesktopIcon). |
| `compositor/src/render.rs` | TopBar/Dock/DesktopIcon/FileManager 창틀 렌더 + 바탕화면 배경. FileManager 본문은 기존 FileTree/Explorer 렌더 위임. |
| `compositor/src/hit_test.rs` | 새 영역(독·네비바·아이콘·CLI 리사이즈 핸들) hit 매핑. |
| `compositor/src/bin/geulos-vm-compositor.rs` + `main.rs` | 새 HitRole→Invoke 매핑(open/launch/activate/set_cli_height). CLI 리사이즈 드래그 상태. |

## 데이터 흐름

```
[사용자] 아이콘 클릭 → hit_test(DesktopIcon) → Invoke(icon,"open")
[AI]    Invoke(Dock,"launch",{item:"file_manager"})  ─ 동일 경로 ─┐
                                                                  ▼
   desktop-shell launch 핸들러 → 앱 레지스트리(app_id→창 구성)
   → FileManager@1 + (FileTree, Explorer) mount(자식) → StateSet 브로드캐스트
   → 컴포지터가 새 창 렌더 (이동/리사이즈/닫기 = 기존 Window 메서드)

[CLI 리사이즈] 핸들 드래그 → Invoke(Desktop,"set_cli_height",{px}) → layout 반영
[AI 발견]     Query(tree) → Dock.items / DesktopIcon들 / 각 객체 methods → 동일 Invoke
```

## 에러 처리

- 이미 열린 앱 재실행 → 새 창 안 만들고 기존 창 `focus`(Window dedup 패턴 재사용).
- 알 수 없는 `app` id → no-op + 로그(크래시 금지).
- `set_cli_height` 범위 clamp(최소/최대 — 화면 높이 대비). 음수/과대 무시.
- 창 본문(FileTree/Explorer) 동작 실패는 기존 핸들러 에러 경로 유지.

## 테스트 / 검증

- **단위(호스트 `cargo test`)**: layout 영역 계산(주어진 화면크기→TopBar/Dock/Cli/중앙 rect, cli_height 반영), launch 디스패치(app_id→창 구성 선택), hit_test 새 role 매핑, set_cli_height clamp. 순수 함수로 분리해 테스트.
- **크로스컴파일**: musl 빌드 통과(컴포지터/desktop-shell/core).
- **부팅/시각(사용자)**: 성공 기준 (1)~(3) — 바탕화면/네비바/독/아이콘, 아이콘 클릭→파일관리자 창, CLI 리사이즈.
- **AI 패리티(자동화 가능)**: 테스트 클라이언트가 서버에 `Desktop.launch("file_manager")` Invoke → 동일 창(FileManager + FileTree/Explorer) 트리에 등장 확인. = 명령 표면 통일 입증.

## 위험

- `layout_desktop` 확장이 기존 패널 레이아웃을 대체 → 회귀 위험. 단계적(M1은 패널 유지)으로 완화.
- 객체 타입 다수 추가 → ACL/구독 등록 누락 시 mount/invoke 실패. 기존 ACL 헬퍼 패턴 따라야 함.
- FileManager 창 본문에 FileTree+Explorer를 *창 좌표 안*에 재배치 — 기존 렌더가 화면 절대좌표 가정일 수 있어 좌표 오프셋 보정 필요(렌더 함수에 origin 인자 추가 등).
- 다수 떠있는 창 + 새 크롬으로 hit_test z/모달 상호작용 복잡도 증가.

## 마일스톤 (한 스펙 내)

- **M1 — 크롬 + 실행 모델**: `Desktop.wallpaper` 배경, `TopBar`/`Dock`/`DesktopIcon` 타입·렌더·메서드, `Desktop.launch` 앱 레지스트리. 클릭/AI Invoke로 실행(앱 본체는 M2). 기존 FileTree/Explorer 패널은 잠시 유지(회귀 최소화).
- **M2 — FileManager 창**: FileTree+Explorer를 `FileManager@1` 창으로 래핑·실행, 고정 좌/우 패널 제거 → 부팅 시 클린 바탕화면. AI 패리티 테스트.
- **M3 — CLI 리사이즈**: `Desktop.set_cli_height` + 드래그 핸들 + layout 높이 반영.

## 후속 산출물 (이 스펙 이후)

- SP2(파일관리자 심화: 신규폴더/이름변경/삭제 버튼 등 — 전부 메서드), SP3(메모장 창), SP4(한글 IME).
- 바탕화면 이미지, per-app 메뉴바, 창 스냅 등은 별도.
