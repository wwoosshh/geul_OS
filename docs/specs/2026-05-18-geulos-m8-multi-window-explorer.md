# GeulOS M8 Spec — 전체 파일시스템 + 멀티-윈도우 탐색기

**Date:** 2026-05-18
**Status:** Design 승인됨, plan 작성 대기
**Author:** wwoosshh + Claude controller session
**Supersedes part of:** ADR-021 워크스페이스 단방향 (M8에서 워크스페이스 격리 *해제*, M9까지 read-only로 보호)
**Related:** M7 T7.5 완료 (하단 CLI 유지), M7 T7.6/T7.7 일시 보류

---

## 1. 한 줄 요약

워크스페이스 폴더 격리를 해제하고 Windows 전체 드라이브를 트리 root로 자동 mount, 우측을 파일 탐색기 스타일로 재정의하며, 파일 viewer를 *floating multi-window*로 띄운다. M8 단계에서는 **read-only** — 쓰기는 M9의 권한 다이얼로그 마일스톤과 함께 복귀.

---

## 2. Motivation (사용자 결정 — 2026-05-18)

T7.5 직후 사용자 발언:
> "내 글os는 ai가 직접 os를 조작할수 있다는점을 고려해서 ... 윈도우의 다른 파일에 접근을 할수가 없는상태인거야"
> "좌측패널에서는 폴더구조가 펼쳐지고 선택한 폴더의 내부파일및 내부폴더가 우측영역에 파일탐색기처럼 펼쳐지고 이중에서 파일을 선택하면 해당 파일을 창형태로 불러오는것, 폴더를 선택하면 해당 폴더로 더 들어가는것"

해석:
1. 워크스페이스 격리는 데모용으로 의도된 제약. *실제 OS 셸*로 가려면 전체 파일시스템 접근 필수.
2. 좌측 트리는 *폴더 구조*만 (File 노드 제거).
3. 우측은 *현재 폴더의 내용 listing* (Windows Explorer 패턴).
4. 파일 클릭 = *창*으로 등장. 여러 파일 동시 보기 가능 (사용자 명시).
5. AI도 같은 모델로 본다 — 즉 Window·Explorer는 *1급 객체*여야 AI에게 "어떤 윈도우가 열려있는지" 보임 (ADR-009 일관).

---

## 3. Scope

### In scope (M8)
- 시작 시 Windows 모든 드라이브 (`C:\`, `D:\`, ...) 자동 mount
- 폴더 expand 시 직계 자식 lazy mount (성능 — 전체 재귀 mount는 비현실)
- 좌측 FileTree: 폴더만 (File 노드 미표시)
- 우측 Explorer: 활성 폴더의 자식 (Folder + File) list view (이름 + 타입 아이콘)
- 클릭 시멘틱: 폴더 → navigate, 파일 → 새 Window
- Window: title bar + 본문 + `[x]` 닫기 + 드래그 이동 + 우하 코너 리사이즈 + focus / z-order
- 같은 파일 두 번 open → 기존 윈도우 focus + 최상위
- CLI 패널은 T7.5 그대로 유지 (하단 30%)
- **Read-only**: `aios.std/File@1` / `Folder@1`의 write 관련 메서드 *팩토리에서 제거*

### Out of scope (M9+로 명시 연기)
- 파일 쓰기/생성/삭제/이름변경 — M9 권한 다이얼로그 마일스톤
- 우측 view mode `grid` / `details` — v2
- 키보드 단축키 (Ctrl+W 윈도우 닫기, Alt+Tab 순환) — v2
- 다중 모니터 / DPI scaling
- 윈도우 maximize / minimize
- `read_dir` 권한 거부 시 사용자에게 에러 표시 (M8은 빈 폴더로 silent)
- 한글 IME (M7 T7.6에서 처리)
- AI chat session (M7 T7.7에서 처리)
- collapse 시 자식 unmount (TreeModel 메모리 정리) — v2

---

## 4. 객체 모델

### 4.1 신규: `aios.builtin/Window@1`

플로팅 파일 viewer.

**props:**
- `title: String` — 윈도우 상단 표시 (기본 = 파일명)
- `file_id: ObjectId` — 보여주는 File 객체 (단방향 참조)

**state:**
- `x: i32`, `y: i32` — 좌상단 좌표 (윈도우 영역 내)
- `w: i32`, `h: i32` — 크기 (default 600×400, min 200×120)
- `z: i32` — z-order (큰 값이 위)
- `focused: bool` — 키보드 입력 수신 여부 (M8은 사실상 read-only라 무용하지만 모델 일관성)

**methods:**
- `move(x: i32, y: i32)` — 위치 변경
- `resize(w: i32, h: i32)` — 크기 변경 (min clamp)
- `focus()` — z를 최상위로 + focused=true (다른 윈도우 focused=false)
- `close()` — `emit_destroyed` (KI-011 tombstone 메커니즘 재사용)

부모: `Desktop@1`. focus·move·resize는 컴포지터가 사용자 입력을 받아 invoke를 desktop-shell에 보냄. desktop-shell이 state 갱신 + StateSet broadcast.

### 4.2 신규: `aios.builtin/Explorer@1`

활성 폴더 내용 list view.

**props:** 없음.

**state:**
- `active_folder: Option<ObjectId>` — 현재 표시 중인 Folder. `null`이면 "내 PC" 같은 드라이브 일람 (FileTree root와 같은 children).
- `view_mode: String` — `"list"` 고정 (M8). 향후 `"grid"` / `"details"`.

**methods:**
- `navigate_to(folder_id: ObjectId)` — active_folder 변경. desktop-shell이 lazy expand 트리거.
- `open_file(file_id: ObjectId)` — 새 Window mount (이미 열려있으면 그것 focus).

부모: `Desktop@1`. 자식 없음 (active_folder의 children을 *간접 참조*로 렌더, 자체 tree 자식은 X).

### 4.3 변경: `aios.std/Folder@1` / `aios.std/File@1` — Read-only

**제거할 메서드 (팩토리에서 빼고, M9 trigger 주석):**
- `Folder.create_file(name)`
- `Folder.create_folder(name)`
- `Folder.delete()`
- `File.write(content)`
- `File.rename(new_name)`
- `File.delete()`

**유지:**
- `File.read()` (M8은 사실 wire 메서드보다 직접 props/state로 preview 노출 → read 호출 안 함; 메서드는 둠)

**구현 결정:** 메서드 시그니처를 *팩토리에서 누락*. 클라이언트가 invoke해도 `MethodNotFound`. ACL deny가 아닌 *존재 안 함*이라 더 깨끗 (KI-001 wildcard ACL과 직교).

기존 T7 `apps/desktop-shell/src/fs_ops.rs`의 `create_empty_file` / `atomic_write` / `delete_file` 함수는 *제거하지 않고* dead code로 둔다 (`#[allow(dead_code)]` + 주석 `// M9 권한 다이얼로그 마일스톤에서 재활성화`). 호출 분기는 `apps/desktop-shell/src/main.rs`에서 제거 (`create_file`/`write`/`delete` match arm 제거).

---

## 5. Desktop 구조 변경

```
Desktop@1 (root, win 전체)
├── FileTree@1   (좌 25% 폭, 폴더만 — File 노드 안 그림)
├── Explorer@1   (우 75% 폭, active_folder.children을 list로)
├── Cli@1        (하단 30% 높이, 기존 T7.5 유지)
└── Window@1 × N (자식 순서 무관 — 컴포지터가 z 기준으로 정렬해 오버레이)
```

레이아웃 비율 (가로):
- 상단 70% 높이 안에 좌(25%) + 우(75%)
- 하단 30% 높이 = CLI 풀폭
- Window는 z-order로 상단 영역 위에 떠있음 (CLI 위로는 가지 않음 — drag clamp)

좌측 트리가 25%로 좁아짐 (T7.5는 30%): Explorer가 정보량 더 많아 우측 비중 확대.

---

## 6. 드라이브 자동 mount + Lazy expand

### 6.1 시작 시 드라이브 열거

`apps/desktop-shell/src/drives.rs` (신규):
```rust
pub fn list_drives() -> Vec<PathBuf>
```
- Windows: `GetLogicalDrives` Win32 API (winapi crate 사용) 또는 `std::fs::read_dir` 루트별 시도. 첫 선택지 권장 (가벼움).
- 비-Windows: `vec!["/".into()]` fallback (테스트 가능성 위해).

각 드라이브를 `aios.std/Folder@1`로 mount:
- props: path=`"C:\\"`, name=`"C:\\"`
- children=[] (지연 mount), child_count=0 (expand 후 갱신)

FileTree.children = 모든 드라이브 Folder IDs.

### 6.2 Lazy expand

`apps/desktop-shell/src/lazy_mount.rs` (신규):
```rust
pub fn expand_folder(
    owner: &ActorId,
    folder_path: &Path,
) -> io::Result<Vec<Object>>
```
- `read_dir(folder_path)` → 직계 자식만.
- 각 entry: Folder면 `std_types::folder(..., children=[])`, File이면 `std_types::file(..., preview=텍스트 첫 512바이트)`.
- `safe_join` 등 검증 불필요 (read-only이므로).
- 권한 거부 시 `Err(io::ErrorKind::PermissionDenied)` → 빈 vec 반환 + `eprintln`.
- 심볼릭 링크: 따라가지 않음 (`std::fs::read_dir` default behavior).

호출 시점:
- FileTree에서 폴더 클릭 시 `expand` invoke → desktop-shell이 lazy_mount + 새 객체 mount + 부모 children 갱신 + StateSet broadcast.
- Explorer `navigate_to` 시 active_folder의 children이 비어있으면 동일 expand.

이미 expand된 폴더는 *재호출 안 함* (children이 비어있지 않으면 skip). collapse는 children 유지 (메모리 정리는 v2).

### 6.3 사이즈 한계

한 폴더 자식이 1만개 넘으면? — M8은 *clip 없이 전부 mount*. 성능 문제 시 v2에서 페이지네이션. 일반 사용자 폴더는 100~1000건 수준이라 OK.

---

## 7. UX 클릭 시멘틱

| 위치 | 입력 | 효과 |
|---|---|---|
| 좌 FileTree 폴더 항목 | 좌클릭 | FileTree.expand/collapse + Explorer.navigate_to(폴더) |
| 좌 FileTree 빈 영역 | 좌클릭 | 무시 |
| 우 Explorer 폴더 항목 | 좌클릭 | Explorer.navigate_to(폴더) — 그 안으로 진입. 좌 트리도 그 경로 expand 자동. |
| 우 Explorer 파일 항목 | 좌클릭 | Explorer.open_file(file) — 새 Window mount + focus. 이미 열려있으면 그 Window focus. |
| Window title bar | 좌클릭 + 드래그 | Window.move(x, y). drag end 시점에 한 번 invoke (drag 중에는 컴포지터 local position). |
| Window 우하 코너 (10×10) | 좌클릭 + 드래그 | Window.resize(w, h). drag end 시점에 한 번 invoke. |
| Window 본문 | 좌클릭 | Window.focus() (이미 focused면 no-op). |
| Window `[x]` 버튼 | 좌클릭 | Window.close() → emit_destroyed → 컴포지터 트리에서 제거 → 다른 윈도우 중 z 최대값이 focused. |
| CLI 패널 | 좌클릭 | CLI focus (키 입력 받음). 다른 윈도우는 focused=false. |
| 윈도우 외 데스크톱 | 좌클릭 | 모든 윈도우 focused=false. 다음 키 입력은 CLI 받음. |

**디자인 결정:**
- *Drag 중 컴포지터 local position* + *drag end에 invoke 한 번* — 매 mouse move마다 invoke 보내면 latency 큼 (T7.5의 키 입력과 같은 패턴). 컴포지터 local이 server보다 한 frame 앞설 수 있지만 무방.
- 같은 파일 중복 open 방지: Explorer.open_file 핸들러 (desktop-shell)가 기존 Window 검사 → 있으면 focus invoke로 우회.
- 첫 Window: cascade base = Explorer rect 중앙 — (Explorer.x + 100, Explorer.y + 80). 이후 윈도우: 마지막 윈도우 + (30, 30).

---

## 8. Read-only Enforcement

### 8.1 객체 단

- `std_types::file` / `folder` 팩토리에서 write·delete·create_file·create_folder·rename 메서드 시그니처 *제거*.
- `core/tests/std_types_test.rs` 라운드트립 테스트도 동기화 (메서드 수 검증).

### 8.2 desktop-shell

`apps/desktop-shell/src/main.rs` invoke match arm:
- `"create_file"` / `"write"` / `"delete"` arm *제거*.
- 대신 신규 arm: `"navigate_to"` / `"open_file"` / `"close"` / `"move"` / `"resize"` / `"focus"`.
- 기존 `"expand"` / `"collapse"` / `"select"` / `"set_file"` 유지 + `expand`에서 lazy_mount 트리거 추가.

### 8.3 컴포지터

`compositor/src/main.rs` `dispatch_click`:
- 파일 클릭 → 기존 `select` + `set_file` 제거, 대신 Explorer.open_file 호출.
- 폴더 클릭 (FileTree) → 기존 expand/collapse 유지 + Explorer.navigate_to 추가.

### 8.4 fs_ops.rs

함수 유지 + `#[allow(dead_code)]` + 모듈-레벨 주석:
```rust
//! 디스크 쓰기 함수들. M8에서 *호출 분기 제거* — read-only 마일스톤 동안 dead.
//! M9 권한 다이얼로그 마일스톤에서 재호출 예정.
```

테스트는 유지 (함수 자체가 회귀 없이 작동하는지 확인).

---

## 9. 입력 라우팅 / Focus

### Focus 상태

컴포지터 local + Window 객체 state:
- 컴포지터가 마우스 클릭으로 focus 대상 결정.
- 결정된 대상이 Window면 `focus(window_id)` invoke → desktop-shell이 모든 윈도우 focused=false → 그 윈도우 focused=true → StateSet broadcast.
- CLI focus는 컴포지터 local (Cli 객체 state에 focused 추가하지 않음 — T7.5 단순성 유지).

### 키보드 라우팅

컴포지터의 `App.keyboard_focus: Option<KeyboardFocus>`:
```rust
enum KeyboardFocus { Cli, Window(ObjectId), None }
```
- CLI 클릭 → `Cli`
- Window 본문 클릭 → `Window(id)` (M8은 read-only라 키 효과 없지만 모델 유지)
- 그 외 → 마지막 focus 유지. 시작 시 `Cli` default.

키 이벤트:
- `Cli`: 기존 T7.5 cli_handler + keyboard.rs 그대로.
- `Window(id)`: 일단 *무시* (read-only). 향후 Ctrl+W 닫기 등 단축키.

### Z-order

Window들만 z를 가짐 (다른 객체는 fixed 위치). 렌더 시:
1. Desktop 배경 → FileTree → Explorer → Cli (기존 layout 순)
2. Window들을 z 오름차순으로 그 위에 그림

z 갱신: focus 시 desktop-shell이 모든 윈도우의 z를 +1하거나, focused 윈도우만 max(z)+1로. 후자가 단순 — 채택.

---

## 10. 알려진 한계 / 후속 마일스톤

- **M9 — 권한 다이얼로그**: write/create/delete/rename 메서드 복귀. 사용자 동의 다이얼로그 (KI-001/KI-002 해소 트리거). spec 별도.
- **M10+ view modes**: Explorer grid/details. preview thumbnail.
- **단축키**: Ctrl+W (active window 닫기), Alt+Tab (window focus 순환), Ctrl+L (CLI focus).
- **Performance**: `read_dir` 1만+ 엔트리 페이지네이션. TreeModel collapse 시 자식 unmount.
- **OS 확장성**: Linux/macOS 드라이브 mount (Windows API 의존성 분리). M8은 Windows 우선.
- **권한 거부 UX**: 빈 폴더 대신 `[권한 없음]` 표시.
- **다중 모니터**: 윈도우가 메인 모니터만 가정.

---

## 11. ADR 시드

본 spec에서 파생되는 ADR (M8 시작 시 본문 작성):

- **ADR-026 — 멀티-윈도우 객체 모델.** Window@1을 1급 객체로 도입한 결정. 컴포지터-local 모델과의 trade-off, AI 가시성(ADR-009 일관) 이유. focus / z-order / lifecycle (open via Explorer, close via emit_destroyed) 정의.
- **ADR-027 — M8 Read-only.** 워크스페이스 격리 해제와 동시에 *쓰기 차단*하는 이유. ADR-009 "AI 기본 불신" 보호. M9 권한 다이얼로그가 도착할 때 복귀. fs_ops 코드는 dead로 유지.
- **ADR-028 — 드라이브 자동 mount + Lazy expand.** Windows 전체 드라이브 열거 정책. Lazy expand 결정 이유 (전체 재귀 mount 비현실성). 권한 거부 폴더 처리.

번호 예약: T7.6 한글 IME는 ADR-029, T7.7 AI chat session은 ADR-030으로 재배치 (cli-extension plan의 ADR-024/025 시드는 *번호 충돌 회피를 위해 재번호*). M8 plan 작성 시 cli-extension plan 헤더에 번호 변경 메모 추가.

---

## 12. Sub-task 분해 (12 task ≈ 4~6주)

본격 분해는 `writing-plans` 단계에서. 미리보기:

| Task | 주제 | 추정 |
|---|---|---|
| T8.0 | ADR-026/027/028 작성 | 1~2일 |
| T8.1 | core: Window@1 + Explorer@1 std_types 팩토리 + 라운드트립 | 1일 |
| T8.2 | core: Folder/File 메서드 축소 (read-only) + 기존 테스트 동기화 | 1일 |
| T8.3 | desktop-shell: drives.rs + lazy_mount.rs + 기본 mount 흐름 | 2~3일 |
| T8.4 | compositor: server_client STD_TYPES + layout 좌25/우75 변경 + FileTree 폴더 전용 렌더 | 1~2일 |
| T8.5 | desktop-shell: Explorer 객체 mount + navigate_to + lazy expand 트리거 | 2일 |
| T8.6 | compositor: Explorer list 렌더 + 클릭 dispatch (navigate / open_file) | 2일 |
| T8.7 | desktop-shell: open_file 핸들러 = Window mount + 중복 검출 | 2일 |
| T8.8 | compositor: Window 오버레이 렌더 (title bar + 본문 + [x] + 우하 코너) | 2~3일 |
| T8.9 | compositor: 마우스 입력 — focus + drag move + drag resize | 3일 |
| T8.10 | desktop-shell: move / resize / focus / close invoke 핸들러 + z-order 갱신 | 2일 |
| T8.11 | Acceptance — 도그푸딩: 호스트 컴포지터에서 D:\GeulOS 탐색, 여러 파일 동시 열기, AI 시연 | 2~3일 |
| T8.12 | Final review (T8.0~T8.11 일괄) | 2일 |

---

## 13. 자체 점검

### 스펙 커버리지 (사용자 요청 매핑)
- "윈도우 다른 파일에 접근" → §6 드라이브 자동 mount ✅
- "최상위 폴더부터 제대로 출력" → §4.3 + §6 드라이브 root, 절대 경로 표시 ✅
- "좌측은 폴더 구조" → §5 FileTree 폴더만 ✅
- "선택한 폴더 내부가 우측 탐색기처럼" → §4.2 Explorer + §7 navigate_to ✅
- "파일 선택 시 창" → §4.1 Window + §7 open_file ✅
- "폴더 선택 시 더 들어가는" → §7 Explorer navigate_to ✅

### Tradeoff 위험
| 위험 | 완충 |
|---|---|
| Window 멀티-인스턴스 = 컴포지터 입력 라우팅 복잡 | §9 단순 마지막-클릭 focus 모델 (M8). 단축키는 v2. |
| Lazy expand가 *재귀 부모 expand* (예: Explorer navigate D:\foo\bar\baz)에서 N step | M8은 navigate_to 시 그 폴더만 expand. 부모 트리는 사용자가 좌측에서 직접 클릭. v2에 자동 trail expand. |
| 권한 거부 폴더에서 sliently 빈 — 사용자 혼동 | §10 후속에 명시. v1은 trade-off로 수용. |
| AI가 전체 FS 읽음 = 민감 파일(`C:\Users\...\AppData`) 노출 가능 | M8은 *솔로 dogfooding* 가정. 사용자 본인 머신, 본인 AI. ADR-027 명시. M9에서 read도 권한 모델 진입. |
| 12 task / 6주는 큰 추정 — 한 sub-system 실패 시 마일스톤 통째로 미끄러짐 | T8.7~T8.10 (Window) 부분이 가장 위험. plan 단계에서 T8.7 직후 *중간 acceptance* (Window 1개 open + close만 동작) 체크포인트 추가. |

### 알려진 design 갭 (의도적)
- Window 본문 렌더 = preview 텍스트 첫 512바이트만 (T7 모델 재사용). *실제 파일 열기*는 read 호출 후 contents 표시인데, M8은 props.preview로 충분하다고 결정. M9에서 *full read + scroll* 지원.
- Explorer view_mode는 "list" 고정. 다른 mode는 객체 state로 받지만 컴포지터는 list만 그림.

---

## 14. 다음 단계

1. 본 spec을 사용자가 review 후 → `writing-plans` 스킬로 implementation plan 작성
2. Plan은 task별 *파일 변경 + 핵심 단계 + 디자인 결정*을 펼침
3. `subagent-driven-development` 스킬로 task별 implementer → spec/quality review 디스패치 (T7.5와 동일 흐름)
4. M8 끝 = M7 T7.6 재개

---

## 부록 A. 메모리 / 환경 변경

- 작업 디렉터리: `D:\GeulOS` (이전 메모리는 `F:\GeulOS`로 적힘 — M8 plan 시작 시 메모리 갱신)
- 사용자 시각 검증 통과 시점: T7.5 회귀 fix (commit `f0c58f6`) 직후
- T7.6 / T7.7은 *일시 보류*. cli-extension plan은 유지 (M8 끝 후 재개의 출발점).
