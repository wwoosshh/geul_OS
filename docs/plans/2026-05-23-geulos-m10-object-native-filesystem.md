# M10 — 객체-네이티브 파일시스템 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> **NEVER push** — controller가 마일스톤 끝에 batch push. subagent는 commit만.

**Goal:** GeulOS의 "사용자 GUI ≡ AI tree" 철학을 파일시스템에 적용. cwd 안은 자동 mount + watcher로 객체-네이티브, cwd 밖은 escape hatch.

**Architecture:** 3 phase. Phase 1 = Folder/File에 create/delete/rename 메서드 (M9 Dialog 인프라 재사용). Phase 2 = desktop-shell이 cwd를 watcher로 감시 → 외부 변경 자동 broadcast → AI는 subscribe만으로 인지. Phase 3 = cwd 밖 path-API singleton (`Filesystem@1`).

**Tech Stack:**
- 기존 Rust workspace + tokio + serde_json + M9 Dialog/permission 인프라
- Phase 2: `notify-rs` 7.x (cross-platform file watcher — ReadDirectoryChangesW / inotify / FSEvents)

**Spec parent:** `docs/specs/2026-05-23-geulos-m10-object-native-filesystem.md`

---

## Plan 구조

이 plan은 **Phase 1만 자세히** 기술 (Task 1~7). Phase 2/3은 *outline*만 — Phase 1 구현·검증 후 별도 plan으로 확장 작성.

- **Phase 1** (Task 1~7): Folder/File 객체 메서드 + Dialog grant 모델 + acceptance
- **Phase 2** (Task 8~12 outline): cwd auto-mount + notify-rs watcher
- **Phase 3** (Task 13~15 outline): Filesystem@1 escape hatch

---

# Phase 1 — 객체 메서드 + Grant 모델

## File Structure (Phase 1)

| 신규/수정 | 경로 | 책임 |
|---|---|---|
| Create | `docs/adr/036-object-native-filesystem.md` | ADR-036 본문 (Approach E 결정 근거) |
| Create | `apps/desktop-shell/src/granted_dirs.rs` | HashSet<PathBuf> in-memory + permission 통합 helper |
| Create | `apps/desktop-shell/src/folder_ops.rs` | create_file/create_folder/delete/rename 핸들러 + 단위 테스트 |
| Create | `apps/desktop-shell/src/file_ops.rs` | File.delete/File.rename 핸들러 + 단위 테스트 |
| Modify | `core/src/object/std_types.rs` | Folder@1에 create_file/create_folder/delete/rename, File@1에 delete/rename 메서드 등록 + 단위 테스트 |
| Modify | `apps/desktop-shell/src/permission.rs` | Op enum 확장 + `judge_with_path(actor, op, path, granted)` helper |
| Modify | `apps/desktop-shell/src/dialog_ops.rs` | `PendingFs` enum (Save / CreateFile / CreateFolder / Delete / Rename) |
| Modify | `apps/desktop-shell/src/main.rs` | 6 새 invoke 분기 + Dialog 흐름 통합 + GrantedDirs 인스턴스 |
| Modify | `apps/desktop-shell/src/lib.rs` | `pub mod granted_dirs/folder_ops/file_ops` |
| Modify | `ai-bridge/src/system_prompt.md` | Folder/File 생성·삭제·rename 흐름 + 디렉터리 grant 안내 |
| Create | `docs/manual-tests/m10-phase1-acceptance.md` | Phase 1 시나리오 H/I/L |

server 무변경.

---

## Task 1: ADR-036 + core 메서드 등록

**Files:**
- Create: `docs/adr/036-object-native-filesystem.md`
- Modify: `core/src/object/std_types.rs`

- [ ] **Step 1.1: ADR-036 작성**

Create `docs/adr/036-object-native-filesystem.md`:

```markdown
# ADR-036 — 객체-네이티브 파일시스템 (Approach E 하이브리드)

- **상태:** Accepted
- **결정일:** 2026-05-23
- **부모 spec:** `docs/specs/2026-05-23-geulos-m10-object-native-filesystem.md`

## Context

M9까지 AI는 사용자가 열어둔 File만 invoke 가능. 큰 프로젝트나 작업 이어가기는 불가.
일반적 해결 (Claude Code/Cursor)은 path-based fs API 도구 — *AI가 보는 정보가 사용자 GUI와
다름*. 사용자가 매번 상태 설명/캡처 필요.

GeulOS는 *모든 게 객체화*되어 AI auto-context가 가능해야 한다는 차별성 (README §3·4).

## Decision

Approach E — 하이브리드:
1. cwd 안 = 객체-네이티브. Folder/File에 create/delete/rename 메서드. desktop-shell이
   notify-rs로 외부 변경 감지 → 객체 state 자동 갱신.
2. cwd 밖 = path-API escape hatch (`aios.builtin/Filesystem@1.read_external/write_external`).
   사용자 Dialog 매번.
3. 권한: cwd 안 디렉터리 단위 grant + 삭제 항상 confirm. cwd 밖 모든 작업 confirm.

## Alternatives 거부

- **C (path-API wrapper)** — Claude Code 모방. 객체 모델의 강점 0. AI auto-context X. 거부.
- **D (cwd 밖 일체 금지)** — 외부 import/조회 불가능. 실용성 낮음. 거부.

## Consequences

- desktop-shell 부담 증가: file watcher 인프라 + granted_dirs 정책 + 3 phase 구현
- AI tooling 비약적 개선 — subscribe만으로 cwd 상태 실시간 인지
- M9의 Dialog/permission 인프라 그대로 활용
- 새 의존성 1개 (notify-rs 7.x)

## Trade-offs

- 큰 cwd (node_modules 포함)는 mount 폭발 가능 — v2에서 .gitignore + lazy + LRU
- granted_dirs 세션 영속 X — 재실행 시 reset (v2)
- Bash/glob/grep 도구 보류 — v2/M11

## 참고

- ADR-035 (M9 권한 Dialog)
- README §3·4 (객체 OS 차별성)
- T8.7 (lazy_mount 패턴)
```

- [ ] **Step 1.2: std_types.rs — Folder 메서드 추가**

Modify `core/src/object/std_types.rs` `pub fn folder(...)` 의 *메서드 등록 블록 끝*에 추가:

```rust
// M10 Phase 1 (ADR-036): 객체-네이티브 파일시스템 메서드.
// create_file/create_folder는 그 폴더 *안*에 새 파일/폴더를 만들고, 해당 객체 mount.
// delete는 폴더 자체를 삭제 (recursive=true면 자식 포함).
// rename은 폴더 자체의 이름 변경 — props.name + props.path 갱신.
obj.methods
    .push(MethodSig::new("create_file").with_arg(ArgSpec::new("name", "string")));
obj.methods
    .push(MethodSig::new("create_folder").with_arg(ArgSpec::new("name", "string")));
obj.methods.push(MethodSig::new("delete").with_arg(ArgSpec::new("recursive", "bool")));
obj.methods.push(MethodSig::new("rename").with_arg(ArgSpec::new("new_name", "string")));
```

- [ ] **Step 1.3: std_types.rs — File 메서드 추가**

Modify `core/src/object/std_types.rs` `pub fn file(...)` 의 메서드 등록 블록 끝 (기존 read/save 옆)에:

```rust
// M10 Phase 1 (ADR-036): 객체-네이티브 파일시스템 메서드.
obj.methods.push(MethodSig::new("delete"));
obj.methods.push(MethodSig::new("rename").with_arg(ArgSpec::new("new_name", "string")));
```

- [ ] **Step 1.4: 단위 테스트 — Folder/File 메서드 등록**

Add to `core/src/object/std_types.rs` 의 `#[cfg(test)] mod tests`:

```rust
#[test]
fn folder_has_fs_methods() {
    let f = folder(ActorId::local_user(), "/p", "p", 0);
    assert!(f.methods.iter().any(|m| m.name() == "create_file"));
    assert!(f.methods.iter().any(|m| m.name() == "create_folder"));
    assert!(f.methods.iter().any(|m| m.name() == "delete"));
    assert!(f.methods.iter().any(|m| m.name() == "rename"));
}

#[test]
fn file_has_fs_methods() {
    let f = file(ActorId::local_user(), "/x.txt", "x.txt", "text/plain", 0);
    assert!(f.methods.iter().any(|m| m.name() == "delete"));
    assert!(f.methods.iter().any(|m| m.name() == "rename"));
}
```

- [ ] **Step 1.5: 빌드 + 테스트**

Run: `cargo test -p geulos-core --lib std_types`
Expected: 새 2개 + 기존 std_types 테스트 모두 PASS

- [ ] **Step 1.6: Commit**

```
git add docs/adr/036-object-native-filesystem.md core/src/object/std_types.rs
git commit -m "feat(core)+adr: M10 T1 — Folder/File create·delete·rename 메서드 + ADR-036"
```

(Co-Authored-By 트레일러 포함)

---

## Task 2: granted_dirs 모듈

**Files:**
- Create: `apps/desktop-shell/src/granted_dirs.rs`
- Modify: `apps/desktop-shell/src/lib.rs` (`pub mod granted_dirs;`)

- [ ] **Step 2.1: granted_dirs.rs 신규**

Create `apps/desktop-shell/src/granted_dirs.rs`:

```rust
//! AI가 부여받은 *디렉터리 grant* in-memory 캐시 (M10 Phase 1 / ADR-036).
//!
//! 한 dir에 대해 [허용] Dialog를 한 번 처리하면 그 dir 안 후속 write/create/rename은
//! confirm 없이 통과. 세션 = desktop-shell process 한 번 실행 — AI 채팅 세션 (`/ai start
//! ... /exit`) 무관. process 종료 시 자연 reset.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Default)]
pub struct GrantedDirs {
    inner: Mutex<HashSet<PathBuf>>,
}

impl GrantedDirs {
    pub fn new() -> Self {
        Self::default()
    }

    /// 특정 dir에 대한 grant 여부.
    pub fn contains(&self, dir: &Path) -> bool {
        self.inner.lock().expect("GrantedDirs poisoned").contains(dir)
    }

    /// dir grant 추가. 이미 있으면 무동작.
    pub fn insert(&self, dir: PathBuf) {
        self.inner.lock().expect("GrantedDirs poisoned").insert(dir);
    }

    /// 현재 grant된 모든 dir 목록 (UI 표시·테스트용).
    pub fn list(&self) -> Vec<PathBuf> {
        let g = self.inner.lock().expect("GrantedDirs poisoned");
        g.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("GrantedDirs poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_then_contains() {
        let g = GrantedDirs::new();
        let d = PathBuf::from("/tmp/x");
        assert!(!g.contains(&d));
        g.insert(d.clone());
        assert!(g.contains(&d));
    }

    #[test]
    fn insert_duplicate_is_no_op() {
        let g = GrantedDirs::new();
        let d = PathBuf::from("/tmp/x");
        g.insert(d.clone());
        g.insert(d.clone());
        assert_eq!(g.len(), 1);
    }

    #[test]
    fn different_dirs_independent() {
        let g = GrantedDirs::new();
        g.insert(PathBuf::from("/a"));
        g.insert(PathBuf::from("/b"));
        assert!(g.contains(Path::new("/a")));
        assert!(g.contains(Path::new("/b")));
        assert!(!g.contains(Path::new("/c")));
        assert_eq!(g.len(), 2);
    }
}
```

- [ ] **Step 2.2: lib.rs 노출**

Modify `apps/desktop-shell/src/lib.rs`:

```rust
pub mod granted_dirs;
```

- [ ] **Step 2.3: 빌드 + 테스트**

Run: `cargo test -p geulos-desktop-shell --lib granted_dirs`
Expected: 3 tests PASS

- [ ] **Step 2.4: Commit**

```
git add apps/desktop-shell/src/granted_dirs.rs apps/desktop-shell/src/lib.rs
git commit -m "feat(desktop-shell): M10 T2 — granted_dirs HashSet (3 tests)"
```

---

## Task 3: permission 확장 (Op enum + path-aware judge)

**Files:**
- Modify: `apps/desktop-shell/src/permission.rs`

- [ ] **Step 3.1: Op enum 확장 + judge_with_path 추가**

Modify `apps/desktop-shell/src/permission.rs` — 기존 `Op` enum에 variant 추가 + 새 함수:

```rust
use std::path::Path;

use crate::granted_dirs::GrantedDirs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Save,
    CreateFile,
    CreateFolder,
    Delete,
    Rename,
}

// 기존 Create/Rename은 위 CreateFile/CreateFolder로 흡수. judge() 함수는 호환 위해
// Save만 처리 (M9 흐름 보존), 새 path-aware 로직은 judge_with_path.
//
// v1 정책 (cwd 안 가정 — cwd 밖은 Phase 3에서 별도 처리):
// - 사용자 (local-user): 모두 Allow (UI는 그 자체로 confirm)
// - AI:
//   - Delete: 항상 ConfirmRequired (위험)
//   - Save/CreateFile/CreateFolder/Rename: granted_dirs에 해당 dir 있으면 Allow,
//     없으면 ConfirmRequired (그리고 confirm 후 dir grant 추가)
pub fn judge_with_path(
    actor: &geulos_core::ActorId,
    op: Op,
    dir: &Path,
    granted: &GrantedDirs,
) -> Verdict {
    let is_local_user = actor == &geulos_core::ActorId::local_user();
    if is_local_user {
        // 사용자 직접 액션은 UI 자체가 confirm — permission 우회.
        return Verdict::Allow;
    }
    // AI 액션.
    if op == Op::Delete {
        return Verdict::ConfirmRequired;
    }
    if granted.contains(dir) {
        Verdict::Allow
    } else {
        Verdict::ConfirmRequired
    }
}
```

- [ ] **Step 3.2: 단위 테스트 추가**

Append to `apps/desktop-shell/src/permission.rs` `#[cfg(test)] mod tests`:

```rust
use crate::granted_dirs::GrantedDirs;
use std::path::Path;

#[test]
fn user_always_allowed_path() {
    let g = GrantedDirs::new();
    let user = geulos_core::ActorId::local_user();
    assert_eq!(judge_with_path(&user, Op::Save, Path::new("/x"), &g), Verdict::Allow);
    assert_eq!(judge_with_path(&user, Op::Delete, Path::new("/x"), &g), Verdict::Allow);
    assert_eq!(judge_with_path(&user, Op::CreateFile, Path::new("/x"), &g), Verdict::Allow);
}

#[test]
fn ai_delete_always_confirm() {
    let g = GrantedDirs::new();
    g.insert(std::path::PathBuf::from("/x"));
    let ai = geulos_core::ActorId::new_ai_session();
    // delete는 granted여도 confirm.
    assert_eq!(judge_with_path(&ai, Op::Delete, Path::new("/x"), &g), Verdict::ConfirmRequired);
}

#[test]
fn ai_save_in_granted_dir_allowed() {
    let g = GrantedDirs::new();
    g.insert(std::path::PathBuf::from("/x"));
    let ai = geulos_core::ActorId::new_ai_session();
    assert_eq!(judge_with_path(&ai, Op::Save, Path::new("/x"), &g), Verdict::Allow);
    assert_eq!(judge_with_path(&ai, Op::CreateFile, Path::new("/x"), &g), Verdict::Allow);
    assert_eq!(judge_with_path(&ai, Op::Rename, Path::new("/x"), &g), Verdict::Allow);
}

#[test]
fn ai_save_in_ungranted_dir_confirm() {
    let g = GrantedDirs::new();
    let ai = geulos_core::ActorId::new_ai_session();
    assert_eq!(judge_with_path(&ai, Op::Save, Path::new("/x"), &g), Verdict::ConfirmRequired);
    assert_eq!(
        judge_with_path(&ai, Op::CreateFolder, Path::new("/x"), &g),
        Verdict::ConfirmRequired
    );
}

#[test]
fn ai_grant_is_per_dir() {
    let g = GrantedDirs::new();
    g.insert(std::path::PathBuf::from("/a"));
    let ai = geulos_core::ActorId::new_ai_session();
    assert_eq!(judge_with_path(&ai, Op::Save, Path::new("/a"), &g), Verdict::Allow);
    assert_eq!(judge_with_path(&ai, Op::Save, Path::new("/b"), &g), Verdict::ConfirmRequired);
}
```

(주의: Op enum이 Create/Rename → CreateFile/CreateFolder로 분리됐으니, 기존 M9 `Op::Create` / `Op::Rename` 참조 부분 호환 확인. M9는 *enum variant 정의만* 추가 — 실제 사용 없음 — 별 호환 문제 없을 것. 단 `judge()` 함수의 `(true, Op::Create)` 등 매치 갱신 필요.)

- [ ] **Step 3.3: judge() 함수도 갱신 (Op variant 변경 반영)**

기존 `judge()` 함수의 `match` 패턴 갱신:

```rust
pub fn judge(actor: &geulos_core::ActorId, op: Op) -> Verdict {
    let is_local_user = actor == &geulos_core::ActorId::local_user();
    match (is_local_user, op) {
        (true, Op::Save) => Verdict::Allow,
        (true, Op::CreateFile) => Verdict::Allow,
        (true, Op::CreateFolder) => Verdict::Allow,
        (true, Op::Rename) => Verdict::Allow,
        (true, Op::Delete) => Verdict::ConfirmRequired,
        (false, _) => Verdict::ConfirmRequired,
    }
}
```

(기존 8 단위 테스트 중 `Op::Create`/`Op::Rename` 참조도 `Op::CreateFile`/`Op::Rename`로 갱신 — `user_create_allowed` → 그대로 (Op::CreateFile), `ai_create_requires_confirm` → 그대로 (Op::CreateFile).)

- [ ] **Step 3.4: 빌드 + 테스트**

Run: `cargo test -p geulos-desktop-shell --lib permission`
Expected: 기존 8 + 새 5 = 13 PASS

- [ ] **Step 3.5: fmt + clippy**

```
cargo fmt --all
cargo clippy -p geulos-desktop-shell --all-targets -- -D warnings
```

- [ ] **Step 3.6: Commit**

```
git add apps/desktop-shell/src/permission.rs
git commit -m "feat(desktop-shell): M10 T3 — permission Op 확장 + judge_with_path (13 tests)"
```

---

## Task 4: folder_ops 핸들러

**Files:**
- Create: `apps/desktop-shell/src/folder_ops.rs`
- Modify: `apps/desktop-shell/src/lib.rs`

- [ ] **Step 4.1: folder_ops.rs 신규**

Create `apps/desktop-shell/src/folder_ops.rs`:

```rust
//! Folder@1의 create_file/create_folder/delete/rename 핸들러 (M10 Phase 1 / ADR-036).
//!
//! 각 함수는 *순수 fs operation* + 결과의 새 객체 (또는 destroyed marker). main.rs invoke
//! 분기가 permission 판정 + Dialog 흐름 + state broadcast를 wrap.

use std::path::{Path, PathBuf};

use geulos_core::{std_types, ActorId, Object, ObjectId};

/// 폴더 안에 새 빈 파일 생성. 결과는 mount할 File@1 객체.
///
/// path 충돌 (이미 존재)이면 Err. fs::write가 *replace*하지 않도록 사전 check.
pub fn create_file_in(
    owner: &ActorId,
    folder_path: &Path,
    name: &str,
    now_ms: i64,
) -> Result<Object, String> {
    let new_path = folder_path.join(name);
    if new_path.exists() {
        return Err(format!("이미 존재: {}", new_path.display()));
    }
    std::fs::write(&new_path, "")
        .map_err(|e| format!("파일 생성 실패: {}", e))?;
    let mime = crate::lazy_mount::guess_mime(name);
    let mut obj = std_types::file(
        owner.clone(),
        new_path.to_string_lossy().as_ref(),
        name,
        &mime,
        now_ms,
    );
    obj.set_state("last_change_actor", serde_json::json!("ai"));
    obj.set_state("last_change_ms", serde_json::json!(now_ms));
    Ok(obj)
}

/// 폴더 안에 새 빈 폴더 생성.
pub fn create_folder_in(
    owner: &ActorId,
    folder_path: &Path,
    name: &str,
    now_ms: i64,
) -> Result<Object, String> {
    let new_path = folder_path.join(name);
    if new_path.exists() {
        return Err(format!("이미 존재: {}", new_path.display()));
    }
    std::fs::create_dir(&new_path).map_err(|e| format!("폴더 생성 실패: {}", e))?;
    let mut obj = std_types::folder(
        owner.clone(),
        new_path.to_string_lossy().as_ref(),
        name,
        now_ms,
    );
    obj.set_state("last_change_actor", serde_json::json!("ai"));
    obj.set_state("last_change_ms", serde_json::json!(now_ms));
    Ok(obj)
}

/// 폴더 자체 삭제. recursive=true면 자식 포함.
pub fn delete_folder(path: &Path, recursive: bool) -> Result<(), String> {
    if recursive {
        std::fs::remove_dir_all(path).map_err(|e| format!("폴더 재귀 삭제 실패: {}", e))
    } else {
        std::fs::remove_dir(path).map_err(|e| format!("폴더 삭제 실패: {}", e))
    }
}

/// 폴더 이름 변경. 결과는 새 PathBuf.
pub fn rename_folder(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let parent = path.parent().ok_or_else(|| "부모 디렉터리 없음".to_string())?;
    let new_path = parent.join(new_name);
    if new_path.exists() {
        return Err(format!("이미 존재: {}", new_path.display()));
    }
    std::fs::rename(path, &new_path).map_err(|e| format!("폴더 이름 변경 실패: {}", e))?;
    Ok(new_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_file_in_empty_folder() {
        let dir = tempdir().unwrap();
        let owner = ActorId::local_user();
        let obj = create_file_in(&owner, dir.path(), "x.txt", 0).expect("ok");
        assert_eq!(obj.props.get("name").and_then(|v| v.as_str()), Some("x.txt"));
        assert!(dir.path().join("x.txt").exists());
    }

    #[test]
    fn create_file_conflict_errors() {
        let dir = tempdir().unwrap();
        let owner = ActorId::local_user();
        create_file_in(&owner, dir.path(), "x.txt", 0).expect("ok");
        let err = create_file_in(&owner, dir.path(), "x.txt", 0).unwrap_err();
        assert!(err.contains("이미 존재"));
    }

    #[test]
    fn create_folder_in_empty_folder() {
        let dir = tempdir().unwrap();
        let owner = ActorId::local_user();
        let obj = create_folder_in(&owner, dir.path(), "sub", 0).expect("ok");
        assert_eq!(obj.props.get("name").and_then(|v| v.as_str()), Some("sub"));
        assert!(dir.path().join("sub").is_dir());
    }

    #[test]
    fn delete_folder_recursive() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("a");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("x.txt"), "x").unwrap();
        // 비-recursive는 실패해야 (not empty).
        assert!(delete_folder(&sub, false).is_err());
        // recursive는 성공.
        delete_folder(&sub, true).expect("ok");
        assert!(!sub.exists());
    }

    #[test]
    fn rename_folder_returns_new_path() {
        let dir = tempdir().unwrap();
        let old = dir.path().join("old");
        std::fs::create_dir(&old).unwrap();
        let new = rename_folder(&old, "new").expect("ok");
        assert!(!old.exists());
        assert!(new.is_dir());
        assert_eq!(new.file_name().unwrap(), "new");
    }

    #[test]
    fn rename_folder_conflict_errors() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let err = rename_folder(&a, "b").unwrap_err();
        assert!(err.contains("이미 존재"));
    }
}
```

- [ ] **Step 4.2: lib.rs 노출**

```rust
pub mod folder_ops;
```

- [ ] **Step 4.3: 빌드 + 테스트**

Run: `cargo test -p geulos-desktop-shell --lib folder_ops`
Expected: 6 tests PASS

- [ ] **Step 4.4: Commit**

```
git add apps/desktop-shell/src/folder_ops.rs apps/desktop-shell/src/lib.rs
git commit -m "feat(desktop-shell): M10 T4 — folder_ops create/delete/rename (6 tests)"
```

---

## Task 5: file_ops (File.delete/File.rename)

**Files:**
- Create: `apps/desktop-shell/src/file_ops.rs`
- Modify: `apps/desktop-shell/src/lib.rs`

- [ ] **Step 5.1: file_ops.rs 신규**

Create `apps/desktop-shell/src/file_ops.rs`:

```rust
//! File@1의 delete/rename 핸들러 (M10 Phase 1 / ADR-036).
//! file_write::save와 분리 — save는 *content 갱신*, file_ops는 *fs 객체 자체 조작*.

use std::path::{Path, PathBuf};

pub fn delete_file(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| format!("파일 삭제 실패: {}", e))
}

pub fn rename_file(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let parent = path.parent().ok_or_else(|| "부모 디렉터리 없음".to_string())?;
    let new_path = parent.join(new_name);
    if new_path.exists() {
        return Err(format!("이미 존재: {}", new_path.display()));
    }
    std::fs::rename(path, &new_path).map_err(|e| format!("파일 이름 변경 실패: {}", e))?;
    Ok(new_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn delete_existing_file() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("x.txt");
        std::fs::write(&p, "hi").unwrap();
        delete_file(&p).expect("ok");
        assert!(!p.exists());
    }

    #[test]
    fn delete_missing_returns_err() {
        let err = delete_file(Path::new("/nope/never")).unwrap_err();
        assert!(err.contains("파일 삭제 실패"));
    }

    #[test]
    fn rename_file_to_new_name() {
        let dir = tempdir().unwrap();
        let old = dir.path().join("old.txt");
        std::fs::write(&old, "x").unwrap();
        let new = rename_file(&old, "new.txt").expect("ok");
        assert!(!old.exists());
        assert!(new.is_file());
    }

    #[test]
    fn rename_conflict_errors() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "x").unwrap();
        std::fs::write(&b, "y").unwrap();
        let err = rename_file(&a, "b.txt").unwrap_err();
        assert!(err.contains("이미 존재"));
    }
}
```

- [ ] **Step 5.2: lib.rs 노출**

```rust
pub mod file_ops;
```

- [ ] **Step 5.3: 빌드 + 테스트**

Run: `cargo test -p geulos-desktop-shell --lib file_ops`
Expected: 4 tests PASS

- [ ] **Step 5.4: Commit**

```
git add apps/desktop-shell/src/file_ops.rs apps/desktop-shell/src/lib.rs
git commit -m "feat(desktop-shell): M10 T5 — file_ops delete/rename (4 tests)"
```

---

## Task 6: dialog_ops PendingFs enum 확장

**Files:**
- Modify: `apps/desktop-shell/src/dialog_ops.rs`

- [ ] **Step 6.1: PendingFs enum 신규 + PendingMap 일반화**

Modify `apps/desktop-shell/src/dialog_ops.rs` — 기존 `PendingSave`를 `PendingFs` enum으로 확장:

```rust
//! Dialog@1 mount/respond + Pending (사용자 응답 대기) 매핑 (M9/M10).
//!
//! M10 Phase 1 확장: 한 Dialog가 *다양한 fs 작업* (save/create_file/create_folder/delete/
//! rename)에 대응. PendingFs enum이 그 종류를 카테고리화.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use geulos_core::ObjectId;
use tokio::sync::oneshot;

/// pending 작업의 종류. 사용자가 Dialog에 응답하면 desktop-shell이 이 enum을 보고 분기.
#[derive(Debug)]
pub enum PendingFs {
    /// File@1.save — args.content를 디스크에 commit. file_id로 path lookup.
    Save { file_id: ObjectId, path: PathBuf, content: String },
    /// Folder@1.create_file — folder 안에 새 빈 파일.
    CreateFile { folder_id: ObjectId, folder_path: PathBuf, name: String },
    /// Folder@1.create_folder — folder 안에 새 빈 폴더.
    CreateFolder { folder_id: ObjectId, folder_path: PathBuf, name: String },
    /// File@1.delete — 파일 자체 삭제.
    DeleteFile { file_id: ObjectId, path: PathBuf },
    /// Folder@1.delete — 폴더 자체 삭제 (recursive flag).
    DeleteFolder { folder_id: ObjectId, path: PathBuf, recursive: bool },
    /// File@1.rename or Folder@1.rename.
    Rename { target_id: ObjectId, path: PathBuf, new_name: String, is_folder: bool },
}

pub struct PendingEntry {
    pub op: PendingFs,
    pub tx: oneshot::Sender<String>,
}

#[derive(Default)]
pub struct PendingMap {
    inner: Mutex<HashMap<ObjectId, PendingEntry>>,
}

impl PendingMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, dialog_id: ObjectId, entry: PendingEntry) {
        self.inner.lock().expect("PendingMap poisoned").insert(dialog_id, entry);
    }

    pub fn take(&self, dialog_id: ObjectId) -> Option<PendingEntry> {
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

// 기존 호환 — M9 코드 (save 분기)가 PendingSave를 import해서 사용했음. M10 이후 모든 호출
// 처는 PendingFs::Save로 변환. M9 호환 별 type alias 유지하면 migration 부담 적음.
//
// **deprecated**: PendingSave는 PendingFs::Save로 흡수. M9 호출처 갱신 후 제거 가능.
#[deprecated(note = "use PendingFs::Save + PendingEntry instead")]
pub struct PendingSave {
    pub file_id: ObjectId,
    pub path: PathBuf,
    pub content: String,
    pub tx: oneshot::Sender<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn insert_take_save_entry() {
        let map = PendingMap::new();
        let did = ObjectId::new();
        let (tx, _rx) = oneshot::channel();
        map.insert(
            did,
            PendingEntry {
                op: PendingFs::Save {
                    file_id: ObjectId::new(),
                    path: PathBuf::from("/x"),
                    content: "y".into(),
                },
                tx,
            },
        );
        assert!(map.contains(did));
        let taken = map.take(did).expect("present");
        assert!(matches!(taken.op, PendingFs::Save { .. }));
    }

    #[test]
    fn insert_take_create_file_entry() {
        let map = PendingMap::new();
        let did = ObjectId::new();
        let (tx, _rx) = oneshot::channel();
        map.insert(
            did,
            PendingEntry {
                op: PendingFs::CreateFile {
                    folder_id: ObjectId::new(),
                    folder_path: PathBuf::from("/p"),
                    name: "x.txt".into(),
                },
                tx,
            },
        );
        let taken = map.take(did).expect("present");
        match taken.op {
            PendingFs::CreateFile { name, .. } => assert_eq!(name, "x.txt"),
            _ => panic!("expected CreateFile"),
        }
    }

    #[test]
    fn take_missing_returns_none() {
        let map = PendingMap::new();
        assert!(map.take(ObjectId::new()).is_none());
    }
}
```

- [ ] **Step 6.2: M9 save 분기 마이그레이션 — main.rs save 분기에서 PendingFs::Save 사용**

(이는 *작은 갱신* — desktop-shell main.rs save 분기의 `pending.insert(dialog_id, PendingSave {...})`를 `pending.insert(dialog_id, PendingEntry { op: PendingFs::Save {...}, tx })`로 바꿈. respond 분기도 PendingFs match로 변경.)

main.rs save 분기 갱신 (M10 Task 7과 함께):

```rust
// 기존:
// pending.insert(dialog_id, dialog_ops::PendingSave { file_id: target_id, path: p.clone(), content, tx });
// 변경:
let (tx, _rx) = tokio::sync::oneshot::channel();
pending.insert(
    dialog_id,
    dialog_ops::PendingEntry {
        op: dialog_ops::PendingFs::Save { file_id: target_id, path: p.clone(), content },
        tx,
    },
);
```

respond 분기:

```rust
// 기존:
// if let Some(p) = pending.take(target_id) { ... file_write::save(&p.path, &p.content) ... }
// 변경:
if let Some(entry) = pending.take(target_id) {
    if action == "허용" {
        match entry.op {
            dialog_ops::PendingFs::Save { path, content, .. } => {
                let _ = file_write::save(&path, &content);
            }
            dialog_ops::PendingFs::CreateFile { folder_path, name, .. } => {
                let _ = folder_ops::create_file_in(&owner, &folder_path, &name, /* now_ms */);
                // mount + Folder.children 갱신은 Task 7 main 분기에서 별도 처리.
            }
            // ... 다른 PendingFs variant ...
        }
    }
    drop(entry.tx);
}
```

(전체 흐름은 Task 7 main 분기에서 통합.)

- [ ] **Step 6.3: 빌드 + 테스트**

Run: `cargo test -p geulos-desktop-shell --lib dialog_ops`
Expected: 3 PASS (M9 3 tests를 새 형식으로 갱신)

(M9 기존 tests `insert_and_take_round_trip` / `take_missing_returns_none` / `respond_wakes_oneshot`도 새 `PendingEntry { op: PendingFs::Save {...}, tx }` 형식으로 갱신.)

- [ ] **Step 6.4: fmt + clippy**

```
cargo fmt --all
cargo clippy -p geulos-desktop-shell --all-targets -- -D warnings
```

- [ ] **Step 6.5: Commit**

```
git add apps/desktop-shell/src/dialog_ops.rs
git commit -m "feat(desktop-shell): M10 T6 — PendingFs enum (Save/CreateFile/CreateFolder/Delete/Rename)"
```

---

## Task 7: main.rs invoke 분기 통합 (5 새 method)

**Files:**
- Modify: `apps/desktop-shell/src/main.rs`
- Modify: `ai-bridge/src/system_prompt.md`

- [ ] **Step 7.1: main.rs — GrantedDirs 인스턴스 + create_file/create_folder/delete/rename 분기**

main.rs 메인 task 진입 부분에:

```rust
let granted = std::sync::Arc::new(granted_dirs::GrantedDirs::new());
```

invoke handler `match method`에 5 새 case 추가 (save_to_file/save 분기 옆):

```rust
"create_file" => {
    // Folder@1.create_file(name) — 그 폴더 안에 빈 파일 생성. permission grant 확인.
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let folder_path_opt = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|f| f.props.get("path").and_then(|v| v.as_str()))
        .map(std::path::PathBuf::from);
    match folder_path_opt {
        Some(folder_path) => {
            let verdict = permission::judge_with_path(
                &sender_actor,
                permission::Op::CreateFile,
                &folder_path,
                &granted,
            );
            match verdict {
                permission::Verdict::Allow => {
                    let now = chrono::Utc::now().timestamp_millis();
                    match folder_ops::create_file_in(&owner, &folder_path, &name, now) {
                        Ok(mut new_obj) => {
                            new_obj.parent = Some(target_id);
                            add_wildcard_acl(&mut new_obj);
                            let new_id = new_obj.id;
                            let mm = MountMsg {
                                root_object_id: new_id.to_string(),
                                tree: serde_json::to_value(&new_obj)?,
                            };
                            stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
                            req_seq += 1;
                            let sub = SubscribeMsg {
                                subscription_id: format!("sub-runtime-{}", req_seq),
                                target: new_id.to_string(),
                                kinds: vec![EventKindFilterWire::Invoke],
                                include_initial: false,
                            };
                            stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
                            if let Some(p) =
                                mounted_objects.iter_mut().find(|o| o.id == target_id)
                            {
                                p.children.push(new_id);
                            }
                            mounted_objects.push(new_obj);
                            eprintln!(
                                "[desktop-shell] create_file OK → {}/{}",
                                folder_path.display(),
                                name
                            );
                            invoke_handler::InvokeOutcome::empty()
                        }
                        Err(e) => {
                            eprintln!("[desktop-shell] create_file 실패: {}", e);
                            invoke_handler::InvokeOutcome::empty()
                        }
                    }
                }
                permission::Verdict::ConfirmRequired => {
                    // Dialog mount + Pending에 CreateFile 저장.
                    let mut dialog = geulos_core::std_types::dialog(
                        owner.clone(),
                        "AI 파일 생성 확인",
                        &format!(
                            "AI가 {} 안에 '{}'를 생성하려고 합니다 — 허용?",
                            folder_path.display(),
                            name
                        ),
                        "confirm",
                        vec!["허용".to_string(), "거부".to_string()],
                    );
                    dialog.parent = Some(desktop_id);
                    add_wildcard_acl(&mut dialog);
                    let dialog_id = dialog.id;
                    let mm = MountMsg {
                        root_object_id: dialog_id.to_string(),
                        tree: serde_json::to_value(&dialog)?,
                    };
                    stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
                    req_seq += 1;
                    let sub = SubscribeMsg {
                        subscription_id: format!("sub-runtime-{}", req_seq),
                        target: dialog_id.to_string(),
                        kinds: vec![EventKindFilterWire::Invoke],
                        include_initial: false,
                    };
                    stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
                    mounted_objects.push(dialog);
                    let (tx, _rx) = tokio::sync::oneshot::channel();
                    pending.insert(
                        dialog_id,
                        dialog_ops::PendingEntry {
                            op: dialog_ops::PendingFs::CreateFile {
                                folder_id: target_id,
                                folder_path,
                                name,
                            },
                            tx,
                        },
                    );
                    invoke_handler::InvokeOutcome::empty()
                }
            }
        }
        None => invoke_handler::InvokeOutcome::empty(),
    }
}
"create_folder" => {
    // (위 create_file와 구조 동일 — folder_ops::create_folder_in 호출. Dialog 메시지만
    //  "폴더 생성"으로 변경, PendingFs::CreateFolder.)
    // [이하 구조 동일 — 코드 중복 — implementer가 작성]
}
"delete" => {
    // File 또는 Folder 분기 — target object의 type_uri로 판정.
    let target_obj_kind = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .map(|o| o.type_uri.as_str().to_string());
    let path_opt = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|o| o.props.get("path").and_then(|v| v.as_str()))
        .map(std::path::PathBuf::from);
    let recursive = args.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false);
    match (target_obj_kind.as_deref(), path_opt) {
        (Some("aios.std/File@1"), Some(path)) => {
            // Delete는 *항상 ConfirmRequired* (granted 무관).
            let mut dialog = geulos_core::std_types::dialog(
                owner.clone(),
                "AI 파일 삭제 확인",
                &format!("AI가 {}를 삭제하려고 합니다 — 허용?", path.display()),
                "warn",
                vec!["허용".to_string(), "거부".to_string()],
            );
            dialog.parent = Some(desktop_id);
            add_wildcard_acl(&mut dialog);
            let dialog_id = dialog.id;
            let mm = MountMsg {
                root_object_id: dialog_id.to_string(),
                tree: serde_json::to_value(&dialog)?,
            };
            stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
            req_seq += 1;
            let sub = SubscribeMsg {
                subscription_id: format!("sub-runtime-{}", req_seq),
                target: dialog_id.to_string(),
                kinds: vec![EventKindFilterWire::Invoke],
                include_initial: false,
            };
            stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
            mounted_objects.push(dialog);
            let (tx, _rx) = tokio::sync::oneshot::channel();
            pending.insert(
                dialog_id,
                dialog_ops::PendingEntry {
                    op: dialog_ops::PendingFs::DeleteFile { file_id: target_id, path },
                    tx,
                },
            );
            invoke_handler::InvokeOutcome::empty()
        }
        (Some("aios.std/Folder@1"), Some(path)) => {
            // Folder delete — 동일 Dialog, PendingFs::DeleteFolder.
            // [구조 동일 — implementer 작성]
            invoke_handler::InvokeOutcome::empty()
        }
        _ => invoke_handler::InvokeOutcome::empty(),
    }
}
"rename" => {
    // File 또는 Folder — target type 판정 후 file_ops::rename_file 또는 folder_ops::rename_folder.
    // (위와 동일 패턴 — Dialog grant 흐름, PendingFs::Rename.)
    // [구조 동일 — implementer 작성]
    invoke_handler::InvokeOutcome::empty()
}
```

- [ ] **Step 7.2: respond 분기 확장 — PendingFs 전체 처리**

main.rs `"respond"` 분기에서 *위 모든 PendingFs variant* 처리. 허용 시 각자 다른 fs operation 호출 + 적절한 객체 mount/destroy/state 갱신.

```rust
"respond" => {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("거부").to_string();
    if let Some(entry) = pending.take(target_id) {
        if action == "허용" {
            let now = chrono::Utc::now().timestamp_millis();
            match entry.op {
                dialog_ops::PendingFs::Save { path, content, .. } => {
                    let _ = file_write::save(&path, &content);
                }
                dialog_ops::PendingFs::CreateFile { folder_id, folder_path, name } => {
                    if let Ok(mut new_obj) =
                        folder_ops::create_file_in(&owner, &folder_path, &name, now)
                    {
                        new_obj.parent = Some(folder_id);
                        add_wildcard_acl(&mut new_obj);
                        let new_id = new_obj.id;
                        let mm = MountMsg {
                            root_object_id: new_id.to_string(),
                            tree: serde_json::to_value(&new_obj)?,
                        };
                        stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
                        req_seq += 1;
                        let sub = SubscribeMsg {
                            subscription_id: format!("sub-runtime-{}", req_seq),
                            target: new_id.to_string(),
                            kinds: vec![EventKindFilterWire::Invoke],
                            include_initial: false,
                        };
                        stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
                        if let Some(p) =
                            mounted_objects.iter_mut().find(|o| o.id == folder_id)
                        {
                            p.children.push(new_id);
                        }
                        mounted_objects.push(new_obj);
                    }
                    // dir grant 추가 — 다음 작업부터 confirm 없음.
                    granted.insert(folder_path);
                }
                dialog_ops::PendingFs::CreateFolder { folder_id, folder_path, name } => {
                    // [동일 패턴]
                    granted.insert(folder_path);
                }
                dialog_ops::PendingFs::DeleteFile { file_id, path } => {
                    if file_ops::delete_file(&path).is_ok() {
                        if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == file_id)
                        {
                            o.state.insert("destroyed".into(), serde_json::json!(true));
                        }
                    }
                    // Delete는 grant 안 함 (다음 delete도 confirm).
                }
                dialog_ops::PendingFs::DeleteFolder { folder_id, path, recursive } => {
                    if folder_ops::delete_folder(&path, recursive).is_ok() {
                        if let Some(o) =
                            mounted_objects.iter_mut().find(|o| o.id == folder_id)
                        {
                            o.state.insert("destroyed".into(), serde_json::json!(true));
                        }
                    }
                }
                dialog_ops::PendingFs::Rename { target_id: tid, path, new_name, is_folder } => {
                    let result = if is_folder {
                        folder_ops::rename_folder(&path, &new_name)
                    } else {
                        file_ops::rename_file(&path, &new_name)
                    };
                    if let Ok(new_path) = result {
                        if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == tid) {
                            o.props.insert("name".into(), serde_json::json!(new_name));
                            o.props.insert(
                                "path".into(),
                                serde_json::json!(new_path.to_string_lossy()),
                            );
                        }
                        // grant: 부모 dir 추가.
                        if let Some(parent) = new_path.parent() {
                            granted.insert(parent.to_path_buf());
                        }
                    }
                }
            }
        }
        drop(entry.tx);
    }
    // Dialog destroy — 동일 패턴 (M9).
    if let Some(d) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
        d.state.insert("destroyed".into(), serde_json::json!(true));
    }
    invoke_handler::InvokeOutcome {
        state_sets: vec![(target_id, "destroyed".to_string(), serde_json::json!(true))],
    }
}
```

- [ ] **Step 7.3: ai-bridge system_prompt 확장**

Modify `ai-bridge/src/system_prompt.md` — "Standard types" 섹션의 Folder/File 메서드 갱신:

```
- **aios.std/Folder@1** — 파일시스템 폴더. props.path/name. children = Folder/File.
  Methods: `read`, `create_file(name)`, `create_folder(name)`, `delete(recursive)`, `rename(new_name)`.
  Write/delete/rename은 디렉터리 단위 사용자 확인 Dialog. 한 번 [허용]하면 그 dir 안 후속
  write/create/rename은 자유 (delete는 항상 confirm).
- **aios.std/File@1** — 파일. props.path/name/mime.
  Methods: `read`, `save(content)`, `delete()`, `rename(new_name)`. (save는 M9, 나머지는 M10.)
```

"Saving a file" 섹션 다음에 새 섹션 추가:

```
### Creating/deleting/renaming files (M10)

새 파일/폴더는 *현재 mount된 Folder*에 invoke해서 만든다. arbitrary path는 cwd 밖이면
`Filesystem@1.write_external` (Phase 3) 사용. 그 외엔 항상 Folder/File 객체 메서드.

흐름 예:
1. `list_objects_by_type("aios.std/Folder@1")` → 적합한 folder_id 식별
2. `invoke_method(target=<folder_id>, method="create_file", args={"name": "foo.rs"})`
3. 사용자에게 Dialog가 뜸 → 허용 시 file 생성 + 새 File 객체 mount.

삭제는 *항상* 사용자 confirm. 신중히 — recursive=true는 디렉터리 전체 재귀 삭제.
```

- [ ] **Step 7.4: 빌드 + 회귀 + lint**

```
cargo build --workspace
cargo test --workspace
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: 빌드 + 모든 테스트 PASS

- [ ] **Step 7.5: Commit**

```
git add apps/desktop-shell/src/main.rs ai-bridge/src/system_prompt.md
git commit -m "feat(desktop-shell)+(ai-bridge): M10 T7 — create_file/create_folder/delete/rename invoke 분기 + AI prompt"
```

---

## Task 8: Phase 1 acceptance 문서 + 회귀

**Files:**
- Create: `docs/manual-tests/m10-phase1-acceptance.md`

- [ ] **Step 8.1: acceptance 문서 작성**

Create `docs/manual-tests/m10-phase1-acceptance.md`:

```markdown
# M10 Phase 1 Acceptance — Folder/File 객체 메서드 + Dialog grant

**Spec:** `docs/specs/2026-05-23-geulos-m10-object-native-filesystem.md`
**Plan:** `docs/plans/2026-05-23-geulos-m10-object-native-filesystem.md`

## 사전 조건
- 3 프로세스 spawn
- ANTHROPIC_API_KEY (AI write)
- 쓰기 가능한 작은 디렉터리 (예: D:\GeulOS\scratch\)

## 시나리오 H — AI 파일 생성 + grant
1. CLI `/ai start` → AI에게 "D:\GeulOS\scratch 안에 hello.txt 만들어줘"
2. AI가 list_objects_by_type → Folder.create_file invoke
3. **Dialog** 등장 "AI가 ... 'hello.txt'를 생성하려고 합니다" → [허용]
4. 파일 생성 확인 + Folder children 갱신
5. (이어서) "같은 폴더에 world.txt 만들어줘"
6. **Dialog 없이** 즉시 생성 (grant 적용)

## 시나리오 I — AI 파일 삭제 (항상 confirm)
7. AI에게 "hello.txt 지워줘"
8. **Dialog** 등장 (grant 무관) → [허용] → 파일 삭제
9. (다시) "world.txt도 지워줘" → **Dialog 또 등장** (delete는 항상)

## 시나리오 L — rename
10. AI에게 "world.txt를 final.txt로 이름변경" → Dialog → 허용 → 이름변경
11. (같은 dir에서) 두 번째 rename → Dialog 없음 (rename도 dir grant 따라감)

## 통과 조건
- H/I/L 모두 정확
- M7/M8/M9 회귀 0
- delete가 *grant 무관* 항상 Dialog
- create/rename은 *grant 한 번 후* 자유
- 다른 dir 작업은 다시 Dialog

## 회귀 가드
- core: 5 새 메서드 테스트 + 기존
- desktop-shell: granted_dirs (3), permission (13), folder_ops (6), file_ops (4), dialog_ops (3), 기존
- `cargo test --workspace` FAILED 0
- `cargo fmt --check` / `cargo clippy --workspace -- -D warnings` 클린

## 알려진 한계 (Phase 2/3 이전)
- cwd 자동 mount 없음 — AI가 *사용자가 expand한 dir의 children*만 접근
- 외부 파일 변경 감지 없음 — 외부 에디터 수정 시 객체 갱신 안 됨
- cwd 밖 path 접근 불가 (Phase 3)

## 후속
- Phase 2: cwd auto-mount + notify-rs watcher
- Phase 3: Filesystem@1 escape hatch
```

- [ ] **Step 8.2: 회귀 + commit**

```
cargo test --workspace
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add docs/manual-tests/m10-phase1-acceptance.md
git commit -m "test(m10-phase1): T8 — acceptance H/I/L + 회귀 가드"
```

---

## Phase 1 마감 (Task 9 controller)

- [ ] **Step 9.1: 수동 검증 — 3 프로세스 spawn → 시나리오 H/I/L**
- [ ] **Step 9.2: spec/quality review (subagent)**
- [ ] **Step 9.3: Phase 1 push 결정 사용자 confirm**

---

# Phase 2 (outline) — cwd auto-mount + file watcher

추후 별도 plan으로 확장. Task outline:

- **T10** Cargo.toml notify-rs 7.x dep + lib import
- **T11** `fs_watcher.rs` 신규 — notify-rs spawn + mpsc + 이벤트 → invoke 변환 + echo 무시 (방금 우리가 write한 변경은 actor=ai/local-user로 추적해 외부 이벤트와 구분)
- **T12** main.rs cwd 결정 (`std::env::current_dir()` 또는 `--root` CLI) + 시작 시 직계 children auto-mount + watcher spawn
- **T13** 외부 이벤트 → Folder/File state SetState (last_change_actor="external") + Window content reload (해당 file_id의 active Window 있고 editor_state 비활성일 때)
- **T14** `/root <path>` CLI 명령 (옵션) — root 변경 → 기존 mount 해제 + 새 watcher
- **T15** acceptance G/J + 회귀

# Phase 3 (outline) — Filesystem@1 escape hatch

- **T16** core std_types — `filesystem(owner, root_path)` factory + 메서드 (read_external/write_external)
- **T17** desktop-shell main — Filesystem@1 mount (singleton) + 분기. cwd 안 호출 거부 + 안내.
- **T18** acceptance K + AI prompt + 회귀

---

## Phase별 push 전략

- **Phase 1 commit 후 push** — 사용자 검증 → 동의 → main push
- **Phase 2 commit 후 push** — 동일
- **Phase 3 commit 후 push** — 동일

각 Phase는 독립적으로 working/testable. Phase 1만으로도 AI가 새 파일 생성 가능.
