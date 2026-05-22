# M9 — 편집/저장 + 권한 다이얼로그 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> **NEVER push** — controller가 마일스톤 끝에 batch push. subagent는 commit만.

**Goal:** Window를 편집 가능 모드로 토글하고 Ctrl+S로 디스크에 저장. AI가 write를 시도하면 Dialog@1 모달로 사용자 확인. dirty 추적 + close 시 확인. 권한 정책은 actor × op 표로 추상화 (M10에서 create/delete/rename으로 확장).

**Architecture:** core에 `File.save` / `Window.edit_mode·dirty` / `Dialog@1`만 새로 등록. desktop-shell이 권한 판정 + fs write + Dialog mount/응답을 처리. compositor는 editor cursor·키 입력·Dialog 모달 렌더만 추가. 기존 read 경로(M8)는 무변경.

**Tech Stack:**
- 기존 Rust workspace + tokio + serde_json
- `std::fs::write` (직접 — v1은 atomic write 미적용)
- `tokio::sync::oneshot` (Pending save 응답 채널)

**Spec parent:** `docs/specs/2026-05-22-geulos-m9-edit-save-permission.md`

---

## File Structure

| 신규/수정 | 경로 | 책임 |
|---|---|---|
| Create | `docs/adr/035-edit-save-permission.md` | ADR-035 본문 (편집 모드 + 권한 + Dialog 결정 근거) |
| Create | `apps/desktop-shell/src/file_write.rs` | `save(path, content) -> Result` — UTF-8 1MB cap, fs::write |
| Create | `apps/desktop-shell/src/permission.rs` | `Op`/`Verdict` enum + `judge(actor, op)` 함수 |
| Create | `apps/desktop-shell/src/dialog_ops.rs` | Dialog mount/respond/destroy + PendingSave 등록 |
| Create | `compositor/src/editor.rs` | `EditorState` — cursor pos, char insert/delete, dirty 검출 |
| Create | `docs/manual-tests/m9-acceptance.md` | 시나리오 A~E |
| Modify | `core/src/object/std_types.rs` | `File.save`, `Window edit_mode/dirty/toggle_edit/save_to_file/close_confirm`, `Dialog@1` factory |
| Modify | `apps/desktop-shell/src/lib.rs` | `pub mod file_write/permission/dialog_ops` |
| Modify | `apps/desktop-shell/src/main.rs` | save/toggle_edit/respond invoke 분기, dirty close 흐름 |
| Modify | `apps/desktop-shell/src/window_ops.rs` | dirty 추적 helper |
| Modify | `compositor/src/lib.rs` | `pub mod editor` |
| Modify | `compositor/src/render.rs` | edit_mode 시 cursor, dirty면 title에 `*`, Dialog@1 모달 렌더 |
| Modify | `compositor/src/main.rs` | edit_mode 키 입력 라우팅, Dialog 클릭, modal 입력 block |
| Modify | `compositor/src/keyboard.rs` | Ctrl+S/Ctrl+E/Esc 단축키 분기 |

server-side ACL은 *무변경* — T8.19에서 wildcard pass 처리됨. 모든 권한 정책은 desktop-shell 내부.

---

## Task 1: ADR-035 + core 타입 등록 (File.save / Window edit_mode·dirty / Dialog@1)

**Files:**
- Create: `docs/adr/035-edit-save-permission.md`
- Modify: `core/src/object/std_types.rs`

- [ ] **Step 1.1: ADR-035 작성**

Create `docs/adr/035-edit-save-permission.md`:

```markdown
# ADR-035 — 편집/저장 + 권한 다이얼로그 (M9)

- **상태:** Accepted
- **결정일:** 2026-05-22
- **부모 spec:** `docs/specs/2026-05-22-geulos-m9-edit-save-permission.md`

## Context

M8까지 Window는 read-only viewer. 편집/저장이 없어 텍스트 OS로 자라기 위한 *기초 write 메서드*가 부재. AI bridge가 write할 때 사용자 동의 흐름이 없으면 자동화가 위험.

## Decision

1. `File@1`에 `save(content)` 메서드 추가. desktop-shell이 핸들러 — `std::fs::write` + UTF-8 1MB cap.
2. `Window@1`에 `edit_mode: bool`, `dirty: bool` 상태 + `toggle_edit`/`save_to_file`/`close_confirm` 메서드.
3. `Dialog@1` 신규 builtin — props `(title, message, kind, actions)` + state `result`. modal (z 최상위 + 다른 입력 block).
4. desktop-shell에 `permission` 모듈: `judge(actor, op) -> Allow | ConfirmRequired`. v1 표는 `(local-user, Save)=Allow`, `(ai, Save)=Confirm`. M10에서 create/delete/rename 추가.
5. AI write 흐름: Dialog mount + `tokio::sync::oneshot`으로 응답 대기.

## Alternatives 검토

- **Inline edit (Window 항상 편집 가능)** — viewer/editor 구분 없음. UX 단순하지만 실수 우려 (사용자 보고 우려). 거부.
- **별도 `Editor@1` 타입** — Window 둘로 분리. 코드 중복 + Explorer 메뉴 분리. 거부.
- **세션 grant 권한** — 첫 AI write OK면 세션 동안 자유. v1 단순화 위해 매 작업 confirm. v2 재검토.
- **server-side ACL 확장** — set_state ACL은 이미 wildcard pass (T8.19). 권한 정책을 server 측에 두면 두 곳에 흩어짐 — desktop-shell 단일 모듈로 격리.

## Consequences

- compositor 입력 처리에 *모드 분기* 등장 (Cli / Window viewer / Window editor / Dialog modal)
- desktop-shell이 비동기 응답 대기 패턴 도입 (PendingSave map + oneshot)
- M10 같은 권한 프레임워크 위에서 create/delete/rename 빠르게 추가
- v2: atomic write (temp+rename), undo/redo, multi-byte cursor 정확도

## Trade-offs

- v1은 utf-8 텍스트만 (binary edit_mode 비활성)
- 1MB 초과 파일 부분 편집 X (전체 save만, 미만 한도)
- 동시 AI write 큐잉 X — 두 번째는 즉시 reject
```

- [ ] **Step 1.2: std_types.rs — File.save 메서드 등록**

Modify `core/src/object/std_types.rs` — `pub fn file(...)` 의 메서드 등록부에 한 줄 추가:

```rust
// 기존
obj.methods.push(MethodSig::new("read"));
// 추가
obj.methods.push(MethodSig::new("save").with_arg(ArgSpec::new("content", "string")));
```

- [ ] **Step 1.3: std_types.rs — Window edit_mode/dirty/메서드**

Modify `core/src/object/std_types.rs` — `pub fn window(...)` 의 state/method 블록에 추가:

```rust
// state 추가 (기존 scroll_y/content/content_too_large 옆에)
obj.set_state("edit_mode", json!(false));
obj.set_state("dirty", json!(false));
// methods 추가 (기존 move/resize/focus/close 옆에)
obj.methods.push(MethodSig::new("toggle_edit"));
obj.methods.push(MethodSig::new("save_to_file"));
obj.methods.push(MethodSig::new("close_confirm")); // dirty 시 desktop-shell이 Dialog 띄움
```

- [ ] **Step 1.4: std_types.rs — Dialog@1 factory 신규**

Append to `core/src/object/std_types.rs`:

```rust
// ───────────────────────── M9: Dialog@1 (modal confirm/warn) ─────────────────────────

/// 모달 다이얼로그. Desktop의 자식으로 mount되어 z-최상위 오버레이로 떠있음.
///
/// props:
/// - `title: String`
/// - `message: String`
/// - `kind: String` — `"confirm"` | `"warn"`
/// - `actions: [String]` — 버튼 라벨 배열 (예: `["허용", "거부"]`).
///
/// state:
/// - `result: Option<String>` — 사용자가 클릭한 action 라벨. null=pending.
///
/// 메서드: `respond(action: String)` — compositor가 사용자 클릭 결과 전달.
///
/// Modal: compositor가 layout에서 *항상 z-최상위*로 push하고, hit_test가 Dialog 떠있을 때
/// Dialog rect 밖 클릭을 *consume(무시)*하여 다른 Window/CLI/Explorer 입력을 block.
pub fn dialog(owner: ActorId, title: &str, message: &str, kind: &str, actions: Vec<String>) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.builtin/Dialog@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("title", json!(title));
    obj.set_prop("message", json!(message));
    obj.set_prop("kind", json!(kind));
    obj.set_prop("actions", json!(actions));
    obj.set_state("result", json!(null));
    obj.methods.push(MethodSig::new("respond").with_arg(ArgSpec::new("action", "string")));
    obj
}
```

- [ ] **Step 1.5: 단위 테스트 — File.save / Window edit_mode / Dialog@1 factory**

Add to `core/src/object/std_types.rs` 내 `#[cfg(test)] mod tests`:

```rust
#[test]
fn file_has_save_method() {
    let f = file(ActorId::local_user(), "/x.txt", "x.txt", "text/plain", 0);
    assert!(f.methods.iter().any(|m| m.name == "save"));
}

#[test]
fn window_has_edit_mode_and_dirty_state() {
    let w = window(ActorId::local_user(), "t", ObjectId::new(), 0, 0, 600, 400);
    assert_eq!(w.state.get("edit_mode"), Some(&json!(false)));
    assert_eq!(w.state.get("dirty"), Some(&json!(false)));
    assert!(w.methods.iter().any(|m| m.name == "toggle_edit"));
    assert!(w.methods.iter().any(|m| m.name == "save_to_file"));
    assert!(w.methods.iter().any(|m| m.name == "close_confirm"));
}

#[test]
fn dialog_factory_sets_props_state_methods() {
    let d = dialog(
        ActorId::local_user(),
        "확인",
        "정말?",
        "confirm",
        vec!["허용".to_string(), "거부".to_string()],
    );
    assert_eq!(d.type_uri.as_str(), "aios.builtin/Dialog@1");
    assert_eq!(d.props.get("title"), Some(&json!("확인")));
    assert_eq!(d.props.get("actions"), Some(&json!(["허용", "거부"])));
    assert_eq!(d.state.get("result"), Some(&json!(null)));
    assert!(d.methods.iter().any(|m| m.name == "respond"));
}
```

- [ ] **Step 1.6: 빌드 + 테스트**

Run: `cargo test -p geulos-core --lib std_types`
Expected: 위 3개 새 테스트 + 기존 테스트 모두 PASS

- [ ] **Step 1.7: Commit**

```
git add docs/adr/035-edit-save-permission.md core/src/object/std_types.rs
git commit -m "feat(core)+adr: M9 T1 — File.save/Window edit·dirty/Dialog@1 + ADR-035"
```

---

## Task 2: permission 모듈 (desktop-shell)

**Files:**
- Create: `apps/desktop-shell/src/permission.rs`
- Modify: `apps/desktop-shell/src/lib.rs`

- [ ] **Step 2.1: permission.rs 신규 + 테스트 작성**

Create `apps/desktop-shell/src/permission.rs`:

```rust
//! write 권한 정책 — `actor + op -> Allow | ConfirmRequired` (M9 / ADR-035).
//!
//! v1: 사용자 직접 Save는 Allow, 그 외 모두 ConfirmRequired. M10에서 create/delete/rename으로
//! Op 확장. 표는 spec §"권한 정책" 참고.

use geulos_core::ActorId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Save,
    /// M10 예약 — v1은 사용 안 함.
    Create,
    Delete,
    Rename,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    ConfirmRequired,
}

/// `actor`가 `op`를 수행할 때 다이얼로그 confirm이 필요한지 판정.
///
/// v1 정책:
/// - 사용자(local-user) Save: Allow
/// - 사용자 Delete: ConfirmRequired (M10에서 작동)
/// - 그 외 사용자 op (Create/Rename): Allow
/// - 그 외 모든 actor (ai 등) 모든 op: ConfirmRequired
pub fn judge(actor: &ActorId, op: Op) -> Verdict {
    let is_local_user = actor == &ActorId::local_user();
    match (is_local_user, op) {
        (true, Op::Save) => Verdict::Allow,
        (true, Op::Create) => Verdict::Allow,
        (true, Op::Rename) => Verdict::Allow,
        (true, Op::Delete) => Verdict::ConfirmRequired,
        (false, _) => Verdict::ConfirmRequired,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ai() -> ActorId {
        ActorId::from_string("ai".to_string())
    }

    #[test]
    fn user_save_allowed() {
        assert_eq!(judge(&ActorId::local_user(), Op::Save), Verdict::Allow);
    }

    #[test]
    fn user_create_allowed() {
        assert_eq!(judge(&ActorId::local_user(), Op::Create), Verdict::Allow);
    }

    #[test]
    fn user_rename_allowed() {
        assert_eq!(judge(&ActorId::local_user(), Op::Rename), Verdict::Allow);
    }

    #[test]
    fn user_delete_requires_confirm() {
        assert_eq!(judge(&ActorId::local_user(), Op::Delete), Verdict::ConfirmRequired);
    }

    #[test]
    fn ai_save_requires_confirm() {
        assert_eq!(judge(&ai(), Op::Save), Verdict::ConfirmRequired);
    }

    #[test]
    fn ai_create_requires_confirm() {
        assert_eq!(judge(&ai(), Op::Create), Verdict::ConfirmRequired);
    }

    #[test]
    fn ai_delete_requires_confirm() {
        assert_eq!(judge(&ai(), Op::Delete), Verdict::ConfirmRequired);
    }

    #[test]
    fn ai_rename_requires_confirm() {
        assert_eq!(judge(&ai(), Op::Rename), Verdict::ConfirmRequired);
    }
}
```

- [ ] **Step 2.2: lib.rs 노출**

Modify `apps/desktop-shell/src/lib.rs` — `pub mod` 줄들 옆에 추가:

```rust
pub mod permission;
```

- [ ] **Step 2.3: 빌드 + 테스트**

Run: `cargo test -p geulos-desktop-shell --lib permission`
Expected: 8 tests passed (위 8개)

- [ ] **Step 2.4: Commit**

```
git add apps/desktop-shell/src/permission.rs apps/desktop-shell/src/lib.rs
git commit -m "feat(desktop-shell): M9 T2 — permission 모듈 (actor×op → Verdict, 8 tests)"
```

---

## Task 3: file_write 모듈

**Files:**
- Create: `apps/desktop-shell/src/file_write.rs`
- Modify: `apps/desktop-shell/src/lib.rs`

- [ ] **Step 3.1: file_write.rs 신규**

Create `apps/desktop-shell/src/file_write.rs`:

```rust
//! 파일 저장 — `save(path, content)` (M9 / ADR-035).
//!
//! UTF-8 1MB cap. v1은 직접 `fs::write` (atomic 아님 — crash 시 원본 손상 가능, v2에서
//! temp+rename 검토). 모든 실패는 `Err(String)` — 호출자가 invoke 응답이나 CLI 안내에 사용.

use std::path::Path;

const MAX_BYTES: usize = 1024 * 1024;

/// content를 path에 UTF-8로 저장. 1MB 초과면 에러.
pub fn save(path: &Path, content: &str) -> Result<(), String> {
    if content.len() > MAX_BYTES {
        return Err(format!("1MB 초과 ({}B > {}B) — v1 미지원", content.len(), MAX_BYTES));
    }
    std::fs::write(path, content).map_err(|e| format!("저장 실패: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn save_writes_content_to_path() {
        let mut tmp = tempfile::NamedTempFile::new().expect("tempfile");
        // tempfile은 *기존 파일* — save가 덮어쓰기 동작.
        writeln!(tmp, "old").unwrap();
        let path = tmp.path().to_path_buf();
        save(&path, "new content\n").expect("save ok");
        let read = std::fs::read_to_string(&path).expect("read");
        assert_eq!(read, "new content\n");
    }

    #[test]
    fn save_rejects_over_1mb() {
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let big: String = "a".repeat(MAX_BYTES + 1);
        let err = save(tmp.path(), &big).unwrap_err();
        assert!(err.contains("1MB 초과"), "got: {}", err);
    }

    #[test]
    fn save_to_nonexistent_dir_returns_err() {
        let path = Path::new("/this/path/does/not/exist/nope.txt");
        let err = save(path, "x").unwrap_err();
        assert!(err.contains("저장 실패"), "got: {}", err);
    }
}
```

- [ ] **Step 3.2: Cargo.toml — tempfile dev-dep 추가**

Modify `apps/desktop-shell/Cargo.toml` — `[dev-dependencies]` 섹션에:

```toml
[dev-dependencies]
tempfile = "3"
```

(섹션이 없으면 추가)

- [ ] **Step 3.3: lib.rs 노출**

Modify `apps/desktop-shell/src/lib.rs`:

```rust
pub mod file_write;
```

- [ ] **Step 3.4: 빌드 + 테스트**

Run: `cargo test -p geulos-desktop-shell --lib file_write`
Expected: 3 tests passed

- [ ] **Step 3.5: Commit**

```
git add apps/desktop-shell/src/file_write.rs apps/desktop-shell/src/lib.rs apps/desktop-shell/Cargo.toml
git commit -m "feat(desktop-shell): M9 T3 — file_write::save (1MB cap, 3 tests)"
```

---

## Task 4: dialog_ops 모듈 (Dialog mount + PendingSave 매핑)

**Files:**
- Create: `apps/desktop-shell/src/dialog_ops.rs`
- Modify: `apps/desktop-shell/src/lib.rs`

- [ ] **Step 4.1: dialog_ops.rs 신규**

Create `apps/desktop-shell/src/dialog_ops.rs`:

```rust
//! Dialog@1 mount/respond + Pending(actor 응답 대기) 매핑 (M9 / ADR-035).
//!
//! AI write가 ConfirmRequired면 Dialog mount + 원래 save args를 PendingSave에 보관.
//! 사용자가 Dialog.respond("허용"/"거부")로 응답하면 결과를 oneshot으로 깨워준다.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use geulos_core::ObjectId;
use tokio::sync::oneshot;

/// AI invoke 응답을 기다리는 한 건. file_id/path/content와 깨움 채널.
pub struct PendingSave {
    pub file_id: ObjectId,
    pub path: PathBuf,
    pub content: String,
    /// Dialog 응답이 도착하면 보내는 채널.
    /// payload: 사용자가 클릭한 라벨 ("허용" / "거부" 등).
    pub tx: oneshot::Sender<String>,
}

/// dialog_id → PendingSave 매핑.
#[derive(Default)]
pub struct PendingMap {
    inner: Mutex<HashMap<ObjectId, PendingSave>>,
}

impl PendingMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, dialog_id: ObjectId, p: PendingSave) {
        self.inner.lock().expect("PendingMap poisoned").insert(dialog_id, p);
    }

    pub fn take(&self, dialog_id: ObjectId) -> Option<PendingSave> {
        self.inner.lock().expect("PendingMap poisoned").remove(&dialog_id)
    }

    pub fn contains(&self, dialog_id: ObjectId) -> bool {
        self.inner.lock().expect("PendingMap poisoned").contains_key(&dialog_id)
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("PendingMap poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn insert_and_take_round_trip() {
        let map = PendingMap::new();
        let dialog_id = ObjectId::new();
        let file_id = ObjectId::new();
        let (tx, _rx) = oneshot::channel();
        map.insert(
            dialog_id,
            PendingSave {
                file_id,
                path: PathBuf::from("/x"),
                content: "y".into(),
                tx,
            },
        );
        assert!(map.contains(dialog_id));
        assert_eq!(map.len(), 1);
        let taken = map.take(dialog_id).expect("present");
        assert_eq!(taken.file_id, file_id);
        assert!(!map.contains(dialog_id));
    }

    #[test]
    fn take_missing_returns_none() {
        let map = PendingMap::new();
        assert!(map.take(ObjectId::new()).is_none());
    }

    /// respond → oneshot 깨움 시나리오.
    #[tokio::test]
    async fn respond_wakes_oneshot() {
        let map = PendingMap::new();
        let dialog_id = ObjectId::new();
        let (tx, rx) = oneshot::channel();
        map.insert(
            dialog_id,
            PendingSave {
                file_id: ObjectId::new(),
                path: PathBuf::from("/x"),
                content: "z".into(),
                tx,
            },
        );
        // 가짜 respond — take 후 tx로 전송.
        let p = map.take(dialog_id).expect("present");
        p.tx.send("허용".to_string()).expect("send");
        let got = rx.await.expect("recv");
        assert_eq!(got, "허용");
    }
}
```

- [ ] **Step 4.2: lib.rs 노출**

Modify `apps/desktop-shell/src/lib.rs`:

```rust
pub mod dialog_ops;
```

- [ ] **Step 4.3: 빌드 + 테스트**

Run: `cargo test -p geulos-desktop-shell --lib dialog_ops`
Expected: 3 tests passed

- [ ] **Step 4.4: Commit**

```
git add apps/desktop-shell/src/dialog_ops.rs apps/desktop-shell/src/lib.rs
git commit -m "feat(desktop-shell): M9 T4 — dialog_ops PendingMap + oneshot (3 tests)"
```

---

## Task 5: compositor/editor.rs (EditorState — cursor, char insert/delete)

**Files:**
- Create: `compositor/src/editor.rs`
- Modify: `compositor/src/lib.rs`

- [ ] **Step 5.1: editor.rs 신규**

Create `compositor/src/editor.rs`:

```rust
//! Window edit_mode 시 사용되는 컴포지터-사이드 editor state (M9 / ADR-035).
//!
//! cursor는 *byte offset* (UTF-8 char boundary 위에 항상 있도록 char insert/delete가 보장).
//! content는 server-state Window.content의 *컴포지터 측 미러* — 키 입력마다 즉시 미러 갱신 +
//! debounced로 server에 SetState. v1은 *모든 변경*을 즉시 SetState (debounce는 v2).

use geulos_core::ObjectId;

/// 한 Window의 편집 상태. compositor가 KeyboardFocus::Window(id) 시 active로 유지.
#[derive(Debug, Clone)]
pub struct EditorState {
    pub window_id: ObjectId,
    pub content: String,
    /// byte offset (항상 char boundary).
    pub cursor: usize,
}

impl EditorState {
    pub fn new(window_id: ObjectId, content: String) -> Self {
        let cursor = content.len();
        Self { window_id, content, cursor }
    }

    /// 한 char 삽입 — cursor 위치에. cursor를 char width(byte 수)만큼 전진.
    pub fn insert_char(&mut self, c: char) {
        self.content.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Backspace — cursor 바로 앞의 한 char 삭제. cursor가 0이면 무동작.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.content[..self.cursor]
            .chars()
            .next_back()
            .expect("cursor > 0 → prev char 존재");
        let prev_byte_len = prev.len_utf8();
        self.cursor -= prev_byte_len;
        self.content.drain(self.cursor..self.cursor + prev_byte_len);
    }

    /// cursor 왼쪽으로 한 char.
    pub fn cursor_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev =
            self.content[..self.cursor].chars().next_back().expect("cursor > 0 → prev 존재");
        self.cursor -= prev.len_utf8();
    }

    /// cursor 오른쪽으로 한 char.
    pub fn cursor_right(&mut self) {
        if self.cursor >= self.content.len() {
            return;
        }
        let next = self.content[self.cursor..].chars().next().expect("cursor < len → next 존재");
        self.cursor += next.len_utf8();
    }

    /// 엔터 — '\n' 삽입.
    pub fn newline(&mut self) {
        self.insert_char('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(s: &str) -> EditorState {
        EditorState::new(ObjectId::new(), s.to_string())
    }

    #[test]
    fn new_cursor_at_end() {
        let e = ed("hello");
        assert_eq!(e.cursor, 5);
    }

    #[test]
    fn insert_ascii_advances_one() {
        let mut e = ed("");
        e.insert_char('a');
        assert_eq!(e.content, "a");
        assert_eq!(e.cursor, 1);
    }

    #[test]
    fn insert_korean_advances_three_bytes() {
        let mut e = ed("");
        e.insert_char('한');
        assert_eq!(e.content, "한");
        assert_eq!(e.cursor, 3);
    }

    #[test]
    fn backspace_removes_prev_char() {
        let mut e = ed("ab");
        e.backspace();
        assert_eq!(e.content, "a");
        assert_eq!(e.cursor, 1);
    }

    #[test]
    fn backspace_removes_korean_char_three_bytes() {
        let mut e = ed("한글");
        e.backspace();
        assert_eq!(e.content, "한");
        assert_eq!(e.cursor, 3);
    }

    #[test]
    fn backspace_at_zero_no_op() {
        let mut e = ed("");
        e.backspace();
        assert_eq!(e.content, "");
        assert_eq!(e.cursor, 0);
    }

    #[test]
    fn cursor_left_right_respect_korean_boundary() {
        let mut e = ed("a한b");
        e.cursor = 0;
        e.cursor_right();
        assert_eq!(e.cursor, 1); // 'a' 뒤
        e.cursor_right();
        assert_eq!(e.cursor, 4); // '한' 뒤 (1+3)
        e.cursor_right();
        assert_eq!(e.cursor, 5); // 'b' 뒤
        e.cursor_right();
        assert_eq!(e.cursor, 5); // end no-op
        e.cursor_left();
        assert_eq!(e.cursor, 4);
        e.cursor_left();
        assert_eq!(e.cursor, 1);
    }

    #[test]
    fn newline_inserts_lf() {
        let mut e = ed("ab");
        e.cursor = 1;
        e.newline();
        assert_eq!(e.content, "a\nb");
        assert_eq!(e.cursor, 2);
    }
}
```

- [ ] **Step 5.2: lib.rs 노출**

Modify `compositor/src/lib.rs`:

```rust
pub mod editor;
```

- [ ] **Step 5.3: 빌드 + 테스트**

Run: `cargo test -p geulos-compositor --lib editor`
Expected: 8 tests passed

- [ ] **Step 5.4: Commit**

```
git add compositor/src/editor.rs compositor/src/lib.rs
git commit -m "feat(compositor): M9 T5 — editor.rs EditorState (UTF-8 cursor, 8 tests)"
```

---

## Task 6: render.rs — dirty `*`, edit_mode cursor, Dialog@1 모달

**Files:**
- Modify: `compositor/src/render.rs`

- [ ] **Step 6.1: render_window — dirty면 title에 `*` 접두**

`render.rs` `render_window` 함수의 title 그리기 직전 수정:

```rust
let dirty = obj.state.get("dirty").and_then(|v| v.as_bool()).unwrap_or(false);
let raw_title = obj.props.get("title").and_then(|v| v.as_str()).unwrap_or("(window)");
let title = if dirty { format!("* {}", raw_title) } else { raw_title.to_string() };
draw_text(buffer, w, h, &title, title_rect.x + 8, title_rect.y + 4, COLOR_WINDOW_TITLE_TEXT);
```

(기존 `let title = obj.props...` + `draw_text(... title ...)` 두 줄 대체)

- [ ] **Step 6.2: render_window — edit_mode 시 cursor 그리기**

`render.rs` `render_window` 함수의 content 그리기 *후*에 추가 (else 분기, content 있을 때):

```rust
// edit_mode + focused이면 cursor 그리기. cursor pos는 EditorState에서 받아야 하지만
// render_frame signature를 깨지 않기 위해 *현재는 시각화 생략* — Task 7에서 main이
// 별도 helper로 그림. 여기선 placeholder만 표시.
let edit_mode = obj.state.get("edit_mode").and_then(|v| v.as_bool()).unwrap_or(false);
if edit_mode && focused {
    // 안내 텍스트 — 우상단에 "[편집]" 작게.
    draw_text(
        buffer,
        w,
        h,
        "[편집]",
        title_rect.x + title_rect.w - 60,
        title_rect.y + 4,
        COLOR_WINDOW_TITLE_TEXT,
    );
}
```

(cursor의 실제 시각화는 Task 7의 main.rs에서 별도 layer로 — render_frame signature 단순 유지)

- [ ] **Step 6.3: render_frame — Dialog@1 모달 분기**

`render.rs` `render_frame`의 `match obj.type_uri.as_str()` 에 새 case 추가 (Window 분기 다음에):

```rust
"aios.builtin/Dialog@1" => {
    render_dialog(buffer, width, height, &rect, obj);
}
```

새 함수 `render_dialog`를 `render_window` 옆에 추가:

```rust
/// Dialog@1 모달 렌더 — 화면 중앙 박스 + title + message + buttons.
///
/// rect는 layout이 산출한 *Dialog 자체 rect* (예: 화면 중앙 400×200). 클릭 hit는 layout이
/// 별도 Body rect들로 (각 버튼) push — 여기서는 그리기만.
fn render_dialog(
    buffer: &mut [u32],
    w: usize,
    h: usize,
    rect: &Rect,
    obj: &geulos_core::Object,
) {
    // 외곽 어두운 반투명 overlay는 layout에서 별도 rect로 push (Task 7) — 여기는 박스만.
    fill_rect(buffer, w, h, rect, COLOR_WINDOW_BORDER);
    let inner = Rect { x: rect.x + 1, y: rect.y + 1, w: rect.w - 2, h: rect.h - 2 };
    fill_rect(buffer, w, h, &inner, COLOR_WINDOW_BG);

    let title = obj.props.get("title").and_then(|v| v.as_str()).unwrap_or("(dialog)");
    draw_text(buffer, w, h, title, inner.x + 12, inner.y + 12, COLOR_TEXT);

    let message = obj.props.get("message").and_then(|v| v.as_str()).unwrap_or("");
    draw_text(buffer, w, h, message, inner.x + 12, inner.y + 44, COLOR_TEXT);

    let actions = obj
        .props
        .get("actions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let n = actions.len().max(1);
    let btn_w = 100;
    let btn_h = 32;
    let gap = 12;
    let total_w = n as i32 * btn_w + (n as i32 - 1) * gap;
    let mut bx = inner.x + (inner.w - total_w) / 2;
    let by = inner.y + inner.h - btn_h - 12;
    for a in &actions {
        let label = a.as_str().unwrap_or("?");
        let br = Rect { x: bx, y: by, w: btn_w, h: btn_h };
        fill_rect(buffer, w, h, &br, COLOR_BUTTON);
        // 라벨 가운데 정렬은 measure_text_width로 — 단순 padding로 시작.
        draw_text(buffer, w, h, label, br.x + 12, br.y + 6, COLOR_BUTTON_TEXT);
        bx += btn_w + gap;
    }
}
```

- [ ] **Step 6.4: 빌드 + 테스트 회귀**

Run: `cargo build -p geulos-compositor` (signature 변경 없음 확인)
Run: `cargo test -p geulos-compositor --lib`
Expected: 기존 테스트 + editor 8개, 전부 PASS

- [ ] **Step 6.5: fmt + clippy**

Run: `cargo fmt --all` + `cargo clippy -p geulos-compositor --all-targets -- -D warnings`
Expected: 클린

- [ ] **Step 6.6: Commit**

```
git add compositor/src/render.rs
git commit -m "feat(compositor): M9 T6 — dirty * / edit_mode 안내 / Dialog@1 모달 렌더"
```

---

## Task 7: compositor/main.rs — edit_mode 키 입력, Dialog 모달 hit-block, cursor 시각화

**Files:**
- Modify: `compositor/src/main.rs`
- Modify: `compositor/src/keyboard.rs`
- Modify: `compositor/src/layout.rs`

- [ ] **Step 7.1: layout.rs — Dialog rect + 버튼 hit rect push**

Modify `compositor/src/layout.rs` `layout_desktop`의 Window 오버레이 push 다음에 추가:

```rust
// Dialog 오버레이 — Window보다 z 위. 화면 중앙 400×200 고정. 버튼별 ExplorerParentNav와
// 동일한 별도 hit rect (HitRole::Body의 보조로 새 variant 추가 가능하나 v1 단순화: 버튼은
// id=Dialog id + Body로 push하고 main이 cursor x로 어느 버튼인지 산출).
let dialogs: Vec<&geulos_core::Object> = obj
    .children
    .iter()
    .filter_map(|&cid| tree.get(cid))
    .filter(|o| o.type_uri.as_str() == "aios.builtin/Dialog@1")
    .filter(|o| o.state.get("result").map(|v| v.is_null()).unwrap_or(true))
    .collect();
for d in dialogs {
    let dw = 400i32;
    let dh = 200i32;
    let dx = (win_w - dw) / 2;
    let dy = (win_h - dh) / 2;
    out.push((d.id, Rect { x: dx, y: dy, w: dw, h: dh }, HitRole::Body));
}
```

- [ ] **Step 7.2: hit_test.rs — Dialog 떠있으면 *Dialog rect 안만* 허용**

Modify `compositor/src/hit_test.rs` (전체 봐서 *Dialog가 존재할 때만 그 hit rect를 우선*):

```rust
pub fn hit_test(
    tree: &TreeModel,
    layout: &LayoutResult,
    px: i32,
    py: i32,
) -> Option<(ObjectId, HitRole)> {
    // Dialog가 *떠있으면 modal* — Dialog rect 안만 매칭, 밖이면 consume(None).
    let dialog_rect = layout.rects.iter().rev().find_map(|(id, r, _role)| {
        let obj = tree.get(*id)?;
        if obj.type_uri.as_str() == "aios.builtin/Dialog@1"
            && obj.state.get("result").map(|v| v.is_null()).unwrap_or(true)
        {
            Some((*id, *r))
        } else {
            None
        }
    });
    if let Some((dialog_id, dr)) = dialog_rect {
        return if dr.contains(px, py) {
            Some((dialog_id, HitRole::Body))
        } else {
            None // modal — Dialog 밖은 무시
        };
    }
    // Dialog 없음 → 일반 역순 hit.
    for (id, rect, role) in layout.rects.iter().rev() {
        if rect.contains(px, py) {
            return Some((*id, *role));
        }
    }
    None
}
```

(기존 hit_test 시그니처 변경: `tree` 인자 추가 필요. main.rs 호출처에서 `hit_test(&tree, &lay, cx, cy)`로 — 이미 그렇게 호출 중인지 확인. 아니면 시그니처 갱신.)

- [ ] **Step 7.3: keyboard.rs — Ctrl+S/Ctrl+E/Esc 라우팅**

Modify `compositor/src/keyboard.rs` — `KeyAction` enum에 variant 추가:

```rust
pub enum KeyAction {
    // ... 기존 ...
    /// edit_mode일 때 Ctrl+S → save_to_file invoke
    SaveToFile,
    /// Ctrl+E → toggle_edit invoke
    ToggleEdit,
    /// edit_mode일 때 Esc → toggle_edit (편집 종료, dirty면 confirm은 close 시점에만)
    ExitEdit,
}
```

`handle_key`에 분기 — `KeyboardFocus::Window(id)` + edit_mode일 때 Ctrl+S/Ctrl+E/Esc를 위 variant로 반환.

(정확한 코드는 keyboard.rs의 현재 구조에 맞게 — *모든* 다른 키는 char 입력으로 editor.insert_char 호출하도록 EditorState로 전달 필요. v1은 main.rs가 KeyboardFocus::Window 시 editor_state를 보유.)

- [ ] **Step 7.4: main.rs — Window별 EditorState + cursor 시각화 후처리**

Modify `compositor/src/main.rs`:

1. App struct에 `editor_state: Option<EditorState>` 추가
2. `KeyboardFocus::Window(id)` + edit_mode 시 키 입력 → editor_state.insert_char/backspace/...
3. 각 char/backspace/newline 후:
   - `editor_state.content` → `UiAction::SetState(window_id, "content", json!(content))`
   - `dirty=true` → `UiAction::SetState(window_id, "dirty", json!(true))`
4. Ctrl+S → `UiAction::Invoke { target: window_id, method: "save_to_file", args: null }`
5. Ctrl+E (또는 더블클릭) → `UiAction::Invoke { target: window_id, method: "toggle_edit", args: null }`
6. Dialog Body 클릭 → x 좌표로 어느 버튼인지 계산:

```rust
// Dialog 분기 (uri == "aios.builtin/Dialog@1"):
if let Some(actions) = obj.props.get("actions").and_then(|v| v.as_array()) {
    let dw = 400i32;
    let dh = 200i32;
    let win_rect = lay.get(target).unwrap_or(Rect { x: 0, y: 0, w: 0, h: 0 });
    let n = actions.len();
    let btn_w = 100;
    let gap = 12;
    let total_w = n as i32 * btn_w + (n as i32 - 1) * gap;
    let by = win_rect.y + dh - 32 - 12;
    if cy >= by && cy < by + 32 {
        let bx_start = win_rect.x + (dw - total_w) / 2;
        let rel = cx - bx_start;
        if rel >= 0 {
            let idx = rel / (btn_w + gap);
            if (idx as usize) < n && rel < idx * (btn_w + gap) + btn_w {
                let label = actions[idx as usize].as_str().unwrap_or("");
                let _ = self.ui_tx.try_send(UiAction::Invoke {
                    target,
                    method: "respond".to_string(),
                    args: json!({ "action": label }),
                });
            }
        }
    }
}
```

7. render 후 *cursor 시각화* — editor_state가 있고 그 window_id가 보이면 그 위에 cursor (얇은 세로 막대):

```rust
// render_frame 호출 다음, surface present 직전:
if let Some(ed) = &self.editor_state {
    if let Some(win_rect) = lay.get(ed.window_id) {
        // text content_rect 계산 — render_window와 동일 식.
        let inner_x = win_rect.x + 1 + 8;
        let inner_y = win_rect.y + 1 + WINDOW_TITLE_H + 8;
        let content_w = win_rect.w - 2 - 16;
        // cursor가 어느 라인의 어느 column인지 추정.
        let chars_per_line = (content_w / 14).max(1) as usize;
        let prefix = &ed.content[..ed.cursor];
        let mut line = 0;
        let mut col = 0;
        for c in prefix.chars() {
            if c == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
                if col >= chars_per_line {
                    line += 1;
                    col = 0;
                }
            }
        }
        let cx_px = inner_x + (col as i32) * 14;
        let cy_px = inner_y + (line as i32) * 20;
        let cur_rect = Rect { x: cx_px, y: cy_px + 2, w: 2, h: 18 };
        // Re-use compositor surface buffer은 render_frame 후라 직접 접근 X.
        // → cursor는 render_frame 안에서 그리는 게 더 깔끔. 이 step의 분리 구현은
        //   render_frame signature에 editor_state를 추가하는 후속 patch로 (T7.5).
    }
}
```

**대안 (권장):** render_frame signature에 `editor: Option<&EditorState>`를 추가하고 render_window가 직접 cursor를 그림. signature change OK (단일 호출자 main).

수정된 step:
- `render_frame`에 `editor: Option<&EditorState>` 추가
- `render_window`도 동일 인자 전달
- render_window의 content 그리기 후, `obj.id == editor.window_id` 일 때 cursor 위 식으로 그림

- [ ] **Step 7.5: 빌드 + 통합 회귀 테스트**

Run: `cargo build --workspace`
Run: `cargo test --workspace`
Expected: 모두 PASS, 새 회귀 0

- [ ] **Step 7.6: fmt + clippy**

Run: `cargo fmt --all` + `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 클린

- [ ] **Step 7.7: Commit**

```
git add compositor/src/main.rs compositor/src/keyboard.rs compositor/src/layout.rs compositor/src/hit_test.rs compositor/src/render.rs
git commit -m "feat(compositor): M9 T7 — edit_mode 입력 / Dialog 모달 hit-block / cursor 시각화"
```

---

## Task 8: desktop-shell/main.rs — invoke 분기 (save_to_file / save / toggle_edit / respond / close_confirm)

**Files:**
- Modify: `apps/desktop-shell/src/main.rs`
- Modify: `apps/desktop-shell/src/window_ops.rs`

- [ ] **Step 8.1: window_ops.rs — dirty/edit_mode helper**

Modify `apps/desktop-shell/src/window_ops.rs` — `toggle_edit` / `set_dirty` 헬퍼 추가:

```rust
use serde_json::{json, Value};

use crate::invoke_handler::InvokeOutcome;
use geulos_core::ObjectId;

pub fn handle_toggle_edit(window_id: ObjectId, current_edit_mode: bool) -> InvokeOutcome {
    let new_mode = !current_edit_mode;
    InvokeOutcome {
        state_sets: vec![(window_id, "edit_mode".to_string(), json!(new_mode))],
    }
}

#[cfg(test)]
mod tests_m9 {
    use super::*;

    #[test]
    fn toggle_edit_flips_value() {
        let id = ObjectId::new();
        let o = handle_toggle_edit(id, false);
        assert_eq!(o.state_sets[0].2, Value::Bool(true));
        let o2 = handle_toggle_edit(id, true);
        assert_eq!(o2.state_sets[0].2, Value::Bool(false));
    }
}
```

- [ ] **Step 8.2: main.rs — App state에 PendingMap + invoke 분기**

Modify `apps/desktop-shell/src/main.rs`:

1. `use desktop_shell::{permission, file_write, dialog_ops};`
2. 메인 task 내부에 `let pending = Arc::new(dialog_ops::PendingMap::new());`
3. invoke 핸들러의 `match method` 에 새 case 추가:

```rust
"toggle_edit" => {
    let current = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|w| w.state.get("edit_mode").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    if let Some(w) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
        w.state.insert("edit_mode".into(), json!(!current));
    }
    window_ops::handle_toggle_edit(target_id, current)
}
"save_to_file" => {
    // Window.save_to_file = window.content를 file에 commit. file_id는 Window prop.
    let (file_id_opt, content_opt) = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .map(|w| {
            let fid = w.props.get("file_id").and_then(|v| v.as_str()).and_then(parse_object_id);
            let c = w.state.get("content").and_then(|v| v.as_str()).map(String::from);
            (fid, c)
        })
        .unwrap_or((None, None));
    match (file_id_opt, content_opt) {
        (Some(file_id), Some(content)) => {
            let path_opt = lookup_file_path(&mounted_objects, file_id);
            match path_opt {
                Some(path) => {
                    // 권한 판정 — Window.save_to_file은 사용자 액션 (Ctrl+S).
                    let verdict = permission::judge(&owner, permission::Op::Save);
                    if verdict == permission::Verdict::Allow {
                        match file_write::save(&path, &content) {
                            Ok(()) => {
                                if let Some(w) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
                                    w.state.insert("dirty".into(), json!(false));
                                }
                                InvokeOutcome { state_sets: vec![(target_id, "dirty".to_string(), json!(false))] }
                            }
                            Err(e) => {
                                eprintln!("[desktop-shell] save 실패: {}", e);
                                InvokeOutcome::empty()
                            }
                        }
                    } else {
                        // 사용자 액션이 Confirm 필요한 케이스는 v1 없음 — 무시.
                        InvokeOutcome::empty()
                    }
                }
                None => InvokeOutcome::empty(),
            }
        }
        _ => InvokeOutcome::empty(),
    }
}
"save" => {
    // File.save(content) — AI 또는 외부 actor가 직접 호출.
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let path = lookup_file_path(&mounted_objects, target_id);
    let actor = sender_actor.clone(); // wire 메시지에서 추출 (Invoke msg의 actor 필드)
    match path {
        Some(p) => {
            let verdict = permission::judge(&actor, permission::Op::Save);
            match verdict {
                permission::Verdict::Allow => match file_write::save(&p, &content) {
                    Ok(()) => InvokeOutcome { state_sets: vec![(target_id, "dirty".to_string(), json!(false))] },
                    Err(e) => {
                        eprintln!("[desktop-shell] save 실패: {}", e);
                        InvokeOutcome::empty()
                    }
                },
                permission::Verdict::ConfirmRequired => {
                    // Dialog mount + pending 등록 + oneshot 대기.
                    let dialog = geulos_core::std_types::dialog(
                        owner.clone(),
                        "AI 저장 확인",
                        &format!("AI가 {}를 저장하려고 합니다 — 허용?", p.display()),
                        "confirm",
                        vec!["허용".to_string(), "거부".to_string()],
                    );
                    let dialog_id = dialog.id;
                    let mut dialog_with_parent = dialog;
                    dialog_with_parent.parent = Some(desktop_id);
                    add_wildcard_acl(&mut dialog_with_parent);
                    mounted_objects.push(dialog_with_parent.clone());
                    send_mount(&mut stream, &dialog_with_parent, &mut req_seq).await?;
                    subscribe(&mut stream, dialog_id, &mut req_seq).await?;

                    let (tx, rx) = tokio::sync::oneshot::channel();
                    pending.insert(
                        dialog_id,
                        dialog_ops::PendingSave { file_id: target_id, path: p.clone(), content, tx },
                    );
                    // 응답 대기 — main loop를 막지 않기 위해 spawn된 task에서 처리.
                    let pending_clone = pending.clone();
                    let stream_handle = stream_writer.clone(); // 별도 cloneable writer 필요
                    tokio::spawn(async move {
                        let label = rx.await.unwrap_or_else(|_| "거부".to_string());
                        if label == "허용" {
                            if let Some(p) = pending_clone.take(dialog_id) {
                                let _ = file_write::save(&p.path, &p.content);
                            }
                        }
                        // Dialog destroy (SetState destroyed=true).
                        // ... (T8.10 패턴과 동일) ...
                    });
                    InvokeOutcome::empty() // 즉시 응답 없음 — 비동기.
                }
            }
        }
        None => InvokeOutcome::empty(),
    }
}
"respond" => {
    // Dialog.respond(action) — pending이 있으면 oneshot으로 전달.
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("거부").to_string();
    if let Some(p) = pending.take(target_id) {
        let _ = p.tx.send(action);
    }
    // Dialog destroy (state.destroyed=true → layout/render 자연 제외).
    if let Some(d) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
        d.state.insert("destroyed".into(), json!(true));
    }
    InvokeOutcome { state_sets: vec![(target_id, "destroyed".to_string(), json!(true))] }
}
"close_confirm" => {
    // dirty=true인 Window의 [x] 클릭 흐름. desktop-shell이 Dialog 띄움.
    let dirty = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|w| w.state.get("dirty").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    if !dirty {
        // 즉시 close (기존 close 분기와 동일 — destroyed=true).
        InvokeOutcome { state_sets: vec![(target_id, "destroyed".to_string(), json!(true))] }
    } else {
        // Dialog mount + pending(close 정보)
        // v1 단순화: 3 버튼 다이얼로그 + 응답에 따라 분기. 본 구현은 plan T8 마지막에 통합.
        // 일단 close 무시(취소) + 안내 CLI 라인.
        InvokeOutcome::empty()
    }
}
```

**주의:** 위 코드 일부는 helper (`lookup_file_path`, `add_wildcard_acl`, `send_mount`, `subscribe`)에 의존 — 이미 코드베이스에 있음. `sender_actor`는 wire Invoke 메시지에서 추출해야 하므로 invoke 매개변수에 추가 필요할 수 있음 (proto Invoke 메시지 봐서 갱신).

- [ ] **Step 8.3: 컴포지터의 close 클릭 → close_confirm invoke로 변경**

Modify `compositor/src/main.rs`의 close button 클릭 분기:

```rust
if close_rect.contains(cx, cy) {
    let _ = self.ui_tx.try_send(UiAction::Invoke {
        target,
        method: "close_confirm".to_string(),  // 기존 "close" → "close_confirm"
        args: serde_json::Value::Null,
    });
}
```

(기존 close 메서드는 무조건 destroyed=true. close_confirm은 dirty 검사 후 분기.)

- [ ] **Step 8.4: 빌드 + 회귀 테스트**

Run: `cargo build --workspace`
Run: `cargo test --workspace`
Expected: 모두 PASS

- [ ] **Step 8.5: fmt + clippy**

Run: `cargo fmt --all` + `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 클린

- [ ] **Step 8.6: Commit**

```
git add apps/desktop-shell/src/main.rs apps/desktop-shell/src/window_ops.rs compositor/src/main.rs
git commit -m "feat(desktop-shell): M9 T8 — invoke 분기 save_to_file/save/toggle_edit/respond/close_confirm"
```

---

## Task 9: 통합 — 사용자 직접 편집/저장 end-to-end 수동 검증

**Files:** 없음 (검증만)

- [ ] **Step 9.1: 3 프로세스 spawn**

Run (각 별 터미널 또는 background):
```
cargo run -p geulos-server-host
cargo run -p geulos-desktop-shell
cargo run -p geulos-compositor
```

- [ ] **Step 9.2: 편집 흐름 검증**

1. compositor 창에서 좌측 FileTree → C:\ expand → 작은 .txt 파일 클릭 → Window 등장
2. Window 더블클릭 (또는 Ctrl+E) → title에 "[편집]" 표시
3. 본문 클릭 → cursor 등장
4. 키 입력 → content 즉시 갱신, title에 "* " 접두 추가 (dirty)
5. Ctrl+S → "*" 사라짐
6. 외부 에디터(notepad)로 같은 파일 열어 확인 → 변경 반영

- [ ] **Step 9.3: dirty close 흐름 (옵션 — v1은 단순 무시 또는 confirm dialog)**

1. dirty 상태에서 [x] 클릭
2. Dialog 등장 → 3 버튼 ("저장 후 닫기", "그냥 닫기", "취소")
3. 각 버튼 동작 확인

- [ ] **Step 9.4: 회귀 검증**

- M7 CLI 입력 OK
- M7 AI chat OK
- M8 viewer 모드 (edit_mode=false) 그대로 동작
- M8 scroll OK
- 아이콘 / zebra striping 회귀 X

문제 발견 시 → 해당 Task로 돌아가 수정 후 재검증.

- [ ] **Step 9.5: spawn 정리**

`PowerShell: Get-Process | Where-Object { $_.Name -in @('geulos-compositor','geulos-desktop-shell','geulos-server-host') } | Stop-Process -Force`

---

## Task 10: AI write 흐름 end-to-end 수동 검증

**Files:** 없음

- [ ] **Step 10.1: 3 프로세스 spawn (Task 9와 동일)**

- [ ] **Step 10.2: AI 채팅 모드 진입**

CLI에서 `/ai start test`. ANTHROPIC_API_KEY 입력 또는 이미 설정.

- [ ] **Step 10.3: AI에게 파일 저장 요청**

`{프로젝트 경로}/scratch.txt에 "hello from ai" 라고 저장해줘`

- [ ] **Step 10.4: Dialog 등장 확인**

화면 중앙에 Dialog 모달 등장. title "AI 저장 확인", message "AI가 ... 저장하려고 합니다", 버튼 ["허용", "거부"].

- [ ] **Step 10.5: 허용 흐름**

[허용] 클릭 → Dialog 사라짐 + 외부에서 파일 확인 → "hello from ai" 저장됨 + AI 응답 메시지 도착

- [ ] **Step 10.6: 거부 흐름**

(다시 같은 요청 후) [거부] 클릭 → Dialog 사라짐 + 파일 변경 안 됨 + AI 응답에 "거부됨" 같은 에러

- [ ] **Step 10.7: Modal block 확인**

(다른 요청 후 Dialog 떠있을 때) 좌측 FileTree 클릭 → 동작 안 함 (modal). Dialog 응답 후 정상 복귀.

- [ ] **Step 10.8: spawn 정리**

---

## Task 11: Acceptance 문서 + 회귀 가드

**Files:**
- Create: `docs/manual-tests/m9-acceptance.md`

- [ ] **Step 11.1: acceptance 문서 작성**

Create `docs/manual-tests/m9-acceptance.md`:

```markdown
# M9 Acceptance (편집/저장 + 권한 다이얼로그)

**Spec:** `docs/specs/2026-05-22-geulos-m9-edit-save-permission.md`
**Plan:** `docs/plans/2026-05-22-geulos-m9-edit-save-permission.md`

## 사전 조건
- 3 프로세스 (server-host → desktop-shell → compositor, KI-004 회피 순서)
- ANTHROPIC_API_KEY (시나리오 C/D — AI write)
- 쓰기 가능 텍스트 파일 (예: `~/scratch.txt` 또는 임의의 .txt)

## 시나리오 A — 사용자 직접 편집/저장
1. FileTree → .txt 파일 클릭 → Window viewer 등장
2. Ctrl+E (또는 더블클릭) → title에 "[편집]" 등장, edit_mode=true
3. 키 입력 → content 변경 + title에 "* " 접두
4. Ctrl+S → "* " 사라짐, dirty=false
5. 외부에서 파일 read → 변경 반영

## 시나리오 B — Dirty close 확인
6. 편집 후 [x] 클릭 → Dialog "저장 안 함" 등장 (3 버튼)
7. "저장 후 닫기" → save + Window destroy
8. (다시 편집 후) "그냥 닫기" → Window destroy + 디스크 변경 없음
9. (다시) "취소" → Window 유지

## 시나리오 C — AI 저장 허용
10. `/ai start test` → AI 채팅 진입
11. AI에게 "scratch.txt에 hello 저장해줘" → Dialog "AI 저장 확인" 등장
12. [허용] → Dialog 사라짐 + 파일 변경 + AI 응답 OK

## 시나리오 D — AI 저장 거부
13. (다시 같은 요청) → Dialog → [거부]
14. 파일 변경 X + AI 응답에 "거부" 에러

## 시나리오 E — Modal block
15. Dialog 떠있을 때 FileTree 클릭 → 무동작
16. CLI 키 입력 → 무동작
17. Dialog 응답 → 정상 복귀

## 통과 조건
- A~E 모든 단계 시각/동작 정확
- 회귀 0 — M7 (CLI, AI chat, IME) / M8 (viewer, scroll, multi-window, icons, zebra, parent nav) / M8.5 (stride 28, case-insensitive 정렬)

## 알려진 한계 (v2 / 이후)
- atomic write X — crash 시 원본 손상 가능
- multi-byte cursor가 grapheme 단위 X (한글 jamo 분해 분리)
- 1MB 초과 부분 편집 X
- 동시 AI write 큐잉 X — 두 번째 즉시 reject
- undo/redo X
- Binary edit_mode 비활성

## 회귀 가드
- core: `cargo test -p geulos-core --lib std_types` (Dialog/File.save/Window edit_mode 3개)
- desktop-shell: `cargo test -p geulos-desktop-shell --lib` (permission 8, file_write 3, dialog_ops 3, window_ops 1+)
- compositor: `cargo test -p geulos-compositor --lib editor` (8) + 기존 42

## 후속
- M10: 생성/삭제/rename (같은 권한 프레임워크 활용)
- v2: atomic write, undo, syntax highlight, multi-cursor, multi-byte cursor 정확도
```

- [ ] **Step 11.2: 회귀 가드 실행**

Run:
- `cargo test --workspace` (전 패키지 FAIL 0)
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

Expected: 모두 PASS / 클린

- [ ] **Step 11.3: Commit**

```
git add docs/manual-tests/m9-acceptance.md
git commit -m "test(m9): T11 — acceptance 시나리오 A~E + 회귀 가드"
```

---

## 마무리 (선택)

모든 T1~T11 완료 후 controller가:
1. T11 마지막 회귀 한 번 더 (`cargo test --workspace`)
2. ADR-035/spec/plan/acceptance review
3. main으로 push (M9 공식 마감)
4. M10 (생성/삭제/rename) 브레인스토밍 시작
