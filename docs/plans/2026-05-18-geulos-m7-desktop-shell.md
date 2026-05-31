> **Status:** completed (2026-05-18)
> **Note:** M7 데스크톱 셸 정식 마감 — FileTree + Canvas + AI 가시성(노란 점). 후속 M8에서 멀티 윈도우 + 전체 드라이브 mount로 확장.

# GeulOS M7 — 데스크톱 셸: 동적 파일 트리 + 캔버스 + AI 가시성

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> **NEVER push** — controller batches push at end of milestone.

**Goal:** GeulOS의 *바탕화면*을 데스크톱 셸로 구현 — 좌측에 워크스페이스 파일 트리(폴더·파일 1급 객체), 우측에 활성 파일 콘텐츠/앱 캔버스. AI가 파일을 만들거나 수정하면 **즉시 좌측 트리에 반영** + 노란 점 5초 페이드로 시각화. Windows 사용자 폴더(`%USERPROFILE%\GeulOS\workspace`)와 단방향 동기(객체→디스크)되어 Windows 탐색기로도 그대로 접근 가능.

**Why this milestone:** M6.5까지의 인프라(객체 모델, 와이어, 4층 통신, VM 부팅)는 *대화 상대가 없는 OS*였다. echo-app은 데모일 뿐. 사용자가 정의한 비전 — *"AI가 일하는 모습이 실시간으로 보이는 파일 시스템"* — 은 데스크톱 셸이 있어야 비로소 화면에 살아난다. 메모장(M8)·코드 뷰어 등 모든 후속 앱은 이 셸의 *Canvas 슬롯*에 mount되므로, 데스크톱 셸은 GeulOS의 시각적 ID + 모든 앱의 컨테이너.

**Scope choice — 호스트 모드 우선, 단방향, 단일 캔버스:**
- 호스트 모드 (M4 컴포지터, Windows 네이티브 창) 그대로 — VM GUI(Phase D)는 8~12주 별도 마일스톤. 같은 데스크톱 셸이 Phase D 도착 시 Linux 백엔드(virtio-gpu)로 자동 포팅.
- 단방향 동기화 (객체→디스크). 외부 변경 감지(FS watcher)는 M9+로 연기 — M7은 GeulOS·AI가 만든 변경만 디스크에 기록. 사용자가 Windows 탐색기에서 직접 만든 파일은 *재시작*하면 트리에 등장.
- 단일 캔버스(M7) → 부동 창(M8+ 사용자 도그푸딩 결과로 필요성 입증된 뒤).

**Architecture:**
```
사용자 키보드/마우스 ──┐
                       ▼
                  ┌────────────┐         ┌──────────────┐
                  │ compositor │ ──TCP──▶│ geulosd      │◀─────┐
                  │ (winit +   │◀── ──── │ (server-host)│      │
                  │  softbuffer│         │              │      │
                  │  + Desktop │         └─────┬────────┘      │
                  │  + 분할 레이아웃)               │               │
                  └────────────┘                │ ACL gate     │
                       ▲                        ▼              │
                       │ Mount/StateSet  ┌──────────────┐      │
                       │  이벤트         │ desktop-shell│      │
                       │                 │  (별 process)│      │
                       │                 │              │      │
                       │                 │ fs_ops ──────┼─▶ %USERPROFILE%\GeulOS\workspace\
                       │                 │              │
                       │                 │ scan (부팅)─┘
                       │                 └──────────────┘
                       │                        ▲
                       │                        │ invoke(file/folder)
                       │                 ┌──────┴───────┐
                       └──── 사용자/AI ──┤ ai-bridge or│
                                          │ compositor   │
                                          └──────────────┘
```

**Tech Stack:**
- 신규 객체 타입 (M7):
  - `aios.builtin/Desktop@1` — 루트 셸
  - `aios.builtin/FileTree@1` — 좌측 파일 트리 패널
  - `aios.builtin/Canvas@1` — 우측 콘텐츠/앱 패널
  - `aios.std/Folder@1` — 폴더 노드
  - `aios.std/File@1` — 파일 노드
- 신규 크레이트: `apps/desktop-shell/` (geulosd 부팅 시 자동 시작될 예정 — M7에선 별 프로세스 수동 실행, M7.5에서 자동 시작 옵션)
- 컴포지터 확장: Desktop 분할 레이아웃, FileTree/Canvas/Folder/File 렌더, 폴더 클릭/파일 클릭 hit-test
- 호스트 fs (`std::fs`) 평문 파일 읽기/쓰기
- AI 가시화: 각 File/Folder의 `last_change_actor` + `last_change_ms` state — 렌더가 시각 비교

**Selection criteria (M7 완료 조건):**
- `cargo build --workspace --all-targets` 그린, 경고 0
- `cargo test --workspace` 그린 (M0~M6.5 회귀 + 신규 desktop-shell 테스트)
- `cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` 그린
- 호스트 4-터미널 시연:
  1. `server-host` 시작
  2. `desktop-shell` 시작 → `%USERPROFILE%\GeulOS\workspace` 자동 생성 + 스캔 + 트리 mount
  3. `compositor` 시작 → 좌측에 파일 트리, 우측에 빈 캔버스 표시
  4. `ai-bridge`로 시나리오 실행 → AI가 워크스페이스에 `ai-hello.md` 생성 → 컴포지터 트리에 *즉시* 등장 + 파일명 우측에 노란 점 ● + 5초 후 페이드아웃
- 폴더 클릭으로 펼치기/접기 동작
- 파일 클릭 → 우측 캔버스에 텍스트 콘텐츠 미리보기 표시
- KI-001 (wildcard ACL) 제거 — echo-app 포함 grep 0건
- 매니페스트 `permissions.fs` path prefix 강제 (단순 검사) — 매니페스트에 선언하지 않은 경로 invoke는 서버가 거부
- Phase D 인터페이스 보존: desktop-shell의 모든 fs 호출은 `std::fs` 추상에 격리 — Linux 백엔드 포팅 시 같은 코드 그대로

**Scope estimate:** 설계 §9.2의 *4주+* 추정이 메모장(원래 M7)이었고, 본 재정의 M7(데스크톱 셸)은 **7주** 추정. T1·T2(이미 main에 들어간 Memo/TextArea/MemoList + notepad-app 크레이트)는 *보존*하고 M8 메모장에서 그대로 활용 — M7은 해당 코드 건드리지 않음.

---

## ADR 시드 (T1에서 본문 작성)

- **ADR-020 — 데스크톱 셸 아키텍처.** 데스크톱은 *별 프로세스* (desktop-shell). 컴포지터·서버와 *동등한 레벨* 앱이지만 *항상 켜진다*는 점에서 builtin. Desktop/FileTree/Canvas는 `aios.builtin/*` 네임스페이스 — std와 구분되는 *셸 전용* 타입.
- **ADR-021 — 단방향 동기화 + 워크스페이스 루트.** M7은 객체→디스크 단방향. 워크스페이스 루트는 `%USERPROFILE%\GeulOS\workspace` 고정(환경변수 `GEULOS_WORKSPACE`로 override). 양방향 watcher는 M9+. Windows 탐색기 변경은 재시작 시만 반영.
- **ADR-022 — AI 작업 시각화 v1.** `File`/`Folder` state에 `last_change_actor` + `last_change_ms`. 렌더가 `now - last_change_ms < 5000 && actor == "ai"`일 때 파일명 우측에 노란 점. 더 정교한 시각 언어(글로우·배지·세션 그룹 강조)는 향후 사용자 설정으로 — M7은 가장 단순한 1차 시제품.

---

## 파일 구조 (사전 매핑)

```
apps/desktop-shell/                          # 신규 크레이트
├── Cargo.toml
├── src/
│   ├── main.rs                              # 진입점, server-host 연결, 이벤트 루프
│   ├── lib.rs                               # Desktop 트리 빌더
│   ├── workspace.rs                         # 워크스페이스 루트 결정 + 생성
│   ├── scan.rs                              # 재귀 디렉터리 스캔 → Folder/File 객체
│   ├── fs_ops.rs                            # invoke 핸들러: 파일 쓰기·이름 변경·삭제
│   └── manifest.toml                        # 매니페스트
└── tests/
    ├── workspace_test.rs                    # 루트 결정·생성
    ├── scan_test.rs                         # tempdir 스캔 → 객체 트리
    └── fs_ops_test.rs                       # invoke 디스패치 + atomic write

core/src/object/std_types.rs                 # 수정: desktop, file_tree, canvas, folder, file 추가
core/tests/std_types_test.rs                 # 수정: 새 타입 라운드트립

compositor/src/
├── layout.rs                                # 수정: Desktop 좌/우 분할, FileTree 들여쓰기, Canvas 영역
├── render.rs                                # 수정: FileTree/Canvas/Folder/File 분기 + 노란 점
├── hit_test.rs                              # 수정: Folder 클릭(expand/collapse), File 클릭(set_file)
├── messages.rs                              # 수정 (필요 시): UiAction에 인자 전달 형태 확장
└── tree_model.rs                            # 수정 없음 (state 갱신은 기존 set_state로 충분)

core/src/server/invoke.rs                    # 수정: manifest.permissions.fs path prefix 강제

apps/echo-app/src/lib.rs                     # 수정: KI-001 wildcard ACL 제거
apps/echo-app/src/main.rs                    # 수정: 명시적 ACL 부여로 회귀 (m3_smoke 회귀 방지)

ai-bridge/scenarios/
└── 08_ai_creates_file.toml                  # 신규: AI가 워크스페이스에 파일 1개 생성

docs/
├── adr/020-desktop-shell.md                 # 신규
├── adr/021-workspace-unidirectional.md      # 신규
├── adr/022-ai-visualization-v1.md           # 신규
├── manual-tests/m7-acceptance.md            # 신규
└── plans/2026-05-18-geulos-m7-desktop-shell.md  # 이 문서

기존 m7-notepad.md는 *deprecate* 표시 (헤더에 한 줄 추가) — 본문 보존, M8 메모장 plan 작성 시 참조.
```

---

## Task T1 — ADR + 표준 타입 추가 (Desktop, FileTree, Canvas, Folder, File)

**Files:**
- Create: `docs/adr/020-desktop-shell.md`
- Create: `docs/adr/021-workspace-unidirectional.md`
- Create: `docs/adr/022-ai-visualization-v1.md`
- Modify: `core/src/object/std_types.rs`
- Modify: `core/tests/std_types_test.rs` (또는 신규)
- Modify: `core/src/lib.rs` (re-export 필요 시)

타입 정의 요약:

| TypeUri | 역할 | props | state | methods |
|---|---|---|---|---|
| `aios.builtin/Desktop@1` | 루트 셸 | — | — | (없음 — 컨테이너) |
| `aios.builtin/FileTree@1` | 좌측 패널 | `root_path: String` | `expanded: [ObjectId]`, `selected: Option<ObjectId>` | `expand(id)`, `collapse(id)`, `select(id)`, `refresh()` |
| `aios.builtin/Canvas@1` | 우측 패널 | — | `active_file: Option<ObjectId>`, `active_app: Option<ObjectId>` | `set_file(id)`, `clear_file()`, `set_app(id)`, `clear_app()` |
| `aios.std/Folder@1` | 폴더 노드 | `path: String`, `name: String` | `child_count: usize`, `last_change_ms: i64`, `last_change_actor: String` | `create_file(name: String)`, `create_folder(name: String)`, `delete()` |
| `aios.std/File@1` | 파일 노드 | `path: String`, `name: String`, `mime: String` | `size_bytes: u64`, `last_change_ms: i64`, `last_change_actor: String`, `preview: String` (앞 512바이트 텍스트 한정, 비텍스트면 `""`) | `read()`, `write(content: String)`, `rename(new_name: String)`, `delete()` |

**중요 — last_change_actor 값:** `"ai" | "user" | "system"` (string enum, 단순). 부팅 시 스캔으로 만든 객체는 `"system"`. 컴포지터 입력에서 invoke된 변경은 `"user"`. ai-bridge에서 invoke된 변경은 `"ai"` — actor token의 origin 필드로 구분.

- [ ] **Step 1: ADR-020 작성**

`docs/adr/020-desktop-shell.md`:
```markdown
# ADR-020 — 데스크톱 셸 아키텍처

**Status:** Accepted (2026-05-18)

## Context
M7에서 *바탕화면*이 필요. 옵션 검토:
- 컴포지터에 내장 → 컴포지터가 너무 큼, 셸 교체 불가
- builtin 라이브러리 → 단일 라이터 이벤트 루프 위배
- **별 프로세스 (desktop-shell)** → 컴포지터·서버와 동등

## Decision
desktop-shell은 별 프로세스. 부팅 시 server-host 다음으로 시작. Desktop/FileTree/Canvas는
`aios.builtin/*` 네임스페이스 — `aios.std/*`(앱이 자유로이 게시)와 구분.

## Consequences
- 단일 라이터 보존 (ADR-003)
- 셸 교체 가능 (다른 desktop-shell 구현 가능)
- 부팅 시 server-host → desktop-shell → compositor 순서 의존성 (M7.5에서 geulosd가 자동 supervise)
```

- [ ] **Step 2: ADR-021 작성**

`docs/adr/021-workspace-unidirectional.md`:
```markdown
# ADR-021 — 워크스페이스 단방향 동기화 (M7)

**Status:** Accepted (2026-05-18)

## Context
GeulOS 객체 ↔ 호스트 디스크 동기화 방향:
- 단방향(객체→디스크): 단순, GeulOS 안 변경만 디스크에 기록
- 양방향(+FS watcher): 강력, Windows 탐색기 변경도 GeulOS에 반영. 충돌 처리 필요.

## Decision
M7은 *단방향*. 워크스페이스 루트는 `%USERPROFILE%\GeulOS\workspace` (env `GEULOS_WORKSPACE`로
override). 외부 변경은 desktop-shell 재시작 또는 명시적 `FileTree.refresh()` 호출 시만 반영.
양방향은 M9+.

## Consequences
- M7 스코프 7주 유지
- 사용자가 Windows 탐색기로 워크스페이스를 *읽고 편집*하는 것은 가능하지만, 편집 결과가
  실시간으로 트리에 안 나타남 — refresh 또는 재시작 필요. 명시적 한계로 README/매뉴얼에 문서화.
```

- [ ] **Step 3: ADR-022 작성**

`docs/adr/022-ai-visualization-v1.md`:
```markdown
# ADR-022 — AI 작업 시각화 v1: 노란 점 + 5초 페이드

**Status:** Accepted (2026-05-18)

## Context
"AI가 일하는 모습이 실시간으로 보이는 파일 시스템" — 사용자 비전의 핵심. 시각 언어 선택지가
많음(글로우/배지/색상/세션 그룹 강조). 1차에는 *가장 단순한 것*, 향후 사용자 설정으로.

## Decision
v1: 각 File/Folder의 `state.last_change_actor` (`ai|user|system`) + `state.last_change_ms`
(Unix ms). 컴포지터 렌더가 매 프레임 `now - last_change_ms`를 계산:
- `< 5000ms && actor == "ai"`: 파일명 우측 8px에 8×8 노란 사각 점 ●
- 그 외: 점 없음
페이드는 시간 비교만으로 자동 (별도 타이머·이벤트 없음). 5초 후 다음 redraw에서 사라짐.

## Consequences
- 구현 비용 매우 낮음 (state 2개 + 렌더 분기 1개)
- 사용자가 자리 비웠을 때 변경 누락 인지 가능 → M8+에서 *누적 카운터* 추가 가능
- 향후 사용자 설정으로 시각 언어 교체 가능 (글로우/색상/배지 등) — ADR 따로 갱신
```

- [ ] **Step 4: core/src/object/std_types.rs에 5개 팩토리 추가**

`core/src/object/std_types.rs` 끝에 추가:
```rust
// ───────────────────────── M7: 데스크톱 셸 타입 ─────────────────────────

/// 데스크톱 루트 셸. 컴포지터가 좌/우 분할로 그림.
///
/// 자식: [FileTree, Canvas] 순서.
pub fn desktop(owner: ActorId) -> Object {
    Object::new(TypeUri::parse("aios.builtin/Desktop@1").expect("유효한 TypeUri"), owner)
}

/// 좌측 파일 트리 패널.
///
/// props:
/// - `root_path: String` — 워크스페이스 절대 경로 (표시·디버그용)
///
/// state:
/// - `expanded: [ObjectId]` — 펼쳐진 폴더 ID 목록
/// - `selected: Option<ObjectId>` — 현재 선택된 노드 (FileTree 안에서 강조)
///
/// 메서드: `expand(id)`, `collapse(id)`, `select(id)`, `refresh()` — 재스캔
pub fn file_tree(owner: ActorId, root_path: &str) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.builtin/FileTree@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("root_path", json!(root_path));
    obj.set_state("expanded", json!([] as [&str; 0]));
    obj.set_state("selected", json!(null));
    obj.methods.push(MethodSig::new("expand").with_arg(ArgSpec::new("id", "ObjectId")));
    obj.methods.push(MethodSig::new("collapse").with_arg(ArgSpec::new("id", "ObjectId")));
    obj.methods.push(MethodSig::new("select").with_arg(ArgSpec::new("id", "ObjectId")));
    obj.methods.push(MethodSig::new("refresh"));
    obj
}

/// 우측 캔버스 패널. 활성 파일 미리보기 또는 활성 앱 트리 슬롯.
///
/// state:
/// - `active_file: Option<ObjectId>` — 선택된 File 객체. 텍스트면 본문 미리보기 렌더.
/// - `active_app: Option<ObjectId>` — 캔버스에 mount된 앱의 루트 객체 (M8+ 메모장 등).
///   `active_app`이 Some이면 그쪽이 우선, `active_file`은 무시.
pub fn canvas(owner: ActorId) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.builtin/Canvas@1").expect("유효한 TypeUri"), owner);
    obj.set_state("active_file", json!(null));
    obj.set_state("active_app", json!(null));
    obj.methods.push(MethodSig::new("set_file").with_arg(ArgSpec::new("id", "ObjectId")));
    obj.methods.push(MethodSig::new("clear_file"));
    obj.methods.push(MethodSig::new("set_app").with_arg(ArgSpec::new("id", "ObjectId")));
    obj.methods.push(MethodSig::new("clear_app"));
    obj
}

/// 폴더 노드. children에 Folder/File 객체.
///
/// props:
/// - `path: String` — 절대 경로
/// - `name: String` — 폴더명 (path의 basename)
///
/// state:
/// - `child_count: usize` — 자식 수 (UI 빠른 표시)
/// - `last_change_ms: i64` — 마지막 변경 Unix ms (자식 추가/제거 포함)
/// - `last_change_actor: String` — "ai" | "user" | "system"
pub fn folder(owner: ActorId, path: &str, name: &str, created_ms: i64) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.std/Folder@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("path", json!(path));
    obj.set_prop("name", json!(name));
    obj.set_state("child_count", json!(0));
    obj.set_state("last_change_ms", json!(created_ms));
    obj.set_state("last_change_actor", json!("system"));
    obj.methods.push(MethodSig::new("create_file").with_arg(ArgSpec::new("name", "string")));
    obj.methods.push(MethodSig::new("create_folder").with_arg(ArgSpec::new("name", "string")));
    obj.methods.push(MethodSig::new("delete"));
    obj
}

/// 파일 노드.
///
/// props:
/// - `path: String` — 절대 경로
/// - `name: String` — 파일명
/// - `mime: String` — 추론된 MIME (확장자 기반, 예: "text/plain", "text/markdown",
///   "application/octet-stream")
///
/// state:
/// - `size_bytes: u64`
/// - `last_change_ms: i64`
/// - `last_change_actor: String`
/// - `preview: String` — 텍스트 파일에 한해 앞 512바이트, 그 외는 ""
pub fn file(owner: ActorId, path: &str, name: &str, mime: &str, created_ms: i64) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.std/File@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("path", json!(path));
    obj.set_prop("name", json!(name));
    obj.set_prop("mime", json!(mime));
    obj.set_state("size_bytes", json!(0u64));
    obj.set_state("last_change_ms", json!(created_ms));
    obj.set_state("last_change_actor", json!("system"));
    obj.set_state("preview", json!(""));
    obj.methods.push(MethodSig::new("read"));
    obj.methods.push(MethodSig::new("write").with_arg(ArgSpec::new("content", "string")));
    obj.methods.push(MethodSig::new("rename").with_arg(ArgSpec::new("new_name", "string")));
    obj.methods.push(MethodSig::new("delete"));
    obj
}
```

- [ ] **Step 5: 5개 타입의 라운드트립 테스트 추가**

`core/tests/std_types_test.rs`에 (파일이 없으면 신규):
```rust
use geulos_core::{std_types, ActorId};

#[test]
fn desktop_shell_types_roundtrip_through_serde() {
    let owner: ActorId = "user:test".parse().unwrap();
    let candidates = vec![
        std_types::desktop(owner.clone()),
        std_types::file_tree(owner.clone(), "/tmp/workspace"),
        std_types::canvas(owner.clone()),
        std_types::folder(owner.clone(), "/tmp/workspace/a", "a", 1_700_000_000_000),
        std_types::file(owner.clone(), "/tmp/workspace/a.txt", "a.txt", "text/plain", 1_700_000_000_000),
    ];
    for obj in candidates {
        let json = serde_json::to_string(&obj).expect("serialize");
        let back: geulos_core::Object = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.type_uri, obj.type_uri);
        assert_eq!(back.id, obj.id);
        assert_eq!(back.props, obj.props);
        assert_eq!(back.state, obj.state);
        assert_eq!(back.methods.len(), obj.methods.len());
    }
}

#[test]
fn file_state_includes_visualization_fields() {
    let owner: ActorId = "user:t".parse().unwrap();
    let f = std_types::file(owner, "/x/y.md", "y.md", "text/markdown", 1_700_000_000_000);
    assert!(f.state.contains_key("last_change_ms"));
    assert!(f.state.contains_key("last_change_actor"));
    assert_eq!(f.state.get("last_change_actor").unwrap(), &serde_json::json!("system"));
}
```

- [ ] **Step 6: 빌드 + 테스트**

Run: `cargo test -p geulos-core --test std_types_test`
Expected: PASS — 2 테스트 모두

Run: `cargo build --workspace --all-targets`
Expected: 그린, 경고 0

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 그린

- [ ] **Step 7: 커밋**

```powershell
git add docs/adr/020-desktop-shell.md docs/adr/021-workspace-unidirectional.md docs/adr/022-ai-visualization-v1.md core/src/object/std_types.rs core/tests/std_types_test.rs
git commit -m @'
feat(core)+docs: M7 T1 — Desktop/FileTree/Canvas/Folder/File 표준 타입 + ADR-020/021/022

데스크톱 셸 5개 표준 타입 추가. last_change_actor/ms로 AI 시각화 시드.
ADR-020(셸 아키텍처)/021(단방향)/022(시각화 v1) 작성.
'@
```

---

## Task T2 — desktop-shell 크레이트 스캐폴드 + 워크스페이스 루트

**Files:**
- Create: `apps/desktop-shell/Cargo.toml`
- Create: `apps/desktop-shell/src/main.rs`
- Create: `apps/desktop-shell/src/lib.rs`
- Create: `apps/desktop-shell/src/workspace.rs`
- Create: `apps/desktop-shell/src/manifest.toml`
- Create: `apps/desktop-shell/tests/workspace_test.rs`
- Modify: `Cargo.toml` (workspace members에 추가)

- [ ] **Step 1: workspace_test.rs 실패 테스트부터**

`apps/desktop-shell/tests/workspace_test.rs`:
```rust
use geulos_desktop_shell::workspace;
use std::env;

#[test]
fn resolve_uses_env_when_set() {
    let dir = tempfile::tempdir().expect("tempdir");
    let p = dir.path().to_string_lossy().to_string();
    unsafe { env::set_var("GEULOS_WORKSPACE", &p) };
    let resolved = workspace::resolve().expect("resolve");
    unsafe { env::remove_var("GEULOS_WORKSPACE") };
    assert_eq!(resolved, std::path::PathBuf::from(p));
}

#[test]
fn resolve_falls_back_to_userprofile_default() {
    unsafe { env::remove_var("GEULOS_WORKSPACE") };
    let resolved = workspace::resolve().expect("resolve");
    let s = resolved.to_string_lossy();
    // %USERPROFILE%\GeulOS\workspace 또는 $HOME/GeulOS/workspace
    assert!(s.contains("GeulOS"), "expected GeulOS in path, got {}", s);
    assert!(s.ends_with("workspace"), "expected ends with workspace, got {}", s);
}

#[test]
fn ensure_exists_creates_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("inner").join("workspace");
    workspace::ensure_exists(&target).expect("ensure");
    assert!(target.exists() && target.is_dir());
}
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `cargo test -p geulos-desktop-shell --test workspace_test`
Expected: FAIL with "crate not found" (T2 step 3 이전)

- [ ] **Step 3: apps/desktop-shell/Cargo.toml + 스켈레톤**

`apps/desktop-shell/Cargo.toml`:
```toml
[package]
name = "geulos-desktop-shell"
version = "0.0.1"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "GeulOS desktop shell — workspace 파일 트리 + 캔버스 (M7)"

[[bin]]
name = "geulos-desktop-shell"
path = "src/main.rs"

[lib]
name = "geulos_desktop_shell"
path = "src/lib.rs"

[dependencies]
geulos-core = { path = "../../core" }
geulos-proto = { path = "../../proto" }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = "1.0"
chrono = { workspace = true }

[dev-dependencies]
tempfile = "3.27"
```

`Cargo.toml` (workspace 루트) `members`에 `"apps/desktop-shell"` 추가.

`apps/desktop-shell/src/lib.rs`:
```rust
//! GeulOS 데스크톱 셸 라이브러리.
//!
//! M7: 워크스페이스 루트 스캔 → Desktop/FileTree/Canvas + Folder/File 트리 mount.
//! 단방향 동기화 — 객체 변경만 디스크에 기록 (FS watcher는 M9+).

pub mod workspace;
```

`apps/desktop-shell/src/workspace.rs`:
```rust
//! 워크스페이스 루트 결정 + 생성.

use std::path::{Path, PathBuf};

/// 워크스페이스 루트 경로 결정.
///
/// 우선순위:
/// 1. 환경변수 `GEULOS_WORKSPACE` — 명시적 override
/// 2. `%USERPROFILE%\GeulOS\workspace` (Windows) / `$HOME/GeulOS/workspace` (그 외)
///
/// 둘 다 못 찾으면 에러 — 호스트가 사용자 디렉터리 환경변수를 갖고 있지 않은
/// 극단적 환경이므로 명시적 실패가 적절.
pub fn resolve() -> Result<PathBuf, String> {
    if let Ok(s) = std::env::var("GEULOS_WORKSPACE") {
        if !s.is_empty() {
            return Ok(PathBuf::from(s));
        }
    }
    // Windows: USERPROFILE. Unix: HOME.
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| "USERPROFILE/HOME 환경변수 없음".to_string())?;
    Ok(PathBuf::from(home).join("GeulOS").join("workspace"))
}

/// 디렉터리가 없으면 *재귀적*으로 생성.
pub fn ensure_exists(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}
```

`apps/desktop-shell/src/main.rs` (T2에선 최소 스캐폴드):
```rust
//! desktop-shell 진입점 (T2: 스캐폴드 + 워크스페이스 확보까지).
//!
//! T3에서 server-host 연결 + 스캔 + mount 추가.

use geulos_desktop_shell::workspace;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace::resolve()?;
    workspace::ensure_exists(&root)?;
    println!("[desktop-shell] workspace root: {}", root.display());
    println!("[desktop-shell] T2 scaffold complete — T3에서 스캔·mount 추가");
    Ok(())
}
```

`apps/desktop-shell/src/manifest.toml`:
```toml
# GeulOS desktop-shell 매니페스트
#
# 셸이 *항상* 워크스페이스 루트 전체에 접근해야 함. 다른 경로는 X.

[app]
id = "aios.builtin/desktop-shell"
version = "0.1.0"
title = "데스크톱 셸"
description = "GeulOS 바탕화면 — 파일 트리 + 캔버스"

[permissions]
# 워크스페이스 전체 read-write — 환경변수로 결정되므로 path_env 사용
fs = [
    { path_env = "GEULOS_WORKSPACE", access = "read-write" },
]

publishes = [
    "aios.builtin/Desktop@1",
    "aios.builtin/FileTree@1",
    "aios.builtin/Canvas@1",
    "aios.std/Folder@1",
    "aios.std/File@1",
]
```

- [ ] **Step 4: 테스트 통과 + 빌드**

Run: `cargo test -p geulos-desktop-shell --test workspace_test`
Expected: PASS (3 tests)

Run: `cargo build --workspace --all-targets`
Expected: 그린, 경고 0

- [ ] **Step 5: 수동 실행 확인**

Run: `cargo run -p geulos-desktop-shell`
Expected: 출력에 `workspace root: C:\Users\<user>\GeulOS\workspace` (또는 $HOME 변형) + 폴더가 실제로 만들어졌음.

- [ ] **Step 6: 커밋**

```powershell
git add Cargo.toml apps/desktop-shell/
git commit -m @'
feat(desktop-shell): M7 T2 — 크레이트 스캐폴드 + 워크스페이스 루트 결정

GEULOS_WORKSPACE env override + %USERPROFILE%\GeulOS\workspace 기본값.
ensure_exists로 첫 실행 시 자동 생성. tempdir 단위 테스트 3건.
'@
```

---

## Task T3 — 워크스페이스 스캔 → Folder/File 객체 트리 mount

**Files:**
- Create: `apps/desktop-shell/src/scan.rs`
- Create: `apps/desktop-shell/tests/scan_test.rs`
- Modify: `apps/desktop-shell/src/lib.rs` (pub mod scan)
- Modify: `apps/desktop-shell/src/main.rs` (T3: 스캔 + mount 추가)

스캔 규칙:
- 재귀 (`walkdir` 없이 표준 `std::fs::read_dir`로 충분 — 외부 deps 줄임)
- 숨김 파일/폴더 제외 (`.`로 시작)
- `.git`, `node_modules`, `target` 디렉터리 제외 (성능 — workspace에 우연히 있을 수 있음)
- 파일은 mime 추론: `.txt`/`.md`/`.toml`/`.json`/`.rs`/`.py`/`.js`/`.html`/`.css`/`.yaml`/`.yml` → `text/<ext>` 또는 `text/plain`, 그 외 → `application/octet-stream`
- preview: 텍스트 파일에 한해 앞 512바이트 (UTF-8 경계 보정)

- [ ] **Step 1: scan_test.rs 실패 테스트**

`apps/desktop-shell/tests/scan_test.rs`:
```rust
use geulos_core::ActorId;
use geulos_desktop_shell::scan;
use std::fs;

#[test]
fn scan_empty_directory_returns_empty_children() {
    let dir = tempfile::tempdir().unwrap();
    let owner: ActorId = "system:shell".parse().unwrap();
    let result = scan::scan_tree(&owner, dir.path()).expect("scan");
    assert_eq!(result.objects.len(), 0);
}

#[test]
fn scan_flat_directory_returns_file_objects() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "hello").unwrap();
    fs::write(dir.path().join("b.md"), "# md").unwrap();
    let owner: ActorId = "system:shell".parse().unwrap();
    let result = scan::scan_tree(&owner, dir.path()).expect("scan");
    assert_eq!(result.objects.len(), 2);
    let mimes: Vec<&str> = result.objects.iter()
        .filter_map(|o| o.props.get("mime").and_then(|v| v.as_str()))
        .collect();
    assert!(mimes.contains(&"text/plain"));
    assert!(mimes.contains(&"text/markdown"));
}

#[test]
fn scan_nested_directory_returns_folder_and_files() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub").join("c.txt"), "nested").unwrap();
    let owner: ActorId = "system:shell".parse().unwrap();
    let result = scan::scan_tree(&owner, dir.path()).expect("scan");
    // Folder("sub") + File("c.txt") = 2
    assert_eq!(result.objects.len(), 2);
    let has_folder = result.objects.iter().any(|o| o.type_uri.as_str() == "aios.std/Folder@1");
    let has_file = result.objects.iter().any(|o| o.type_uri.as_str() == "aios.std/File@1");
    assert!(has_folder && has_file);
}

#[test]
fn scan_skips_hidden_and_noisy_dirs() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join(".git").join("HEAD"), "x").unwrap();
    fs::create_dir(dir.path().join("node_modules")).unwrap();
    fs::create_dir(dir.path().join("target")).unwrap();
    fs::write(dir.path().join(".hidden"), "x").unwrap();
    fs::write(dir.path().join("visible.txt"), "x").unwrap();
    let owner: ActorId = "system:shell".parse().unwrap();
    let result = scan::scan_tree(&owner, dir.path()).expect("scan");
    assert_eq!(result.objects.len(), 1, "only visible.txt should remain");
}

#[test]
fn scan_attaches_preview_to_text_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("note.md"), "hello world").unwrap();
    let owner: ActorId = "system:shell".parse().unwrap();
    let result = scan::scan_tree(&owner, dir.path()).expect("scan");
    let file = result.objects.iter().find(|o| o.type_uri.as_str() == "aios.std/File@1").unwrap();
    assert_eq!(file.state.get("preview").and_then(|v| v.as_str()), Some("hello world"));
}
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test -p geulos-desktop-shell --test scan_test`
Expected: FAIL (`scan` module not found)

- [ ] **Step 3: scan.rs 구현**

`apps/desktop-shell/src/scan.rs`:
```rust
//! 워크스페이스 디렉터리 → Folder/File 객체 트리 스캔.

use std::path::Path;

use geulos_core::{std_types, ActorId, Object};

const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", ".vs", ".idea"];
const TEXT_EXTS: &[(&str, &str)] = &[
    ("txt", "text/plain"),
    ("md", "text/markdown"),
    ("toml", "text/plain"),
    ("json", "text/json"),
    ("rs", "text/rust"),
    ("py", "text/python"),
    ("js", "text/javascript"),
    ("html", "text/html"),
    ("css", "text/css"),
    ("yaml", "text/yaml"),
    ("yml", "text/yaml"),
];

pub struct ScanResult {
    /// 모든 발견된 객체. Folder가 자식 ObjectId들을 children에 담음.
    pub objects: Vec<Object>,
}

pub fn scan_tree(owner: &ActorId, root: &Path) -> std::io::Result<ScanResult> {
    let mut out = Vec::new();
    walk(owner, root, &mut out)?;
    Ok(ScanResult { objects: out })
}

fn walk(owner: &ActorId, dir: &Path, out: &mut Vec<Object>) -> std::io::Result<Vec<geulos_core::ObjectId>> {
    let mut child_ids = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return Ok(child_ids),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            let created_ms = chrono::Utc::now().timestamp_millis();
            let mut folder = std_types::folder(
                owner.clone(),
                path.to_string_lossy().as_ref(),
                &name,
                created_ms,
            );
            let nested = walk(owner, &path, out)?;
            folder.state.insert("child_count".into(), serde_json::json!(nested.len()));
            for id in &nested {
                if let Some(child) = out.iter_mut().find(|o| o.id == *id) {
                    child.parent = Some(folder.id);
                }
            }
            folder.children = nested;
            child_ids.push(folder.id);
            out.push(folder);
        } else if ft.is_file() {
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
            let mime = TEXT_EXTS.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
                .unwrap_or("application/octet-stream");
            let created_ms = chrono::Utc::now().timestamp_millis();
            let mut file = std_types::file(
                owner.clone(),
                path.to_string_lossy().as_ref(),
                &name,
                mime,
                created_ms,
            );
            let meta = std::fs::metadata(&path).ok();
            if let Some(m) = meta {
                file.state.insert("size_bytes".into(), serde_json::json!(m.len()));
            }
            if mime.starts_with("text/") {
                if let Ok(bytes) = std::fs::read(&path) {
                    let cap = bytes.len().min(512);
                    let safe = utf8_safe_slice(&bytes, cap);
                    if let Ok(s) = std::str::from_utf8(safe) {
                        file.state.insert("preview".into(), serde_json::json!(s));
                    }
                }
            }
            child_ids.push(file.id);
            out.push(file);
        }
    }
    Ok(child_ids)
}

/// UTF-8 경계를 침범하지 않는 가장 긴 prefix를 반환.
fn utf8_safe_slice(bytes: &[u8], max: usize) -> &[u8] {
    let mut end = max.min(bytes.len());
    while end > 0 && (bytes[end - 1] & 0b1100_0000) == 0b1000_0000 {
        end -= 1;
    }
    // 마지막 multi-byte 시작 byte도 잘리지 않게
    if end > 0 && bytes[end - 1] >= 0b1100_0000 {
        end -= 1;
    }
    &bytes[..end]
}
```

`apps/desktop-shell/src/lib.rs`에 `pub mod scan;` 추가.

- [ ] **Step 4: 테스트 통과**

Run: `cargo test -p geulos-desktop-shell --test scan_test`
Expected: PASS (5 tests)

- [ ] **Step 5: main.rs에 server-host 연결 + Desktop 트리 mount**

`apps/desktop-shell/src/main.rs` 교체:
```rust
//! desktop-shell 진입점 — server-host 연결 + 워크스페이스 스캔 + Desktop 트리 mount.

use geulos_core::{std_types, ActorId, Object};
use geulos_desktop_shell::{scan, workspace};
use geulos_proto::{decode_frame, encode_frame, Hello, HelloAck, MountAck, MountMsg, Role};
use serde_json::json;
use std::str::FromStr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SERVER_ADDR: &str = "127.0.0.1:5550";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace::resolve()?;
    workspace::ensure_exists(&root)?;
    println!("[desktop-shell] workspace root: {}", root.display());

    let addr = std::env::args().nth(1).unwrap_or_else(|| SERVER_ADDR.to_string());
    println!("[desktop-shell] connecting to {}...", addr);
    let mut stream = TcpStream::connect(&addr).await?;

    let manifest = json!({
        "manifest": {
            "id": "desktop-shell",
            "permissions": [],
            "ui_types": [
                "aios.builtin/Desktop@1",
                "aios.builtin/FileTree@1",
                "aios.builtin/Canvas@1",
                "aios.std/Folder@1",
                "aios.std/File@1",
            ]
        }
    });
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::App,
        auth: manifest,
        client_id: "desktop-shell".to_string(),
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&hello)?)).await?;

    let mut buf = vec![0u8; 16384];
    let mut accum: Vec<u8> = Vec::new();
    let actor_str = loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 { return Err("closed before HelloAck".into()); }
        accum.extend_from_slice(&buf[..n]);
        let mut slice = accum.as_slice();
        if let Ok(body) = decode_frame(&mut slice) {
            let consumed = accum.len() - slice.len();
            accum.drain(..consumed);
            let ack: HelloAck = serde_json::from_slice(&body)?;
            println!("[desktop-shell] HelloAck: actor={}", ack.actor_id);
            break ack.actor_id;
        }
    };
    let owner = ActorId::from_str(&actor_str)?;

    // Desktop + FileTree + Canvas 루트 구성
    let mut desktop = std_types::desktop(owner.clone());
    let mut file_tree = std_types::file_tree(owner.clone(), &root.to_string_lossy());
    let mut canvas = std_types::canvas(owner.clone());
    file_tree.parent = Some(desktop.id);
    canvas.parent = Some(desktop.id);
    desktop.children = vec![file_tree.id, canvas.id];

    // 워크스페이스 스캔 → 자식 Folder/File들 FileTree 아래로
    let scan_result = scan::scan_tree(&owner, &root)?;
    let mut all_objects: Vec<Object> = vec![desktop.clone(), file_tree.clone(), canvas.clone()];
    let mut top_level_ids = Vec::new();
    // scan_result.objects는 walk가 후위로 push 했으므로 자식 → 부모 순. parent가 없는 것이 top-level.
    // top-level만 file_tree의 자식으로 직접 부착, 나머지는 parent 그대로.
    for mut obj in scan_result.objects {
        if obj.parent.is_none() {
            obj.parent = Some(file_tree.id);
            top_level_ids.push(obj.id);
        }
        all_objects.push(obj);
    }
    // file_tree.children 갱신은 mount 전 마지막에 — all_objects의 file_tree 항목을 직접 수정.
    if let Some(ft) = all_objects.iter_mut().find(|o| o.id == file_tree.id) {
        ft.children = top_level_ids;
    }

    // Mount 순서: 부모가 먼저 와야 컴포지터 구독자가 트리를 구성하기 쉬움.
    // all_objects는 [desktop, file_tree, canvas, ...scan...] 순. scan은 자식→부모 순이라
    // mount 순서 보장 위해 *역순*으로 보내야 부모가 먼저 → 단순화 위해 그냥 일괄 mount,
    // compositor TreeModel은 upsert 순서에 의존 안 함.
    for obj in &all_objects {
        let msg = MountMsg {
            root_object_id: obj.id.to_string(),
            tree: serde_json::to_value(obj)?,
        };
        stream.write_all(&encode_frame(&serde_json::to_vec(&msg)?)).await?;
        // ack 대기
        loop {
            let n = stream.read(&mut buf).await?;
            if n == 0 { return Err("closed during mount".into()); }
            accum.extend_from_slice(&buf[..n]);
            let mut slice = accum.as_slice();
            if let Ok(b) = decode_frame(&mut slice) {
                let consumed = accum.len() - slice.len();
                accum.drain(..consumed);
                let _: MountAck = serde_json::from_slice(&b)?;
                break;
            }
        }
    }
    println!("[desktop-shell] mounted {} objects", all_objects.len());

    // idle 유지 (T7에서 invoke 핸들러로 교체)
    loop {
        let n = match stream.read(&mut buf).await {
            Ok(n) => n,
            Err(e) => { eprintln!("[desktop-shell] read error: {}", e); break; }
        };
        if n == 0 { break; }
        accum.extend_from_slice(&buf[..n]);
        loop {
            let mut slice = accum.as_slice();
            if decode_frame(&mut slice).is_err() { break; }
            let consumed = accum.len() - slice.len();
            accum.drain(..consumed);
        }
    }
    println!("[desktop-shell] exit");
    Ok(())
}
```

- [ ] **Step 6: 빌드 + lint**

Run: `cargo build --workspace --all-targets`
Expected: 그린

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 그린

- [ ] **Step 7: 수동 4-터미널 시연 검증**

터미널 1: `cargo run -p geulos-server-host`
터미널 2: `cargo run -p geulos-desktop-shell`

Expected: 터미널 2 출력에 `mounted N objects` (N = 3 + 스캔된 자식 수). 워크스페이스가 비어있으면 N=3.

테스트 직전에 워크스페이스에 파일을 몇 개 두고 확인:
```powershell
New-Item -ItemType File "$env:USERPROFILE\GeulOS\workspace\hello.md" -Force
Set-Content -Path "$env:USERPROFILE\GeulOS\workspace\hello.md" -Value "hi" -Encoding utf8
```
재시작 후 `mounted 4 objects` (Desktop+FileTree+Canvas+File).

- [ ] **Step 8: 커밋**

```powershell
git add apps/desktop-shell/
git commit -m @'
feat(desktop-shell): M7 T3 — 워크스페이스 스캔 + Desktop/FileTree/Canvas mount

재귀 스캔, 숨김/.git/node_modules/target 제외. mime 확장자 추론.
텍스트 파일 앞 512바이트 preview. server-host에 일괄 mount.
'@
```

---

## Task T4 — 컴포지터 Desktop 좌/우 분할 레이아웃

**Files:**
- Modify: `compositor/src/layout.rs`
- Modify: `compositor/src/layout_test.rs` (없으면 신규)

레이아웃 규칙:
- `aios.builtin/Desktop@1`: 자식이 정확히 2개 [FileTree, Canvas]. 좌측 30% 폭, 우측 70% 폭, 풀 높이.
- `aios.builtin/FileTree@1`: 자식들을 세로 stack, 들여쓰기 없음 (top-level), 자식 폴더가 펼쳐져 있으면 (FileTree.state.expanded에 ID 포함) 그 폴더 자식들을 들여쓰기 16px로 재귀.
- `aios.builtin/Canvas@1`: 자식이 있으면(active_app) 풀 영역. 없으면 빈 영역에 active_file preview를 직접 그림 (자식 레이아웃 없음).
- `aios.std/Folder@1`: 한 줄 (높이 28px). 자식 폴더/파일은 부모 FileTree가 들여쓰기로 처리.
- `aios.std/File@1`: 한 줄 (높이 24px).

- [ ] **Step 1: layout_test.rs 실패 테스트**

`compositor/tests/layout_test.rs`:
```rust
use geulos_compositor::layout::layout;
use geulos_compositor::tree_model::TreeModel;
use geulos_core::{std_types, ActorId};

#[test]
fn desktop_splits_left_thirty_right_seventy() {
    let owner: ActorId = "u".parse().unwrap();
    let mut desktop = std_types::desktop(owner.clone());
    let mut ft = std_types::file_tree(owner.clone(), "/tmp");
    let mut cv = std_types::canvas(owner.clone());
    ft.parent = Some(desktop.id);
    cv.parent = Some(desktop.id);
    desktop.children = vec![ft.id, cv.id];
    let (ft_id, cv_id) = (ft.id, cv.id);
    let mut tm = TreeModel::new();
    tm.upsert(desktop);
    tm.upsert(ft);
    tm.upsert(cv);
    let lay = layout(&tm, 1000, 600);
    let ft_rect = lay.get(ft_id).expect("ft rect");
    let cv_rect = lay.get(cv_id).expect("cv rect");
    assert_eq!(ft_rect.x, 0);
    assert_eq!(ft_rect.w, 300);
    assert_eq!(cv_rect.x, 300);
    assert_eq!(cv_rect.w, 700);
    assert_eq!(ft_rect.h, 600);
    assert_eq!(cv_rect.h, 600);
}

#[test]
fn file_tree_lists_top_level_children_vertically() {
    let owner: ActorId = "u".parse().unwrap();
    let mut desktop = std_types::desktop(owner.clone());
    let mut ft = std_types::file_tree(owner.clone(), "/tmp");
    let cv = std_types::canvas(owner.clone());
    let mut f1 = std_types::folder(owner.clone(), "/tmp/a", "a", 0);
    let mut f2 = std_types::file(owner.clone(), "/tmp/b.txt", "b.txt", "text/plain", 0);
    ft.parent = Some(desktop.id);
    f1.parent = Some(ft.id);
    f2.parent = Some(ft.id);
    ft.children = vec![f1.id, f2.id];
    desktop.children = vec![ft.id, cv.id];
    let (f1_id, f2_id) = (f1.id, f2.id);
    let mut tm = TreeModel::new();
    tm.upsert(desktop);
    tm.upsert(ft);
    tm.upsert(cv);
    tm.upsert(f1);
    tm.upsert(f2);
    let lay = layout(&tm, 1000, 600);
    let r1 = lay.get(f1_id).expect("f1");
    let r2 = lay.get(f2_id).expect("f2");
    // 둘 다 좌측 패널 안. x = FileTree x + 들여쓰기, y는 r2 > r1.
    assert!(r1.x >= 0 && r1.x < 300);
    assert!(r2.y > r1.y);
}

#[test]
fn expanded_folder_shows_children_indented() {
    let owner: ActorId = "u".parse().unwrap();
    let mut desktop = std_types::desktop(owner.clone());
    let mut ft = std_types::file_tree(owner.clone(), "/tmp");
    let cv = std_types::canvas(owner.clone());
    let mut f1 = std_types::folder(owner.clone(), "/tmp/a", "a", 0);
    let mut nested = std_types::file(owner.clone(), "/tmp/a/n.txt", "n.txt", "text/plain", 0);
    nested.parent = Some(f1.id);
    f1.children = vec![nested.id];
    f1.parent = Some(ft.id);
    ft.children = vec![f1.id];
    desktop.children = vec![ft.id, cv.id];
    // expanded에 f1.id 추가
    ft.state.insert("expanded".into(), serde_json::json!([f1.id.to_string()]));
    let (f1_id, n_id) = (f1.id, nested.id);
    let mut tm = TreeModel::new();
    tm.upsert(desktop);
    tm.upsert(ft);
    tm.upsert(cv);
    tm.upsert(f1);
    tm.upsert(nested);
    let lay = layout(&tm, 1000, 600);
    let f1_rect = lay.get(f1_id).expect("f1");
    let n_rect = lay.get(n_id).expect("n");
    assert!(n_rect.x > f1_rect.x, "nested should be indented");
    assert!(n_rect.y > f1_rect.y);
}
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test -p geulos-compositor --test layout_test`
Expected: FAIL — 새 타입 처리 없음

- [ ] **Step 3: layout.rs 확장**

`compositor/src/layout.rs`의 `item_height`, `layout_object`, `layout` 수정:
```rust
fn item_height(type_uri: &TypeUri) -> i32 {
    match type_uri.as_str() {
        "aios.std/Text@1" => 40,
        "aios.std/Button@1" => 60,
        "aios.std/Toggle@1" => 40,
        "aios.std/Folder@1" => 28,
        "aios.std/File@1" => 24,
        _ => 0,
    }
}

const INDENT: i32 = 16;

/// FileTree 전용: 자식 폴더가 expanded면 자식의 자식도 들여쓰기로 재귀 렌더.
fn layout_tree_node(
    tree: &TreeModel,
    expanded: &[ObjectId],
    id: ObjectId,
    x: i32,
    y: i32,
    avail_w: i32,
    out: &mut Vec<(ObjectId, Rect)>,
) -> i32 {
    let obj = match tree.get(id) {
        Some(o) => o,
        None => return 0,
    };
    let mut cur_y = y;
    let h = item_height(&obj.type_uri);
    out.push((id, Rect { x, y: cur_y, w: avail_w, h }));
    cur_y += h;
    let is_folder = obj.type_uri.as_str() == "aios.std/Folder@1";
    if is_folder && expanded.contains(&id) {
        for &child_id in &obj.children {
            cur_y += layout_tree_node(tree, expanded, child_id, x + INDENT, cur_y, avail_w - INDENT, out);
        }
    }
    cur_y - y
}

fn layout_desktop(
    tree: &TreeModel,
    id: ObjectId,
    win_w: i32,
    win_h: i32,
    out: &mut Vec<(ObjectId, Rect)>,
) {
    let obj = match tree.get(id) { Some(o) => o, None => return };
    out.push((id, Rect { x: 0, y: 0, w: win_w, h: win_h }));
    let left_w = (win_w as f32 * 0.30) as i32;
    let right_w = win_w - left_w;
    // 자식 순서 [FileTree, Canvas] 가정
    if let Some(&ft_id) = obj.children.first() {
        out.push((ft_id, Rect { x: 0, y: 0, w: left_w, h: win_h }));
        // FileTree 내부의 폴더/파일 들여쓰기 레이아웃
        let expanded = extract_expanded(tree, ft_id);
        if let Some(ft) = tree.get(ft_id) {
            let mut y = 4i32;
            for &cid in &ft.children {
                y += layout_tree_node(tree, &expanded, cid, 4, y, left_w - 8, out);
            }
        }
    }
    if let Some(&cv_id) = obj.children.get(1) {
        out.push((cv_id, Rect { x: left_w, y: 0, w: right_w, h: win_h }));
        // Canvas active_app 트리가 있으면 그 트리 레이아웃(기존 layout_object 사용)
        if let Some(cv) = tree.get(cv_id) {
            if let Some(active_app) = cv.state.get("active_app").and_then(|v| v.as_str()) {
                if let Ok(uuid) = uuid::Uuid::parse_str(active_app) {
                    let app_id = ObjectId::from_uuid(uuid);
                    layout_object(tree, app_id, left_w, 0, right_w, out);
                }
            }
        }
    }
}

fn extract_expanded(tree: &TreeModel, ft_id: ObjectId) -> Vec<ObjectId> {
    let ft = match tree.get(ft_id) { Some(o) => o, None => return vec![] };
    let arr = match ft.state.get("expanded").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return vec![],
    };
    arr.iter()
        .filter_map(|v| v.as_str())
        .filter_map(|s| uuid::Uuid::parse_str(s).ok())
        .map(ObjectId::from_uuid)
        .collect()
}

pub fn layout(tree: &TreeModel, win_w: i32, win_h: i32) -> LayoutResult {
    let mut out = Vec::new();
    for &root in tree.roots() {
        let obj = match tree.get(root) { Some(o) => o, None => continue };
        if obj.type_uri.as_str() == "aios.builtin/Desktop@1" {
            layout_desktop(tree, root, win_w, win_h, &mut out);
            return LayoutResult { rects: out };
        }
    }
    // Desktop 루트가 없으면 기존 동작 (echo-app 호환)
    let mut y = 0i32;
    for &root in tree.roots() {
        let used = layout_object(tree, root, 0, y, win_w, &mut out);
        y += used;
        if y >= win_h { break; }
    }
    LayoutResult { rects: out }
}
```

**중요 추가:** `ObjectId::from_uuid(uuid)` 메서드 필요. `core/src/object/identity.rs`에 추가:
```rust
/// Uuid → ObjectId 변환.
pub fn from_uuid(uuid: uuid::Uuid) -> Self {
    Self(uuid)
}
```
또한 `compositor/Cargo.toml`에 `uuid` 추가 필요 — 이미 transitive로 있을 가능성 높음. 없으면:
```toml
uuid = { workspace = true }
```

- [ ] **Step 4: 빌드 + 테스트**

Run: `cargo test -p geulos-compositor --test layout_test`
Expected: PASS (3 tests)

Run: `cargo test --workspace`
Expected: 모든 테스트 그린 (M0~M6.5 회귀 + 신규)

- [ ] **Step 5: 커밋**

```powershell
git add compositor/src/layout.rs compositor/tests/layout_test.rs core/src/object/identity.rs compositor/Cargo.toml
git commit -m @'
feat(compositor)+core: M7 T4 — Desktop 좌/우 분할 + FileTree 들여쓰기

Desktop(30%/70% split) + FileTree(expanded 폴더 재귀 들여쓰기) +
Canvas(active_app slot) 레이아웃. ObjectId::from_uuid 추가.
'@
```

---

## Task T5 — 컴포지터 렌더 (FileTree/Canvas/Folder/File + AI 노란 점)

**Files:**
- Modify: `compositor/src/render.rs`

렌더 규칙:
- FileTree 배경: `#F0F0F0` (밝은 회색)
- Canvas 배경: `#FFFFFF` (흰색)
- Folder: 텍스트 `[+] <name>` (collapsed) 또는 `[-] <name>` (expanded), 진한 회색
- File: 텍스트 `  <name>` (선두 공백), 더 밝은 회색
- 선택된 노드(FileTree.state.selected): 배경 강조 `#D0E4FF`
- Canvas active_file: 우측 상단에 파일명, 그 아래 preview 텍스트 (mono font 흉내, 그냥 draw_text 좌측 정렬)
- **AI 노란 점**: File/Folder 객체의 `state.last_change_actor == "ai"` && `now_ms - state.last_change_ms < 5000` 일 때, 이름 우측 8px에 8×8 노란 사각형 (`#FFD500`) — fill_rect

- [ ] **Step 1: render.rs 수정**

`compositor/src/render.rs`에 새 분기 추가. 핵심 부분:
```rust
const COLOR_TREE_BG: u32 = 0xFF_F0_F0_F0;
const COLOR_CANVAS_BG: u32 = 0xFF_FF_FF_FF;
const COLOR_FOLDER_TEXT: u32 = 0xFF_22_22_22;
const COLOR_FILE_TEXT: u32 = 0xFF_44_44_44;
const COLOR_SELECTED_BG: u32 = 0xFF_D0_E4_FF;
const COLOR_AI_DOT: u32 = 0xFF_FF_D5_00;
const AI_HIGHLIGHT_MS: i64 = 5000;

pub fn render_frame(
    tree: &TreeModel,
    layout: &LayoutResult,
    buffer: &mut [u32],
    width: usize,
    height: usize,
) {
    fill_rect(buffer, width, height, &Rect { x: 0, y: 0, w: width as i32, h: height as i32 }, COLOR_BG);

    let now_ms = chrono::Utc::now().timestamp_millis();
    // FileTree에서 selected 추출 (있으면)
    let mut selected_id: Option<geulos_core::ObjectId> = None;
    for id in tree.ids() {
        if let Some(o) = tree.get(id) {
            if o.type_uri.as_str() == "aios.builtin/FileTree@1" {
                if let Some(s) = o.state.get("selected").and_then(|v| v.as_str()) {
                    if let Ok(u) = uuid::Uuid::parse_str(s) {
                        selected_id = Some(geulos_core::ObjectId::from_uuid(u));
                    }
                }
            }
        }
    }

    for (id, rect) in layout.iter() {
        let obj = match tree.get(id) { Some(o) => o, None => continue };
        match obj.type_uri.as_str() {
            "aios.builtin/Desktop@1" => {
                // 배경만 (자식이 알아서 덮음)
            }
            "aios.builtin/FileTree@1" => {
                fill_rect(buffer, width, height, &rect, COLOR_TREE_BG);
            }
            "aios.builtin/Canvas@1" => {
                fill_rect(buffer, width, height, &rect, COLOR_CANVAS_BG);
                render_canvas_preview(buffer, width, height, &rect, tree, obj);
            }
            "aios.std/Folder@1" => {
                let is_sel = selected_id == Some(id);
                if is_sel { fill_rect(buffer, width, height, &rect, COLOR_SELECTED_BG); }
                let name = obj.props.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let prefix = if is_folder_expanded(tree, id) { "[-]" } else { "[+]" };
                let label = format!("{} {}", prefix, name);
                draw_text(buffer, width, height, &label, rect.x + 4, rect.y + 6, COLOR_FOLDER_TEXT);
                draw_ai_dot_if_recent(buffer, width, height, &rect, obj, now_ms);
            }
            "aios.std/File@1" => {
                let is_sel = selected_id == Some(id);
                if is_sel { fill_rect(buffer, width, height, &rect, COLOR_SELECTED_BG); }
                let name = obj.props.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let label = format!("  {}", name);
                draw_text(buffer, width, height, &label, rect.x + 4, rect.y + 4, COLOR_FILE_TEXT);
                draw_ai_dot_if_recent(buffer, width, height, &rect, obj, now_ms);
            }
            "aios.std/Container@1" => fill_rect(buffer, width, height, &rect, COLOR_CONTAINER),
            "aios.std/Text@1" => {
                fill_rect(buffer, width, height, &rect, COLOR_BG);
                let content = obj.state.get("content").and_then(|v| v.as_str()).unwrap_or("(empty)");
                draw_text(buffer, width, height, content, rect.x + 8, rect.y + 8, COLOR_TEXT);
            }
            "aios.std/Button@1" => {
                fill_rect(buffer, width, height, &rect, COLOR_BUTTON);
                let label = obj.state.get("label").and_then(|v| v.as_str()).unwrap_or("(button)");
                draw_text(buffer, width, height, label, rect.x + 16, rect.y + 16, COLOR_BUTTON_TEXT);
            }
            "aios.std/Toggle@1" => {
                let on = obj.state.get("on").and_then(|v| v.as_bool()).unwrap_or(false);
                let color = if on { 0xFF_4C_AF_50 } else { 0xFF_9E_9E_9E };
                fill_rect(buffer, width, height, &rect, color);
                draw_text(buffer, width, height, if on { "ON" } else { "OFF" }, rect.x + 16, rect.y + 8, COLOR_BUTTON_TEXT);
            }
            _ => {}
        }
    }
}

fn is_folder_expanded(tree: &TreeModel, folder_id: geulos_core::ObjectId) -> bool {
    for id in tree.ids() {
        if let Some(o) = tree.get(id) {
            if o.type_uri.as_str() == "aios.builtin/FileTree@1" {
                if let Some(arr) = o.state.get("expanded").and_then(|v| v.as_array()) {
                    for v in arr {
                        if let Some(s) = v.as_str() {
                            if let Ok(u) = uuid::Uuid::parse_str(s) {
                                if geulos_core::ObjectId::from_uuid(u) == folder_id { return true; }
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

fn draw_ai_dot_if_recent(
    buffer: &mut [u32], w: usize, h: usize, rect: &Rect, obj: &geulos_core::Object, now_ms: i64,
) {
    let actor = obj.state.get("last_change_actor").and_then(|v| v.as_str()).unwrap_or("");
    if actor != "ai" { return; }
    let ts = obj.state.get("last_change_ms").and_then(|v| v.as_i64()).unwrap_or(0);
    if now_ms - ts >= AI_HIGHLIGHT_MS { return; }
    // 우측 8px, 수직 중앙. 8×8 사각.
    let dot_x = rect.x + rect.w - 16;
    let dot_y = rect.y + rect.h / 2 - 4;
    fill_rect(buffer, w, h, &Rect { x: dot_x, y: dot_y, w: 8, h: 8 }, COLOR_AI_DOT);
}

fn render_canvas_preview(
    buffer: &mut [u32], w: usize, h: usize, rect: &Rect,
    tree: &TreeModel, canvas: &geulos_core::Object,
) {
    // active_app이 있으면 layout이 알아서 그렸으므로 여기선 active_file만 처리.
    if canvas.state.get("active_app").and_then(|v| v.as_str()).is_some() { return; }
    let file_id_str = match canvas.state.get("active_file").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            draw_text(buffer, w, h, "(파일을 선택하세요)", rect.x + 16, rect.y + 16, 0xFF_99_99_99);
            return;
        }
    };
    let file_id = match uuid::Uuid::parse_str(file_id_str).map(geulos_core::ObjectId::from_uuid) {
        Ok(id) => id, Err(_) => return,
    };
    let file = match tree.get(file_id) { Some(o) => o, None => return };
    let name = file.props.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    draw_text(buffer, w, h, name, rect.x + 16, rect.y + 16, COLOR_TEXT);
    let preview = file.state.get("preview").and_then(|v| v.as_str()).unwrap_or("");
    // 줄 단위로 그리기 (단순 — \n 분리)
    let mut y = rect.y + 48;
    for line in preview.lines().take(20) {
        if y + 16 > rect.y + rect.h { break; }
        draw_text(buffer, w, h, line, rect.x + 16, y, COLOR_TEXT);
        y += 20;
    }
}
```

`compositor/Cargo.toml`에 `chrono` 추가 (없으면):
```toml
chrono = { workspace = true }
```

- [ ] **Step 2: 빌드**

Run: `cargo build --workspace --all-targets`
Expected: 그린, 경고 0

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 그린

- [ ] **Step 3: 수동 시연으로 시각 확인**

3터미널 시작 (server-host, desktop-shell, compositor). 컴포지터 창에:
- 좌측 30% 회색 패널 + 파일/폴더 리스트
- 우측 70% 흰 패널 + "(파일을 선택하세요)" 문구
- 워크스페이스에 미리 둔 파일이 좌측에 보임

- [ ] **Step 4: 커밋**

```powershell
git add compositor/src/render.rs compositor/Cargo.toml
git commit -m @'
feat(compositor): M7 T5 — FileTree/Canvas/Folder/File 렌더 + AI 노란 점

좌측 30% 회색 트리 패널 + 우측 70% 흰 캔버스. Folder [+/-] prefix.
선택 노드 강조. last_change_actor=="ai" && 5초 이내면 우측 노란 점.
Canvas active_file 텍스트 preview.
'@
```

---

## Task T6 — 입력 처리: 폴더 클릭(expand/collapse) + 파일 클릭(set_file)

**Files:**
- Modify: `compositor/src/hit_test.rs`
- Modify: `compositor/src/main.rs` (클릭 처리에서 타입별 분기)
- Modify: `compositor/src/messages.rs` (UiAction 인자 처리 — 이미 args: Value 있음)

- [ ] **Step 1: hit_test.rs 확인**

기존 `hit_test`는 좌표 → ObjectId 반환. 그대로 사용. 다만 클릭한 객체 타입에 따라 호출할 메서드가 다름:
- Folder: `FileTree.expand(folder_id)` 또는 `FileTree.collapse(folder_id)` — 현재 expanded 여부로 분기
- File: `FileTree.select(file_id)` + `Canvas.set_file(file_id)` (두 invoke)
- 그 외: 기존 첫 번째 메서드 호출 (echo 호환)

- [ ] **Step 2: main.rs 클릭 핸들러 수정**

`compositor/src/main.rs`의 `WindowEvent::MouseInput { Pressed, Left }` 분기 교체:
```rust
WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
    let (cx, cy) = (self.cursor.0 as i32, self.cursor.1 as i32);
    if let Some(window) = &self.window {
        let size = window.inner_size();
        let tree = self.tree.lock().unwrap();
        let lay = layout(&tree, size.width as i32, size.height as i32);
        if let Some(target) = hit_test(&tree, &lay, cx, cy) {
            if let Some(obj) = tree.get(target) {
                let actions = dispatch_click(&tree, target, obj);
                for action in actions {
                    let _ = self.ui_tx.try_send(action);
                }
            }
        }
    }
}
```

`compositor/src/main.rs` 하단에 새 함수:
```rust
fn dispatch_click(tree: &TreeModel, target: geulos_core::ObjectId, obj: &geulos_core::Object) -> Vec<UiAction> {
    match obj.type_uri.as_str() {
        "aios.std/Folder@1" => {
            // FileTree 찾아서 expanded 여부 확인 → expand/collapse 결정
            let ft = find_file_tree(tree);
            let is_expanded = ft.is_some_and(|f| {
                f.state.get("expanded").and_then(|v| v.as_array()).is_some_and(|arr| {
                    arr.iter().any(|v| v.as_str() == Some(&target.to_string()))
                })
            });
            let method = if is_expanded { "collapse" } else { "expand" };
            if let Some(ft) = ft {
                vec![UiAction::Invoke {
                    target: ft.id,
                    method: method.to_string(),
                    args: serde_json::json!({ "id": target.to_string() }),
                }]
            } else { vec![] }
        }
        "aios.std/File@1" => {
            // FileTree.select + Canvas.set_file
            let mut out = Vec::new();
            if let Some(ft) = find_file_tree(tree) {
                out.push(UiAction::Invoke {
                    target: ft.id,
                    method: "select".to_string(),
                    args: serde_json::json!({ "id": target.to_string() }),
                });
            }
            if let Some(cv) = find_canvas(tree) {
                out.push(UiAction::Invoke {
                    target: cv.id,
                    method: "set_file".to_string(),
                    args: serde_json::json!({ "id": target.to_string() }),
                });
            }
            out
        }
        _ => {
            // 기존: 첫 번째 메서드 호출
            if let Some(m) = obj.methods.first() {
                vec![UiAction::Invoke {
                    target, method: m.name().to_string(), args: serde_json::Value::Null,
                }]
            } else { vec![] }
        }
    }
}

fn find_file_tree(tree: &TreeModel) -> Option<&geulos_core::Object> {
    for id in tree.ids() {
        if let Some(o) = tree.get(id) {
            if o.type_uri.as_str() == "aios.builtin/FileTree@1" { return Some(o); }
        }
    }
    None
}

fn find_canvas(tree: &TreeModel) -> Option<&geulos_core::Object> {
    for id in tree.ids() {
        if let Some(o) = tree.get(id) {
            if o.type_uri.as_str() == "aios.builtin/Canvas@1" { return Some(o); }
        }
    }
    None
}
```

- [ ] **Step 3: desktop-shell의 invoke 핸들러 (FileTree.expand/collapse/select, Canvas.set_file)**

`apps/desktop-shell/src/main.rs`의 idle 루프 자리에 invoke 디스패치 추가. *시그니처*가 길어지므로 `apps/desktop-shell/src/invoke_handler.rs` 신규 모듈 추출:

`apps/desktop-shell/src/invoke_handler.rs`:
```rust
//! 외부(컴포지터·AI)로부터 받은 invoke를 처리.

use geulos_core::ObjectId;
use serde_json::{json, Value};

pub struct InvokeOutcome {
    /// 상태 갱신 (StateSet으로 broadcast).
    pub state_sets: Vec<(ObjectId, String, Value)>,
    /// 새 객체 mount 필요.
    pub new_mounts: Vec<geulos_core::Object>,
    /// 객체 제거.
    pub removed: Vec<ObjectId>,
}

impl InvokeOutcome {
    pub fn empty() -> Self { Self { state_sets: vec![], new_mounts: vec![], removed: vec![] } }
}

pub fn handle_file_tree_expand(target: ObjectId, expanded: &[ObjectId], folder_id: ObjectId) -> InvokeOutcome {
    let mut new_list: Vec<String> = expanded.iter().map(|i| i.to_string()).collect();
    let s = folder_id.to_string();
    if !new_list.contains(&s) { new_list.push(s); }
    InvokeOutcome {
        state_sets: vec![(target, "expanded".into(), json!(new_list))],
        ..InvokeOutcome::empty()
    }
}

pub fn handle_file_tree_collapse(target: ObjectId, expanded: &[ObjectId], folder_id: ObjectId) -> InvokeOutcome {
    let s = folder_id.to_string();
    let new_list: Vec<String> = expanded.iter()
        .map(|i| i.to_string())
        .filter(|x| x != &s)
        .collect();
    InvokeOutcome {
        state_sets: vec![(target, "expanded".into(), json!(new_list))],
        ..InvokeOutcome::empty()
    }
}

pub fn handle_file_tree_select(target: ObjectId, node_id: ObjectId) -> InvokeOutcome {
    InvokeOutcome {
        state_sets: vec![(target, "selected".into(), json!(node_id.to_string()))],
        ..InvokeOutcome::empty()
    }
}

pub fn handle_canvas_set_file(target: ObjectId, file_id: ObjectId) -> InvokeOutcome {
    InvokeOutcome {
        state_sets: vec![(target, "active_file".into(), json!(file_id.to_string()))],
        ..InvokeOutcome::empty()
    }
}
```

`apps/desktop-shell/src/lib.rs`에 `pub mod invoke_handler;` 추가.

`apps/desktop-shell/src/main.rs`의 idle 루프 — 실제 InvokeMsg를 디코드하고 handler 호출 + StateSet 전송. 핵심 흐름:
```rust
// 메인 루프 (idle loop 교체)
let mut tracked_expanded: Vec<ObjectId> = Vec::new();
loop {
    let n = match stream.read(&mut buf).await {
        Ok(n) => n,
        Err(e) => { eprintln!("[desktop-shell] read error: {}", e); break; }
    };
    if n == 0 { break; }
    accum.extend_from_slice(&buf[..n]);
    loop {
        let mut slice = accum.as_slice();
        let body = match decode_frame(&mut slice) {
            Ok(b) => b,
            Err(_) => break,
        };
        let consumed = accum.len() - slice.len();
        accum.drain(..consumed);
        // InvokeMsg 시도
        if let Ok(inv) = serde_json::from_slice::<InvokeMsg>(&body) {
            let target = ObjectId::from_str(&inv.target_object_id)?;
            let outcome = match inv.method.as_str() {
                "expand" => {
                    let fid_str = inv.args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let fid = ObjectId::from_str(fid_str)?;
                    tracked_expanded.retain(|x| *x != fid);
                    tracked_expanded.push(fid);
                    invoke_handler::handle_file_tree_expand(target, &tracked_expanded[..tracked_expanded.len()-1], fid)
                }
                "collapse" => {
                    let fid_str = inv.args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let fid = ObjectId::from_str(fid_str)?;
                    let outcome = invoke_handler::handle_file_tree_collapse(target, &tracked_expanded, fid);
                    tracked_expanded.retain(|x| *x != fid);
                    outcome
                }
                "select" => {
                    let nid_str = inv.args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let nid = ObjectId::from_str(nid_str)?;
                    invoke_handler::handle_file_tree_select(target, nid)
                }
                "set_file" => {
                    let fid_str = inv.args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let fid = ObjectId::from_str(fid_str)?;
                    invoke_handler::handle_canvas_set_file(target, fid)
                }
                _ => invoke_handler::InvokeOutcome::empty(),
            };
            // state_sets를 SetStateMsg로 서버에 전송 → 서버가 broadcast
            for (oid, key, val) in outcome.state_sets {
                let msg = SetStateMsg {
                    target_object_id: oid.to_string(),
                    key,
                    value: val,
                };
                stream.write_all(&encode_frame(&serde_json::to_vec(&msg)?)).await?;
            }
        }
    }
}
```

`InvokeMsg`, `SetStateMsg`는 `geulos-proto`에 이미 있을 것 — 없으면 추가 필요. 검증 step에서 확인.

- [ ] **Step 4: 빌드 + 수동 시연**

Run: `cargo build --workspace --all-targets`
Expected: 그린 (geulos-proto에 InvokeMsg/SetStateMsg 없으면 컴파일 에러 — 그 경우 proto에 추가)

3터미널 시연:
- 워크스페이스에 폴더 `sub/` + 안에 `nested.txt` 두기
- compositor에서 `[+] sub` 클릭 → `[-] sub` + 들여쓰기로 `nested.txt` 보임
- `nested.txt` 클릭 → 강조 배경 + 우측에 "nested.txt" + preview

- [ ] **Step 5: 단위 테스트**

`apps/desktop-shell/tests/invoke_handler_test.rs`:
```rust
use geulos_core::ObjectId;
use geulos_desktop_shell::invoke_handler::*;

#[test]
fn expand_adds_folder_to_list() {
    let target = ObjectId::new();
    let folder = ObjectId::new();
    let outcome = handle_file_tree_expand(target, &[], folder);
    assert_eq!(outcome.state_sets.len(), 1);
    let (oid, key, val) = &outcome.state_sets[0];
    assert_eq!(*oid, target);
    assert_eq!(key, "expanded");
    let arr = val.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0].as_str(), Some(folder.to_string().as_str()));
}

#[test]
fn collapse_removes_folder() {
    let target = ObjectId::new();
    let folder = ObjectId::new();
    let outcome = handle_file_tree_collapse(target, &[folder], folder);
    let (_, _, val) = &outcome.state_sets[0];
    assert_eq!(val.as_array().unwrap().len(), 0);
}

#[test]
fn select_sets_node_id() {
    let target = ObjectId::new();
    let node = ObjectId::new();
    let outcome = handle_file_tree_select(target, node);
    let (_, key, val) = &outcome.state_sets[0];
    assert_eq!(key, "selected");
    assert_eq!(val.as_str(), Some(node.to_string().as_str()));
}

#[test]
fn canvas_set_file_updates_active_file() {
    let target = ObjectId::new();
    let file = ObjectId::new();
    let outcome = handle_canvas_set_file(target, file);
    let (_, key, val) = &outcome.state_sets[0];
    assert_eq!(key, "active_file");
    assert_eq!(val.as_str(), Some(file.to_string().as_str()));
}
```

Run: `cargo test -p geulos-desktop-shell --test invoke_handler_test`
Expected: PASS (4 tests)

- [ ] **Step 6: 커밋**

```powershell
git add compositor/src/main.rs apps/desktop-shell/src/invoke_handler.rs apps/desktop-shell/src/lib.rs apps/desktop-shell/src/main.rs apps/desktop-shell/tests/invoke_handler_test.rs
git commit -m @'
feat(compositor)+(desktop-shell): M7 T6 — 폴더 expand/collapse + 파일 select

컴포지터 클릭 → 타입별 디스패치(Folder→FileTree.expand/collapse, File→select+set_file).
desktop-shell이 InvokeMsg 받아 state 갱신 → SetStateMsg로 broadcast → 컴포지터 재렌더.
'@
```

---

## Task T7 — 단방향 디스크 기록 + AI 시각화 (last_change_actor)

**Files:**
- Create: `apps/desktop-shell/src/fs_ops.rs`
- Create: `apps/desktop-shell/tests/fs_ops_test.rs`
- Modify: `apps/desktop-shell/src/main.rs` (invoke 핸들러에 fs 작업 추가)
- Create: `ai-bridge/scenarios/08_ai_creates_file.toml`

지원할 fs 메서드 (M7 범위):
- `Folder.create_file(name)` → 빈 파일 생성 + 새 File 객체 mount + 부모 폴더 child_count 갱신 + last_change 갱신
- `File.write(content)` → atomic write (tempfile + rename) + size_bytes, preview, last_change 갱신
- `File.delete()` → 디스크 파일 제거 + 객체 제거

각 메서드는 actor 정보 필요 — InvokeMsg에 actor가 들어있다고 가정 (서버가 채워주는 형태). 없으면 "system" fallback.

- [ ] **Step 1: fs_ops_test.rs 실패 테스트**

`apps/desktop-shell/tests/fs_ops_test.rs`:
```rust
use geulos_desktop_shell::fs_ops;
use std::fs;

#[test]
fn create_file_writes_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("new.md");
    fs_ops::create_empty_file(&target).expect("create");
    assert!(target.exists());
    assert_eq!(fs::read_to_string(&target).unwrap(), "");
}

#[test]
fn write_file_replaces_content_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("a.txt");
    fs::write(&target, "old").unwrap();
    fs_ops::atomic_write(&target, b"new content").expect("write");
    assert_eq!(fs::read_to_string(&target).unwrap(), "new content");
}

#[test]
fn write_file_creates_if_missing() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("fresh.txt");
    fs_ops::atomic_write(&target, b"x").expect("write");
    assert!(target.exists());
}

#[test]
fn delete_removes_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("victim.txt");
    fs::write(&target, "x").unwrap();
    fs_ops::delete_file(&target).expect("delete");
    assert!(!target.exists());
}

#[test]
fn safe_join_rejects_traversal() {
    let dir = tempfile::tempdir().unwrap();
    assert!(fs_ops::safe_join(dir.path(), "ok.txt").is_ok());
    assert!(fs_ops::safe_join(dir.path(), "../escape.txt").is_err());
    assert!(fs_ops::safe_join(dir.path(), "sub/ok.txt").is_ok());
    assert!(fs_ops::safe_join(dir.path(), "sub/../../escape").is_err());
}
```

- [ ] **Step 2: 실패 확인 후 fs_ops.rs 구현**

`apps/desktop-shell/src/fs_ops.rs`:
```rust
//! 단방향 디스크 기록 — 객체 변경만 디스크에 반영. atomic write.

use std::path::{Path, PathBuf};

pub fn create_empty_file(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::File::create(path)?;
    Ok(())
}

/// tempfile에 쓰고 rename — 부분 쓰기 방지.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp.geulos");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn delete_file(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)?;
    Ok(())
}

/// 경로 traversal(`..`)을 거부. base 밖으로 나가면 에러.
pub fn safe_join(base: &Path, rel: &str) -> Result<PathBuf, String> {
    let joined = base.join(rel);
    let canonical = joined.components().fold(PathBuf::new(), |mut acc, c| {
        match c {
            std::path::Component::ParentDir => { acc.pop(); }
            std::path::Component::CurDir => {}
            other => acc.push(other.as_os_str()),
        }
        acc
    });
    if !canonical.starts_with(base) {
        return Err(format!("path traversal: {} escapes {}", rel, base.display()));
    }
    Ok(canonical)
}
```

`apps/desktop-shell/src/lib.rs`에 `pub mod fs_ops;` 추가.

- [ ] **Step 3: 테스트 통과**

Run: `cargo test -p geulos-desktop-shell --test fs_ops_test`
Expected: PASS (5 tests)

- [ ] **Step 4: main.rs invoke 핸들러에 fs 메서드 추가**

기존 invoke 디스패치(T6 step 3)에 추가:
```rust
"create_file" => {
    // target은 Folder. name 인자.
    let name = inv.args.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let folder_path = lookup_folder_path(&mounted_objects, target);
    if let Some(folder_path) = folder_path {
        let safe = fs_ops::safe_join(&folder_path, name).map_err(std::io::Error::other)?;
        fs_ops::create_empty_file(&safe)?;
        // 새 File 객체 mount + 부모 폴더 자식 추가 + last_change_actor 갱신
        let actor = inv.actor.as_deref().unwrap_or("system");
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut new_file = std_types::file(
            owner.clone(),
            safe.to_string_lossy().as_ref(),
            name,
            mime_for(name),
            now_ms,
        );
        new_file.state.insert("last_change_actor".into(), json!(actor));
        new_file.parent = Some(target);
        // mount
        let msg = MountMsg { root_object_id: new_file.id.to_string(), tree: serde_json::to_value(&new_file)? };
        stream.write_all(&encode_frame(&serde_json::to_vec(&msg)?)).await?;
        mounted_objects.push(new_file.clone());
        // 부모 폴더 last_change + child_count 갱신
        let new_count = mounted_objects.iter().filter(|o| o.parent == Some(target)).count();
        send_state_set(&mut stream, target, "child_count", json!(new_count)).await?;
        send_state_set(&mut stream, target, "last_change_ms", json!(now_ms)).await?;
        send_state_set(&mut stream, target, "last_change_actor", json!(actor)).await?;
    }
    InvokeOutcome::empty()
}
"write" => {
    // target은 File. content 인자.
    let content = inv.args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let file_path = lookup_file_path(&mounted_objects, target);
    if let Some(p) = file_path {
        fs_ops::atomic_write(&p, content.as_bytes())?;
        let actor = inv.actor.as_deref().unwrap_or("system");
        let now_ms = chrono::Utc::now().timestamp_millis();
        send_state_set(&mut stream, target, "size_bytes", json!(content.len() as u64)).await?;
        // preview 갱신 (텍스트 한정)
        let preview = if content.len() > 512 { &content[..512] } else { content };
        send_state_set(&mut stream, target, "preview", json!(preview)).await?;
        send_state_set(&mut stream, target, "last_change_ms", json!(now_ms)).await?;
        send_state_set(&mut stream, target, "last_change_actor", json!(actor)).await?;
    }
    InvokeOutcome::empty()
}
"delete" => {
    // target은 File 또는 Folder. M7은 File만 지원.
    let path = lookup_file_path(&mounted_objects, target);
    if let Some(p) = path {
        fs_ops::delete_file(&p)?;
        // remove 객체 (서버에 DestroyMsg 전송 — 없으면 SetState로 안내)
        // 단순화: SetState로 deleted=true 마크하고 후속 mount cleanup은 refresh로
        send_state_set(&mut stream, target, "deleted", json!(true)).await?;
    }
    InvokeOutcome::empty()
}
```

보조 함수:
```rust
fn lookup_folder_path(objects: &[Object], id: ObjectId) -> Option<PathBuf> {
    objects.iter().find(|o| o.id == id && o.type_uri.as_str() == "aios.std/Folder@1")
        .and_then(|o| o.props.get("path").and_then(|v| v.as_str()))
        .map(PathBuf::from)
}
fn lookup_file_path(objects: &[Object], id: ObjectId) -> Option<PathBuf> {
    objects.iter().find(|o| o.id == id && o.type_uri.as_str() == "aios.std/File@1")
        .and_then(|o| o.props.get("path").and_then(|v| v.as_str()))
        .map(PathBuf::from)
}
fn mime_for(name: &str) -> &'static str {
    let ext = std::path::Path::new(name).extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        "txt" | "toml" => "text/plain",
        "md" => "text/markdown",
        "json" => "text/json",
        "rs" => "text/rust",
        _ => "application/octet-stream",
    }
}
async fn send_state_set(
    stream: &mut TcpStream, target: ObjectId, key: &str, value: serde_json::Value,
) -> std::io::Result<()> {
    let msg = SetStateMsg {
        target_object_id: target.to_string(),
        key: key.to_string(),
        value,
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&msg).unwrap())).await?;
    Ok(())
}
```

**중요:** `InvokeMsg.actor` 필드가 proto에 있어야 함. 없으면 proto 확장:
- `geulos-proto/src/lib.rs`에서 `InvokeMsg` struct에 `pub actor: Option<String>` 추가
- server-host가 InvokeMsg 전달 시 발신 actor를 채워서 보내야 함 (auth context에서 가져옴)
- 서버 코드 (`core/src/server/invoke.rs`) 확인 — 이미 actor 정보 갖고 있을 것

- [ ] **Step 5: ai-bridge 시나리오 추가**

`ai-bridge/scenarios/08_ai_creates_file.toml`:
```toml
name = "AI가 워크스페이스에 파일을 생성"

[goal]
text = """
GeulOS 워크스페이스에 새 파일 'ai-hello.md'를 만들고 본문에 다음을 써라:
"# Hello from Claude

GeulOS 데스크톱 셸이 살아있습니다. 이 파일은 AI가 직접 만들었습니다.
좌측 트리에 즉시 등장했나요?"

방법:
1. list_objects_by_type "aios.builtin/FileTree@1" → 워크스페이스 루트 폴더 ID 알기
2. list_objects_by_type "aios.std/Folder@1" → 루트와 일치하는 폴더 찾기
3. invoke_method on the root Folder: create_file with name="ai-hello.md"
4. 반환된 File ID에 write(content=위 본문) 호출
5. report_done

기대: 사용자가 컴포지터 창에서 좌측 트리에 ai-hello.md가 *노란 점*과 함께 등장하는 것을 본다.
"""

[budget]
max_turns = 12
max_wall_secs = 90
```

- [ ] **Step 6: 빌드 + 수동 시연**

Run: `cargo build --workspace --all-targets`
Expected: 그린

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 그린

4터미널 시연 (`server-host`, `desktop-shell`, `compositor`, ai-bridge):
```powershell
cargo run -p geulos-ai-bridge -- run --scenario ai-bridge/scenarios/08_ai_creates_file.toml
```
Expected:
- 컴포지터 좌측 트리에 `ai-hello.md` 등장 (5초 이내)
- 파일명 우측에 노란 점 ●
- 5초 후 노란 점 자동 사라짐
- 클릭하면 우측 캔버스에 본문 preview

- [ ] **Step 7: 커밋**

```powershell
git add apps/desktop-shell/src/fs_ops.rs apps/desktop-shell/src/lib.rs apps/desktop-shell/src/main.rs apps/desktop-shell/tests/fs_ops_test.rs ai-bridge/scenarios/08_ai_creates_file.toml
git commit -m @'
feat(desktop-shell)+ai-bridge: M7 T7 — 단방향 디스크 기록 + AI 시나리오 08

create_file/write/delete invoke → atomic write + last_change 갱신.
safe_join으로 traversal 차단. 시나리오 08: AI가 ai-hello.md 생성.
'@
```

---

## Task T8 — KI-001 정리: echo-app wildcard ACL 제거 + 매니페스트 fs 강제

**Files:**
- Modify: `apps/echo-app/src/lib.rs`
- Modify: `apps/echo-app/src/main.rs`
- Modify: `core/src/server/invoke.rs` (또는 권한 게이트 위치)
- Modify: `core/src/object/manifest.rs` (parsing — path/path_env 둘 다 지원)
- Modify: 회귀 테스트 (`m3_smoke.gsh` 또는 그 상응)

**범위 한정:** 권한 다이얼로그(원래 M7 plan T7)는 M8 메모장으로 이월 — M7 데스크톱 셸은 fs path prefix만 강제. KI-001(wildcard ACL) 제거 + 매니페스트 명시 권한만 통과.

- [ ] **Step 1: 현재 wildcard ACL grep**

Run: `rg "\\*.*\\*" apps/ core/ --type rust`
또는 Grep tool로 wildcard ACL 패턴 정확히 찾기. 발견 위치 기록.

- [ ] **Step 2: echo-app 명시적 ACL로 교체**

기존 echo-app이 `ACL::*` 또는 그 상응으로 모든 actor에게 모든 메서드 허용했을 것. 명시적 acl로:
- echo-app 자기 자신 (owner)만 invoke
- 외부 actor (compositor·ai-bridge)는 *명시적 grant* 후 invoke
- m3_smoke 회귀 통과 위해 server-host 시작 시 echo-app 객체에 *컴포지터 actor* + *ai-bridge actor* 두 명에게만 grant

코드 변경은 `apps/echo-app/src/lib.rs`의 객체 생성 직후 ACL 부여를 명시적 형태로.

- [ ] **Step 3: server invoke 게이트에 매니페스트 fs 검사 추가**

`core/src/server/invoke.rs`:
```rust
// invoke 처리 진입에서:
// 1. 객체 타입이 aios.std/File@1 또는 aios.std/Folder@1 인데
//    메서드가 read/write/create_file/create_folder/delete/rename 중 하나라면,
// 2. 발신 actor의 manifest.permissions.fs를 조회
// 3. 객체의 props.path가 매니페스트 fs 항목 중 어느 prefix에도 안 걸리면 PermissionDenied
```

단순화 시안:
```rust
fn check_fs_permission(actor: &ActorId, target: &Object, registry: &ManifestRegistry) -> Result<(), PermissionDenied> {
    if !is_fs_method(&target.type_uri, method) { return Ok(()); }
    let path = target.props.get("path").and_then(|v| v.as_str())
        .ok_or(PermissionDenied::missing_path())?;
    let manifest = registry.get(actor).ok_or(PermissionDenied::no_manifest())?;
    for entry in &manifest.permissions_fs {
        let base = entry.resolve(); // path 또는 path_env
        if Path::new(path).starts_with(&base) { return Ok(()); }
    }
    Err(PermissionDenied::not_allowed(path))
}
```

세부는 현재 `core/src/server/invoke.rs`와 `manifest.rs` 구조에 맞춰 조정.

- [ ] **Step 4: 회귀 테스트 갱신 + 통과 확인**

Run: `cargo test --workspace`
Expected: 그린. m3_smoke·M4 acceptance 회귀 깨지면 명시적 ACL 부여 또는 매니페스트 보강.

- [ ] **Step 5: wildcard ACL grep 재확인**

Run: 위 T8 Step 1과 동일 패턴
Expected: 0건

- [ ] **Step 6: 커밋**

```powershell
git add apps/echo-app/ core/src/server/invoke.rs core/src/object/manifest.rs
git commit -m @'
fix(security): M7 T8 — KI-001 wildcard ACL 제거 + 매니페스트 fs path prefix 강제

echo-app 명시적 ACL grant. server invoke 게이트가 manifest.permissions.fs로
File/Folder 메서드 접근 검사. m3_smoke 회귀 갱신.
'@
```

---

## Task T9 — M7 acceptance + 도그푸딩 시작

**Files:**
- Create: `docs/manual-tests/m7-acceptance.md`
- Modify: `README.md` (M7 결과 + 4-터미널 실행 방법)
- Modify: `docs/known-issues.md` (KI-001 close, 새 KI 등록 — IME, 양방향 동기 등)
- Modify: `docs/plans/2026-05-18-geulos-m7-notepad.md` (deprecated 헤더 한 줄 추가, 본문 보존)

- [ ] **Step 1: m7-acceptance.md 작성**

`docs/manual-tests/m7-acceptance.md`:
```markdown
# GeulOS M7 — 데스크톱 셸 Acceptance

## 환경
- Host: Windows 11, F:\GeulOS workspace
- Workspace 루트: `%USERPROFILE%\GeulOS\workspace`

## 사전 준비
```powershell
# 워크스페이스 정리 (선택)
Remove-Item "$env:USERPROFILE\GeulOS\workspace\*" -Recurse -Force -ErrorAction SilentlyContinue
# 미리 둘 파일 (시연 풍성)
New-Item -ItemType Directory "$env:USERPROFILE\GeulOS\workspace\notes" -Force | Out-Null
Set-Content "$env:USERPROFILE\GeulOS\workspace\notes\todo.md" "- M7 acceptance" -Encoding utf8
Set-Content "$env:USERPROFILE\GeulOS\workspace\hello.txt" "안녕 GeulOS" -Encoding utf8
```

## 시나리오

### 1. 부팅 + 트리 mount
- 터미널 1: `cargo run -p geulos-server-host`
- 터미널 2: `cargo run -p geulos-desktop-shell`
- 터미널 3: `cargo run -p geulos-compositor`

**기대:**
- 컴포지터 창 좌측에 `notes/` (폴더, `[+]` 접힘) + `hello.txt` 보임
- 우측 캔버스: "(파일을 선택하세요)"

### 2. 폴더 펼치기/접기
- `[+] notes` 클릭 → `[-] notes` + 들여쓰기로 `todo.md` 보임
- `[-] notes` 클릭 → 다시 접힘

### 3. 파일 선택 + preview
- `hello.txt` 클릭 → 좌측에서 강조, 우측에 "hello.txt" + "안녕 GeulOS"

### 4. AI 시연 — 시나리오 08
- 터미널 4: `cargo run -p geulos-ai-bridge -- run --scenario ai-bridge/scenarios/08_ai_creates_file.toml`

**기대:**
- ~30초 안에 좌측 트리에 `ai-hello.md` 등장
- 파일명 우측에 노란 점 ● — 5초 후 사라짐
- 클릭하면 우측에 AI가 쓴 본문 표시

### 5. 디스크 확인 (Windows 호환성)
- Windows 탐색기로 `%USERPROFILE%\GeulOS\workspace` 열기
- `ai-hello.md`가 평문 UTF-8로 존재 → 메모장으로도 열림

## 통과 기준
- [ ] 위 1~5 모두 성공
- [ ] `cargo test --workspace` 그린
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 그린
- [ ] wildcard ACL grep 0건

## 도그푸딩 약속
M7 완료부터 **2주간** 사용자 본인이 매일 GeulOS workspace를 *읽기·탐색* 도구로 사용:
- 코드 스니펫·메모를 workspace에 직접 저장 (Windows에서)
- desktop-shell + compositor 띄워두고 트리에서 확인
- AI 시연 1일 1회 이상

매주 회고 메모 1건 (workspace 안에) — 거슬리는 점/만족스러운 점 → M8 우선순위 결정.
```

- [ ] **Step 2: README.md 업데이트**

README의 마일스톤 섹션에 M7 결과 추가:
```markdown
### M7 — 데스크톱 셸 (2026-05-? 완료)
- `%USERPROFILE%\GeulOS\workspace`가 *바탕화면*
- 좌측 동적 파일 트리(폴더·파일 1급 객체) + 우측 콘텐츠 캔버스
- AI가 파일을 만들면 트리에 즉시 등장 + 노란 점 5초 페이드
- 단방향 동기화(객체→디스크). Windows 탐색기 호환.
- KI-001 해소(wildcard ACL 제거)

실행:
```powershell
cargo run -p geulos-server-host    # 터미널 1
cargo run -p geulos-desktop-shell  # 터미널 2
cargo run -p geulos-compositor     # 터미널 3
# AI 시연
cargo run -p geulos-ai-bridge -- run --scenario ai-bridge/scenarios/08_ai_creates_file.toml
```
```

- [ ] **Step 3: known-issues.md 갱신**

- KI-001: ✅ closed (T8)
- 새 KI 등록:
  - KI-014: FS watcher 없음 — Windows 탐색기 변경은 desktop-shell 재시작 필요 (M9 양방향 동기화)
  - KI-015: 권한 다이얼로그 없음 — manifest path prefix 강제만, AI invoke가 manifest 통과하면 사용자 동의 없이 디스크 기록 (M8 메모장에서 도입 예정)
  - KI-013(이미 등록): 한글 IME (메모장 M8 의존)

- [ ] **Step 4: 기존 M7 notepad plan deprecated 표시**

`docs/plans/2026-05-18-geulos-m7-notepad.md` 맨 위 헤더 바로 아래에:
```markdown
> **DEPRECATED (2026-05-18):** 본 plan은 *데스크톱 셸*(2026-05-18-geulos-m7-desktop-shell.md)로 M7이 재정의되면서 *M8 메모장*에 통합 위임됨. T1·T2(Memo/TextArea/MemoList 타입 + notepad-app 크레이트 스캐폴드)는 이미 main에 들어가 *보존* — M8 메모장 plan이 이 자산을 활용한다. 본 문서는 *역사적 참고* 용으로만 유지.
```

- [ ] **Step 5: 메모리 갱신**

`C:\Users\user\.claude\projects\F--GeulOS\memory\project_current_status.md` 갱신:
- M7 데스크톱 셸 완료 표시
- 다음 후보: M8 메모장 (T1·T2 자산 활용) 또는 Phase D VM GUI

- [ ] **Step 6: 최종 빌드 + 모든 검증**

Run:
```powershell
cargo build --workspace --all-targets
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```
Expected: 모두 그린.

수동 시연 (위 1~5) 완전 수행 후 통과 기준 모두 체크.

- [ ] **Step 7: 커밋 + push (controller가 일괄)**

```powershell
git add docs/manual-tests/m7-acceptance.md README.md docs/known-issues.md docs/plans/2026-05-18-geulos-m7-notepad.md
git commit -m @'
docs: M7 acceptance + 데스크톱 셸 완료 기록

m7-acceptance.md(4-터미널 시연·도그푸딩 약속), README에 실행법.
KI-001 closed, KI-014/015 신규 등록. M7 notepad plan deprecated(M8 위임).
'@
```

---

## 자체 점검

**스펙 커버리지:**
- ① 단방향 동기 (객체→디스크) — T7 ✓
- ② 워크스페이스 루트 `%USERPROFILE%\GeulOS\workspace` 고정 — T2 ✓
- ③ AI 시각화 노란 점 + 5초 페이드 — T1 (state 필드) + T5 (렌더) + T7 (actor 갱신) ✓
- ④ T1·T2 산출물(Memo/TextArea/MemoList + notepad-app) 보존, M8 위임 — T9 step 4 (deprecated 헤더만 추가) ✓
- 데스크톱 = Desktop/FileTree/Canvas 트리 — T1 ✓
- 좌측 파일 트리 동적 동작 — T3 (스캔) + T4 (들여쓰기) + T6 (expand/collapse) ✓
- 우측 캔버스 (파일 선택 시 미리보기, 앱 슬롯) — T5 ✓
- 단일 캔버스 (M7) → 부동 창은 M8+ — T1 ADR-020에 명시 ✓
- KI-001 해소 — T8 ✓
- 매니페스트 fs path prefix 강제 — T8 ✓
- Phase D 호환성 — `std::fs` 추상 격리(fs_ops.rs), 컴포지터는 그대로 ✓

**플레이스홀더 스캔:**
- "TBD"/"TODO"/"implement later" 없음
- 코드 블록 모두 *완전*한 형태 또는 명시적 *시안* (T7·T8의 invoke 핸들러는 시안 — 실 구현 시 server-host의 InvokeMsg/SetStateMsg 구조에 맞춰 조정. 이는 plan 한계가 아니라 server-host 코드를 실시간에 확인해야 정확한 형태 결정 가능)

**타입 일관성:**
- `aios.builtin/Desktop@1`, `aios.builtin/FileTree@1`, `aios.builtin/Canvas@1` — T1·T4·T5·T6 일관
- `aios.std/Folder@1`, `aios.std/File@1` — T1·T3·T4·T5·T6·T7 일관
- 메서드명: `expand/collapse/select/refresh` (FileTree), `set_file/clear_file/set_app/clear_app` (Canvas), `create_file/create_folder/delete` (Folder), `read/write/rename/delete` (File) — T1과 T6/T7 핸들러 일치
- state 키: `expanded`, `selected`, `active_file`, `active_app`, `last_change_actor`, `last_change_ms`, `preview`, `size_bytes`, `child_count` — T1과 T3/T5/T7 사용 일치
- actor 값: `"ai"|"user"|"system"` — T1(default) + T7(invoke 시 채움) + T5(렌더 비교) 일치

**알려진 한계 (M7 범위 밖, 명시):**
- 한글 IME (KI-013) — M8 메모장 의존
- FS watcher (KI-014 신규) — M9+
- 권한 다이얼로그 (KI-015 신규) — M8 메모장에서 도입
- 부동 창 / 다중 창 — M8+
- 시각화 글로우·배지·세션 그룹 — 사용자 설정으로 후속

**위험과 완충:**

| 위험 | 완충 |
|---|---|
| T7의 InvokeMsg.actor 필드가 proto에 없을 가능성 | proto 확장 명시 step 포함, server-host도 함께 갱신 |
| Windows의 `std::fs::rename` 대상이 이미 존재하면 실패 | atomic_write에서 명시적 처리 또는 try-remove-then-rename — fs_ops_test에서 검증 |
| desktop-shell이 단일 라이터 가정 깨기 (컴포지터와 동시 SetState) | desktop-shell이 *모든* state 갱신의 *유일한* 출처. 컴포지터는 invoke만 보냄 — broadcast SetState는 desktop-shell이 받아서 다시 보냄(이미 single writer) |
| 노란 점이 5초 안에 redraw 안 트리거되면 안 보임 | StateSet broadcast가 자동 redraw 유발 (compositor main.rs의 `Redraw` user event) |
| AI 시나리오가 invoke 흐름을 모르고 헤맴 | 시나리오 08에 단계 명시. ai-bridge 또는 server에서 list_objects_by_type 같은 발견 API 검증 (M5 결과) |

**Phase D와의 인터페이스:**
- desktop-shell의 `std::fs` 호출은 fs_ops.rs에만 격리 — Linux 백엔드에서 같은 API 사용 가능
- 컴포지터 렌더·레이아웃은 백엔드 무관 (winit + softbuffer은 호스트 한정이지만, Phase D는 별 컴포지터 구현)
- 객체 타입·와이어 프로토콜 그대로 — Phase D에서 같은 desktop-shell이 VM 안 컴포지터에 트리 전달

---

## Execution Handoff

Plan complete and saved to `docs/plans/2026-05-18-geulos-m7-desktop-shell.md`.

**두 가지 실행 옵션:**

1. **Subagent-Driven (recommended)** — Task별 fresh subagent 디스패치, two-stage 리뷰, 빠른 iteration
2. **Inline Execution** — 현재 세션에서 task 일괄 실행, checkpoint 리뷰

---

## 2026-05-18 추가 — 하단 CLI 보조 plan

T7까지 진행한 뒤 사용자가 *데스크톱 셸 본질*로 **항상 보이는 하단 CLI**를 추가 요구함 (`AI 대화는 CLI의 한 명령일 뿐, CLI 자체가 셸 일급 구성요소`). 본 plan은 *T7까지의 구조*를 보존하며, T7.5~T7.7는 별도 보조 plan에서 다룸:

→ **`docs/plans/2026-05-18-geulos-m7-cli-extension.md`**

본 plan의 Desktop 레이아웃(`좌측 FileTree + 우측 Canvas`)은 보조 plan에서 *3분할*(좌/우 상단 + 하단 CLI)로 확장됨. ADR-020(데스크톱 셸 아키텍처)에 CLI 패널이 *4번째 builtin 자식*으로 합류. 일정은 7주 → **10주** 재추정.
