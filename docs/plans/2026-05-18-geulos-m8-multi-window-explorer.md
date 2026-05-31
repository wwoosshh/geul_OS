> **Status:** completed (2026-05-20)
> **Note:** M8 정식 마감 — Window@1/Explorer@1 + 드라이브 mount + 멀티 윈도우 read-only. 후속 M9에서 편집/저장 복귀.

# GeulOS M8 — 전체 파일시스템 + 멀티-윈도우 탐색기 (Implementation Plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement task-by-task. Steps use checkbox (`- [ ]`) syntax. **NEVER push** — controller batches push at end of milestone.

**Spec:** `docs/specs/2026-05-18-geulos-m8-multi-window-explorer.md` (T7.5 직후 사용자 결정 — M7 T7.6/T7.7 보류, M8 4~6주)

**Goal:** Windows 전체 드라이브를 트리 root로 자동 mount + 좌측 폴더 트리/우측 파일 탐색기로 UX 재정의 + 파일을 floating multi-window viewer로. Read-only — write는 M9 권한 다이얼로그와 함께 복귀.

**Architecture:** `Window@1`·`Explorer@1` 신규 1급 객체. 드라이브 자동 mount + click-driven lazy expand. 컴포지터는 Window를 z-order 오버레이로 렌더, 마우스 focus + drag move/resize 입력 라우팅. `Folder@1`/`File@1`은 write 메서드 팩토리에서 제거 → invoke 시 MethodNotFound (read-only 강제). fs_ops 함수는 dead code로 유지 (M9 복귀용).

**Tech Stack:** Rust 1.x (musl target for Linux paths), winit 0.30 + softbuffer 컴포지터, tokio, winapi crate (Windows drive enumeration), serde_json. 기존 GeulOS 아키텍처 그대로 — core/proto/server-host/compositor/desktop-shell.

---

## 파일 구조

```
core/src/object/std_types.rs              # 수정: window(), explorer() 팩토리 + Folder/File 메서드 축소
core/tests/std_types_test.rs              # 수정: 신규 타입 라운드트립 + Folder/File 메서드 수 검증

apps/desktop-shell/src/drives.rs          # 신규: Windows 드라이브 열거
apps/desktop-shell/src/lazy_mount.rs      # 신규: 폴더 expand 시 직계 자식 mount
apps/desktop-shell/src/window_ops.rs      # 신규: Window mount/close/focus/z-order 헬퍼
apps/desktop-shell/src/explorer_ops.rs    # 신규: navigate_to/open_file 핸들러
apps/desktop-shell/src/lib.rs             # 수정: 새 모듈 export
apps/desktop-shell/src/main.rs            # 수정: workspace::resolve 제거 + 드라이브 mount + Explorer/Window invoke 처리
apps/desktop-shell/src/fs_ops.rs          # 수정: 모듈 doc 주석 + #[allow(dead_code)]
apps/desktop-shell/src/workspace.rs       # 수정: dead code 마킹 (M8은 사용 X)
apps/desktop-shell/src/scan.rs            # 수정: dead code 마킹 (lazy_mount로 대체)
apps/desktop-shell/src/invoke_handler.rs  # 수정: file_tree/canvas/file/folder는 일부 unused (cleanup)
apps/desktop-shell/Cargo.toml             # 수정: winapi dependency 추가

compositor/src/layout.rs                  # 수정: 좌 25%/우 75% + Window 오버레이 layout (z 정렬)
compositor/src/render.rs                  # 수정: Explorer(list) + Window(title bar + content + [x] + resize handle) 렌더
compositor/src/main.rs                    # 수정: 마우스 focus + drag move/resize + 키보드 라우팅 확장
compositor/src/server_client.rs           # 수정: STD_TYPES에 Window/Explorer 추가
compositor/src/hit_test.rs                # 수정: Window는 z 역순 우선
compositor/tests/layout_test.rs           # 수정: 신규 4분할 + Window 오버레이 테스트

docs/adr/026-multi-window.md              # 신규
docs/adr/027-fs-readonly-m8.md            # 신규
docs/adr/028-drive-auto-mount.md          # 신규
docs/plans/2026-05-18-geulos-m7-cli-extension.md  # 수정: ADR-024/025 → 029/030 재번호 메모
docs/known-issues.md                      # 수정: M8 영향으로 KI-001/KI-002 부분 해소(메서드 자체 부재) 기록
docs/manual-tests/m8-acceptance.md        # 신규 (T8.11에서 작성)
```

---

## Task T8.0 — ADR 작성 (026/027/028) + cli-extension plan 재번호

**Estimated:** 1~2일

**Files:**
- Create: `docs/adr/026-multi-window.md`
- Create: `docs/adr/027-fs-readonly-m8.md`
- Create: `docs/adr/028-drive-auto-mount.md`
- Modify: `docs/plans/2026-05-18-geulos-m7-cli-extension.md` (ADR 번호 메모)

### 단계

- [ ] **Step 1:** ADR-026 작성 — `Window@1`을 1급 객체로 도입한 결정. spec §4.1 + §11 본문 풀어씀. 컴포지터-local 대안과의 trade-off, ADR-009 일관성, lifecycle (Explorer.open_file → mount, [x] → close → emit_destroyed), z-order 정책 (focus 시 max+1).
- [ ] **Step 2:** ADR-027 작성 — M8 Read-only 전략. 워크스페이스 격리 해제와 동시에 *팩토리에서 write 메서드 제거*하는 이유. ACL deny가 아닌 *메서드 부재*인 이유 (KI-001과 직교). M9 권한 다이얼로그 마일스톤이 도착할 때 메서드 복귀 + fs_ops 호출 분기 재활성. 솔로 dogfooding 가정.
- [ ] **Step 3:** ADR-028 작성 — 드라이브 자동 mount 결정. Windows `GetLogicalDrives` 사용 (winapi crate), 비-Windows fallback. Lazy expand 결정 이유 (전체 재귀 mount = 메모리 폭주). 권한 거부 폴더는 silent 빈 폴더 (M8 trade-off, M9 UX).
- [ ] **Step 4:** `docs/plans/2026-05-18-geulos-m7-cli-extension.md` 헤더 직후에 메모 추가:
   ```markdown
   > **ADR 번호 갱신 (2026-05-18, M8 spec 작성 시):** ADR-024/025 시드(M7 T7.7/T7.6)는 M8 ADR-026~028이 먼저 작성됨에 따라 029(AI chat session)/030(한글 IME)로 재번호 예정. M7 T7.6/T7.7 재개 시점에 본문 작성 시 새 번호 사용.
   ```
- [ ] **Step 5:** 각 ADR 통과 검증 — 4종 ADR 모두 `## 결정` / `## 컨텍스트` / `## 결과` 섹션 (기존 ADR 컨벤션 일치) 확인. 기존 020~023 패턴 참고.
- [ ] **Step 6:** Commit.
   ```bash
   git add docs/adr/026-multi-window.md docs/adr/027-fs-readonly-m8.md docs/adr/028-drive-auto-mount.md docs/plans/2026-05-18-geulos-m7-cli-extension.md
   git commit -m "docs(adr): M8 T8.0 — ADR-026/027/028 멀티-윈도우+read-only+드라이브 자동 mount"
   ```

### 디자인 결정
- ADR 본문은 *결정*과 *이유*만. 구현 디테일은 plan/spec.
- 기존 020(desktop-shell)/021(workspace-unidirectional)을 directly *supersede* 표시. 021은 워크스페이스 격리 결정이었고 M8이 정면 해제.

---

## Task T8.1 — core: Window@1 + Explorer@1 std_types 팩토리

**Estimated:** 1일 (TDD)

**Files:**
- Modify: `core/src/object/std_types.rs`
- Modify: `core/tests/std_types_test.rs`

### 단계

- [ ] **Step 1:** `core/tests/std_types_test.rs`에 실패하는 테스트 추가:

```rust
#[test]
fn window_factory_initializes_geometry_and_methods() {
    let owner = ActorId::local_user();
    let file_id = ObjectId::new();
    let w = std_types::window(owner.clone(), "todo.md", file_id, 100, 80, 600, 400);
    assert_eq!(w.type_uri.as_str(), "aios.builtin/Window@1");
    assert_eq!(w.props.get("title").and_then(|v| v.as_str()), Some("todo.md"));
    assert_eq!(w.state.get("x").and_then(|v| v.as_i64()), Some(100));
    assert_eq!(w.state.get("w").and_then(|v| v.as_i64()), Some(600));
    assert_eq!(w.state.get("z").and_then(|v| v.as_i64()), Some(0));
    assert_eq!(w.state.get("focused").and_then(|v| v.as_bool()), Some(false));
    let methods: Vec<&str> = w.methods.iter().map(|m| m.name()).collect();
    assert!(methods.contains(&"move"));
    assert!(methods.contains(&"resize"));
    assert!(methods.contains(&"focus"));
    assert!(methods.contains(&"close"));
}

#[test]
fn window_round_trip_preserves_all_fields() {
    let owner = ActorId::local_user();
    let file_id = ObjectId::new();
    let w = std_types::window(owner, "x", file_id, 10, 20, 300, 200);
    let json = serde_json::to_string(&w).unwrap();
    let parsed: Object = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, w);
}

#[test]
fn explorer_factory_has_navigate_and_open_file() {
    let owner = ActorId::local_user();
    let e = std_types::explorer(owner);
    assert_eq!(e.type_uri.as_str(), "aios.builtin/Explorer@1");
    assert_eq!(e.state.get("active_folder"), Some(&serde_json::Value::Null));
    assert_eq!(e.state.get("view_mode").and_then(|v| v.as_str()), Some("list"));
    let methods: Vec<&str> = e.methods.iter().map(|m| m.name()).collect();
    assert!(methods.contains(&"navigate_to"));
    assert!(methods.contains(&"open_file"));
}
```

- [ ] **Step 2:** 실패 확인. `cargo test -p geulos-core std_types -- window explorer` → unresolved import errors.
- [ ] **Step 3:** `core/src/object/std_types.rs` 끝에 추가 (기존 desktop/file_tree/canvas/cli 팩토리 직후, M8 섹션 헤더):

```rust
// ───────────────────────── M8: 멀티-윈도우 탐색기 ─────────────────────────

/// 플로팅 파일 viewer 윈도우. Desktop의 자식으로 mount되어 오버레이로 떠있음.
///
/// props:
/// - `title: String` — 윈도우 상단 표시 (기본 = 파일명)
/// - `file_id: ObjectId` — 보여주는 File 객체
///
/// state:
/// - `x: i32`, `y: i32` — 좌상단 좌표
/// - `w: i32`, `h: i32` — 크기 (min 200×120)
/// - `z: i32` — z-order (큰 값이 위)
/// - `focused: bool` — 키보드 입력 수신 여부
///
/// 메서드: `move(x, y)`, `resize(w, h)`, `focus()`, `close()`
pub fn window(
    owner: ActorId,
    title: &str,
    file_id: ObjectId,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.builtin/Window@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("title", json!(title));
    obj.set_prop("file_id", json!(file_id));
    obj.set_state("x", json!(x));
    obj.set_state("y", json!(y));
    obj.set_state("w", json!(w));
    obj.set_state("h", json!(h));
    obj.set_state("z", json!(0));
    obj.set_state("focused", json!(false));
    obj.methods.push(
        MethodSig::new("move")
            .with_arg(ArgSpec::new("x", "i32"))
            .with_arg(ArgSpec::new("y", "i32")),
    );
    obj.methods.push(
        MethodSig::new("resize")
            .with_arg(ArgSpec::new("w", "i32"))
            .with_arg(ArgSpec::new("h", "i32")),
    );
    obj.methods.push(MethodSig::new("focus"));
    obj.methods.push(MethodSig::new("close"));
    obj
}

/// 우측 파일 탐색기 패널. active_folder의 자식을 list로.
///
/// state:
/// - `active_folder: Option<ObjectId>` — 현재 표시 폴더. None이면 드라이브 일람 (FileTree root와 동일).
/// - `view_mode: String` — "list" (M8 고정). 향후 grid/details.
///
/// 메서드:
/// - `navigate_to(folder_id: ObjectId)` — 다른 폴더로 진입
/// - `open_file(file_id: ObjectId)` — 새 Window mount (이미 열려있으면 그것 focus)
pub fn explorer(owner: ActorId) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.builtin/Explorer@1").expect("유효한 TypeUri"), owner);
    obj.set_state("active_folder", json!(null));
    obj.set_state("view_mode", json!("list"));
    obj.methods.push(MethodSig::new("navigate_to").with_arg(ArgSpec::new("folder_id", "ObjectId")));
    obj.methods.push(MethodSig::new("open_file").with_arg(ArgSpec::new("file_id", "ObjectId")));
    obj
}
```

- [ ] **Step 4:** `cargo test -p geulos-core std_types` → 3 신규 테스트 pass.
- [ ] **Step 5:** Commit.
   ```bash
   git add core/src/object/std_types.rs core/tests/std_types_test.rs
   git commit -m "feat(core): M8 T8.1 — Window@1 + Explorer@1 std_types 팩토리"
   ```

### 디자인 결정
- `Window`의 `file_id`는 `props`에 (불변 — 한 윈도우 = 한 파일). M9에서 *복수 파일 / 폴더* 지원 시 state로 이동 검토.
- `Explorer.active_folder`가 None이면 FileTree와 같은 드라이브 일람. 컴포지터가 None일 때 FileTree.children을 렌더 — Explorer가 FileTree에 *간접 참조*.

---

## Task T8.2 — core: Folder/File 메서드 축소 (read-only) + 기존 테스트 동기화

**Estimated:** 1일

**Files:**
- Modify: `core/src/object/std_types.rs`
- Modify: `core/tests/std_types_test.rs`

### 단계

- [ ] **Step 1:** `core/tests/std_types_test.rs`에 실패하는 테스트 추가 (메서드 축소 의도 확정):

```rust
#[test]
fn folder_factory_has_no_write_methods_in_m8() {
    let owner = ActorId::local_user();
    let f = std_types::folder(owner, "/", "/", 0);
    let names: Vec<&str> = f.methods.iter().map(|m| m.name()).collect();
    // M8 read-only — write/create/delete 모두 제거. M9 권한 다이얼로그와 함께 복귀.
    assert!(!names.contains(&"create_file"), "M8: Folder.create_file 제거됨");
    assert!(!names.contains(&"create_folder"), "M8: Folder.create_folder 제거됨");
    assert!(!names.contains(&"delete"), "M8: Folder.delete 제거됨");
}

#[test]
fn file_factory_has_no_write_methods_in_m8() {
    let owner = ActorId::local_user();
    let f = std_types::file(owner, "/x", "x", "text/plain", 0);
    let names: Vec<&str> = f.methods.iter().map(|m| m.name()).collect();
    assert!(!names.contains(&"write"), "M8: File.write 제거됨");
    assert!(!names.contains(&"delete"), "M8: File.delete 제거됨");
    assert!(!names.contains(&"rename"), "M8: File.rename 제거됨");
    // read는 유지 (M8은 컴포지터가 preview props로 봄 — 직접 invoke X이지만 메서드는 둠).
    assert!(names.contains(&"read"), "File.read는 유지");
}
```

- [ ] **Step 2:** 실패 확인 — 기존 메서드들이 여전히 있음.
- [ ] **Step 3:** `core/src/object/std_types.rs`의 `folder()` 함수에서 다음 라인 *제거*:

```rust
obj.methods.push(MethodSig::new("create_file").with_arg(ArgSpec::new("name", "string")));
obj.methods.push(MethodSig::new("create_folder").with_arg(ArgSpec::new("name", "string")));
obj.methods.push(MethodSig::new("delete"));
```

함수 doc 주석에 "M8 read-only — write 메서드 부재 (M9 권한 다이얼로그와 함께 복귀)" 추가.

- [ ] **Step 4:** `file()` 함수에서 다음 라인 *제거*:

```rust
obj.methods.push(MethodSig::new("write").with_arg(ArgSpec::new("content", "string")));
obj.methods.push(MethodSig::new("rename").with_arg(ArgSpec::new("new_name", "string")));
obj.methods.push(MethodSig::new("delete"));
```

doc 주석에 동일 메모.

- [ ] **Step 5:** `cargo test -p geulos-core` → 신규 2건 pass + 기존 라운드트립 통과 확인. 기존 테스트가 메서드 수에 의존하는 게 있으면 동기화.
- [ ] **Step 6:** Commit.
   ```bash
   git add core/src/object/std_types.rs core/tests/std_types_test.rs
   git commit -m "feat(core): M8 T8.2 — Folder/File write 메서드 제거 (read-only, M9 복귀 예정)"
   ```

### 디자인 결정
- 메서드 부재가 ACL Deny보다 깨끗 — `MethodNotFound`로 자연 거부. M9 복귀는 한 줄 add.
- `read` 메서드는 유지 — M8 컴포지터는 invoke 안 함이지만 미래 사용 + 시그니처 안정성.

---

## Task T8.3 — desktop-shell: drives.rs + lazy_mount.rs + 기본 mount

**Estimated:** 2~3일

**Files:**
- Modify: `apps/desktop-shell/Cargo.toml`
- Create: `apps/desktop-shell/src/drives.rs`
- Create: `apps/desktop-shell/src/lazy_mount.rs`
- Modify: `apps/desktop-shell/src/lib.rs`
- Test: `apps/desktop-shell/tests/drives_test.rs`
- Test: `apps/desktop-shell/tests/lazy_mount_test.rs`

### 단계

- [ ] **Step 1:** `Cargo.toml`에 dependency 추가:
   ```toml
   [target.'cfg(windows)'.dependencies]
   winapi = { version = "0.3", features = ["fileapi"] }
   ```

- [ ] **Step 2:** `apps/desktop-shell/tests/drives_test.rs` 신규:

```rust
use geulos_desktop_shell::drives;

#[test]
fn list_drives_returns_at_least_one_path() {
    let ds = drives::list_drives();
    assert!(!ds.is_empty(), "최소 한 드라이브는 있어야 함");
    for d in &ds {
        assert!(d.exists() || cfg!(not(windows)), "{} 가 실제 디렉터리여야 함", d.display());
    }
}

#[cfg(windows)]
#[test]
fn list_drives_includes_drive_letter() {
    let ds = drives::list_drives();
    let paths: Vec<String> = ds.iter().map(|p| p.display().to_string()).collect();
    // C: 또는 D: 중 하나는 존재한다고 가정.
    assert!(
        paths.iter().any(|p| p.starts_with("C:") || p.starts_with("D:")),
        "C:\\ 또는 D:\\ 중 하나는 있어야: {:?}",
        paths
    );
}
```

- [ ] **Step 3:** 실패 확인. `cargo test -p geulos-desktop-shell drives` → module not found.

- [ ] **Step 4:** `apps/desktop-shell/src/drives.rs` 신규:

```rust
//! Windows 드라이브 열거. 비-Windows는 단일 root("/") fallback.

use std::path::PathBuf;

/// 시스템의 모든 root 경로를 반환.
///
/// Windows: `GetLogicalDrives` Win32 API로 비트마스크 → 알파벳별 드라이브 letter.
/// 비-Windows: `["/"]` 단일 fallback (테스트/디자인 단순성 목적, 실제 Linux/macOS 지원은 후속).
pub fn list_drives() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        list_drives_windows()
    }
    #[cfg(not(windows))]
    {
        vec![PathBuf::from("/")]
    }
}

#[cfg(windows)]
fn list_drives_windows() -> Vec<PathBuf> {
    use winapi::um::fileapi::GetLogicalDrives;
    let mask = unsafe { GetLogicalDrives() };
    if mask == 0 {
        // API 실패 시 fallback — 적어도 C 시도.
        return vec![PathBuf::from("C:\\")];
    }
    let mut out = Vec::new();
    for i in 0..26 {
        if mask & (1 << i) != 0 {
            let letter = (b'A' + i as u8) as char;
            out.push(PathBuf::from(format!("{}:\\", letter)));
        }
    }
    out
}
```

- [ ] **Step 5:** `lib.rs`에 `pub mod drives;` 추가. `cargo test -p geulos-desktop-shell drives` → 통과.

- [ ] **Step 6:** `apps/desktop-shell/tests/lazy_mount_test.rs` 신규:

```rust
use geulos_core::ActorId;
use geulos_desktop_shell::lazy_mount;
use std::fs;
use tempfile::tempdir;

#[test]
fn expand_folder_returns_direct_children_only() {
    let tmp = tempdir().unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    fs::write(tmp.path().join("a.txt"), b"hello").unwrap();
    fs::write(tmp.path().join("subdir").join("nested.txt"), b"x").unwrap();

    let owner = ActorId::local_user();
    let objs = lazy_mount::expand_folder(&owner, tmp.path(), 0).unwrap();
    let names: Vec<String> = objs
        .iter()
        .map(|o| o.props.get("name").and_then(|v| v.as_str()).unwrap_or("?").to_string())
        .collect();
    assert!(names.contains(&"subdir".to_string()));
    assert!(names.contains(&"a.txt".to_string()));
    assert!(!names.iter().any(|n| n == "nested.txt"), "재귀 mount 안 됨");
}

#[test]
fn expand_folder_returns_empty_on_permission_denied() {
    // 존재하지 않는 경로는 io::Error — 빈 vec 반환 (silent).
    let owner = ActorId::local_user();
    let result = lazy_mount::expand_folder(&owner, std::path::Path::new("/no/such/path"), 0);
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[test]
fn expand_folder_sets_parent_none_for_caller_to_fill() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("x.txt"), b"y").unwrap();
    let owner = ActorId::local_user();
    let objs = lazy_mount::expand_folder(&owner, tmp.path(), 0).unwrap();
    for o in &objs {
        assert!(o.parent.is_none(), "호출자가 parent 채움");
    }
}
```

(Cargo.toml `[dev-dependencies]`에 `tempfile = "3"` 필요 — workspace에 이미 있을 확률, 없으면 추가.)

- [ ] **Step 7:** 실패 확인 → `apps/desktop-shell/src/lazy_mount.rs` 신규:

```rust
//! 폴더 expand 시 직계 자식 mount. M8 — 전체 재귀 mount는 비현실 (메모리).

use std::io;
use std::path::Path;

use geulos_core::{std_types, ActorId, Object};

/// `folder_path` 직계 자식 (Folder + File) 객체 목록을 반환.
///
/// 권한 거부 / 경로 없음 등 io 에러는 빈 vec로 silent (M8 trade-off).
/// 반환된 객체의 `parent`는 None — 호출자가 부모 ObjectId로 채워야 함.
pub fn expand_folder(
    owner: &ActorId,
    folder_path: &Path,
    now_ms: i64,
) -> io::Result<Vec<Object>> {
    let entries = match std::fs::read_dir(folder_path) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[lazy_mount] read_dir 실패 {}: {} — 빈 폴더로 처리", folder_path.display(), e);
            return Ok(Vec::new());
        }
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match entry.file_name().to_str() {
            Some(s) => s.to_string(),
            None => continue, // 비-UTF8 이름은 skip
        };
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let obj = if meta.is_dir() {
            std_types::folder(
                owner.clone(),
                path.to_string_lossy().as_ref(),
                &name,
                now_ms,
            )
        } else if meta.is_file() {
            let mime = guess_mime(&name);
            let mut f = std_types::file(
                owner.clone(),
                path.to_string_lossy().as_ref(),
                &name,
                mime,
                now_ms,
            );
            // size_bytes는 빠르게 채움 — preview는 첫 클릭 시 별 호출 (이번 task는 빈 채).
            f.set_state("size_bytes", serde_json::json!(meta.len()));
            f
        } else {
            continue; // 심볼릭 링크 등 skip
        };
        out.push(obj);
    }
    Ok(out)
}

fn guess_mime(name: &str) -> &'static str {
    let ext = std::path::Path::new(name).extension().and_then(|s| s.to_str()).unwrap_or("");
    match ext.to_ascii_lowercase().as_str() {
        "txt" | "log" | "ini" | "cfg" | "toml" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "json" => "text/json",
        "rs" => "text/rust",
        "py" => "text/python",
        "js" | "ts" => "text/javascript",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "yaml" | "yml" => "text/yaml",
        _ => "application/octet-stream",
    }
}
```

- [ ] **Step 8:** `lib.rs`에 `pub mod lazy_mount;`. `cargo test -p geulos-desktop-shell lazy_mount` → 3건 pass.

- [ ] **Step 9:** Commit.
   ```bash
   git add apps/desktop-shell/Cargo.toml apps/desktop-shell/src/drives.rs apps/desktop-shell/src/lazy_mount.rs apps/desktop-shell/src/lib.rs apps/desktop-shell/tests/drives_test.rs apps/desktop-shell/tests/lazy_mount_test.rs
   git commit -m "feat(desktop-shell): M8 T8.3 — drives 열거 + lazy_mount (Windows GetLogicalDrives + 직계 자식 mount)"
   ```

### 디자인 결정
- `expand_folder`가 `parent=None`으로 반환 — 호출자(main.rs)가 부모 ID 알고 채움. lazy_mount는 부모 모름.
- preview는 비워둠 — *클릭 시 별 호출*은 v2 (M8은 size만 채우고 본문은 Window 열 때).
- 권한 거부 silent: spec §3 out-of-scope.

---

## Task T8.4 — compositor: STD_TYPES + layout 좌25/우75 + FileTree 폴더 전용 렌더

**Estimated:** 1~2일

**Files:**
- Modify: `compositor/src/server_client.rs`
- Modify: `compositor/src/layout.rs`
- Modify: `compositor/src/render.rs`
- Modify: `compositor/tests/layout_test.rs`

### 단계

- [ ] **Step 1:** `server_client.rs:STD_TYPES`에 두 줄 추가:
```rust
"aios.builtin/Window@1",
"aios.builtin/Explorer@1",
```
T7.5 회귀 fix의 `std_types_query_coverage_smoke` 테스트가 자동으로 두 신규 타입 cover.

- [ ] **Step 2:** `layout.rs::layout_desktop`에서 좌측 비율 0.30 → 0.25 변경 + `has_explorer` 분기. Desktop 자식 구조: `[FileTree, Explorer, Cli, Window*...]` (Explorer가 두 번째 자리). 기존 Canvas-based 코드 삭제.

```rust
// 새 children 구조: [FileTree, Explorer, Cli, Window*]
let left_w = (win_w as f32 * 0.25) as i32;
let right_w = win_w - left_w;
let has_cli = obj.children.iter().any(|&cid| {
    tree.get(cid).map(|o| o.type_uri.as_str()) == Some("aios.builtin/Cli@1")
});
let top_h = if has_cli { (win_h as f32 * 0.70) as i32 } else { win_h };
let bottom_h = win_h - top_h;

// FileTree (좌측 상단)
if let Some(ft) = find_child_by_type(tree, obj, "aios.builtin/FileTree@1") {
    out.push((ft.id, Rect { x: 0, y: 0, w: left_w, h: top_h }));
    let expanded = extract_expanded(tree, ft.id);
    let mut y = 4i32;
    for &cid in &ft.children {
        y += layout_tree_node_folders_only(tree, &expanded, cid, 4, y, left_w - 8, out);
    }
}

// Explorer (우측 상단)
if let Some(ex) = find_child_by_type(tree, obj, "aios.builtin/Explorer@1") {
    out.push((ex.id, Rect { x: left_w, y: 0, w: right_w, h: top_h }));
    // 자식 list 렌더는 render.rs가 active_folder를 직접 lookup
}

// Cli (하단)
if has_cli {
    if let Some(cli) = find_child_by_type(tree, obj, "aios.builtin/Cli@1") {
        out.push((cli.id, Rect { x: 0, y: top_h, w: win_w, h: bottom_h }));
    }
}

// Window 오버레이 (z 오름차순)
let mut windows: Vec<&Object> = obj
    .children
    .iter()
    .filter_map(|&id| tree.get(id))
    .filter(|o| o.type_uri.as_str() == "aios.builtin/Window@1")
    .collect();
windows.sort_by_key(|w| w.state.get("z").and_then(|v| v.as_i64()).unwrap_or(0));
for w in windows {
    let x = w.state.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let y = w.state.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let wid = w.state.get("w").and_then(|v| v.as_i64()).unwrap_or(600) as i32;
    let hgt = w.state.get("h").and_then(|v| v.as_i64()).unwrap_or(400) as i32;
    out.push((w.id, Rect { x, y, w: wid, h: hgt }));
}
```

`find_child_by_type` 헬퍼와 `layout_tree_node_folders_only` (기존 `layout_tree_node` 복사 + File branch skip) 신규.

- [ ] **Step 3:** `layout_test.rs`에 테스트 추가:

```rust
#[test]
fn layout_desktop_renders_explorer_in_right_top() {
    let mut tree = TreeModel::new();
    let owner = ActorId::local_user();
    let mut desktop = std_types::desktop(owner.clone());
    let ft = std_types::file_tree(owner.clone(), "/");
    let ex = std_types::explorer(owner.clone());
    let cli = std_types::cli(owner.clone());
    desktop.children = vec![ft.id, ex.id, cli.id];
    tree.upsert(desktop.clone()); tree.upsert(ft.clone()); tree.upsert(ex.clone()); tree.upsert(cli.clone());

    let lay = layout(&tree, 1000, 600);
    let ft_rect = lay.get(ft.id).unwrap();
    let ex_rect = lay.get(ex.id).unwrap();
    let cli_rect = lay.get(cli.id).unwrap();
    assert_eq!(ft_rect.w, 250); // 25% × 1000
    assert_eq!(ex_rect.x, 250);
    assert_eq!(ex_rect.w, 750);
    assert_eq!(ex_rect.h, 420); // 70% × 600
    assert_eq!(cli_rect.y, 420);
    assert_eq!(cli_rect.h, 180);
}

#[test]
fn layout_desktop_overlays_windows_in_z_order() {
    let mut tree = TreeModel::new();
    let owner = ActorId::local_user();
    let mut desktop = std_types::desktop(owner.clone());
    let ft = std_types::file_tree(owner.clone(), "/");
    let ex = std_types::explorer(owner.clone());
    let cli = std_types::cli(owner.clone());
    let fid = ObjectId::new();
    let mut w1 = std_types::window(owner.clone(), "a", fid, 10, 10, 200, 100);
    w1.set_state("z", serde_json::json!(1));
    let mut w2 = std_types::window(owner.clone(), "b", fid, 50, 50, 200, 100);
    w2.set_state("z", serde_json::json!(2));
    desktop.children = vec![ft.id, ex.id, cli.id, w1.id, w2.id];
    for o in [desktop.clone(), ft.clone(), ex.clone(), cli.clone(), w1.clone(), w2.clone()] {
        tree.upsert(o);
    }
    let lay = layout(&tree, 1000, 600);
    let r1_pos = lay.rects.iter().position(|(id, _)| *id == w1.id).unwrap();
    let r2_pos = lay.rects.iter().position(|(id, _)| *id == w2.id).unwrap();
    assert!(r1_pos < r2_pos, "z 낮은 윈도우가 먼저 (밑에) 그려져야");
}
```

- [ ] **Step 4:** `render.rs`에서 `aios.builtin/Canvas@1` 분기 제거, `aios.builtin/Explorer@1` 추가 (active_folder children list 렌더). 폴더는 `[D] name`, 파일은 `[F] name` 한 줄씩.

```rust
"aios.builtin/Explorer@1" => {
    fill_rect(buffer, width, height, &rect, COLOR_CANVAS_BG); // 흰 배경 재사용
    render_explorer_list(buffer, width, height, &rect, tree, obj);
}
```

`render_explorer_list` 헬퍼: `obj.state["active_folder"]` 가져와 None이면 FileTree.children (드라이브), Some이면 그 폴더의 children. 각 자식 한 줄 그림 (24px line height). 폴더 먼저, 파일 뒤. 정렬: 이름순.

- [ ] **Step 5:** `cargo test -p geulos-compositor` → 모든 layout 테스트 통과. `std_types_query_coverage_smoke`도 통과.

- [ ] **Step 6:** Commit.
   ```bash
   git add compositor/src/server_client.rs compositor/src/layout.rs compositor/src/render.rs compositor/tests/layout_test.rs
   git commit -m "feat(compositor): M8 T8.4 — STD_TYPES Window/Explorer + 4분할 layout + Explorer list 렌더 (Window 오버레이 z-order)"
   ```

### 디자인 결정
- Window 오버레이는 `layout` 결과 끝에 추가 — 기존 코드의 `out` 순서가 *그리는 순서* (배경부터 위로) 가정 유지. z 정렬은 layout 단에서.
- `find_child_by_type`을 `layout.rs`에 자유 함수로 — `compositor/src/main.rs::find_file_tree` 같은 패턴 재사용 가능. main.rs의 중복도 cleanup.

---

## Task T8.5 — desktop-shell: drives mount + Explorer mount + navigate_to/lazy expand

**Estimated:** 2일

**Files:**
- Create: `apps/desktop-shell/src/explorer_ops.rs`
- Modify: `apps/desktop-shell/src/lib.rs`
- Modify: `apps/desktop-shell/src/main.rs`

### 단계

- [ ] **Step 1:** `explorer_ops.rs` 신규:

```rust
//! Explorer 객체의 navigate_to/open_file 핸들러 (M8).

use geulos_core::{Object, ObjectId};
use serde_json::json;

use crate::invoke_handler::InvokeOutcome;

/// `navigate_to(folder_id)` — Explorer.state.active_folder 갱신.
pub fn handle_navigate_to(explorer_id: ObjectId, folder_id: ObjectId) -> InvokeOutcome {
    InvokeOutcome {
        state_sets: vec![(
            explorer_id,
            "active_folder".to_string(),
            json!(folder_id.to_string()),
        )],
    }
}

/// 활성 폴더가 비어있으면 (children=[]) lazy expand 필요한지 판정.
pub fn needs_expand(mounted_objects: &[Object], folder_id: ObjectId) -> bool {
    mounted_objects
        .iter()
        .find(|o| o.id == folder_id)
        .map(|f| f.children.is_empty())
        .unwrap_or(false)
}
```

- [ ] **Step 2:** `lib.rs`에 `pub mod explorer_ops;` 및 `pub mod window_ops;` (다음 task용 placeholder도 추가).

- [ ] **Step 3:** `main.rs` 변경 — *workspace 흐름 완전 교체*:

기존:
```rust
let root = workspace::resolve()?;
workspace::ensure_exists(&root)?;
// ...
let mut root_folder = std_types::folder(...);
let scan_result = scan::scan_tree(&owner, &root)?;
```

새:
```rust
let drive_paths = drives::list_drives();
println!("[desktop-shell] {} 드라이브 mount", drive_paths.len());

// Desktop = [FileTree, Explorer, Cli, Window*]
let mut desktop = std_types::desktop(owner.clone());
let mut file_tree = std_types::file_tree(owner.clone(), "/"); // root_path는 절대경로 없음 (multi-root)
let mut explorer = std_types::explorer(owner.clone());
let mut cli = std_types::cli(owner.clone());
file_tree.parent = Some(desktop.id);
explorer.parent = Some(desktop.id);
cli.parent = Some(desktop.id);

// 드라이브 Folder mount
let now_ms = chrono::Utc::now().timestamp_millis();
let mut drive_folders: Vec<Object> = drive_paths
    .iter()
    .map(|p| {
        let mut f = std_types::folder(
            owner.clone(),
            p.to_string_lossy().as_ref(),
            p.to_string_lossy().as_ref(),
            now_ms,
        );
        f.parent = Some(file_tree.id);
        f
    })
    .collect();
file_tree.children = drive_folders.iter().map(|f| f.id).collect();
desktop.children = vec![file_tree.id, explorer.id, cli.id];

for o in [&mut desktop, &mut file_tree, &mut explorer, &mut cli] {
    add_wildcard_acl(o);
}
for f in &mut drive_folders {
    add_wildcard_acl(f);
}

let file_tree_id = file_tree.id;
let explorer_id = explorer.id;
let cli_id = cli.id;
let desktop_id = desktop.id;

let mut all_objects: Vec<Object> = vec![desktop.clone(), file_tree.clone(), explorer.clone(), cli.clone()];
all_objects.extend(drive_folders);
```

기존 `scan::scan_tree` 호출 + `root_folder` 분기는 모두 제거. Mount loop 그대로 (모든 객체 순회).

Subscribe 대상: FileTree / Explorer / Cli / 모든 Folder + (런타임에 추가되는 File / Window). 초기:
```rust
let mut subscribe_targets: Vec<ObjectId> = vec![file_tree_id, explorer_id, cli_id, desktop_id];
for obj in &all_objects {
    if obj.type_uri.as_str() == "aios.std/Folder@1" {
        subscribe_targets.push(obj.id);
    }
}
```

Desktop도 subscribe — Window mount/close 시 자식 변경 추적용.

- [ ] **Step 4:** Invoke 처리 — `match method` 분기 *전면 재구성*:

기존 `create_file` / `write` / `delete` arm *제거*. 기존 `expand` / `collapse` / `select` 유지 + `expand`에서 lazy mount 추가. 신규 `navigate_to` / `open_file` / `move` / `resize` / `focus` / `close` arm. `submit_input` / `clear` / `append_line` (CLI, T7.5) 유지.

`expand` arm 변경 핵심:
```rust
"expand" => {
    let fid_str = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
    match parse_object_id(fid_str) {
        Some(fid) => {
            // Lazy mount: 폴더의 children이 비어있으면 expand_folder 호출.
            if explorer_ops::needs_expand(&mounted_objects, fid) {
                if let Some(folder_path) = lookup_folder_path(&mounted_objects, fid) {
                    let now = chrono::Utc::now().timestamp_millis();
                    match lazy_mount::expand_folder(&owner, &folder_path, now) {
                        Ok(children) => {
                            let mut child_ids = Vec::new();
                            for mut child in children {
                                child.parent = Some(fid);
                                add_wildcard_acl(&mut child);
                                let child_id = child.id;
                                child_ids.push(child_id);
                                // mount
                                let mm = MountMsg { root_object_id: child_id.to_string(), tree: serde_json::to_value(&child)? };
                                stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
                                // subscribe (Folder만; File은 클릭 시점에)
                                if child.type_uri.as_str() == "aios.std/Folder@1" {
                                    req_seq += 1;
                                    let sub = SubscribeMsg {
                                        subscription_id: format!("sub-runtime-{}", req_seq),
                                        target: child_id.to_string(),
                                        kinds: vec![EventKindFilterWire::Invoke],
                                        include_initial: false,
                                    };
                                    stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
                                }
                                mounted_objects.push(child);
                            }
                            // 부모 Folder.children 갱신
                            if let Some(parent) = mounted_objects.iter_mut().find(|o| o.id == fid) {
                                parent.children = child_ids;
                            }
                        }
                        Err(e) => eprintln!("[desktop-shell] expand_folder 실패 {}: {}", fid, e),
                    }
                }
            }
            // 그 다음 기존 expanded 트래킹.
            let outcome = invoke_handler::handle_file_tree_expand(target_id, &tracked_expanded, fid);
            if !tracked_expanded.contains(&fid) {
                tracked_expanded.push(fid);
            }
            outcome
        }
        None => invoke_handler::InvokeOutcome::empty(),
    }
}
```

`navigate_to` arm:
```rust
"navigate_to" => {
    let fid_str = args.get("folder_id").and_then(|v| v.as_str()).unwrap_or("");
    match parse_object_id(fid_str) {
        Some(fid) => {
            // 같이 lazy expand
            if explorer_ops::needs_expand(&mounted_objects, fid) {
                // ... (위 expand와 동일 로직 — 헬퍼로 추출 권장)
            }
            explorer_ops::handle_navigate_to(target_id, fid)
        }
        None => invoke_handler::InvokeOutcome::empty(),
    }
}
```

`select` / `set_file` arm은 *제거* (T7 Canvas active_file 모델 — M8에선 사용 안 함). `open_file` / `move` / `resize` / `focus` / `close` arm은 T8.7~T8.10에서 추가.

- [ ] **Step 5:** `cargo build -p geulos-desktop-shell` 통과. `cargo test --all` 회귀 X.

- [ ] **Step 6:** Commit.
   ```bash
   git add apps/desktop-shell/src/explorer_ops.rs apps/desktop-shell/src/lib.rs apps/desktop-shell/src/main.rs
   git commit -m "feat(desktop-shell): M8 T8.5 — 드라이브 자동 mount + Explorer navigate_to + expand lazy mount"
   ```

### 디자인 결정
- `workspace.rs` / `scan.rs` 함수는 *호출 안 함*. 파일 자체는 dead — T8.12 cleanup에서 `#[allow(dead_code)]` 또는 삭제 결정.
- File에는 *초기 subscribe 안 함* — open_file 시점에 subscribe (file lifecycle은 Window 통해서).

---

## Task T8.6 — compositor: Explorer 클릭 dispatch (navigate / open_file)

**Estimated:** 2일

**Files:**
- Modify: `compositor/src/main.rs::dispatch_click`
- Modify: `compositor/src/hit_test.rs`

### 단계

- [ ] **Step 1:** `dispatch_click` 변경 — type_uri 분기에 Explorer 영역 처리 추가. Explorer는 자체 자식이 없으므로 hit_test가 *active_folder의 자식*을 Explorer rect 안 line으로 hit해야 함.

근데 Explorer는 layout 결과에 *자체 rect만* 있고 children rect는 없음. 두 가지 선택:
  - (i) layout이 Explorer 안에 children rect도 추가 (자식 객체 ID + 위치)
  - (ii) hit_test가 Explorer rect 안 클릭이면 y 좌표로 어떤 자식인지 계산

(i)이 더 일관 — layout 한 곳에서 결정. (i) 채택.

- [ ] **Step 2:** `layout.rs::render_explorer_list` 영역 layout에 *자식 rect들도* 추가. 

```rust
if let Some(ex) = find_child_by_type(tree, obj, "aios.builtin/Explorer@1") {
    let ex_rect = Rect { x: left_w, y: 0, w: right_w, h: top_h };
    out.push((ex.id, ex_rect));
    // 자식 list: active_folder의 children을 24px height로
    let children = explorer_children(tree, ex);
    let mut y = 4i32;
    for child_id in children {
        out.push((child_id, Rect { x: left_w + 4, y, w: right_w - 8, h: 24 }));
        y += 24;
        if y > top_h { break; }
    }
}
```

`explorer_children` 헬퍼:
```rust
fn explorer_children(tree: &TreeModel, ex: &Object) -> Vec<ObjectId> {
    let active = ex.state.get("active_folder").and_then(|v| v.as_str());
    let target = match active {
        Some(s) if !s.is_empty() => {
            uuid::Uuid::parse_str(s).ok().map(ObjectId::from_uuid)
        }
        _ => {
            // None → FileTree.children (드라이브 일람)
            tree.ids().into_iter().find_map(|id| {
                tree.get(id).filter(|o| o.type_uri.as_str() == "aios.builtin/FileTree@1").map(|o| o.id)
            })
        }
    };
    let folder_id = match target { Some(id) => id, None => return vec![] };
    let folder = match tree.get(folder_id) { Some(o) => o, None => return vec![] };
    let mut kids: Vec<ObjectId> = folder.children.clone();
    // 정렬: 폴더 먼저 (이름순), 파일 뒤 (이름순)
    kids.sort_by_key(|id| {
        tree.get(*id).map(|o| {
            let is_folder = o.type_uri.as_str() == "aios.std/Folder@1";
            let name = o.props.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            (!is_folder, name)
        }).unwrap_or((true, String::new()))
    });
    kids
}
```

- [ ] **Step 3:** `dispatch_click`에 Explorer 자식 분기:
```rust
"aios.std/Folder@1" => {
    // FileTree 트리 자식인 경우: 기존 expand/collapse + Explorer.navigate_to
    // Explorer list 자식인 경우: Explorer.navigate_to만
    // 구분: hit한 rect의 x가 left_w 이상이면 Explorer 영역.
    // 더 정확히는 layout이 어디서 push했는지지만, rect 위치로 판정 OK.
    let mut actions = Vec::new();
    if let Some(explorer) = find_explorer(tree) {
        actions.push(UiAction::Invoke {
            target: explorer.id,
            method: "navigate_to".to_string(),
            args: serde_json::json!({ "folder_id": target.to_string() }),
        });
    }
    // 좌측 트리 자식이면 expand/collapse도. layout 결과로 rect 위치 확인.
    if let Some(rect) = layout.get(target) {
        let ft_threshold = (window_width as f32 * 0.25) as i32;
        if rect.x < ft_threshold {
            // FileTree 영역 — expand/collapse 토글
            if let Some(ft) = find_file_tree(tree) {
                let is_expanded = ft
                    .state
                    .get("expanded")
                    .and_then(|v| v.as_array())
                    .is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some(&target.to_string())));
                actions.push(UiAction::Invoke {
                    target: ft.id,
                    method: if is_expanded { "collapse" } else { "expand" }.to_string(),
                    args: serde_json::json!({ "id": target.to_string() }),
                });
            }
        }
    }
    actions
}
"aios.std/File@1" => {
    // M8: 클릭 시 Explorer.open_file (Window mount). 기존 select/set_file 제거.
    if let Some(explorer) = find_explorer(tree) {
        vec![UiAction::Invoke {
            target: explorer.id,
            method: "open_file".to_string(),
            args: serde_json::json!({ "file_id": target.to_string() }),
        }]
    } else {
        vec![]
    }
}
```

`current_rect_of`는 dispatch_click에서 layout 결과를 매개로 받게 시그니처 확장 필요.

- [ ] **Step 4:** `hit_test.rs` — Window 영역은 z 역순 우선 (위에 있는 윈도우가 먼저 hit). 기존 hit_test는 layout iteration 순서 마지막부터. layout이 z 오름차순으로 push했으므로 hit_test도 역순 iterate → 자연스럽게 위 윈도우 우선.

```rust
pub fn hit_test(tree: &TreeModel, layout: &LayoutResult, x: i32, y: i32) -> Option<ObjectId> {
    // 역순: 마지막에 push된 (가장 위) 객체부터.
    for (id, rect) in layout.iter().rev() {
        if rect.contains(x, y) {
            if let Some(obj) = tree.get(id) {
                let uri = obj.type_uri.as_str();
                // 컨테이너성 타입은 hit 무시 (자식이 진짜 target)
                if matches!(uri, "aios.builtin/Desktop@1" | "aios.builtin/FileTree@1") {
                    continue;
                }
            }
            return Some(id);
        }
    }
    None
}
```

Explorer / Cli는 hit 대상 (각각 자체 클릭 동작 가능 — 단, M8은 Explorer 자식만 클릭 의미). 컨테이너 skip list에 Explorer 포함 검토 — 그러나 Explorer 빈 영역 클릭 = no-op 으로 두는 게 단순. 일단 *Desktop / FileTree만 skip*.

- [ ] **Step 5:** `cargo test -p geulos-compositor` → 모든 테스트 통과 + 신규 hit_test 회귀 X.

- [ ] **Step 6:** Commit.
   ```bash
   git add compositor/src/main.rs compositor/src/layout.rs compositor/src/hit_test.rs
   git commit -m "feat(compositor): M8 T8.6 — Explorer 자식 layout + 클릭 dispatch (navigate_to / open_file)"
   ```

### 디자인 결정
- Explorer 자식 line은 layout이 결정 — render는 layout 결과 따라 그리기만. 클릭과 렌더가 같은 좌표.
- FileTree 트리 클릭 = expand/collapse + navigate_to *둘 다* (사용자가 폴더 클릭하면 트리도 펼치고 우측도 navigate).

---

## Task T8.7 — desktop-shell: open_file 핸들러 (Window mount + 중복 검출)

**Estimated:** 2일

**Files:**
- Create: `apps/desktop-shell/src/window_ops.rs`
- Modify: `apps/desktop-shell/src/main.rs`

### 단계

- [ ] **Step 1:** `window_ops.rs` 신규:

```rust
//! Window 객체 lifecycle 헬퍼 (M8).

use geulos_core::{std_types, ActorId, Object, ObjectId};

/// 같은 파일을 이미 열어둔 Window가 있으면 그 ID. 없으면 None.
pub fn find_window_for_file(mounted_objects: &[Object], file_id: ObjectId) -> Option<ObjectId> {
    mounted_objects.iter().find(|o| {
        o.type_uri.as_str() == "aios.builtin/Window@1"
            && o.props.get("file_id").and_then(|v| v.as_str())
                == Some(file_id.to_string().as_str())
    }).map(|o| o.id)
}

/// 현재 mounted Window들 중 최대 z. 없으면 0.
pub fn max_z(mounted_objects: &[Object]) -> i32 {
    mounted_objects
        .iter()
        .filter(|o| o.type_uri.as_str() == "aios.builtin/Window@1")
        .filter_map(|o| o.state.get("z").and_then(|v| v.as_i64()))
        .max()
        .map(|z| z as i32)
        .unwrap_or(0)
}

/// Cascade 위치 — 마지막 Window의 위치 + (30, 30). 첫 Window는 default.
pub fn next_window_position(mounted_objects: &[Object], default: (i32, i32)) -> (i32, i32) {
    let last = mounted_objects
        .iter()
        .filter(|o| o.type_uri.as_str() == "aios.builtin/Window@1")
        .last();
    match last {
        Some(w) => {
            let x = w.state.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32 + 30;
            let y = w.state.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32 + 30;
            (x, y)
        }
        None => default,
    }
}

/// 새 Window 객체 (Desktop 자식, focused, max z + 1).
pub fn build_new_window(
    owner: &ActorId,
    desktop_id: ObjectId,
    file_id: ObjectId,
    title: &str,
    pos: (i32, i32),
    size: (i32, i32),
    new_z: i32,
) -> Object {
    let mut w = std_types::window(owner.clone(), title, file_id, pos.0, pos.1, size.0, size.1);
    w.parent = Some(desktop_id);
    w.set_state("z", serde_json::json!(new_z));
    w.set_state("focused", serde_json::json!(true));
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn find_window_for_file_matches_existing() {
        let owner = ActorId::local_user();
        let fid = ObjectId::new();
        let w = std_types::window(owner, "x", fid, 0, 0, 100, 100);
        let mounted = vec![w.clone()];
        assert_eq!(find_window_for_file(&mounted, fid), Some(w.id));
    }

    #[test]
    fn max_z_returns_zero_when_empty() {
        assert_eq!(max_z(&[]), 0);
    }

    #[test]
    fn max_z_finds_highest() {
        let owner = ActorId::local_user();
        let fid = ObjectId::new();
        let mut w1 = std_types::window(owner.clone(), "a", fid, 0, 0, 1, 1);
        w1.set_state("z", json!(3));
        let mut w2 = std_types::window(owner.clone(), "b", fid, 0, 0, 1, 1);
        w2.set_state("z", json!(7));
        assert_eq!(max_z(&[w1, w2]), 7);
    }

    #[test]
    fn next_position_cascades_30_30() {
        let owner = ActorId::local_user();
        let fid = ObjectId::new();
        let w = std_types::window(owner, "a", fid, 100, 80, 1, 1);
        assert_eq!(next_window_position(&[w], (50, 40)), (130, 110));
    }

    #[test]
    fn next_position_uses_default_when_empty() {
        assert_eq!(next_window_position(&[], (50, 40)), (50, 40));
    }
}
```

- [ ] **Step 2:** `lib.rs`에 `pub mod window_ops;` 확인. `cargo test -p geulos-desktop-shell window_ops` → 5건 pass.

- [ ] **Step 3:** `main.rs`의 invoke loop에 `open_file` arm 추가 (Explorer target):

```rust
"open_file" => {
    let fid_str = args.get("file_id").and_then(|v| v.as_str()).unwrap_or("");
    match parse_object_id(fid_str) {
        Some(file_id) => {
            // 중복 검출
            if let Some(existing_window_id) = window_ops::find_window_for_file(&mounted_objects, file_id) {
                // focus만
                let new_z = window_ops::max_z(&mounted_objects) + 1;
                // 다른 윈도우 focused=false, 이건 true + z=new_z
                let mut outs = vec![];
                for o in &mut mounted_objects {
                    if o.type_uri.as_str() == "aios.builtin/Window@1" {
                        let is_target = o.id == existing_window_id;
                        o.state.insert("focused".into(), json!(is_target));
                        outs.push((o.id, "focused".to_string(), json!(is_target)));
                        if is_target {
                            o.state.insert("z".into(), json!(new_z));
                            outs.push((o.id, "z".to_string(), json!(new_z)));
                        }
                    }
                }
                invoke_handler::InvokeOutcome { state_sets: outs }
            } else {
                // 새 Window mount
                let file_obj = mounted_objects.iter().find(|o| o.id == file_id);
                let title = file_obj
                    .and_then(|f| f.props.get("name").and_then(|v| v.as_str()))
                    .unwrap_or("(파일)")
                    .to_string();
                let pos = window_ops::next_window_position(&mounted_objects, (300, 200));
                let new_z = window_ops::max_z(&mounted_objects) + 1;
                let mut new_window = window_ops::build_new_window(
                    &owner, desktop_id, file_id, &title, pos, (600, 400), new_z,
                );
                add_wildcard_acl(&mut new_window);
                let new_id = new_window.id;
                // 다른 윈도우 focused=false
                let mut outs = vec![];
                for o in &mut mounted_objects {
                    if o.type_uri.as_str() == "aios.builtin/Window@1" {
                        o.state.insert("focused".into(), json!(false));
                        outs.push((o.id, "focused".to_string(), json!(false)));
                    }
                }
                // Window mount + Desktop.children 갱신
                let mm = MountMsg { root_object_id: new_id.to_string(), tree: serde_json::to_value(&new_window)? };
                stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
                // subscribe (move/resize/focus/close)
                req_seq += 1;
                let sub = SubscribeMsg {
                    subscription_id: format!("sub-runtime-{}", req_seq),
                    target: new_id.to_string(),
                    kinds: vec![EventKindFilterWire::Invoke],
                    include_initial: false,
                };
                stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
                // mounted_objects + desktop.children 갱신
                if let Some(d) = mounted_objects.iter_mut().find(|o| o.id == desktop_id) {
                    d.children.push(new_id);
                }
                mounted_objects.push(new_window);
                invoke_handler::InvokeOutcome { state_sets: outs }
            }
        }
        None => invoke_handler::InvokeOutcome::empty(),
    }
}
```

- [ ] **Step 4:** `cargo build -p geulos-desktop-shell` 통과. `cargo test --all` 회귀 X.

- [ ] **Step 5:** **중간 acceptance 체크포인트** (spec §13 제안). controller가 직접 시각 확인:
  - server-host + desktop-shell + compositor 3 띄움
  - 좌측에 모든 드라이브 root 보임
  - 드라이브 expand → 직계 자식
  - 폴더 클릭 → 우측 Explorer가 그 폴더 내용 보여줌
  - 파일 클릭 → Window가 떠야 함 (렌더는 T8.8에서 — 일단 빈 사각형이라도 layout에 등장)
  - 같은 파일 두 번 → 새 윈도우 안 생김, 기존 focused로

- [ ] **Step 6:** Commit.
   ```bash
   git add apps/desktop-shell/src/window_ops.rs apps/desktop-shell/src/lib.rs apps/desktop-shell/src/main.rs
   git commit -m "feat(desktop-shell): M8 T8.7 — open_file → Window mount + 중복 검출 + focus + z 최상위"
   ```

### 디자인 결정
- focused/z 동기 갱신은 한 invoke에서 — 모든 윈도우 state_sets 한 번에 emit. 컴포지터는 batch로 받음.
- Window subscribe 시점 = mount 직후. close 시 *unsubscribe*는 명시 안 함 — emit_destroyed가 tombstone (KI-011) 처리.

---

## Task T8.8 — compositor: Window 오버레이 렌더 (title bar + content + [x] + resize handle)

**Estimated:** 2~3일

**Files:**
- Modify: `compositor/src/render.rs`

### 단계

- [ ] **Step 1:** `render.rs`에 색상 상수 추가 + `render_window` 함수:

```rust
const COLOR_WINDOW_BG: u32 = 0xFF_FA_FA_FA;
const COLOR_WINDOW_BORDER: u32 = 0xFF_99_99_99;
const COLOR_WINDOW_TITLE_BG: u32 = 0xFF_42_75_E0;
const COLOR_WINDOW_TITLE_BG_FOCUSED: u32 = 0xFF_22_55_C0;
const COLOR_WINDOW_TITLE_TEXT: u32 = 0xFF_FF_FF_FF;
const COLOR_WINDOW_CLOSE: u32 = 0xFF_E5_3E_3E;
const COLOR_WINDOW_RESIZE_HANDLE: u32 = 0xFF_CC_CC_CC;
const WINDOW_TITLE_H: i32 = 24;
const WINDOW_RESIZE_HANDLE: i32 = 10;
const WINDOW_CLOSE_BTN: i32 = 16;
```

`match` arm에 추가:
```rust
"aios.builtin/Window@1" => {
    let focused = obj.state.get("focused").and_then(|v| v.as_bool()).unwrap_or(false);
    render_window(buffer, width, height, &rect, tree, obj, focused);
}
```

`render_window` 헬퍼:
```rust
fn render_window(
    buffer: &mut [u32],
    w: usize,
    h: usize,
    rect: &Rect,
    tree: &TreeModel,
    obj: &geulos_core::Object,
    focused: bool,
) {
    // 외곽 border (1px)
    fill_rect(buffer, w, h, rect, COLOR_WINDOW_BORDER);
    let inner = Rect { x: rect.x + 1, y: rect.y + 1, w: rect.w - 2, h: rect.h - 2 };
    fill_rect(buffer, w, h, &inner, COLOR_WINDOW_BG);

    // Title bar
    let title_rect = Rect { x: inner.x, y: inner.y, w: inner.w, h: WINDOW_TITLE_H };
    let title_bg = if focused { COLOR_WINDOW_TITLE_BG_FOCUSED } else { COLOR_WINDOW_TITLE_BG };
    fill_rect(buffer, w, h, &title_rect, title_bg);
    let title = obj.props.get("title").and_then(|v| v.as_str()).unwrap_or("(window)");
    draw_text(buffer, w, h, title, title_rect.x + 8, title_rect.y + 4, COLOR_WINDOW_TITLE_TEXT);

    // [x] 닫기 버튼 (title bar 우상단 16×16)
    let close_rect = Rect {
        x: title_rect.x + title_rect.w - WINDOW_CLOSE_BTN - 4,
        y: title_rect.y + 4,
        w: WINDOW_CLOSE_BTN,
        h: WINDOW_CLOSE_BTN,
    };
    fill_rect(buffer, w, h, &close_rect, COLOR_WINDOW_CLOSE);
    draw_text(buffer, w, h, "x", close_rect.x + 4, close_rect.y, COLOR_WINDOW_TITLE_TEXT);

    // Content (title bar 아래)
    let content_rect = Rect {
        x: inner.x + 8,
        y: inner.y + WINDOW_TITLE_H + 8,
        w: inner.w - 16,
        h: inner.h - WINDOW_TITLE_H - 16,
    };
    // file_id로 File 객체 lookup → preview 출력 (M8은 props.preview 텍스트, 첫 줄들)
    if let Some(file_id_str) = obj.props.get("file_id").and_then(|v| v.as_str()) {
        if let Ok(uuid) = uuid::Uuid::parse_str(file_id_str) {
            let file_id = geulos_core::ObjectId::from_uuid(uuid);
            if let Some(file) = tree.get(file_id) {
                let preview = file.state.get("preview").and_then(|v| v.as_str()).unwrap_or("");
                let mut y = content_rect.y;
                for line in preview.lines().take((content_rect.h / 20) as usize) {
                    if y + 16 > content_rect.y + content_rect.h { break; }
                    draw_text(buffer, w, h, line, content_rect.x, y, COLOR_TEXT);
                    y += 20;
                }
                if preview.is_empty() {
                    draw_text(buffer, w, h, "(미리보기 없음)", content_rect.x, content_rect.y, COLOR_PLACEHOLDER);
                }
            }
        }
    }

    // Resize handle (우하 10×10)
    let resize_rect = Rect {
        x: inner.x + inner.w - WINDOW_RESIZE_HANDLE,
        y: inner.y + inner.h - WINDOW_RESIZE_HANDLE,
        w: WINDOW_RESIZE_HANDLE,
        h: WINDOW_RESIZE_HANDLE,
    };
    fill_rect(buffer, w, h, &resize_rect, COLOR_WINDOW_RESIZE_HANDLE);
}
```

- [ ] **Step 2:** `cargo build` 통과. 단위 테스트는 추가 어려움 (시각 렌더) — 다음 step에서 수동 시연.

- [ ] **Step 3:** **시각 확인** — 컴포지터 띄워 파일 클릭 시 윈도우 등장. title bar / 본문 / [x] / resize handle 모두 보이는지.

- [ ] **Step 4:** Commit.
   ```bash
   git add compositor/src/render.rs
   git commit -m "feat(compositor): M8 T8.8 — Window 오버레이 렌더 (title bar + 본문 preview + [x] + resize handle)"
   ```

### 디자인 결정
- preview는 File 객체의 `state.preview` (T7 모델 — 첫 512바이트). M9에서 *full read*.
- title bar 색 = focused/unfocused 구분으로 사용자 hint.

---

## Task T8.9 — compositor: 마우스 입력 — focus + drag move + drag resize

**Estimated:** 3일

**Files:**
- Modify: `compositor/src/main.rs`
- Modify: `compositor/src/hit_test.rs`

### 단계

- [ ] **Step 0:** `compositor/src/render.rs`에서 main.rs가 쓸 상수들을 `pub` export — `WINDOW_TITLE_H`, `WINDOW_RESIZE_HANDLE`, `WINDOW_CLOSE_BTN`. 또는 `compositor/src/window_geom.rs` 신규 module로 추출. **추천: window_geom.rs 신규 module** — render와 main이 같은 상수 공유, 새 module은 5줄짜리 단순 const 모음.

```rust
// compositor/src/window_geom.rs (신규)
//! Window 오버레이 영역 상수 — render와 입력 처리가 공유.

pub const WINDOW_TITLE_H: i32 = 24;
pub const WINDOW_RESIZE_HANDLE: i32 = 10;
pub const WINDOW_CLOSE_BTN: i32 = 16;
pub const WINDOW_MIN_W: i32 = 200;
pub const WINDOW_MIN_H: i32 = 120;
```

`lib.rs`에 `pub mod window_geom;`. render.rs는 이걸 use. main.rs도 use.

- [ ] **Step 1:** `App` 구조체에 drag state 추가:

```rust
enum DragState {
    None,
    MovingWindow { window_id: ObjectId, start_cursor: (i32, i32), start_pos: (i32, i32) },
    ResizingWindow { window_id: ObjectId, start_cursor: (i32, i32), start_size: (i32, i32) },
}

struct App {
    // ... 기존 필드
    drag: DragState,
    keyboard_focus: KeyboardFocus,
}

enum KeyboardFocus {
    Cli,
    Window(ObjectId),
    None,
}
```

- [ ] **Step 2:** Mouse press 처리 — hit한 객체 타입별 분기 + 영역 세부 (title bar / close btn / resize handle / content):

```rust
WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
    let (cx, cy) = (self.cursor.0 as i32, self.cursor.1 as i32);
    let size = self.window.as_ref().unwrap().inner_size();
    let tree = self.tree.lock().unwrap();
    let lay = layout(&tree, size.width as i32, size.height as i32);
    if let Some(target) = hit_test(&tree, &lay, cx, cy) {
        if let Some(obj) = tree.get(target) {
            if obj.type_uri.as_str() == "aios.builtin/Window@1" {
                // 영역 분석
                let win_rect = lay.get(target).unwrap();
                let title_rect = Rect { x: win_rect.x + 1, y: win_rect.y + 1, w: win_rect.w - 2, h: WINDOW_TITLE_H };
                let close_rect = Rect {
                    x: title_rect.x + title_rect.w - WINDOW_CLOSE_BTN - 4,
                    y: title_rect.y + 4,
                    w: WINDOW_CLOSE_BTN,
                    h: WINDOW_CLOSE_BTN,
                };
                let resize_rect = Rect {
                    x: win_rect.x + win_rect.w - WINDOW_RESIZE_HANDLE - 1,
                    y: win_rect.y + win_rect.h - WINDOW_RESIZE_HANDLE - 1,
                    w: WINDOW_RESIZE_HANDLE,
                    h: WINDOW_RESIZE_HANDLE,
                };
                if close_rect.contains(cx, cy) {
                    let _ = self.ui_tx.try_send(UiAction::Invoke {
                        target,
                        method: "close".to_string(),
                        args: serde_json::Value::Null,
                    });
                } else if title_rect.contains(cx, cy) {
                    let start_pos = (
                        obj.state.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                        obj.state.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                    );
                    self.drag = DragState::MovingWindow {
                        window_id: target,
                        start_cursor: (cx, cy),
                        start_pos,
                    };
                    // focus 함께
                    let _ = self.ui_tx.try_send(UiAction::Invoke {
                        target,
                        method: "focus".to_string(),
                        args: serde_json::Value::Null,
                    });
                    self.keyboard_focus = KeyboardFocus::Window(target);
                } else if resize_rect.contains(cx, cy) {
                    let start_size = (
                        obj.state.get("w").and_then(|v| v.as_i64()).unwrap_or(600) as i32,
                        obj.state.get("h").and_then(|v| v.as_i64()).unwrap_or(400) as i32,
                    );
                    self.drag = DragState::ResizingWindow {
                        window_id: target,
                        start_cursor: (cx, cy),
                        start_size,
                    };
                } else {
                    // 본문 클릭 → focus only
                    let _ = self.ui_tx.try_send(UiAction::Invoke {
                        target,
                        method: "focus".to_string(),
                        args: serde_json::Value::Null,
                    });
                    self.keyboard_focus = KeyboardFocus::Window(target);
                }
            } else if obj.type_uri.as_str() == "aios.builtin/Cli@1" {
                self.keyboard_focus = KeyboardFocus::Cli;
                drop(tree);
                // 기존 CLI 클릭 동작 (필요시)
            } else {
                // 일반 dispatch_click
                drop(tree);
                let tree2 = self.tree.lock().unwrap();
                let actions = dispatch_click(&tree2, &lay, target, obj, size.width as i32);
                for a in actions { let _ = self.ui_tx.try_send(a); }
            }
        }
    } else {
        // 빈 영역 — focus 해제
        self.keyboard_focus = KeyboardFocus::None;
    }
}
```

- [ ] **Step 3:** `CursorMoved`에 drag 처리 추가:

```rust
WindowEvent::CursorMoved { position, .. } => {
    self.cursor = (position.x, position.y);
    match self.drag {
        DragState::MovingWindow { .. } | DragState::ResizingWindow { .. } => {
            // 컴포지터 local position을 화면에 반영하려면 별 state 필요.
            // M8 v1: drag 중에는 redraw 안 함 — drag end 시 invoke만.
            // → 살짝 lag. UX 개선은 v2.
        }
        DragState::None => {}
    }
}
```

(주의: drag 중 시각 피드백 없음 = 사용자가 *어디로 이동 중인지* 안 보임. 단순성 우선, 후속 개선.)

- [ ] **Step 4:** `MouseInput Released`에 drag end 처리:

```rust
WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
    let (cx, cy) = (self.cursor.0 as i32, self.cursor.1 as i32);
    match self.drag {
        DragState::MovingWindow { window_id, start_cursor, start_pos } => {
            let dx = cx - start_cursor.0;
            let dy = cy - start_cursor.1;
            let new_x = start_pos.0 + dx;
            let new_y = start_pos.1 + dy;
            let _ = self.ui_tx.try_send(UiAction::Invoke {
                target: window_id,
                method: "move".to_string(),
                args: serde_json::json!({ "x": new_x, "y": new_y }),
            });
        }
        DragState::ResizingWindow { window_id, start_cursor, start_size } => {
            let dw = cx - start_cursor.0;
            let dh = cy - start_cursor.1;
            let new_w = (start_size.0 + dw).max(200);
            let new_h = (start_size.1 + dh).max(120);
            let _ = self.ui_tx.try_send(UiAction::Invoke {
                target: window_id,
                method: "resize".to_string(),
                args: serde_json::json!({ "w": new_w, "h": new_h }),
            });
        }
        DragState::None => {}
    }
    self.drag = DragState::None;
}
```

- [ ] **Step 5:** Keyboard input 라우팅 갱신 — `keyboard_focus` 기반:

```rust
WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
    match self.keyboard_focus {
        KeyboardFocus::Cli => {
            // 기존 T7.5 동작 (cli_state.handle_key)
        }
        KeyboardFocus::Window(_) => {
            // M8: read-only — 키 입력 무시. v2에서 Ctrl+W 등.
        }
        KeyboardFocus::None => {
            // 무시
        }
    }
}
```

- [ ] **Step 6:** `cargo build -p geulos-compositor` 통과 + `cargo test --all` 회귀 X.

- [ ] **Step 7:** **시각 확인** — title bar drag로 윈도우 이동, [x] 클릭으로 close invoke 발신 (실제 close는 T8.10에서 처리), 우하 코너 drag로 resize.

- [ ] **Step 8:** Commit.
   ```bash
   git add compositor/src/main.rs compositor/src/hit_test.rs
   git commit -m "feat(compositor): M8 T8.9 — Window 마우스 입력 (focus + title drag move + corner drag resize + [x] close)"
   ```

### 디자인 결정
- Drag 중 시각 피드백 없음 = trade-off. drop 시점에 한 번 invoke. v2에 *local preview rect* 오버레이.
- min size = 200×120 (제목 + [x] + 최소 한 줄 텍스트가 보일 정도).

---

## Task T8.10 — desktop-shell: move / resize / focus / close 핸들러 + z-order

**Estimated:** 2일

**Files:**
- Modify: `apps/desktop-shell/src/main.rs`

### 단계

- [ ] **Step 1:** `main.rs` invoke loop에 4개 arm 추가:

```rust
"move" => {
    let x = args.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let y = args.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    if let Some(w) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
        w.state.insert("x".into(), json!(x));
        w.state.insert("y".into(), json!(y));
    }
    invoke_handler::InvokeOutcome {
        state_sets: vec![
            (target_id, "x".to_string(), json!(x)),
            (target_id, "y".to_string(), json!(y)),
        ],
    }
}
"resize" => {
    let w_val = (args.get("w").and_then(|v| v.as_i64()).unwrap_or(600) as i32).max(200);
    let h_val = (args.get("h").and_then(|v| v.as_i64()).unwrap_or(400) as i32).max(120);
    if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
        o.state.insert("w".into(), json!(w_val));
        o.state.insert("h".into(), json!(h_val));
    }
    invoke_handler::InvokeOutcome {
        state_sets: vec![
            (target_id, "w".to_string(), json!(w_val)),
            (target_id, "h".to_string(), json!(h_val)),
        ],
    }
}
"focus" => {
    let new_z = window_ops::max_z(&mounted_objects) + 1;
    let mut outs = vec![];
    for o in &mut mounted_objects {
        if o.type_uri.as_str() == "aios.builtin/Window@1" {
            let is_target = o.id == target_id;
            o.state.insert("focused".into(), json!(is_target));
            outs.push((o.id, "focused".to_string(), json!(is_target)));
            if is_target {
                o.state.insert("z".into(), json!(new_z));
                outs.push((o.id, "z".to_string(), json!(new_z)));
            }
        }
    }
    invoke_handler::InvokeOutcome { state_sets: outs }
}
"close" => {
    // tombstone: emit_destroyed 와이어 메시지가 server-host에 있음.
    // KI-011 — destroyed flag로 처리.
    // M8 단순화: state에 destroyed=true SetState + Desktop.children에서 제거.
    // 정식 emit_destroyed는 별 proto 메시지 필요 — server-host 측 호환 확인 후.
    // 여기선 SetState로 시각적 close, mounted_objects에서도 제거.
    let close_id = target_id;
    mounted_objects.retain(|o| o.id != close_id);
    if let Some(d) = mounted_objects.iter_mut().find(|o| o.id == desktop_id) {
        d.children.retain(|c| *c != close_id);
    }
    // proto에 DestroyMsg 또는 동등 메시지가 있는지 확인 — 없으면 SetState로 destroyed=true.
    // T7의 KI-011 해소 commit (255e06e? 또는 이전) 참조 — Object.destroyed: bool 필드 사용.
    invoke_handler::InvokeOutcome {
        state_sets: vec![(close_id, "destroyed".to_string(), json!(true))],
    }
}
```

- [ ] **Step 2:** **확인 필요:** `proto`에 destroyed 시그널 정식 메시지가 있는지 + server-host가 `state.destroyed=true`를 받으면 query에서 제외하는지. KI-011 해소 commit 확인 (memory). 없으면 SetState 우회 + 컴포지터 render가 `destroyed=true` 객체 skip 추가.

- [ ] **Step 3:** `compositor/src/render.rs`와 `layout.rs`에서 `destroyed=true` 객체 skip 보장:
```rust
if obj.state.get("destroyed").and_then(|v| v.as_bool()).unwrap_or(false) {
    continue; // 트리에서 destroy된 객체는 렌더 X, layout X
}
```
T7에서 `query`가 이미 destroyed 제외하므로 컴포지터의 tree.upsert가 안 받음. 하지만 *Destroyed 이벤트 emit*가 필요 — KI-011 commit 확인 후 결정. (T7에 이미 처리됐을 수도. 확인 후 충분하면 step skip.)

- [ ] **Step 4:** `cargo build` + `cargo test --all` 통과.

- [ ] **Step 5:** **시각 확인** — Window drag로 이동, resize, [x] 누르면 사라짐, 같은 파일 두 번 = focus 이동.

- [ ] **Step 6:** Commit.
   ```bash
   git add apps/desktop-shell/src/main.rs compositor/src/render.rs compositor/src/layout.rs
   git commit -m "feat(desktop-shell): M8 T8.10 — Window move/resize/focus/close invoke 핸들러 + z 갱신"
   ```

### 디자인 결정
- close = destroyed flag SetState (KI-011 모델 재사용). 정식 DestroyMsg는 별 task로 분리.
- z 갱신 정책: focus 시 max+1. 모노톤 증가 — overflow는 i32라 사실상 안전.

---

## Task T8.11 — Acceptance + 도그푸딩

**Estimated:** 2~3일

**Files:**
- Create: `docs/manual-tests/m8-acceptance.md`
- Modify: `docs/known-issues.md` (KI-001/KI-002 부분 해소 메모)

### 단계

- [ ] **Step 1:** `m8-acceptance.md` 작성 — 다음 시나리오 cover:

```markdown
# M8 Acceptance Test

## 사전 조건
- Windows 11, D:\GeulOS 작업 디렉터리
- cargo build --release 완료
- 3개 PowerShell 창 (Ctrl+C로 종료)

## 시나리오 A — 드라이브 자동 mount + 탐색
1. `cargo run -p geulos-server-host`
2. `cargo run -p geulos-desktop-shell` — 로그에 "N 드라이브 mount"
3. `cargo run -p geulos-compositor`
4. **확인**: 좌측 트리에 `[+] C:\`, `[+] D:\` (시스템 드라이브 모두)
5. `[+] D:\` 클릭 → expand → D:\ 직계 자식 등장
6. `D:\GeulOS` 클릭 → 좌측 expand + 우측 Explorer가 GeulOS 내용 리스트로

## 시나리오 B — 멀티 윈도우 + drag/resize/close
7. 우측 Explorer에서 README.md (또는 임의 파일) 클릭 → Window 1 등장, focused
8. Cargo.toml 클릭 → Window 2 등장, focused (Window 1 unfocus, title bar 색 변화)
9. Window 1 title bar 클릭 + 드래그 → 위치 이동
10. Window 1 우하 코너 드래그 → 크기 변경
11. Window 2 [x] 클릭 → Window 2 사라짐
12. 다시 README.md 클릭 → 새 윈도우 안 생기고 Window 1 focus (중복 검출)

## 시나리오 C — Read-only 검증
13. CLI에 `help` 입력 (T7.5 보존)
14. (직접 invoke 테스트 — gsh 또는 별 client로 `invoke <file_id> write '{"content":"x"}'`) → MethodNotFound 응답

## 시나리오 D — AI 시연 (옵션)
15. ai-bridge 시나리오 09 (M8용 신규) 작성: "현재 데스크톱에 열린 윈도우 목록을 알려줘" — AI가 `query type aios.builtin/Window@1`로 응답
```

- [ ] **Step 2:** ai-bridge 시나리오 09 (옵션) `ai-bridge/scenarios/09_m8_explore.toml`:
```toml
goal = "M8 데스크톱 셸 — 윈도우/탐색기 객체 탐색"
model = "claude-sonnet-4-6"
turns_max = 3
user_prompt = "현재 GeulOS 데스크톱에 어떤 객체들이 열려있는지 query로 확인해줘. Window가 있으면 각 title도 알려줘."
```

- [ ] **Step 3:** `docs/known-issues.md` 갱신 — KI-001/KI-002 부분 해소 (메서드 자체가 부재라 wildcard ACL이 무의미):
```markdown
**M8 (2026-05-X) 영향:**
- KI-001 부분 완화: write 메서드들이 std_types::file/folder 팩토리에서 *부재* — wildcard ACL이 있어도 invoke 자체가 MethodNotFound. M9 권한 다이얼로그 + 메서드 복귀 시 wildcard ACL 정리 트리거.
- KI-002 무변: 매니페스트 permissions는 여전히 강제 안 됨.
```

- [ ] **Step 4:** 본 시나리오 A/B/C 모두 직접 수행 + 스크린샷/콘솔 로그를 m8-acceptance.md에 첨부.

- [ ] **Step 5:** Commit.
   ```bash
   git add docs/manual-tests/m8-acceptance.md docs/known-issues.md ai-bridge/scenarios/09_m8_explore.toml
   git commit -m "test(m8): T8.11 — acceptance + 도그푸딩 시나리오 + KI-001 부분 해소 메모"
   ```

### 디자인 결정
- ai-bridge 시나리오 09는 *옵션* — 안 만들어도 T8.11 통과. AI 가시성 확인 추가 점수.

---

## Task T8.12 — Final review (T8.0~T8.11 일괄)

**Estimated:** 2일

**Files:**
- Modify: 필요 시 dead code cleanup (workspace.rs, scan.rs, invoke_handler.rs 일부)

### 단계

- [ ] **Step 1:** controller가 `requesting-code-review` 스킬로 전체 diff (range: M7 T7.5 마지막 commit `f0c58f6` ~ T8.11 마지막) 리뷰 디스패치.

- [ ] **Step 2:** Reviewer 발견 사항 fix.

- [ ] **Step 3:** Dead code cleanup:
- `apps/desktop-shell/src/workspace.rs` — M8에서 호출 X. `#[allow(dead_code)]` 또는 *삭제 + git 히스토리에 남김*. 결정: M9 권한 다이얼로그 마일스톤에서 *workspace 격리 옵션*이 다시 필요할 수 있음 → 유지 + `#[allow(dead_code)]` + 모듈 doc에 "M9에서 사용자 워크스페이스 옵트인 시 재사용".
- `apps/desktop-shell/src/scan.rs` — lazy_mount로 대체. 같은 결정 (유지 + dead code).
- `apps/desktop-shell/src/fs_ops.rs` — write 함수들 dead. spec §8.4 따라 `#[allow(dead_code)]` + 주석.
- `apps/desktop-shell/src/invoke_handler.rs::handle_canvas_set_file` — Canvas 자체가 M8에 없음. 함수 dead. `#[allow(dead_code)]`.

- [ ] **Step 4:** `cargo fmt --all` + `cargo clippy --all-targets -- -D warnings` 클린.

- [ ] **Step 5:** `cargo test --all` 전체 통과 (M8 신규 + 회귀 X).

- [ ] **Step 6:** 컴포지터 시각 검증 (T8.11 시나리오 재실행) 통과.

- [ ] **Step 7:** Commit.
   ```bash
   git add -A
   git commit -m "chore(m8): T8.12 — Final review fixes + dead code 마킹 (workspace/scan/fs_ops)"
   ```

- [ ] **Step 8:** Controller가 사용자에게 M8 완료 보고. 사용자가 push 결정 시 `git push origin main`.

### 디자인 결정
- workspace.rs / scan.rs는 *삭제 안 함* — M9 옵션 워크스페이스로 재활용 가능성.
- M7 T7.6/T7.7 재개는 별 마일스톤 — M8 final review에 포함 X.

---

## 자체 점검 (plan)

### 스펙 커버리지

| Spec 섹션 | 커버 task |
|---|---|
| §4.1 Window@1 객체 | T8.1 |
| §4.2 Explorer@1 객체 | T8.1, T8.5 |
| §4.3 Folder/File 메서드 축소 | T8.2 |
| §5 Desktop 구조 | T8.4, T8.5 |
| §6.1 드라이브 자동 mount | T8.3, T8.5 |
| §6.2 Lazy expand | T8.3, T8.5 |
| §7 UX 클릭 시멘틱 | T8.6, T8.9 |
| §8 Read-only enforcement | T8.2, T8.5, T8.12 |
| §9 입력 라우팅 / focus | T8.9, T8.10 |
| §10 알려진 한계 / 후속 | T8.11 (KI 갱신) |
| §11 ADR 시드 | T8.0 |
| §13 위험 — Window 중간 acceptance | T8.7 Step 5 |

### Placeholder scan
- 모든 step에 actual code 또는 정확한 명령 + 파일 경로. TBD/TODO 없음.
- 예외: T8.10 close의 "DestroyMsg 정식 여부 확인 필요" — implementer가 *확인 후 진행*. 이건 placeholder 아니라 *결정 분기 명시*.

### Type 일관성
- `Window` props: title, file_id / state: x, y, w, h, z, focused / methods: move, resize, focus, close — T8.1 정의 후 T8.4 layout / T8.7 mount / T8.8 render / T8.9 입력 / T8.10 handler 모두 동일 시그니처.
- `Explorer` props: 없음 / state: active_folder, view_mode / methods: navigate_to(folder_id), open_file(file_id) — T8.1 정의 후 T8.5 mount / T8.6 클릭 / T8.7 open_file handler 모두 동일.

### Risk
- T8.10 close 처리가 `proto::DestroyMsg` 또는 동등 메시지에 의존 — Step 2가 *확인 필요*로 명시. implementer가 막히면 controller에 escalate.
- T8.7 중간 acceptance 체크포인트가 있어 큰 빌드(T8.8/T8.9/T8.10)가 시작 전 *Window 객체 흐름*이 작동함을 보장.
- Drag 중 시각 피드백 없음 — UX 약점 but M8 v1 trade-off (spec §13).

---

## 다음 단계

본 plan 완료 후:
1. controller가 `subagent-driven-development` 스킬로 T8.0부터 순차 implementer 디스패치
2. 각 task: implementer DONE → spec compliance review → quality review → next
3. T8.12 끝나면 controller가 사용자 push 동의 받고 마일스톤 push
4. M9 또는 M7 T7.6 재개 결정
