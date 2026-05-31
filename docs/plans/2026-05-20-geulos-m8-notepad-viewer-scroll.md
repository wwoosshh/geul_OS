> **Status:** completed (2026-05-20)
> **Note:** M8 part 2 정식 마감 — Window text viewer (1MB cap) + 세 영역 공통 scroll_y.

# GeulOS M8 part 2 — 메모장 viewer + 공통 스크롤 (Implementation Plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement task-by-task. Steps use checkbox (`- [ ]`) syntax. **NEVER push** — controller batches push at milestone end.

**Spec:** `docs/specs/2026-05-20-geulos-m8-notepad-viewer-scroll.md`

**Goal:** Window 본문에 *전체 텍스트 파일* (1MB cap) 표시 + Window·FileTree·Explorer 세 영역 *공통 라인 단위 스크롤*. v1 read-only (편집은 M9).

**Architecture:** Window가 type-aware text viewer (별 객체 X). `state.content`/`state.content_too_large`/`state.scroll_y` 추가. FileTree·Explorer는 `state.scroll_y` 추가. 컴포지터가 마우스 휠/PageUp/Down 받아 *직접 SetState* (invoke 우회). 가시 라인만 clip render.

**Tech Stack:** 기존 그대로 — Rust, winit 0.30 (MouseWheel + ScanCode), softbuffer, tokio. 신규 외부 의존 *없음*.

---

## 파일 구조

```
core/src/object/std_types.rs       # 수정: window/file_tree/explorer에 scroll_y + window에 content/content_too_large
core/tests/std_types_test.rs       # 수정: 신규 state 라운드트립

apps/desktop-shell/src/main.rs     # 수정: open_file 분기에서 file content read + Window state.content 채움
apps/desktop-shell/src/file_read.rs # 신규: read_file_to_window 헬퍼 (mime 필터, 1MB cap, UTF-8 검증)
apps/desktop-shell/src/lib.rs      # 수정: pub mod file_read
apps/desktop-shell/tests/file_read_test.rs  # 신규: 단위 테스트

compositor/src/render.rs           # 수정: render_window text 분기 + FileTree/Explorer clip render
compositor/src/main.rs             # 수정: MouseWheel 핸들러 + PageUp/Down + scroll_y SetState 송신
compositor/src/layout.rs           # 수정: FileTree/Explorer layout에 scroll_y offset 반영
compositor/tests/layout_test.rs    # 수정: scroll_y 테스트 추가

docs/adr/033-notepad-viewer-scroll.md  # 신규
docs/manual-tests/m8-notepad-acceptance.md  # 신규 (T8.18)
```

---

## Task T8.13 — ADR-033 + core std_types 확장 (Window content + 세 영역 scroll_y)

**Files:**
- Create: `docs/adr/033-notepad-viewer-scroll.md`
- Modify: `core/src/object/std_types.rs`
- Modify: `core/tests/std_types_test.rs`

### 단계

- [ ] **Step 1:** ADR-033 작성. Status / Context / Decision / Consequences / 참고. 핵심:
  - Decision: Window 내장 type-aware viewer (별 객체 X). 라인 단위 scroll_y. 컴포지터가 직접 SetState (invoke 우회). 1MB content cap.
  - Context: M8 part 1 viewer는 preview 512바이트만. 사용자 보고 "txt외에 md파일을 열수가 없다" + "스크롤 없어서 잘림".
  - Consequences: viewer 작동, *AI는 file content를 Window state로 봄* (M9 권한 모델까지 noted), Explorer/FileTree도 같은 메커니즘.
  - Alternatives rejected: 별 `Notepad@1` 객체 (M9 별 프로세스 앱과 함께), invoke 통합 SetState (v1 단순화).
  - Cross-refs: ADR-026 (Window 모델), ADR-027 (M8 read-only), KI-015 (민감 파일 노출).

- [ ] **Step 2:** `core/tests/std_types_test.rs`에 실패 테스트 4건 추가:

```rust
#[test]
fn window_factory_has_scroll_y_content_state() {
    let owner = ActorId::local_user();
    let file_id = ObjectId::new();
    let w = std_types::window(owner, "x", file_id, 0, 0, 600, 400);
    assert_eq!(w.state.get("scroll_y").and_then(|v| v.as_i64()), Some(0));
    assert_eq!(w.state.get("content").and_then(|v| v.as_str()), Some(""));
    assert_eq!(w.state.get("content_too_large").and_then(|v| v.as_bool()), Some(false));
}

#[test]
fn file_tree_factory_has_scroll_y() {
    let owner = ActorId::local_user();
    let ft = std_types::file_tree(owner, "/");
    assert_eq!(ft.state.get("scroll_y").and_then(|v| v.as_i64()), Some(0));
}

#[test]
fn explorer_factory_has_scroll_y() {
    let owner = ActorId::local_user();
    let ex = std_types::explorer(owner);
    assert_eq!(ex.state.get("scroll_y").and_then(|v| v.as_i64()), Some(0));
}

#[test]
fn window_round_trip_preserves_new_state_fields() {
    let owner = ActorId::local_user();
    let file_id = ObjectId::new();
    let mut w = std_types::window(owner, "x", file_id, 0, 0, 600, 400);
    w.set_state("scroll_y", serde_json::json!(42));
    w.set_state("content", serde_json::json!("hello\nworld"));
    w.set_state("content_too_large", serde_json::json!(true));
    let json = serde_json::to_string(&w).unwrap();
    let parsed: Object = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, w);
}
```

- [ ] **Step 3:** 실패 확인:
  ```powershell
  cargo test -p geulos-core std_types -- scroll content
  ```
  Expected: 4 tests fail (state 필드 없음).

- [ ] **Step 4:** `core/src/object/std_types.rs::window` 함수에 state 3건 추가 (기존 set_state 블록 끝):

```rust
obj.set_state("scroll_y", json!(0));
obj.set_state("content", json!(""));
obj.set_state("content_too_large", json!(false));
```

`file_tree` 함수의 set_state 블록 끝에:
```rust
obj.set_state("scroll_y", json!(0));
```

`explorer` 함수의 set_state 블록 끝에:
```rust
obj.set_state("scroll_y", json!(0));
```

각 함수 doc 주석에 신규 state 명시 (예: Window doc의 state 섹션에 `scroll_y: i32`, `content: String`, `content_too_large: bool` 추가).

- [ ] **Step 5:** `cargo test -p geulos-core` 전체 통과 (신규 4 + 기존 회귀 X).

- [ ] **Step 6:** Commit.

```bash
git add docs/adr/033-notepad-viewer-scroll.md core/src/object/std_types.rs core/tests/std_types_test.rs
git commit -m "feat(core)+(docs): M8 T8.13 — std_types에 scroll_y + Window content state 추가 (ADR-033)"
```

commit msg 끝에:
```
Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
```

### 디자인 결정
- scroll_y는 *라인 단위 i32* (px 아님). 음수는 컴포지터에서 clamp.
- content는 *최대 1MB* — std_types는 cap 안 함 (호출자 책임 — desktop-shell file_read).
- content_too_large default false — 1MB 초과한 경우만 true.

---

## Task T8.14 — desktop-shell: file_read 헬퍼 + open_file 통합

**Files:**
- Create: `apps/desktop-shell/src/file_read.rs`
- Modify: `apps/desktop-shell/src/lib.rs`
- Modify: `apps/desktop-shell/src/main.rs` (open_file 분기)
- Create: `apps/desktop-shell/tests/file_read_test.rs`

### 단계

- [ ] **Step 1:** `apps/desktop-shell/tests/file_read_test.rs` 신규 (TDD 실패 먼저):

```rust
use geulos_desktop_shell::file_read::{read_file_for_window, FileContent};
use std::fs;
use tempfile::tempdir;

#[test]
fn read_small_text_file_returns_content() {
    let tmp = tempdir().unwrap();
    let p = tmp.path().join("hello.txt");
    fs::write(&p, "Hello\nWorld\n한글\n").unwrap();
    let result = read_file_for_window(&p, "text/plain");
    assert!(matches!(result.too_large, false));
    assert_eq!(result.text, "Hello\nWorld\n한글\n");
}

#[test]
fn read_non_text_mime_returns_unsupported_message() {
    let tmp = tempdir().unwrap();
    let p = tmp.path().join("img.png");
    fs::write(&p, &[0x89, 0x50, 0x4E, 0x47]).unwrap();
    let result = read_file_for_window(&p, "image/png");
    assert!(result.text.contains("viewer 미지원"));
    assert_eq!(result.too_large, false);
}

#[test]
fn read_invalid_utf8_returns_error_message() {
    let tmp = tempdir().unwrap();
    let p = tmp.path().join("bin.txt");
    fs::write(&p, &[0xFF, 0xFE, 0xFD]).unwrap();
    let result = read_file_for_window(&p, "text/plain");
    assert!(result.text.contains("텍스트 파일 아님"));
    assert_eq!(result.too_large, false);
}

#[test]
fn read_oversized_file_truncates_to_1mb() {
    let tmp = tempdir().unwrap();
    let p = tmp.path().join("big.txt");
    let big = "a".repeat(2 * 1024 * 1024); // 2MB
    fs::write(&p, &big).unwrap();
    let result = read_file_for_window(&p, "text/plain");
    assert_eq!(result.too_large, true);
    assert_eq!(result.text.len(), 1024 * 1024);
}

#[test]
fn read_missing_file_returns_error_message() {
    let result = read_file_for_window(std::path::Path::new("/no/such/file"), "text/plain");
    assert!(result.text.contains("읽기 실패"));
}
```

- [ ] **Step 2:** 실패 확인:
  ```powershell
  cargo test -p geulos-desktop-shell --test file_read_test
  ```
  Expected: unresolved import.

- [ ] **Step 3:** `apps/desktop-shell/src/file_read.rs` 신규:

```rust
//! Window mount 시점에 file 본문 read — viewer 용 (M8 part 2, ADR-033).
//!
//! mime 필터 (`text/*`만), UTF-8 검증, 1MB cap, 파일 누락/권한 거부 안전 처리.

use std::path::Path;

/// Window.state.content + content_too_large 채울 결과.
#[derive(Debug, Clone)]
pub struct FileContent {
    pub text: String,
    pub too_large: bool,
}

const MAX_BYTES: usize = 1024 * 1024;

/// 파일을 *viewer용으로* 읽음. 실패 시 사용자에게 보일 안내 메시지를 text에 담아 반환.
///
/// 흐름:
/// 1. mime이 `text/*`가 아니면 → "[viewer 미지원: <mime>]"
/// 2. 파일 read 시도 (raw bytes)
/// 3. UTF-8 검증 (invalid → "[텍스트 파일 아님]")
/// 4. 1MB 초과 → 첫 1MB만 + too_large=true. UTF-8 char boundary 안전.
pub fn read_file_for_window(path: &Path, mime: &str) -> FileContent {
    if !mime.starts_with("text/") {
        return FileContent {
            text: format!("[viewer 미지원: {}]", mime),
            too_large: false,
        };
    }

    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            return FileContent {
                text: format!("[읽기 실패: {}]", e),
                too_large: false,
            };
        }
    };

    let (slice, too_large) = if bytes.len() > MAX_BYTES {
        (utf8_safe_prefix(&bytes, MAX_BYTES), true)
    } else {
        (&bytes[..], false)
    };

    match std::str::from_utf8(slice) {
        Ok(s) => FileContent { text: s.to_string(), too_large },
        Err(_) => FileContent {
            text: "[텍스트 파일 아님 — UTF-8 디코딩 실패]".to_string(),
            too_large: false,
        },
    }
}

/// UTF-8 경계 안전한 prefix (멀티바이트 char가 max에서 잘리지 않게 뒤로 줄임).
fn utf8_safe_prefix(bytes: &[u8], max: usize) -> &[u8] {
    let mut end = max.min(bytes.len());
    if end == bytes.len() {
        return &bytes[..end];
    }
    while end > 0 && (bytes[end - 1] & 0b1100_0000) == 0b1000_0000 {
        end -= 1;
    }
    if end > 0 && bytes[end - 1] >= 0b1100_0000 {
        end -= 1;
    }
    &bytes[..end]
}
```

- [ ] **Step 4:** `apps/desktop-shell/src/lib.rs`에 `pub mod file_read;` 추가 (알파벳 순 또는 기존 패턴).

- [ ] **Step 5:** `cargo test -p geulos-desktop-shell --test file_read_test` → 5 tests pass.

- [ ] **Step 6:** `apps/desktop-shell/src/main.rs::open_file` 분기 갱신 — Window mount 직전에 content 채움:

기존 (대략):
```rust
let mut new_window = window_ops::build_new_window(
    &owner, desktop_id, file_id, &title, pos, (600, 400), new_z,
);
add_wildcard_acl(&mut new_window);
```

갱신:
```rust
let mut new_window = window_ops::build_new_window(
    &owner, desktop_id, file_id, &title, pos, (600, 400), new_z,
);
add_wildcard_acl(&mut new_window);

// M8 part 2 (ADR-033): Window mount 시점에 file 본문 read.
let file_info = mounted_objects.iter().find(|o| o.id == file_id);
let (file_path, mime) = match file_info {
    Some(f) => {
        let p = f.props.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let m = f.props.get("mime").and_then(|v| v.as_str()).unwrap_or("application/octet-stream");
        (std::path::PathBuf::from(p), m.to_string())
    }
    None => (std::path::PathBuf::new(), "application/octet-stream".to_string()),
};
let fc = file_read::read_file_for_window(&file_path, &mime);
new_window.state.insert("content".into(), serde_json::json!(fc.text));
new_window.state.insert("content_too_large".into(), serde_json::json!(fc.too_large));
```

`use geulos_desktop_shell::file_read;` import 추가 (이미 lib.rs에 module 등록되어 있음).

- [ ] **Step 7:** `cargo test --all` 통과 + `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` 클린.

- [ ] **Step 8:** Commit.

```bash
git add apps/desktop-shell/src/file_read.rs apps/desktop-shell/src/lib.rs apps/desktop-shell/src/main.rs apps/desktop-shell/tests/file_read_test.rs
git commit -m "feat(desktop-shell): M8 T8.14 — open_file에 file_read 통합 + 1MB cap + mime 필터"
```

### 디자인 결정
- mime 필터 *prefix* 매칭 (`text/`로 시작) — text/plain, text/markdown, text/rust 등 모두 cover.
- UTF-8 invalid는 *부분 디코딩 안 함* — 사용자에게 "[텍스트 파일 아님]" 안내. 단순.
- 파일 누락 / 권한 거부 → "[읽기 실패: <io err>]" 메시지. 사용자가 *그 윈도우 닫고 다른 파일 시도* 가능.

---

## Task T8.15 — compositor render_window text + 스크롤

**Files:**
- Modify: `compositor/src/render.rs::render_window`

### 단계

- [ ] **Step 1:** `compositor/src/render.rs::render_window` 함수의 *content 분기* 재구성. 현재 file.state.preview 읽는 부분을 *Window.state.content* 읽기로 교체 + scroll_y offset + 가시 라인만 그림.

```rust
fn render_window(
    buffer: &mut [u32],
    w: usize,
    h: usize,
    rect: &Rect,
    _tree: &TreeModel,  // file_id로 File 객체 lookup 더 이상 안 함 (Window.state.content 자체 보유)
    obj: &geulos_core::Object,
    focused: bool,
) {
    // ... 기존 border / title bar / [x] / inner rect 계산 그대로 ...
    
    // Content 영역 (title bar 아래 8px 패딩)
    let content_rect = Rect {
        x: inner.x + 8,
        y: inner.y + WINDOW_TITLE_H + 8,
        w: inner.w - 16,
        h: inner.h - WINDOW_TITLE_H - 16,
    };
    
    let content = obj.state.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let too_large = obj.state.get("content_too_large").and_then(|v| v.as_bool()).unwrap_or(false);
    let scroll_y = obj.state.get("scroll_y").and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
    
    const LINE_HEIGHT: i32 = 20;
    let visible_lines = (content_rect.h / LINE_HEIGHT).max(0) as usize;
    
    if content.is_empty() {
        draw_text(buffer, w, h, "(빈 파일 또는 viewer 미지원)", content_rect.x, content_rect.y, COLOR_PLACEHOLDER);
    } else {
        let all_lines: Vec<&str> = content.lines().collect();
        let total = all_lines.len();
        let start = scroll_y.min(total.saturating_sub(visible_lines));
        let end = (start + visible_lines).min(total);
        
        let max_chars_per_line = (content_rect.w / 9).max(1) as usize; // ~9px per ASCII char heuristic
        let mut y = content_rect.y;
        for line in &all_lines[start..end] {
            let display = if line.chars().count() > max_chars_per_line {
                let truncated: String = line.chars().take(max_chars_per_line.saturating_sub(1)).collect();
                format!("{}…", truncated)
            } else {
                line.to_string()
            };
            draw_text(buffer, w, h, &display, content_rect.x, y, COLOR_TEXT);
            y += LINE_HEIGHT;
        }
        
        // 1MB 초과 안내 — content 끝에 한 줄
        if too_large && end == total {
            draw_text(
                buffer,
                w,
                h,
                "[파일이 1MB 초과 — 일부만 표시]",
                content_rect.x,
                y,
                COLOR_PLACEHOLDER,
            );
        }
    }
    
    // ... 기존 resize handle 그대로 ...
}
```

- [ ] **Step 2:** `cargo build -p geulos-compositor` 통과.

- [ ] **Step 3:** 시각 검증은 T8.18에 묶음 (단위 테스트 어려움 — render 픽셀은 통합 테스트 영역).

- [ ] **Step 4:** Commit.

```bash
git add compositor/src/render.rs
git commit -m "feat(compositor): M8 T8.15 — render_window text 분기 + scroll_y clip + truncate (긴 줄 …)"
```

### 디자인 결정
- `max_chars_per_line = content_rect.w / 9` 휴리스틱 — ASCII 9px 가정. 정확한 fontdue 측정은 v2 (measure_text_width per char은 비용). truncate가 한 두 char 빗나가도 사용성 OK.
- 빈 content는 placeholder. content_too_large 안내는 *content 끝 라인 직후*만 — scroll 중에는 안 보임.

---

## Task T8.16 — compositor FileTree + Explorer scroll_y clip

**Files:**
- Modify: `compositor/src/layout.rs` (FileTree 자손 + Explorer 자식 layout에 scroll_y 반영)
- Modify: `compositor/tests/layout_test.rs`

### 단계

- [ ] **Step 1:** `compositor/tests/layout_test.rs`에 실패 테스트 2건:

```rust
#[test]
fn file_tree_with_scroll_y_skips_first_lines() {
    use geulos_core::std_types;
    let mut tree = TreeModel::new();
    let owner = geulos_core::ActorId::local_user();
    let mut desktop = std_types::desktop(owner.clone());
    let mut ft = std_types::file_tree(owner.clone(), "/");
    let ex = std_types::explorer(owner.clone());
    let cli = std_types::cli(owner.clone());
    // ft.state.scroll_y = 2 — 첫 2 라인은 보이지 않아야
    ft.set_state("scroll_y", serde_json::json!(2));
    let f1 = std_types::folder(owner.clone(), "/a", "a", 0);
    let f2 = std_types::folder(owner.clone(), "/b", "b", 0);
    let f3 = std_types::folder(owner.clone(), "/c", "c", 0);
    ft.children = vec![f1.id, f2.id, f3.id];
    desktop.children = vec![ft.id, ex.id, cli.id];
    for o in [desktop, ft.clone(), ex, cli, f1.clone(), f2.clone(), f3.clone()] {
        tree.upsert(o);
    }
    let lay = layout(&tree, 1000, 600);
    // f1, f2는 layout에 없거나 visible 영역 밖이어야. f3는 첫 가시 라인 (y=4 부근).
    assert!(lay.get(f1.id).is_none() || lay.get(f1.id).unwrap().y < 0);
    assert!(lay.get(f3.id).is_some());
}

#[test]
fn explorer_with_scroll_y_skips_first_lines() {
    use geulos_core::std_types;
    let mut tree = TreeModel::new();
    let owner = geulos_core::ActorId::local_user();
    let mut desktop = std_types::desktop(owner.clone());
    let ft = std_types::file_tree(owner.clone(), "/");
    let mut ex = std_types::explorer(owner.clone());
    let cli = std_types::cli(owner.clone());
    ex.set_state("scroll_y", serde_json::json!(3));
    desktop.children = vec![ft.id, ex.id, cli.id];
    // explorer의 active_folder가 null이면 FileTree.children 표시 — 5개 더미 children
    // (실제로 explorer_children helper가 reads FileTree.children)
    let mut ft2 = ft.clone();
    let drives: Vec<_> = (0..5).map(|i| std_types::folder(owner.clone(), &format!("/d{}", i), &format!("d{}", i), 0)).collect();
    ft2.children = drives.iter().map(|d| d.id).collect();
    for o in [desktop.clone(), ft2, ex.clone(), cli.clone()] { tree.upsert(o); }
    for d in &drives { tree.upsert(d.clone()); }
    let lay = layout(&tree, 1000, 600);
    // d0, d1, d2는 skip (scroll_y=3). d3, d4는 위에서부터.
    assert!(lay.get(drives[0].id).is_none() || lay.get(drives[0].id).unwrap().y < 4);
    assert!(lay.get(drives[3].id).is_some());
}
```

- [ ] **Step 2:** `cargo test -p geulos-compositor --test layout_test -- scroll` — 실패 확인.

- [ ] **Step 3:** `compositor/src/layout.rs::layout_desktop` 안 FileTree 분기 수정:

기존:
```rust
if let Some(ft) = find_child_by_type(tree, obj, "aios.builtin/FileTree@1") {
    out.push((ft.id, Rect { x: 0, y: 0, w: left_w, h: top_h }, HitRole::Body));
    let expanded = extract_expanded(tree, ft.id);
    let mut y = 4i32;
    for &cid in &ft.children {
        y += layout_tree_node_folders_only(tree, &expanded, cid, 4, y, left_w - 8, out);
    }
}
```

새:
```rust
if let Some(ft) = find_child_by_type(tree, obj, "aios.builtin/FileTree@1") {
    out.push((ft.id, Rect { x: 0, y: 0, w: left_w, h: top_h }, HitRole::Body));
    let expanded = extract_expanded(tree, ft.id);
    let scroll_y = ft.state.get("scroll_y").and_then(|v| v.as_i64()).unwrap_or(0).max(0) as i32;
    let scroll_px = scroll_y * 24; // 행 높이 24px (item_height for Folder@1)
    let mut y = 4i32 - scroll_px;
    for &cid in &ft.children {
        let used = layout_tree_node_folders_only(tree, &expanded, cid, 4, y, left_w - 8, out);
        y += used;
        // 가시 영역 (0..top_h) 벗어나면 break (성능 — 단 layout이 *모든 노드 일관* 위해 break 안 해도 OK)
    }
    // 가시 영역 밖 rect는 hit_test/render에서 자연 클립
}
```

Explorer 분기도 동일:

```rust
if let Some(ex) = find_child_by_type(tree, obj, "aios.builtin/Explorer@1") {
    out.push((ex.id, Rect { x: left_w, y: 0, w: right_w, h: top_h }, HitRole::Body));
    let scroll_y = ex.state.get("scroll_y").and_then(|v| v.as_i64()).unwrap_or(0).max(0) as i32;
    let scroll_px = scroll_y * 24;
    let kids = explorer_children(tree, ex);
    let mut y = 4i32 - scroll_px;
    for child_id in kids {
        out.push((child_id, Rect { x: left_w + 4, y, w: right_w - 8, h: 24 }, HitRole::Body));
        y += 24;
        if y > top_h { break; }
    }
}
```

- [ ] **Step 4:** render.rs가 *가시 영역 밖 rect*를 안전 처리. `fill_rect`는 이미 `.max(0)`/`.min(w)` 클립이라 음수 y는 자연 skip. 단 *과한 음수* (예: scroll_y=1000, rect.y = -10000)이면 *그래도 계산은 통과*. 무해.

- [ ] **Step 5:** `cargo test -p geulos-compositor` — 신규 2건 + 기존 회귀 X.

- [ ] **Step 6:** Commit.

```bash
git add compositor/src/layout.rs compositor/tests/layout_test.rs
git commit -m "feat(compositor): M8 T8.16 — FileTree + Explorer scroll_y offset (라인 단위)"
```

### 디자인 결정
- scroll_y 단위 = *라인* (24px 곱). render에 *클립 안 함* — rect의 y가 *영역 밖*이면 fill_rect/draw_text가 자연 안 그림 (음수 좌표 클립). 단순.
- top_h 초과 break는 성능 최적화 — N개 자식 중 가시 영역 이후는 layout 안 함.

---

## Task T8.17 — compositor 마우스 휠 + PageUp/Down + SetState 송신

**Files:**
- Modify: `compositor/src/main.rs`

### 단계

- [ ] **Step 1:** `App` 구조체에 변경 없음 (state는 server에 broadcast). `WindowEvent::MouseWheel` 핸들러 신규:

```rust
WindowEvent::MouseWheel { delta, .. } => {
    let (cx, cy) = (self.cursor.0 as i32, self.cursor.1 as i32);
    let lines = match delta {
        winit::event::MouseScrollDelta::LineDelta(_, y) => -(y as i32) * 3, // 1 notch = 3 lines, 위로면 y>0 → scroll_y 감소
        winit::event::MouseScrollDelta::PixelDelta(p) => -(p.y as i32) / 16, // 16px = 1 line 휴리스틱
    };
    if lines == 0 {
        return;
    }
    if let Some(window) = &self.window {
        let size = window.inner_size();
        let tree = self.tree.lock().unwrap();
        let lay = layout(&tree, size.width as i32, size.height as i32);
        if let Some((target, _role)) = hit_test(&tree, &lay, cx, cy) {
            if let Some(obj) = tree.get(target) {
                // 타입 별로 scroll_y SetState
                let scroll_target = match obj.type_uri.as_str() {
                    "aios.builtin/Window@1" => Some(target),
                    _ => {
                        // Folder/File을 hit한 경우 부모 영역 (FileTree or Explorer) scroll
                        find_scroll_target(&tree, cx, size.width as i32)
                    }
                };
                if let Some(scroll_target_id) = scroll_target {
                    let cur = tree.get(scroll_target_id)
                        .and_then(|o| o.state.get("scroll_y").and_then(|v| v.as_i64()))
                        .unwrap_or(0);
                    let new_scroll_y = (cur + lines as i64).max(0); // 음수 clamp
                    drop(tree); // lock 해제 후 ui_tx
                    let _ = self.ui_tx.try_send(UiAction::SetState {
                        target: scroll_target_id,
                        key: "scroll_y".to_string(),
                        value: serde_json::json!(new_scroll_y),
                    });
                }
            }
        }
    }
}
```

- [ ] **Step 2:** `compositor/src/messages.rs`의 `UiAction`에 `SetState` variant 추가:

```rust
#[derive(Debug, Clone)]
pub enum UiAction {
    Invoke { target: ObjectId, method: String, args: serde_json::Value },
    Quit,
    SetState { target: ObjectId, key: String, value: serde_json::Value },  // 신규 (M8 T8.17)
}
```

`compositor/src/server_client.rs`의 UiAction 처리 분기에 SetState arm 추가:

```rust
UiAction::SetState { target, key, value } => {
    let req_id = format!("ss-{}", target);
    let m = StateSetMsg {
        request_id: req_id,
        target: target.to_string(),
        key,
        value,
    };
    let _ = write_msg(&mut stream, &m).await;
}
```

`StateSetMsg` import 확인.

- [ ] **Step 3:** `find_scroll_target` 헬퍼 (`compositor/src/main.rs` 안):

```rust
/// 마우스 X 좌표로 FileTree (좌 < 25%) 또는 Explorer (우 25~100%) ID 반환.
fn find_scroll_target(tree: &TreeModel, cx: i32, window_w: i32) -> Option<geulos_core::ObjectId> {
    let ft_threshold = (window_w as f32 * 0.25) as i32;
    if cx < ft_threshold {
        find_file_tree(tree).map(|o| o.id)
    } else {
        find_explorer(tree).map(|o| o.id)
    }
}
```

- [ ] **Step 4:** PageUp/Down — `WindowEvent::KeyboardInput`의 `KeyboardFocus::Window(id)` 분기에 추가:

```rust
KeyboardFocus::Window(window_id) => {
    use winit::keyboard::NamedKey;
    let delta_lines = match &logical_key {
        Key::Named(NamedKey::PageUp) => Some(-10), // 1 페이지 ≈ 10 라인 추정
        Key::Named(NamedKey::PageDown) => Some(10),
        _ => None,
    };
    if let Some(d) = delta_lines {
        let tree = self.tree.lock().unwrap();
        let cur = tree.get(*window_id)
            .and_then(|o| o.state.get("scroll_y").and_then(|v| v.as_i64()))
            .unwrap_or(0);
        let new_scroll_y = (cur + d).max(0);
        drop(tree);
        let _ = self.ui_tx.try_send(UiAction::SetState {
            target: *window_id,
            key: "scroll_y".to_string(),
            value: serde_json::json!(new_scroll_y),
        });
    }
}
```

기존 KeyboardFocus::Window(_) 무시 분기 *제거* 또는 위 분기로 교체.

- [ ] **Step 5:** `cargo build --all` + `cargo test --all` + fmt + clippy 클린.

- [ ] **Step 6:** Commit.

```bash
git add compositor/src/main.rs compositor/src/messages.rs compositor/src/server_client.rs
git commit -m "feat(compositor): M8 T8.17 — MouseWheel + PageUp/Down → scroll_y SetState 송신 (UiAction::SetState 신규)"
```

### 디자인 결정
- 1 wheel notch = 3 라인 (Windows 표준).
- PixelDelta 16px = 1 라인 (macOS/터치패드).
- PageUp/Down = 10 라인 (visible_lines 정확 계산은 v2 — 시각 정확도 vs 단순성 tradeoff).
- 음수 scroll_y clamp는 컴포지터 측 (max는 클램프 X — render가 자연 처리).
- *직접 SetState invoke* (UiAction::SetState 신규) — desktop-shell이 자기 흐름 없이 broadcast만. AI 가시성 유지.

---

## Task T8.18 — Acceptance + spec/quality review

**Files:**
- Create: `docs/manual-tests/m8-notepad-acceptance.md`

### 단계

- [ ] **Step 1:** acceptance 문서 작성:

```markdown
# M8 part 2 Acceptance — 메모장 viewer + 스크롤

## 사전 조건
- ANTHROPIC_API_KEY 무관 (이 task는 AI 불필요)
- 3 cmd: server-host, desktop-shell, compositor

## 시나리오 A — Window 본문 viewer
1. 컴포지터 띄움
2. 우측 Explorer → 임의 `.md` 또는 `.rs` 파일 클릭
3. Window 등장 — **전체 내용**이 들어있어야 함 (이전 v1: preview 512바이트만)
4. 휠 위/아래 → 텍스트 스크롤
5. Window focus → PageUp/Down → 10 라인씩 점프

## 시나리오 B — 1MB 초과
6. 큰 파일 (예: `Cargo.lock`이 1MB+) 클릭
7. Window 본문 끝에 `[파일이 1MB 초과 — 일부만 표시]` 안내

## 시나리오 C — 비-텍스트 파일
8. `.png` 또는 binary 파일 클릭
9. Window 본문에 `[viewer 미지원: image/png]` 또는 `[텍스트 파일 아님]`

## 시나리오 D — FileTree 스크롤
10. 좌측에서 큰 폴더 expand (수십 개 자식)
11. 마우스 휠로 위/아래 스크롤 — 잘렸던 자식들 보임

## 시나리오 E — Explorer 스크롤
12. 우측 Explorer가 큰 폴더 (예: `C:\Windows`) navigate
13. 마우스 휠로 list 스크롤
```

- [ ] **Step 2:** 수동 검증 — controller가 3 프로세스 spawn, 사용자가 위 시나리오 5개 실행, 결과 보고.

- [ ] **Step 3:** spec compliance reviewer 디스패치 (subagent-driven-development의 spec reviewer prompt).

- [ ] **Step 4:** code quality reviewer 디스패치 (subagent-driven-development의 code quality reviewer prompt).

- [ ] **Step 5:** review에서 발견된 issues fix (필요 시).

- [ ] **Step 6:** Commit.

```bash
git add docs/manual-tests/m8-notepad-acceptance.md
git commit -m "test(m8): T8.18 — 메모장 viewer + 스크롤 acceptance 시나리오 5종"
```

### 디자인 결정
- AI 무관 task — ANTHROPIC_API_KEY 없어도 OK.
- 사용자 시각 검증 필수 — 단위 테스트는 layout/file_read만 cover.

---

## 자체 점검

### 스펙 커버리지

| Spec 섹션 | 커버 task |
|---|---|
| §4.1 Window scroll_y/content/content_too_large | T8.13 |
| §4.2 FileTree scroll_y | T8.13 |
| §4.3 Explorer scroll_y | T8.13 |
| §5.1 Content fetch + 1MB cap + mime 필터 | T8.14 |
| §5.2 라인 단위 scroll_y | T8.13/T8.16 |
| §5.3 클립 렌더링 | T8.15/T8.16 |
| §5.4 Truncate | T8.15 |
| §5.5 마우스 휠 | T8.17 |
| §5.6 PageUp/Down (Window focused) | T8.17 |
| §6.1 render_window text 분기 | T8.15 |
| §6.2 MouseWheel 핸들러 | T8.17 |
| §6.3 FileTree/Explorer layout scroll | T8.16 |
| §7.1 open_file 갱신 | T8.14 |
| §7.2 컴포지터 직접 SetState | T8.17 (UiAction::SetState 신규) |
| §9 ADR-033 | T8.13 |
| §10 sub-task 6개 | T8.13~T8.18 ✅ |

빈 spec 항목 없음.

### Placeholder scan
- 모든 step에 actual code 또는 정확한 명령 + 파일 경로.
- T8.15에 `_tree` parameter 매개 미사용 표시 — 의도된 (file_id로 File lookup 더 이상 안 함, Window.state.content 자체).
- T8.16 break 조건 "성능 최적화"라 명시. *기능 핵심 X*.

### Type 일관성
- `scroll_y: i32` — 모든 task 동일 (T8.13 신규, T8.15/T8.16/T8.17 사용).
- `content: String` (T8.13/T8.14/T8.15) 동일.
- `content_too_large: bool` (T8.13/T8.14/T8.15) 동일.
- `UiAction::SetState` (T8.17 신규) — 컴포지터만 사용.
- `FileContent { text: String, too_large: bool }` (T8.14 신규) — desktop-shell 내부.

### 위험
- T8.15의 `max_chars_per_line = w / 9` 휴리스틱이 한국어/IME에서 부정확 — v2에 measure_text_width per char. 현 v1엔 *기능 작동 우선*.
- T8.17 PageUp/Down 10 라인 고정 — 큰 Window에선 적음, 작은 Window엔 많음. visible_lines 정확 계산은 v2.
- 1MB content가 wire에 매번 들어감 — Window mount당 *한 번만*. 이후 SetState로 갱신 X. 무해.

---

## 다음 단계

1. Spec OK 받음 (✅ — `이대로 진행해`)
2. **이 plan 진입** — subagent-driven-development로 T8.13부터
3. 각 task: implementer → spec review → quality review → next
4. T8.18 acceptance 후 → M8 T8.11 (M8 전체 통합 문서) + T8.12 (final review)로 마일스톤 정식 종료
5. 그 다음 M9 (권한 다이얼로그 + 편집·저장)
