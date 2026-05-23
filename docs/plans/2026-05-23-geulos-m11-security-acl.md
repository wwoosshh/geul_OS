# M11 — 보안 ACL 강화 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> **NEVER push** — controller가 마일스톤 끝에 batch push. subagent는 commit만.

**Goal:** KI-001 / KI-016 해소 — desktop-shell 전역의 wildcard ACL 16곳을 명시적 actor allowlist + path-aware AI grant로 교체.

**Architecture:** 5 stage. Stage 1 = core ACL 모델 확장 (새 ActorPattern/MethodPattern/AclEffect, is_allowed 시그니처에 op + GrantContext 인자). Stage 2 = server-host에 grant 저장소 + GrantUpdate wire 메시지 + invoke/set_state ACL 일관화. Stage 3 = desktop-shell의 add_wildcard_acl 16곳을 5개 typed helper로 교체. Stage 4 = echo-app 정리. Stage 5 = grep 가드 + manual acceptance.

**Tech Stack:** 기존 Rust workspace + serde_json. 새 의존성 없음.

**Spec parent:** `docs/specs/2026-05-23-geulos-m11-security-acl.md`

---

## File Structure

| 신규/수정 | 경로 | 책임 |
|---|---|---|
| Create | `docs/adr/037-security-acl-hardening.md` | ADR 본문 (helper 분화 + AllowIfGrantedDir 결정 근거) |
| Modify | `core/src/object/acl.rs` | ActorPattern/MethodPattern/AclEffect 확장, AclOp enum, GrantContext trait |
| Modify | `core/src/object/mod.rs` | `Object::is_allowed` 시그니처 변경 (`AclOp` + `&dyn GrantContext`), `path()` helper |
| Modify | `core/src/server/invoke.rs` | is_allowed 호출에 op + grants 전달 |
| Modify | `core/src/server/set_state.rs` | 별도 wildcard 검사 제거 → is_allowed(AclOp::SetState) 사용 |
| Modify | `core/src/server/mod.rs` | ObjectServer에 `grants: GrantStore` 필드 + GrantContext 구현 + public API (`add_grant`/`remove_grant`) |
| Create | `core/src/server/grants.rs` | `GrantStore` struct (`HashMap<ActorId, HashSet<PathBuf>>`) + 단위 테스트 |
| Modify | `proto/src/lib.rs` (또는 적절한 wire 모듈) | `GrantUpdate` 메시지 struct + `GrantOp` enum |
| Modify | `server-host/src/connection.rs` | `GrantUpdate` 메시지 handle — actor가 `app:desktop-shell:*`일 때만 통과, 그 외 PermissionDenied |
| Modify | `server-host/src/actor.rs` (ObjectServerActor) | GrantUpdate Command 추가 |
| Modify | `apps/desktop-shell/src/handlers/mod.rs` | 기존 `add_wildcard_acl` 제거, 5개 typed helper (`add_ui_object_acl`/`add_fs_object_acl`/`add_dialog_acl`/`add_filesystem_acl`/`add_container_acl`) |
| Modify | `apps/desktop-shell/src/granted_dirs.rs` | `insert`/`remove` 시 `GrantUpdate` wire 메시지 송신 — 새 함수 `send_grant_update(stream, actor, path, op)` |
| Modify | `apps/desktop-shell/src/main.rs` + `handlers/*` | 호출 16곳을 각 객체 타입에 맞는 helper로 교체 |
| Modify | `apps/echo-app/src/lib.rs` | wildcard ACL 제거, 자기 actor만 set_state 허용 |
| Create | `core/tests/m11_acl_path_aware_test.rs` | AllowIfGrantedDir + new patterns 통합 회귀 테스트 |
| Create | `docs/manual-tests/m11-acceptance.md` | 회귀 시나리오 12개 (Dialog 우회 거부, AI granted/ungranted Folder 등) |
| Modify | `docs/known-issues.md` | KI-001/016 해소 표기, 정기 검토 시점 갱신 |

---

## 주의 사항 — 전반

- **Korean docs/comments, English identifiers.** 기존 코드 톤 일관.
- **TDD 엄격**: 모든 task가 *failing test → 구현 → pass → commit* 순. test 코드도 step에 통째로 명시.
- **commit 메시지는 한국어 + Co-Authored-By 라인 포함.** 기존 형식:
  ```
  feat(core)+(server): M11 T1 — ActorPattern 신규 variants

  ...본문...

  Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
  ```
- **각 task가 `cargo test --all` 통과 + `clippy -D warnings` 클린 상태로 끝나야** (binary 호환 깨지지 않도록 stage 내부에서도 일관성 유지).

---

# Stage 1 — core ACL 모델 확장

## Task 1: ActorPattern / MethodPattern 신규 variants

**Files:**
- Modify: `core/src/object/acl.rs`
- Test: `core/src/object/acl.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1.1: 실패하는 단위 테스트 추가**

Append to `core/src/object/acl.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::ActorId;
    use std::str::FromStr;

    #[test]
    fn actor_system_compositor_matches_only_compositor() {
        let pat = ActorPattern::SystemCompositor;
        assert!(pat.matches(&ActorId::system_compositor()));
        assert!(!pat.matches(&ActorId::local_user()));
        assert!(!pat.matches(&ActorId::new_ai_session()));
        assert!(!pat.matches(&ActorId::new_app("foo")));
    }

    #[test]
    fn actor_ai_session_matches_any_ai_uuid() {
        let pat = ActorPattern::AiSession;
        assert!(pat.matches(&ActorId::new_ai_session()));
        assert!(pat.matches(&ActorId::new_ai_session())); // 다른 UUID도
        assert!(!pat.matches(&ActorId::local_user()));
        assert!(!pat.matches(&ActorId::system_compositor()));
        assert!(!pat.matches(&ActorId::new_app("foo")));
    }

    #[test]
    fn actor_app_matches_specific_id_any_uuid() {
        let pat = ActorPattern::App("desktop-shell".to_string());
        assert!(pat.matches(&ActorId::new_app("desktop-shell")));
        assert!(pat.matches(&ActorId::new_app("desktop-shell"))); // 다른 instance UUID도
        assert!(!pat.matches(&ActorId::new_app("echo")));
        assert!(!pat.matches(&ActorId::local_user()));
    }

    #[test]
    fn method_one_of_matches_listed() {
        let pat = MethodPattern::OneOf(vec!["read_external".into(), "write_external".into()]);
        assert!(pat.matches("read_external"));
        assert!(pat.matches("write_external"));
        assert!(!pat.matches("delete"));
    }

    #[test]
    fn method_set_state_matches_set_state_op_only() {
        // SetState pattern은 invoke method 이름 매칭에는 false, op이 SetState일 때만 true.
        // 본 변경은 Object::is_allowed에서 AclOp 인자 도입 후 검증.
        // 단위 수준에서는 method 이름과 비교하지 않음 (별 dispatch).
        let pat = MethodPattern::SetState;
        // 의도: invoke 호출 시 method 문자열 매칭으로는 항상 false.
        assert!(!pat.matches("anything"));
    }
}
```

- [ ] **Step 1.2: 테스트 실행 — 실패 확인**

```
cargo test -p geulos-core acl::tests 2>&1 | tail -10
```

Expected: `SystemCompositor / AiSession / App / OneOf / SetState` variant 미정의로 컴파일 실패.

- [ ] **Step 1.3: ActorPattern 확장**

Edit `core/src/object/acl.rs` — replace `ActorPattern` enum body:

```rust
/// 액터 매칭 패턴.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActorPattern {
    /// 정확히 일치하는 액터.
    Exact(ActorId),
    /// 임의의 액터 (`*`). M11에서 *helper 사용 금지* — 회귀 grep 가드용.
    Wildcard,
    /// `system:compositor` 단독 매칭. M11 신규.
    SystemCompositor,
    /// `ai:<uuid>` 접두사 매칭 — 모든 AI 세션. M11 신규.
    AiSession,
    /// `app:<id>:<uuid>` — 특정 app id 매칭. instance UUID는 무관. M11 신규.
    App(String),
}

impl ActorPattern {
    /// 주어진 액터가 이 패턴과 일치하는지.
    pub fn matches(&self, actor: &ActorId) -> bool {
        match self {
            ActorPattern::Exact(a) => a == actor,
            ActorPattern::Wildcard => true,
            ActorPattern::SystemCompositor => actor.as_str() == "system:compositor",
            ActorPattern::AiSession => actor.as_str().starts_with("ai:"),
            ActorPattern::App(id) => {
                let s = actor.as_str();
                s.starts_with("app:") && s[4..].starts_with(&format!("{}:", id))
            }
        }
    }
}
```

- [ ] **Step 1.4: MethodPattern 확장**

Replace `MethodPattern` enum body in same file:

```rust
/// 메서드 이름 매칭 패턴.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MethodPattern {
    /// 정확히 일치.
    Exact(String),
    /// 임의의 메서드. M11에서 *helper 사용 금지*.
    Wildcard,
    /// 여러 method 중 하나. M11 신규.
    OneOf(Vec<String>),
    /// set_state 호출 한정. invoke method 이름과는 매칭 X (별 dispatch).
    /// M11 신규.
    SetState,
}

impl MethodPattern {
    /// invoke 호출의 method 문자열과 매칭. set_state op은 별도 dispatch.
    pub fn matches(&self, method: &str) -> bool {
        match self {
            MethodPattern::Exact(m) => m == method,
            MethodPattern::Wildcard => true,
            MethodPattern::OneOf(v) => v.iter().any(|m| m == method),
            MethodPattern::SetState => false,
        }
    }
}
```

- [ ] **Step 1.5: 테스트 통과 확인**

```
cargo test -p geulos-core acl::tests 2>&1 | tail -10
```

Expected: 5 tests passed.

- [ ] **Step 1.6: workspace 전체 회귀 확인**

```
cargo build -p geulos-core 2>&1 | tail -5
cargo test -p geulos-core 2>&1 | tail -5
```

Expected: build OK, 모든 기존 test 통과.

- [ ] **Step 1.7: commit**

```
git add core/src/object/acl.rs
git commit -m "$(cat <<'EOF'
feat(core): M11 T1 — ActorPattern / MethodPattern 신규 variants

KI-001/016 해소의 기반. 기존 Wildcard 외에 SystemCompositor / AiSession /
App(id) actor 패턴과 OneOf / SetState method 패턴 추가. Wildcard는 *유지하되
helper에서 더는 사용 X* — 회귀 grep 가드용 target.

Spec: docs/specs/2026-05-23-geulos-m11-security-acl.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: AclOp + AclEffect::AllowIfGrantedDir + GrantContext trait

**Files:**
- Modify: `core/src/object/acl.rs`

- [ ] **Step 2.1: 실패하는 단위 테스트 추가**

Append to `core/src/object/acl.rs` tests 모듈 안:

```rust
    #[test]
    fn acl_op_invoke_carries_method_name() {
        let op = AclOp::Invoke("save".to_string());
        assert_eq!(op.method_name(), Some("save"));
        let setop = AclOp::SetState("scroll_y".to_string());
        assert_eq!(setop.method_name(), None);
    }

    #[test]
    fn grant_context_empty_denies_all() {
        struct Empty;
        impl GrantContext for Empty {
            fn is_granted(&self, _actor: &ActorId, _path: &std::path::Path) -> bool {
                false
            }
        }
        let ctx = Empty;
        assert!(!ctx.is_granted(&ActorId::new_ai_session(), std::path::Path::new("/x")));
    }
```

- [ ] **Step 2.2: 테스트 실행 — 실패 확인**

```
cargo test -p geulos-core acl::tests::acl_op 2>&1 | tail -5
```

Expected: `AclOp` 미정의로 컴파일 실패.

- [ ] **Step 2.3: AclOp + AllowIfGrantedDir + GrantContext 추가**

Append to `core/src/object/acl.rs` (위 import 아래, `AclEffect` 정의 *교체*):

```rust
/// 호출 허용/거부 결정.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AclEffect {
    /// 호출 허용.
    Allow,
    /// 호출 거부.
    Deny,
    /// 객체의 props.path가 호출자(actor)의 granted_dirs에 포함될 때만 Allow.
    /// path prop이 없거나 grant 미등록이면 Deny와 동일. M11 신규.
    AllowIfGrantedDir,
}

/// ACL 검사 시 *어떤 operation*인지 구분 — invoke의 method 이름 vs set_state의 key.
/// M11 신규: set_state ACL 검사를 invoke와 동일한 평가 경로로 통일.
#[derive(Debug, Clone)]
pub enum AclOp {
    /// invoke 호출 — method 이름 포함.
    Invoke(String),
    /// set_state 호출 — 변경 key (참고용, 매칭에는 사용 X — MethodPattern::SetState).
    SetState(String),
}

impl AclOp {
    /// invoke op일 때만 method 이름 반환. MethodPattern::Exact/OneOf와 매칭에 사용.
    pub fn method_name(&self) -> Option<&str> {
        match self {
            AclOp::Invoke(m) => Some(m.as_str()),
            AclOp::SetState(_) => None,
        }
    }
}

/// 동적 권한 컨텍스트 — `AllowIfGrantedDir` 효과 평가 시 호출자의 granted path를 조회.
///
/// 구현체는 server-host의 GrantStore가 일반적. 단위 테스트는 Empty/Fixed 구현 사용.
pub trait GrantContext {
    /// `actor`가 `path` (또는 그 상위)에 대해 grant를 보유하고 있는지.
    fn is_granted(&self, actor: &ActorId, path: &std::path::Path) -> bool;
}
```

- [ ] **Step 2.4: 테스트 통과 확인**

```
cargo test -p geulos-core acl::tests 2>&1 | tail -10
```

Expected: 7 tests passed (이전 5 + 신규 2).

- [ ] **Step 2.5: commit**

```
git add core/src/object/acl.rs
git commit -m "$(cat <<'EOF'
feat(core): M11 T2 — AclOp + AllowIfGrantedDir + GrantContext trait

ACL 검사를 invoke와 set_state에 공통 평가 경로로 통일하기 위한 op 인자
도입. AllowIfGrantedDir 효과는 runtime path 조회가 필요 — GrantContext
trait로 server-host의 GrantStore가 주입.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `Object::is_allowed` 시그니처 변경 + `path()` helper

**Files:**
- Modify: `core/src/object/mod.rs`
- Test: `core/src/object/mod.rs` (`#[cfg(test)] mod tests`) 또는 신규 `core/tests/acl_is_allowed_test.rs`

- [ ] **Step 3.1: 실패하는 통합 테스트 추가**

Create `core/tests/m11_is_allowed_test.rs`:

```rust
//! M11 — Object::is_allowed 신규 시그니처 회귀 테스트.

use geulos_core::{
    object::{AclEffect, AclEntry, AclOp, ActorId, ActorPattern, GrantContext, MethodPattern, Object},
    std_types,
};
use std::path::Path;

struct EmptyGrants;
impl GrantContext for EmptyGrants {
    fn is_granted(&self, _actor: &ActorId, _path: &Path) -> bool {
        false
    }
}

struct FixedGrants {
    actor: ActorId,
    path: std::path::PathBuf,
}
impl GrantContext for FixedGrants {
    fn is_granted(&self, actor: &ActorId, path: &Path) -> bool {
        actor == &self.actor && path == self.path
    }
}

#[test]
fn invoke_op_matches_method_pattern() {
    let owner = ActorId::local_user();
    let mut obj = std_types::folder(owner.clone(), "/x", "x", 0);
    obj.acl.push(AclEntry {
        actor: ActorPattern::SystemCompositor,
        method: MethodPattern::Exact("list".to_string()),
        effect: AclEffect::Allow,
    });
    let g = EmptyGrants;
    assert!(obj.is_allowed(
        &ActorId::system_compositor(),
        AclOp::Invoke("list".to_string()),
        &g
    ));
    assert!(!obj.is_allowed(
        &ActorId::system_compositor(),
        AclOp::Invoke("delete".to_string()),
        &g
    ));
}

#[test]
fn set_state_op_only_matches_set_state_pattern() {
    let owner = ActorId::local_user();
    let mut obj = std_types::folder(owner.clone(), "/x", "x", 0);
    obj.acl.push(AclEntry {
        actor: ActorPattern::App("desktop-shell".to_string()),
        method: MethodPattern::SetState,
        effect: AclEffect::Allow,
    });
    let g = EmptyGrants;
    let shell = ActorId::new_app("desktop-shell");
    // SetState op은 통과.
    assert!(obj.is_allowed(&shell, AclOp::SetState("child_count".to_string()), &g));
    // 같은 actor라도 Invoke op은 SetState 패턴에 매칭 X → 거부.
    assert!(!obj.is_allowed(&shell, AclOp::Invoke("list".to_string()), &g));
}

#[test]
fn allow_if_granted_dir_uses_path_prop() {
    let owner = ActorId::local_user();
    let mut obj = std_types::folder(owner.clone(), "D:/proj/foo", "foo", 0);
    obj.acl.push(AclEntry {
        actor: ActorPattern::AiSession,
        method: MethodPattern::Wildcard,
        effect: AclEffect::AllowIfGrantedDir,
    });
    let ai = ActorId::new_ai_session();
    // grant 없으면 거부.
    let empty = EmptyGrants;
    assert!(!obj.is_allowed(&ai, AclOp::Invoke("create_file".to_string()), &empty));
    // grant 있으면 통과.
    let granted = FixedGrants { actor: ai.clone(), path: "D:/proj/foo".into() };
    assert!(obj.is_allowed(&ai, AclOp::Invoke("create_file".to_string()), &granted));
}

#[test]
fn empty_acl_falls_back_to_owner_only() {
    let owner = ActorId::local_user();
    let obj = std_types::folder(owner.clone(), "/x", "x", 0);
    let g = EmptyGrants;
    // ACL이 비어있으면 owner만 허용 — 기존 동작 유지.
    // std_types::folder가 ACL을 비워둔다는 전제로 검증. (변경 시 helper로 비움 보장)
    assert_eq!(obj.acl.is_empty(), true, "std_types::folder는 ACL이 비어있어야 — helper로 부착");
    assert!(obj.is_allowed(&owner, AclOp::Invoke("list".to_string()), &g));
    assert!(!obj.is_allowed(
        &ActorId::system_compositor(),
        AclOp::Invoke("list".to_string()),
        &g
    ));
}
```

- [ ] **Step 3.2: 테스트 실행 — 실패 확인**

```
cargo test -p geulos-core --test m11_is_allowed_test 2>&1 | tail -10
```

Expected: `is_allowed` 시그니처 불일치로 컴파일 실패.

- [ ] **Step 3.3: `Object::path()` helper 추가**

Edit `core/src/object/mod.rs` — `impl Object { ... }` 안에 추가:

```rust
    /// 객체의 `props.path` 값을 `Path`로 반환. M11 AllowIfGrantedDir 평가용.
    pub fn path(&self) -> Option<std::path::PathBuf> {
        self.props
            .get("path")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from)
    }
```

- [ ] **Step 3.4: `is_allowed` 시그니처 + 본문 교체**

Edit `core/src/object/mod.rs` — `impl Object`의 `is_allowed`를 *전체 교체*:

```rust
    /// ACL 평가. M11에서 `op: AclOp`와 `grants: &dyn GrantContext` 인자 추가.
    ///
    /// 규칙:
    /// - ACL이 비어 있으면 *소유자만* 허용 (기존 동작).
    /// - ACL이 있으면 *순서대로 평가, 마지막 매칭이 승리*:
    ///   - 마지막 Allow → 허용.
    ///   - 마지막 Deny → 거부.
    ///   - 마지막 AllowIfGrantedDir → path prop 조회 후 grants.is_granted → 통과/거부.
    /// - 어떤 entry도 매칭 안 되면 default deny.
    ///
    /// `op` 매칭 규칙:
    /// - `AclOp::Invoke(method)` → MethodPattern::{Exact, OneOf, Wildcard}와 매칭.
    /// - `AclOp::SetState(_)` → MethodPattern::SetState와 매칭 (key 이름 무관).
    pub fn is_allowed(
        &self,
        actor: &ActorId,
        op: AclOp,
        grants: &dyn GrantContext,
    ) -> bool {
        if self.acl.is_empty() {
            return &self.owner == actor;
        }
        let mut decision: Option<AclEffect> = None;
        for entry in &self.acl {
            if !entry.actor.matches(actor) {
                continue;
            }
            let method_match = match (&entry.method, &op) {
                (MethodPattern::SetState, AclOp::SetState(_)) => true,
                (MethodPattern::SetState, _) => false,
                (_, AclOp::SetState(_)) => false,
                (pat, AclOp::Invoke(m)) => pat.matches(m),
            };
            if method_match {
                decision = Some(entry.effect);
            }
        }
        match decision {
            Some(AclEffect::Allow) => true,
            Some(AclEffect::AllowIfGrantedDir) => {
                self.path().map(|p| grants.is_granted(actor, &p)).unwrap_or(false)
            }
            _ => false,
        }
    }
```

`AclEntry::matches`는 이제 `is_allowed` 내부에서 *분리 평가*하므로 unused. 삭제 또는 유지 결정 — 유지 시 deprecated 마커. *삭제* 권장 (DRY):

Edit `core/src/object/acl.rs` — `impl AclEntry { ... }` 블록 *제거*:

```rust
// 삭제할 코드:
impl AclEntry {
    /// 액터와 메서드가 이 항목에 매치되는지.
    pub fn matches(&self, actor: &ActorId, method: &str) -> bool {
        self.actor.matches(actor) && self.method.matches(method)
    }
}
```

- [ ] **Step 3.5: import 갱신 — mod.rs**

Edit `core/src/object/mod.rs` 상단 import:

기존:
```rust
use super::acl::{AclEffect, AclEntry, ActorPattern, MethodPattern};
```
교체:
```rust
use super::acl::{AclEffect, AclEntry, AclOp, ActorPattern, GrantContext, MethodPattern};
```

(이미 `pub use` 하고 있다면 추가 export도 — Step 3.6에서 처리.)

- [ ] **Step 3.6: pub re-export**

Edit `core/src/object/mod.rs` 상단의 `pub use` 줄 (또는 `core/src/lib.rs`):

```rust
pub use acl::{AclEffect, AclEntry, AclOp, ActorPattern, GrantContext, MethodPattern};
```

또는 `core/src/lib.rs`에서 추가:

```rust
pub use object::{AclEffect, AclEntry, AclOp, ActorPattern, GrantContext, MethodPattern};
```

(기존 export 형식 확인해 일관 적용.)

- [ ] **Step 3.7: 테스트 통과 확인**

```
cargo build -p geulos-core 2>&1 | tail -5
cargo test -p geulos-core --test m11_is_allowed_test 2>&1 | tail -10
```

Expected: 4 tests passed.

- [ ] **Step 3.8: 기존 호출처 컴파일 에러 확인**

```
cargo build --workspace 2>&1 | tail -20
```

Expected: `invoke.rs:51`, `set_state.rs`의 `is_allowed` / `obj.acl.iter().any(...)` 호출이 *컴파일 깨짐*. Task 4에서 수정.

(이 시점에 commit하지 말고 Task 4까지 한 번에 진행 — workspace가 일관성 있게 통과.)

---

## Task 4: invoke + set_state ACL 일관화

**Files:**
- Modify: `core/src/server/invoke.rs`
- Modify: `core/src/server/set_state.rs`
- Modify: `core/src/server/mod.rs` (ObjectServer에 임시 grants 필드 추가 — Stage 2에서 정식화)

- [ ] **Step 4.1: ObjectServer에 임시 GrantStore placeholder 추가**

Edit `core/src/server/mod.rs` — `ObjectServer` struct에 필드 추가:

```rust
use crate::object::GrantContext;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// 임시 grant 저장소 — Task 6에서 정식화. M11 진행 중 placeholder.
#[derive(Default, Debug, Clone)]
pub struct GrantStore {
    by_actor: HashMap<crate::object::ActorId, HashSet<PathBuf>>,
}

impl GrantStore {
    pub fn add(&mut self, actor: crate::object::ActorId, path: PathBuf) {
        self.by_actor.entry(actor).or_default().insert(path);
    }
    pub fn remove(&mut self, actor: &crate::object::ActorId, path: &PathBuf) {
        if let Some(set) = self.by_actor.get_mut(actor) {
            set.remove(path);
        }
    }
}

impl GrantContext for GrantStore {
    fn is_granted(&self, actor: &crate::object::ActorId, path: &std::path::Path) -> bool {
        self.by_actor.get(actor).is_some_and(|set| {
            set.iter().any(|granted| path == granted.as_path() || path.starts_with(granted))
        })
    }
}
```

`ObjectServer` struct에:
```rust
pub struct ObjectServer {
    // ...기존 필드...
    pub grants: GrantStore,  // M11 신규
}
```

`ObjectServer::new` 초기화:
```rust
grants: GrantStore::default(),
```

- [ ] **Step 4.2: invoke.rs 갱신**

Edit `core/src/server/invoke.rs` — line 51 `obj.is_allowed(actor, method)` 호출 교체:

```rust
        // 3) ACL — M11: AclOp + GrantContext 인자.
        if !obj.is_allowed(actor, crate::object::AclOp::Invoke(method.to_string()), &self.grants) {
            return Err(InvokeError::PermissionDenied {
                actor: actor.as_str().to_string(),
                target: *target,
                method: method.to_string(),
            });
        }
```

- [ ] **Step 4.3: set_state.rs 갱신 — 별도 wildcard 검사 제거**

Edit `core/src/server/set_state.rs` — line 47-60의 owner/wildcard 검사 블록을 *전체 교체*:

기존:
```rust
        if &obj.owner != actor {
            let allowed_by_wildcard = obj.acl.iter().any(|entry| {
                matches!(entry.effect, AclEffect::Allow)
                    && matches!(entry.actor, ActorPattern::Wildcard)
                    && matches!(entry.method, MethodPattern::Wildcard)
            });
            if !allowed_by_wildcard {
                return Err(SetStateError::PermissionDenied {
                    actor: actor.as_str().to_string(),
                    target: *target,
                    key: key.to_string(),
                });
            }
        }
```

교체:
```rust
        // M11: invoke와 동일한 is_allowed 평가 경로. owner는 ACL 비어있을 때만 short-circuit.
        if !obj.is_allowed(
            actor,
            crate::object::AclOp::SetState(key.to_string()),
            &self.grants,
        ) {
            return Err(SetStateError::PermissionDenied {
                actor: actor.as_str().to_string(),
                target: *target,
                key: key.to_string(),
            });
        }
```

같은 파일 상단의 unused import 정리:
```rust
// 기존: use crate::object::{AclEffect, ActorId, ActorPattern, EventId, MethodPattern, ObjectId};
// 교체:
use crate::object::{ActorId, EventId, ObjectId};
```

- [ ] **Step 4.4: workspace 빌드 + 모든 회귀 테스트**

```
cargo build --workspace 2>&1 | tail -10
cargo test -p geulos-core 2>&1 | tail -10
```

Expected: build OK, 모든 core test 통과 (기존 acl_test.rs / object_struct_test.rs는 method matches 사용 안 함이라 그대로 통과). 단 set_state 회귀 (server_set_state_test.rs)는 *wildcard ACL 테스트가 여전히 동작* — 새 ACL 평가는 Wildcard 패턴도 통과시키므로 호환.

- [ ] **Step 4.5: 외부 client (echo-app/ai-bridge) 회귀 확인**

```
cargo test --workspace 2>&1 | tail -15
```

Expected: 모든 통과. 만약 어딘가 `AclEntry::matches` 직접 호출이 있다면 컴파일 에러 — 그 호출처를 `obj.is_allowed`로 마이그레이션.

- [ ] **Step 4.6: clippy + fmt**

```
cargo clippy --workspace --no-deps -- -D warnings 2>&1 | tail -5
cargo fmt --check 2>&1 | tail -5
```

Expected: 클린.

- [ ] **Step 4.7: commit (Task 3 + 4 묶음)**

```
git add core/src/object/mod.rs core/src/object/acl.rs core/src/server/invoke.rs core/src/server/set_state.rs core/src/server/mod.rs core/tests/m11_is_allowed_test.rs
git commit -m "$(cat <<'EOF'
feat(core)+(server): M11 T3+T4 — is_allowed 시그니처 통일 + GrantStore placeholder

Object::is_allowed가 (actor, AclOp, &dyn GrantContext) 시그니처로 변경.
invoke와 set_state가 동일 평가 경로 — 마지막 매칭 entry의 effect 결정.
AllowIfGrantedDir effect는 path prop 조회 + grants.is_granted로 동적 평가.

ObjectServer.grants 필드 (GrantStore)는 M11 T6에서 wire 메시지 통합 시
정식화. 본 task에서는 always-empty placeholder.

set_state의 기존 wildcard 직접 검사 제거 — invoke와 똑같이 is_allowed 사용.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Stage 2 — server-host grant 통합

## Task 5: `GrantUpdate` wire 메시지 (proto)

**Files:**
- Modify: `proto/src/` 적절한 모듈 (기존 메시지 정의 파일)
- Test: `proto/tests/grant_update_test.rs` (신규)

- [ ] **Step 5.1: 기존 wire 메시지 정의 위치 찾기**

```
grep -rn "pub struct InvokeMsg\|pub struct MountMsg" proto/src/
```

찾은 파일에 GrantUpdate를 추가 (보통 `proto/src/lib.rs` 또는 `proto/src/messages.rs`).

- [ ] **Step 5.2: 실패하는 직렬화 테스트**

Create `proto/tests/grant_update_test.rs`:

```rust
//! M11 — GrantUpdate wire 직렬화 회귀.

use geulos_proto::{GrantUpdate, GrantOp};

#[test]
fn grant_update_add_serializes_round_trip() {
    let g = GrantUpdate {
        actor: "ai:abc-123".to_string(),
        path: "D:/proj/foo".to_string(),
        op: GrantOp::Add,
    };
    let json = serde_json::to_string(&g).unwrap();
    let back: GrantUpdate = serde_json::from_str(&json).unwrap();
    assert_eq!(back.actor, g.actor);
    assert_eq!(back.path, g.path);
    assert!(matches!(back.op, GrantOp::Add));
}

#[test]
fn grant_update_remove_op_serializes() {
    let g = GrantUpdate {
        actor: "ai:abc".to_string(),
        path: "/x".to_string(),
        op: GrantOp::Remove,
    };
    let json = serde_json::to_string(&g).unwrap();
    assert!(json.contains("Remove"));
    let back: GrantUpdate = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.op, GrantOp::Remove));
}
```

- [ ] **Step 5.3: 테스트 실행 — 실패 확인**

```
cargo test -p geulos-proto --test grant_update_test 2>&1 | tail -5
```

Expected: `GrantUpdate / GrantOp` 미정의 컴파일 실패.

- [ ] **Step 5.4: 구현 추가**

Edit (또는 추가 to) `proto/src/lib.rs` (기존 메시지 정의 패턴 따라):

```rust
/// 호출자(desktop-shell)가 server에게 *AI grant* 갱신을 알리는 메시지. M11 신규.
///
/// server-host는 sender의 actor가 `app:desktop-shell:*`일 때만 수락. 그 외 거부.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GrantUpdate {
    /// grant 받을 actor (예: "ai:<uuid>")
    pub actor: String,
    /// grant 대상 디렉터리 경로 (호스트 OS path)
    pub path: String,
    /// Add: grant 등록 / Remove: 철회
    pub op: GrantOp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GrantOp {
    Add,
    Remove,
}
```

(기존 메시지가 별도 dispatch enum을 쓴다면 — 예: `WireMessage::GrantUpdate(GrantUpdate)` — 그 enum에도 variant 추가. 기존 형식 따라.)

- [ ] **Step 5.5: 테스트 통과**

```
cargo test -p geulos-proto --test grant_update_test 2>&1 | tail -5
```

Expected: 2 tests passed.

- [ ] **Step 5.6: commit**

```
git add proto/src/ proto/tests/grant_update_test.rs
git commit -m "$(cat <<'EOF'
feat(proto): M11 T5 — GrantUpdate wire 메시지

desktop-shell → server: AI granted_dirs 변경 통지.
server-host는 sender가 app:desktop-shell:* 일 때만 수락 (T7에서 가드).
GrantOp::Add/Remove로 등록·철회 구분.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: server-host GrantUpdate handle + actor 가드

**Files:**
- Modify: `server-host/src/connection.rs`
- Modify: `server-host/src/actor.rs` (또는 server-host actor 정의 위치)
- Test: `server-host/tests/m11_grant_update_test.rs` (신규)

- [ ] **Step 6.1: 실패하는 통합 테스트**

Create `server-host/tests/m11_grant_update_test.rs`:

```rust
//! M11 — server-host GrantUpdate handle + actor 가드.

use geulos_proto::{decode_frame, encode_frame, Hello, HelloAck, Role, GrantUpdate, GrantOp};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn connect_as(role: Role, manifest: Option<serde_json::Value>) -> (TcpStream, String) {
    // 기존 acceptance test 헬퍼와 같은 방식 — server-host 띄우고 Hello.
    // 자세한 구현은 server-host/tests/m3_acceptance.rs 참고.
    // 본 test에서는 helper 인라인:
    use geulos_server_host::test_helper::start_server;  // 기존 helper 추정
    let addr = start_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let hello = Hello {
        version: "0.1".to_string(),
        role,
        auth: manifest.unwrap_or(serde_json::json!({})),
        client_id: "test-client".to_string(),
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&hello).unwrap())).await.unwrap();
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let body = decode_frame(&mut slice).unwrap();
    let ack: HelloAck = serde_json::from_slice(&body).unwrap();
    (stream, ack.actor_id)
}

#[tokio::test]
async fn grant_update_accepted_from_desktop_shell() {
    let manifest = serde_json::json!({ "manifest": { "id": "desktop-shell", "ui_types": [] }});
    let (mut stream, _actor) = connect_as(Role::App, Some(manifest)).await;
    let g = GrantUpdate {
        actor: "ai:test-uuid".to_string(),
        path: "D:/tmp".to_string(),
        op: GrantOp::Add,
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&g).unwrap())).await.unwrap();
    // 성공 시 server는 *암묵 ack* 또는 명시 ack — 본 test에서는 후속
    // Invoke가 grant 통해 통과하는 것으로 *간접 검증* (다음 test).
    // 본 test는 단순 *연결 유지* 확인 — 100ms 후 server가 끊지 않으면 OK.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut probe = vec![0u8; 16];
    // non-blocking read — connection 살아있는지.
    match tokio::time::timeout(Duration::from_millis(50), stream.read(&mut probe)).await {
        Ok(Ok(0)) => panic!("server가 desktop-shell의 GrantUpdate를 받고 연결 끊었음"),
        _ => {} // timeout = 정상 (server는 아무것도 안 보냄)
    }
}

#[tokio::test]
async fn grant_update_rejected_from_non_desktop_shell() {
    let manifest = serde_json::json!({ "manifest": { "id": "echo-app", "ui_types": [] }});
    let (mut stream, _actor) = connect_as(Role::App, Some(manifest)).await;
    let g = GrantUpdate {
        actor: "ai:test".to_string(),
        path: "/x".to_string(),
        op: GrantOp::Add,
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&g).unwrap())).await.unwrap();
    // 거부 시 server가 연결 끊거나 error 응답.
    let mut buf = vec![0u8; 1024];
    let res = tokio::time::timeout(Duration::from_millis(200), stream.read(&mut buf)).await;
    match res {
        Ok(Ok(0)) => {} // OK — 끊김
        Ok(Ok(n)) => {
            // error 응답 형태도 OK — 정확한 형식은 구현 결정
            let body = String::from_utf8_lossy(&buf[..n]);
            assert!(
                body.contains("PermissionDenied") || body.contains("grant_denied"),
                "기대: error 응답 또는 끊김. 실제: {}",
                body
            );
        }
        _ => panic!("server가 echo-app의 GrantUpdate를 거부하지 않음"),
    }
}
```

(기존 server-host의 test helper 패턴이 다르면 그 패턴 따라 — `server-host/tests/m3_acceptance.rs` 참고.)

- [ ] **Step 6.2: 테스트 실행 — 실패 확인**

```
cargo test -p geulos-server-host --test m11_grant_update_test 2>&1 | tail -10
```

Expected: 컴파일 또는 통신 단계에서 실패 (GrantUpdate handle 없음).

- [ ] **Step 6.3: ObjectServerActor에 Command 추가**

Find `server-host/src/actor.rs` (또는 actor command enum 정의 위치). 새 variant:

```rust
pub enum Command {
    // 기존 variants...
    AddGrant {
        actor: geulos_core::ActorId,
        path: std::path::PathBuf,
        reply: tokio::sync::oneshot::Sender<()>,
    },
    RemoveGrant {
        actor: geulos_core::ActorId,
        path: std::path::PathBuf,
        reply: tokio::sync::oneshot::Sender<()>,
    },
}
```

handle 추가:
```rust
Command::AddGrant { actor, path, reply } => {
    self.server.grants.add(actor, path);
    let _ = reply.send(());
}
Command::RemoveGrant { actor, path, reply } => {
    self.server.grants.remove(&actor, &path);
    let _ = reply.send(());
}
```

- [ ] **Step 6.4: connection.rs에 GrantUpdate frame 분기**

Edit `server-host/src/connection.rs`의 main read loop (Invoke/SetState/Mount 등을 분기하는 곳). GrantUpdate variant도 dispatch에 추가:

```rust
// 메시지 분기 (예시 — 기존 패턴 따라 정확히 교체):
} else if let Ok(g) = serde_json::from_slice::<GrantUpdate>(&body) {
    // M11: GrantUpdate는 *app:desktop-shell:* actor만 수락.
    if !actor.as_str().starts_with("app:desktop-shell:") {
        let err = ErrorMsg {
            code: "PermissionDenied".to_string(),
            detail: "GrantUpdate는 desktop-shell만 발신 가능".to_string(),
        };
        let body = serde_json::to_vec(&err).unwrap();
        let mut w = writer.lock().await;
        let _ = w.write_all(&encode_frame(&body)).await;
        continue;
    }
    let target_actor = match geulos_core::ActorId::from_str(&g.actor) {
        Ok(a) => a,
        Err(_) => continue, // 잘못된 actor 문자열 무시
    };
    let path = std::path::PathBuf::from(&g.path);
    let (tx, rx) = tokio::sync::oneshot::channel();
    let cmd = match g.op {
        GrantOp::Add => Command::AddGrant { actor: target_actor, path, reply: tx },
        GrantOp::Remove => Command::RemoveGrant { actor: target_actor, path, reply: tx },
    };
    let _ = cmd_tx.send(cmd).await;
    let _ = rx.await;
    // ack 없음 — desktop-shell은 fire-and-forget. 후속 invoke가 통과로 검증.
}
```

(이전 분기들은 그대로 — 기존 패턴 정확히 따라야 함. `else if let Ok(...)` 체인 끝에 추가.)

- [ ] **Step 6.5: 테스트 통과 확인**

```
cargo test -p geulos-server-host --test m11_grant_update_test 2>&1 | tail -10
```

Expected: 2 tests passed.

- [ ] **Step 6.6: 전체 회귀**

```
cargo test --workspace 2>&1 | tail -15
cargo clippy --workspace --no-deps -- -D warnings 2>&1 | tail -5
```

Expected: 클린.

- [ ] **Step 6.7: commit**

```
git add server-host/src/ server-host/tests/m11_grant_update_test.rs
git commit -m "$(cat <<'EOF'
feat(server-host): M11 T6 — GrantUpdate handle + desktop-shell actor 가드

ObjectServerActor에 AddGrant/RemoveGrant Command + connection.rs에서
GrantUpdate frame 분기. sender의 actor가 app:desktop-shell:* 일 때만
수락, 그 외 PermissionDenied 응답 후 무시.

GrantStore (T4에서 도입한 ObjectServer.grants)에 반영 — 이후 AI invoke가
AllowIfGrantedDir 효과의 ACL 검사를 통과한다.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: AI invoke path-aware ACL 통합 회귀 테스트

**Files:**
- Test: `server-host/tests/m11_ai_grant_invoke_test.rs` (신규)

이 test는 end-to-end로 *desktop-shell-role*과 *ai-role* connection 둘을 동시에 띄워:
1. desktop-shell이 GrantUpdate(Add) 보냄
2. ai-role이 grant 안 path의 Folder.create_file invoke → 통과
3. ai-role이 grant 밖 path의 Folder invoke → PermissionDenied

- [ ] **Step 7.1: 통합 회귀 테스트 작성**

Create `server-host/tests/m11_ai_grant_invoke_test.rs`:

```rust
//! M11 — AI invoke가 GrantUpdate 후 AllowIfGrantedDir 통과하는지 end-to-end 회귀.

use geulos_proto::{
    decode_frame, encode_frame, GrantOp, GrantUpdate, Hello, HelloAck, InvokeMsg, InvokeResult,
    MountMsg, Role,
};
use geulos_core::{std_types, ActorId};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

// 기존 test helper 사용 — start_server / connect_as 등.
// 자세한 구현은 server-host/tests 다른 통합 테스트 참고.

#[tokio::test]
async fn ai_can_invoke_folder_in_granted_dir_only() {
    let addr = geulos_server_host::test_helper::start_server().await;

    // 1) desktop-shell 연결 + Folder 객체 mount (path="D:/granted/foo")
    let mut shell = TcpStream::connect(&addr).await.unwrap();
    let shell_hello = Hello {
        version: "0.1".to_string(),
        role: Role::App,
        auth: serde_json::json!({ "manifest": { "id": "desktop-shell", "ui_types": [] }}),
        client_id: "shell".to_string(),
    };
    shell.write_all(&encode_frame(&serde_json::to_vec(&shell_hello).unwrap())).await.unwrap();
    let _shell_ack: HelloAck = read_typed(&mut shell).await;

    let owner = ActorId::local_user();
    let mut folder = std_types::folder(owner.clone(), "D:/granted/foo", "foo", 0);
    // AI를 위해 AllowIfGrantedDir ACL 부착 (Stage 3 helper의 시뮬레이션):
    folder.acl.push(geulos_core::AclEntry {
        actor: geulos_core::ActorPattern::AiSession,
        method: geulos_core::MethodPattern::Wildcard,
        effect: geulos_core::AclEffect::AllowIfGrantedDir,
    });
    let folder_id = folder.id;
    let mount = MountMsg { root_object_id: folder_id.to_string(), tree: serde_json::to_value(&folder).unwrap() };
    shell.write_all(&encode_frame(&serde_json::to_vec(&mount).unwrap())).await.unwrap();

    // 2) AI connection
    let mut ai = TcpStream::connect(&addr).await.unwrap();
    let ai_hello = Hello {
        version: "0.1".to_string(),
        role: Role::Ai,
        auth: serde_json::json!({}),
        client_id: "ai".to_string(),
    };
    ai.write_all(&encode_frame(&serde_json::to_vec(&ai_hello).unwrap())).await.unwrap();
    let ai_ack: HelloAck = read_typed(&mut ai).await;
    let ai_actor = ai_ack.actor_id;

    // 3) grant *없는* 상태에서 AI invoke — 거부 기대
    let inv = InvokeMsg {
        request_id: "r1".to_string(),
        target: folder_id.to_string(),
        method: "list".to_string(),
        args: serde_json::json!({}),
    };
    ai.write_all(&encode_frame(&serde_json::to_vec(&inv).unwrap())).await.unwrap();
    let result: InvokeResult = read_typed(&mut ai).await;
    assert_eq!(result.request_id, "r1");
    assert!(!result.ok, "AI는 grant 없이는 invoke 거부되어야");

    // 4) desktop-shell이 GrantUpdate(Add) 송신
    let g = GrantUpdate {
        actor: ai_actor.clone(),
        path: "D:/granted/foo".to_string(),
        op: GrantOp::Add,
    };
    shell.write_all(&encode_frame(&serde_json::to_vec(&g).unwrap())).await.unwrap();
    // server 처리 대기
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // 5) AI invoke 재시도 — 통과 기대
    let inv2 = InvokeMsg {
        request_id: "r2".to_string(),
        target: folder_id.to_string(),
        method: "list".to_string(),
        args: serde_json::json!({}),
    };
    ai.write_all(&encode_frame(&serde_json::to_vec(&inv2).unwrap())).await.unwrap();
    let result2: InvokeResult = read_typed(&mut ai).await;
    assert!(result2.ok, "grant 후 AI invoke가 통과해야");
}

async fn read_typed<T: serde::de::DeserializeOwned>(stream: &mut TcpStream) -> T {
    let mut buf = vec![0u8; 16384];
    let mut accum: Vec<u8> = Vec::new();
    loop {
        let n = stream.read(&mut buf).await.unwrap();
        accum.extend_from_slice(&buf[..n]);
        let mut slice = accum.as_slice();
        if let Ok(body) = decode_frame(&mut slice) {
            return serde_json::from_slice(&body).unwrap();
        }
    }
}
```

(`InvokeResult` 정확 시그니처는 proto 확인. 기존 테스트 패턴 따라 정확화.)

- [ ] **Step 7.2: 테스트 실행**

```
cargo test -p geulos-server-host --test m11_ai_grant_invoke_test 2>&1 | tail -15
```

Expected: 통과. 만약 grant 후에도 거부되면 — server-host의 GrantUpdate handle이 grants에 반영 안 됨 또는 actor 비교 미스매치. 디버그:
- AddGrant Command가 server.grants에 실제 add 되는지 println
- AI ack의 actor_id 형식 (`ai:<uuid>`)이 grants의 key와 정확히 일치하는지

- [ ] **Step 7.3: commit**

```
git add server-host/tests/m11_ai_grant_invoke_test.rs
git commit -m "$(cat <<'EOF'
test(server-host): M11 T7 — AI invoke path-aware ACL end-to-end 회귀

desktop-shell + AI 두 connection 동시 띄우고 grant 전/후 AI invoke 동작 검증.
GrantUpdate 후 AllowIfGrantedDir 효과 통과를 wire 레벨에서 확인.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Stage 3 — desktop-shell helper 교체

## Task 8: 5개 typed helper 추가 + 단위 테스트

**Files:**
- Modify: `apps/desktop-shell/src/handlers/mod.rs`
- Test: `apps/desktop-shell/src/handlers/mod.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 8.1: 실패하는 단위 테스트**

Append to `apps/desktop-shell/src/handlers/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use geulos_core::{std_types, ActorId};

    #[test]
    fn ui_object_acl_allows_compositor_invoke_and_shell_set_state() {
        let owner = ActorId::local_user();
        let mut win = std_types::window(owner.clone(), "title", (0, 0), (100, 100));
        add_ui_object_acl(&mut win);
        let g = geulos_core::server::GrantStore::default();
        let compositor = ActorId::system_compositor();
        let shell = ActorId::new_app("desktop-shell");
        let ai = ActorId::new_ai_session();

        // compositor invoke OK
        assert!(win.is_allowed(&compositor, geulos_core::AclOp::Invoke("focus".into()), &g));
        // shell set_state OK
        assert!(win.is_allowed(&shell, geulos_core::AclOp::SetState("scroll_y".into()), &g));
        // ai invoke 거부
        assert!(!win.is_allowed(&ai, geulos_core::AclOp::Invoke("close".into()), &g));
        // 외부 client invoke 거부
        assert!(!win.is_allowed(&ActorId::new_app("evil"), geulos_core::AclOp::Invoke("close".into()), &g));
    }

    #[test]
    fn fs_object_acl_allows_ai_only_if_granted() {
        let owner = ActorId::local_user();
        let mut folder = std_types::folder(owner.clone(), "D:/x", "x", 0);
        add_fs_object_acl(&mut folder);
        let mut g = geulos_core::server::GrantStore::default();
        let ai = ActorId::new_ai_session();

        // 미grant 상태
        assert!(!folder.is_allowed(&ai, geulos_core::AclOp::Invoke("list".into()), &g));
        // grant 후
        g.add(ai.clone(), std::path::PathBuf::from("D:/x"));
        assert!(folder.is_allowed(&ai, geulos_core::AclOp::Invoke("list".into()), &g));

        // compositor 무조건 OK
        let comp = ActorId::system_compositor();
        assert!(folder.is_allowed(&comp, geulos_core::AclOp::Invoke("delete".into()), &g));
    }

    #[test]
    fn dialog_acl_compositor_respond_only() {
        let owner = ActorId::local_user();
        let mut dlg = std_types::dialog(owner.clone(), "확인?", vec!["허용".into(), "거부".into()]);
        add_dialog_acl(&mut dlg);
        let g = geulos_core::server::GrantStore::default();
        let comp = ActorId::system_compositor();
        let ai = ActorId::new_ai_session();
        let evil_app = ActorId::new_app("evil");

        // compositor respond OK
        assert!(dlg.is_allowed(&comp, geulos_core::AclOp::Invoke("respond".into()), &g));
        // compositor 외 다른 invoke 거부
        assert!(!dlg.is_allowed(&comp, geulos_core::AclOp::Invoke("delete".into()), &g));
        // ai respond 거부 — *핵심*
        assert!(!dlg.is_allowed(&ai, geulos_core::AclOp::Invoke("respond".into()), &g));
        // 외부 app respond 거부
        assert!(!dlg.is_allowed(&evil_app, geulos_core::AclOp::Invoke("respond".into()), &g));
    }

    #[test]
    fn filesystem_acl_allows_ai_external_methods() {
        let owner = ActorId::local_user();
        let mut fs = std_types::filesystem(owner.clone(), "D:/cwd");
        add_filesystem_acl(&mut fs);
        let g = geulos_core::server::GrantStore::default();
        let ai = ActorId::new_ai_session();
        // read_external / write_external OK
        assert!(fs.is_allowed(&ai, geulos_core::AclOp::Invoke("read_external".into()), &g));
        assert!(fs.is_allowed(&ai, geulos_core::AclOp::Invoke("write_external".into()), &g));
        // 다른 method 거부
        assert!(!fs.is_allowed(&ai, geulos_core::AclOp::Invoke("delete".into()), &g));
    }

    #[test]
    fn container_acl_allows_shell_set_state_only() {
        let owner = ActorId::local_user();
        let mut desk = std_types::desktop(owner.clone());
        add_container_acl(&mut desk);
        let g = geulos_core::server::GrantStore::default();
        let shell = ActorId::new_app("desktop-shell");
        let comp = ActorId::system_compositor();

        // shell set_state OK
        assert!(desk.is_allowed(&shell, geulos_core::AclOp::SetState("children".into()), &g));
        // compositor는 invoke/set_state 모두 거부
        assert!(!desk.is_allowed(&comp, geulos_core::AclOp::SetState("focused".into()), &g));
        assert!(!desk.is_allowed(&comp, geulos_core::AclOp::Invoke("any".into()), &g));
    }
}
```

(`std_types::window/dialog/desktop/filesystem` 정확 시그니처는 `core/src/object/std_types.rs` 확인 후 보정.)

- [ ] **Step 8.2: 테스트 실행 — 실패 확인**

```
cargo test -p geulos-desktop-shell --lib handlers::tests 2>&1 | tail -10
```

Expected: helper 미정의 컴파일 실패.

- [ ] **Step 8.3: 기존 `add_wildcard_acl` 함수 *유지하되 deprecated 마커*; 5개 helper 추가**

Edit `apps/desktop-shell/src/handlers/mod.rs` — `add_wildcard_acl` 함수를 *지우지 말고* 아래로 옮기고 5개 helper를 그 위에 추가 (Task 12에서 wildcard 제거):

`use` 갱신:
```rust
use geulos_core::{
    AclEffect, AclEntry, ActorPattern, MethodPattern, Object,
};
```

5 helper:
```rust
/// Window/Explorer/FileTree/Cli — compositor가 user 동작 대표 + desktop-shell set_state.
pub fn add_ui_object_acl(obj: &mut Object) {
    obj.acl.push(AclEntry {
        actor: ActorPattern::SystemCompositor,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
    obj.acl.push(AclEntry {
        actor: ActorPattern::App("desktop-shell".to_string()),
        method: MethodPattern::SetState,
        effect: AclEffect::Allow,
    });
}

/// Folder/File — compositor 무조건 + AI는 path가 granted_dirs 안일 때만 + desktop-shell set_state.
pub fn add_fs_object_acl(obj: &mut Object) {
    obj.acl.push(AclEntry {
        actor: ActorPattern::SystemCompositor,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
    obj.acl.push(AclEntry {
        actor: ActorPattern::AiSession,
        method: MethodPattern::Wildcard,
        effect: AclEffect::AllowIfGrantedDir,
    });
    obj.acl.push(AclEntry {
        actor: ActorPattern::App("desktop-shell".to_string()),
        method: MethodPattern::SetState,
        effect: AclEffect::Allow,
    });
}

/// Dialog — compositor 단독 invoke(respond) + desktop-shell set_state.
/// *외부 actor의 respond 호출 영구 차단 — KI-001 해소의 핵심 가치.*
pub fn add_dialog_acl(obj: &mut Object) {
    obj.acl.push(AclEntry {
        actor: ActorPattern::SystemCompositor,
        method: MethodPattern::Exact("respond".to_string()),
        effect: AclEffect::Allow,
    });
    obj.acl.push(AclEntry {
        actor: ActorPattern::App("desktop-shell".to_string()),
        method: MethodPattern::SetState,
        effect: AclEffect::Allow,
    });
}

/// Filesystem@1 singleton — compositor 무조건 + AI는 read_external/write_external 두 method만.
pub fn add_filesystem_acl(obj: &mut Object) {
    obj.acl.push(AclEntry {
        actor: ActorPattern::SystemCompositor,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
    obj.acl.push(AclEntry {
        actor: ActorPattern::AiSession,
        method: MethodPattern::OneOf(vec!["read_external".into(), "write_external".into()]),
        effect: AclEffect::Allow,
    });
}

/// Desktop/Cli 히스토리 같은 컨테이너 — desktop-shell set_state 단독.
pub fn add_container_acl(obj: &mut Object) {
    obj.acl.push(AclEntry {
        actor: ActorPattern::App("desktop-shell".to_string()),
        method: MethodPattern::SetState,
        effect: AclEffect::Allow,
    });
}
```

- [ ] **Step 8.4: 테스트 통과 확인**

```
cargo test -p geulos-desktop-shell --lib handlers::tests 2>&1 | tail -10
```

Expected: 5 tests passed.

- [ ] **Step 8.5: workspace 회귀**

```
cargo test --workspace 2>&1 | tail -10
```

Expected: 모든 통과 (helper 추가만, 기존 함수 그대로 유지라 회귀 없음).

- [ ] **Step 8.6: commit**

```
git add apps/desktop-shell/src/handlers/mod.rs
git commit -m "$(cat <<'EOF'
feat(desktop-shell): M11 T8 — 5개 typed ACL helper 추가

add_ui_object_acl / add_fs_object_acl / add_dialog_acl / add_filesystem_acl /
add_container_acl. 기존 add_wildcard_acl은 T12에서 제거 — 본 task는 추가만.

Dialog helper는 *외부 actor의 respond 호출 영구 차단* 의도 명시.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: main.rs의 add_wildcard_acl 호출 교체 (초기 mount 객체)

**Files:**
- Modify: `apps/desktop-shell/src/main.rs`

타겟 호출 위치 (이전 grep 결과):
- `main.rs:199` — Open Window 생성 (그러나 handlers/explorer_methods.rs:170으로 이미 이동? 확인)
- `main.rs:469` — Desktop/FileTree/Explorer/Cli/Filesystem 등 초기 객체 loop
- `main.rs:472` — drive_folders loop

- [ ] **Step 9.1: 호출 위치 정확히 재확인**

```
grep -n "add_wildcard_acl" apps/desktop-shell/src/main.rs
```

각 호출의 객체 타입에 따라 helper 매핑:

| 호출 위치 | 객체 | 새 helper |
|---|---|---|
| L469 — desktop/file_tree/explorer/cli/filesystem loop | mixed | 객체별 분기 |
| L472 — drive_folders loop | Folder@1 | `add_fs_object_acl` |
| L199 — Open Window 분기 (있다면) | Window@1 | `add_ui_object_acl` |

- [ ] **Step 9.2: L469 호출 교체**

Read 현재 코드:
```
sed -n '465,475p' apps/desktop-shell/src/main.rs
```

기존:
```rust
for o in [&mut desktop, &mut file_tree, &mut explorer, &mut cli, &mut filesystem_obj] {
    add_wildcard_acl(o);
}
```

교체:
```rust
// M11: 객체 타입별 typed ACL helper 적용. add_wildcard_acl(KI-001/016) 제거.
add_container_acl(&mut desktop);
add_ui_object_acl(&mut file_tree);
add_ui_object_acl(&mut explorer);
add_ui_object_acl(&mut cli);
add_filesystem_acl(&mut filesystem_obj);
```

- [ ] **Step 9.3: L472 drive_folders 교체**

기존:
```rust
for f in &mut drive_folders {
    add_wildcard_acl(f);
}
```

교체:
```rust
// M11: drive Folder도 fs_object — compositor 무조건 + AI는 grant 시만.
for f in &mut drive_folders {
    add_fs_object_acl(f);
}
```

- [ ] **Step 9.4: L199 Open Window 호출 (있다면) 교체**

```
sed -n '195,205p' apps/desktop-shell/src/main.rs
```

`add_wildcard_acl(&mut new_obj)` 패턴이면:
```rust
add_ui_object_acl(&mut new_obj);
```

(객체 타입을 정확히 확인 — Window면 ui_object, Dialog면 dialog.)

- [ ] **Step 9.5: import 갱신**

`main.rs` 상단:
```rust
use geulos_desktop_shell::handlers::{
    add_container_acl, add_dialog_acl, add_filesystem_acl, add_fs_object_acl,
    add_ui_object_acl, // ... 기존 + 새 helper들
};
```

기존 `add_wildcard_acl`은 *남겨둠* (Task 10/11에서 다른 호출처 교체 후 Task 12에서 제거).

- [ ] **Step 9.6: build + manual smoke**

```
cargo build -p geulos-desktop-shell 2>&1 | tail -5
```

이 시점에 *전체 manual smoke*는 부분만 교체된 상태 → 일부 객체는 새 ACL, 일부는 wildcard. ai-bridge / compositor 통신은 *Stage 2의 ObjectServer.grants가 empty*이므로 AI invoke가 거부될 수 있음. 이 stage 끝(Task 11)까지 manual 검증은 미루고 unit test로 충분.

- [ ] **Step 9.7: commit**

```
git add apps/desktop-shell/src/main.rs
git commit -m "$(cat <<'EOF'
refactor(desktop-shell): M11 T9 — main.rs의 add_wildcard_acl 교체

초기 mount 객체 (Desktop/FileTree/Explorer/Cli/Filesystem + drive folders +
Open Window)에서 wildcard helper 호출을 객체 타입별 typed helper로 교체.

다른 호출처 (handlers/*)는 T10/T11에서 처리.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: handlers/* 호출 교체 (fs_methods / window_methods / dialog_methods / external_methods / explorer_methods)

**Files:**
- Modify: `apps/desktop-shell/src/handlers/fs_methods.rs`
- Modify: `apps/desktop-shell/src/handlers/window_methods.rs`
- Modify: `apps/desktop-shell/src/handlers/dialog_methods.rs`
- Modify: `apps/desktop-shell/src/handlers/external_methods.rs`
- Modify: `apps/desktop-shell/src/handlers/explorer_methods.rs`

- [ ] **Step 10.1: 호출 위치 재확인**

```
grep -rn "add_wildcard_acl" apps/desktop-shell/src/handlers/
```

이전 grep 결과 (재확인):
- `fs_methods.rs:146` — Save Dialog mount → **add_dialog_acl**
- `fs_methods.rs:234` — create_file 새 File@1 mount → **add_fs_object_acl**
- `fs_methods.rs:279` — create Dialog → **add_dialog_acl**
- `fs_methods.rs:361` — create_folder 새 Folder@1 mount → **add_fs_object_acl**
- `fs_methods.rs:406` — delete Dialog → **add_dialog_acl**
- `fs_methods.rs:474` — rename Dialog → **add_dialog_acl**
- `fs_methods.rs:517` — rename Dialog 추가 분기 → **add_dialog_acl**
- `fs_methods.rs:634` — ExternalWrite Dialog → **add_dialog_acl**
- `external_methods.rs:116` — read_external Dialog → **add_dialog_acl**
- `dialog_methods.rs:77` — Window mount (Dialog 응답 후) → **add_ui_object_acl**
- `dialog_methods.rs:116` — Window mount (alt 분기) → **add_ui_object_acl**
- `explorer_methods.rs:170` — Open Window mount → **add_ui_object_acl**

- [ ] **Step 10.2: 각 파일에서 import 교체 + 호출 교체**

각 파일 상단 import:
```rust
// 기존: use crate::handlers::add_wildcard_acl;
// 교체:
use crate::handlers::{add_dialog_acl, add_fs_object_acl, add_ui_object_acl};
// (각 파일이 실제 사용하는 helper만 import)
```

각 호출:
```rust
// 기존: add_wildcard_acl(&mut dialog);
// 교체:
add_dialog_acl(&mut dialog);
```

```rust
// 기존 (새 File/Folder 생성 시): add_wildcard_acl(&mut new_obj);
// 교체:
add_fs_object_acl(&mut new_obj);
```

```rust
// 기존 (새 Window 생성 시): add_wildcard_acl(&mut new_window);
// 교체:
add_ui_object_acl(&mut new_window);
```

**한 파일씩** 교체 — 각 교체 후 `cargo build -p geulos-desktop-shell`로 컴파일 OK 확인.

- [ ] **Step 10.3: lazy_expand_if_needed의 child mount 교체**

`apps/desktop-shell/src/handlers/mod.rs:196`의:
```rust
add_wildcard_acl(&mut child);
```
→ child는 Folder/File 모두 가능:
```rust
add_fs_object_acl(&mut child);
```

- [ ] **Step 10.4: 빌드 + 단위 테스트**

```
cargo build -p geulos-desktop-shell 2>&1 | tail -5
cargo test -p geulos-desktop-shell --lib 2>&1 | tail -10
```

Expected: 모두 통과.

- [ ] **Step 10.5: workspace 전체 회귀**

```
cargo test --workspace 2>&1 | tail -15
cargo clippy --workspace --no-deps -- -D warnings 2>&1 | tail -5
```

Expected: 클린.

- [ ] **Step 10.6: commit**

```
git add apps/desktop-shell/src/handlers/
git commit -m "$(cat <<'EOF'
refactor(desktop-shell): M11 T10 — handlers/* add_wildcard_acl 교체

fs_methods (Save/create_file/create_folder/delete/rename/ExternalWrite Dialog +
새 File/Folder mount), external_methods (read_external Dialog), dialog_methods
(Window mount), explorer_methods (Open Window), handlers/mod (lazy_expand 자식)
모두 객체 타입별 typed helper로 교체.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: granted_dirs.rs — GrantUpdate wire 메시지 송신 통합

**Files:**
- Modify: `apps/desktop-shell/src/granted_dirs.rs`
- Modify: `apps/desktop-shell/src/dialog_ops.rs` 또는 grant insert 호출처 — 메시지 송신 인자(stream) 주입

- [ ] **Step 11.1: 현재 granted_dirs 구조 확인**

```
sed -n '1,80p' apps/desktop-shell/src/granted_dirs.rs
```

기존: `Mutex<HashSet<PathBuf>>` + `insert(path)` / `contains(path)` 등.

- [ ] **Step 11.2: GrantUpdate 송신 헬퍼 추가**

Edit `apps/desktop-shell/src/granted_dirs.rs` 끝에 함수 추가:

```rust
use geulos_proto::{encode_frame, GrantUpdate, GrantOp};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use geulos_core::ActorId;

/// granted_dirs에 path 추가 + server-host에 GrantUpdate(Add) 송신.
///
/// 호출자 책임: actor는 grant를 받을 AI session actor (Dialog로 동의한 시점의
/// 활성 ai-bridge connection의 actor_id). stream은 desktop-shell의 server connection.
///
/// wire 송신 실패는 *경고만* 출력하고 local insert는 진행 — server-host가 재시작되면
/// desktop-shell도 곧 끊겨 재시작될 가능성 큼. 회복 정책은 v2.
pub async fn grant_dir(
    granted: &GrantedDirs,
    stream: &mut TcpStream,
    actor: &ActorId,
    path: std::path::PathBuf,
) -> std::io::Result<()> {
    granted.insert(path.clone());
    let msg = GrantUpdate {
        actor: actor.as_str().to_string(),
        path: path.to_string_lossy().to_string(),
        op: GrantOp::Add,
    };
    let body = serde_json::to_vec(&msg)?;
    if let Err(e) = stream.write_all(&encode_frame(&body)).await {
        eprintln!("[granted_dirs] GrantUpdate(Add) wire 송신 실패: {} — local만 반영", e);
    }
    Ok(())
}

/// 철회 — local + wire 동시.
pub async fn revoke_dir(
    granted: &GrantedDirs,
    stream: &mut TcpStream,
    actor: &ActorId,
    path: std::path::PathBuf,
) -> std::io::Result<()> {
    granted.remove(&path);
    let msg = GrantUpdate {
        actor: actor.as_str().to_string(),
        path: path.to_string_lossy().to_string(),
        op: GrantOp::Remove,
    };
    let body = serde_json::to_vec(&msg)?;
    if let Err(e) = stream.write_all(&encode_frame(&body)).await {
        eprintln!("[granted_dirs] GrantUpdate(Remove) wire 송신 실패: {}", e);
    }
    Ok(())
}
```

(만약 `GrantedDirs`에 `remove` 메서드 없으면 추가:
```rust
impl GrantedDirs {
    pub fn remove(&self, path: &std::path::Path) {
        self.0.lock().unwrap().remove(path);
    }
}
```)

- [ ] **Step 11.3: Dialog 응답 핸들러에서 grant_dir 호출**

Find: `apps/desktop-shell/src/handlers/dialog_methods.rs` (또는 dialog_ops.rs) — *Allow 응답 처리 분기*에서 현재 `granted_dirs.insert(...)` 단순 호출 부분.

```
grep -n "granted.*insert\|granted_dirs.insert" apps/desktop-shell/src/
```

각 호출을:
```rust
// 기존:
granted_dirs.insert(dir.clone());

// 교체 (stream + ai_actor 인자 추가):
granted_dirs::grant_dir(granted_dirs, stream, &ai_actor, dir.clone()).await?;
```

호출처에 `ai_actor: &ActorId`를 어떻게 얻을지 — *Dialog가 만들어진 시점의 PendingFs에 ai_actor 저장*해두는 방식. PendingFs enum 각 variant에 `requesting_actor: ActorId` 필드 추가:

```rust
// dialog_ops.rs PendingFs enum 각 variant
PendingFs::Save { file_id, content, requesting_actor: ActorId, ... },
PendingFs::CreateFile { folder_id, name, requesting_actor: ActorId, ... },
// ...
```

생성 시점 (Dialog 띄울 때)의 invoke 핸들러에서 `actor`(현재 핸들러 인자) 그대로 저장. Allow 응답 시 그 actor로 grant.

- [ ] **Step 11.4: 빌드 + 회귀**

```
cargo build -p geulos-desktop-shell 2>&1 | tail -10
cargo test -p geulos-desktop-shell --lib 2>&1 | tail -10
```

- [ ] **Step 11.5: manual smoke — 전체 launcher 띄워 AI write 흐름**

```
.\target\debug\geulos.exe
```

CLI에서 `/ai start test` → API key 있으면 AI 세션 시작 → AI에게 "test1 폴더에 새 파일 만들어줘" 요청 → Dialog "허용" 클릭 → 파일 생성 통과 확인. 두 번째 요청 시 동일 디렉터리는 Dialog 없이 통과 (grant 캐시).

만약 두 번째 요청도 Dialog 뜨거나 PermissionDenied 발생하면 — grant context 미주입 또는 actor 미스매치. 디버그.

- [ ] **Step 11.6: commit**

```
git add apps/desktop-shell/src/granted_dirs.rs apps/desktop-shell/src/handlers/dialog_methods.rs apps/desktop-shell/src/dialog_ops.rs apps/desktop-shell/src/handlers/fs_methods.rs apps/desktop-shell/src/handlers/external_methods.rs
git commit -m "$(cat <<'EOF'
feat(desktop-shell): M11 T11 — granted_dirs ↔ server GrantStore 동기화

grant_dir / revoke_dir helper로 local granted_dirs insert/remove와 동시에
server-host에 GrantUpdate(Add/Remove) wire 메시지 송신. PendingFs 각
variant에 requesting_actor 필드 추가 — Dialog 응답 시점에 정확한 AI actor로
grant.

이제 server-host의 GrantStore가 항상 desktop-shell의 granted_dirs와 일치 →
AI invoke의 AllowIfGrantedDir 효과가 정상 통과.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 12: `add_wildcard_acl` 제거 + grep 가드

**Files:**
- Modify: `apps/desktop-shell/src/handlers/mod.rs` (함수 제거 + 위치 import 정리)

- [ ] **Step 12.1: 잔여 호출 확인**

```
grep -rn "add_wildcard_acl" apps/ compositor/
```

Expected: 0 결과. 1건이라도 남으면 Task 9/10에서 누락한 것 → 해당 위치를 적절한 helper로 교체.

- [ ] **Step 12.2: `add_wildcard_acl` 함수 정의 제거**

Edit `apps/desktop-shell/src/handlers/mod.rs` — `pub fn add_wildcard_acl(obj: &mut Object) { ... }` 블록 *삭제*. 위 doc-comment도 함께 삭제.

- [ ] **Step 12.3: workspace 전체 빌드**

```
cargo build --workspace 2>&1 | tail -10
```

Expected: 모든 binary 빌드 OK. 만약 어딘가에서 `add_wildcard_acl` import 잔존 에러 → 그 파일에서 import 라인 삭제.

- [ ] **Step 12.4: grep 가드 검증**

```
grep -rn "ActorPattern::Wildcard\|MethodPattern::Wildcard" apps/ compositor/ ai-bridge/
```

Expected: 매치 0 (test 디렉터리 외). echo-app은 Task 13에서 처리.

```
grep -rn "ActorPattern::Wildcard\|MethodPattern::Wildcard" apps/
```

만약 echo-app/src에 남아있으면 Task 13까지 보류 — 본 task는 desktop-shell + compositor만.

- [ ] **Step 12.5: 전체 회귀**

```
cargo test --workspace 2>&1 | tail -15
cargo clippy --workspace --no-deps -- -D warnings 2>&1 | tail -5
cargo fmt --check 2>&1 | tail -5
```

Expected: 모두 클린.

- [ ] **Step 12.6: commit**

```
git add apps/desktop-shell/src/handlers/mod.rs
git commit -m "$(cat <<'EOF'
refactor(desktop-shell): M11 T12 — add_wildcard_acl 함수 제거 (KI-001 해소)

desktop-shell의 wildcard ACL helper 영구 제거. 모든 호출처가 T9~T11에서
typed helper로 교체됨. 본 commit으로 desktop-shell 측 KI-001 해소.

echo-app 측 잔여 wildcard는 T13에서 정리. CI grep 가드는 T14에서 추가.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Stage 4 — echo-app 정리

## Task 13: echo-app wildcard 제거

**Files:**
- Modify: `apps/echo-app/src/lib.rs`
- Modify: `apps/echo-app/tests/echo_logic_test.rs` (assertion 갱신)

- [ ] **Step 13.1: 현재 wildcard 위치 확인**

```
grep -n "ActorPattern::Wildcard\|MethodPattern::Wildcard" apps/echo-app/src/lib.rs
```

Expected: 2~3 hit.

```
sed -n '20,35p' apps/echo-app/src/lib.rs
```

기존:
```rust
fn add_acl(obj: &mut Object) {
    obj.acl.push(AclEntry {
        actor: ActorPattern::Wildcard,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
}
```

- [ ] **Step 13.2: echo-app의 정확한 actor 요구사항**

echo-app은 *외부 client (geulosh)가 press 호출 + state 갱신을 보는 시연*이 목적 (M3 acceptance). 따라서:
- 외부 client (compositor 또는 다른 app) — press invoke 호출
- echo-app 자기 자신 — set_state(count) 갱신

가장 정확: 외부 client 매칭은 *AnyApp + Compositor* 또는 *Wildcard 유지하되 method를 press로 좁힘*.

M11 정신: wildcard 제거. 단 echo-app은 *시연용*이므로 actor 검증 약함은 *덜 위험*. 그래도 wildcard 대신 *명시적 패턴 enumerate*:

교체:
```rust
fn add_acl(obj: &mut Object) {
    // M11: wildcard 제거. 외부 client는 SystemCompositor + AI + 다른 app 모두 press 가능.
    // 명시적 enumeration — 'Wildcard 폐지' 정책 일관성 유지.
    for actor_pat in [
        ActorPattern::SystemCompositor,
        ActorPattern::AiSession,
        ActorPattern::App("echo-app".to_string()),
    ] {
        obj.acl.push(AclEntry {
            actor: actor_pat,
            method: MethodPattern::Exact("press".to_string()),
            effect: AclEffect::Allow,
        });
    }
    // 자기 set_state.
    obj.acl.push(AclEntry {
        actor: ActorPattern::App("echo-app".to_string()),
        method: MethodPattern::SetState,
        effect: AclEffect::Allow,
    });
}
```

(다른 app id를 echo invoke 가능하게 하려면 별 entry — M11 범위 외.)

- [ ] **Step 13.3: echo_logic_test 갱신**

```
sed -n '25,40p' apps/echo-app/tests/echo_logic_test.rs
```

`matches!(entry.actor, ActorPattern::Wildcard)` 같은 assertion이면:

```rust
// 기존:
matches!(entry.actor, ActorPattern::Wildcard) && entry.effect == AclEffect::Allow
// 교체:
matches!(
    entry.actor,
    ActorPattern::SystemCompositor | ActorPattern::AiSession | ActorPattern::App(_)
) && entry.effect == AclEffect::Allow
```

- [ ] **Step 13.4: M3 acceptance 통과 확인**

```
cargo test --workspace 2>&1 | tail -20
```

Expected: 모든 통과. 특히 server-host/tests/m3_acceptance.rs가 통과 — echo-app 외부 client press가 거부되지 않아야.

만약 m3_acceptance.rs가 actor 정보를 *manifest id 없이 default*로 connect한다면 — actor가 default app id가 되어 매칭 실패할 수 있음. 그 경우 test의 manifest id를 "compositor" 또는 별 actor로 명시.

- [ ] **Step 13.5: grep 가드 — apps/echo-app 클린 확인**

```
grep -rn "ActorPattern::Wildcard\|MethodPattern::Wildcard" apps/echo-app/src/
```

Expected: 0.

- [ ] **Step 13.6: commit**

```
git add apps/echo-app/src/lib.rs apps/echo-app/tests/echo_logic_test.rs
git commit -m "$(cat <<'EOF'
refactor(echo-app): M11 T13 — wildcard 제거 + 명시적 actor enumeration

echo-app의 ACL을 SystemCompositor / AiSession / App("echo-app")의 press
invoke + 자기 set_state로 한정. wildcard 영구 제거.

M3 acceptance test는 외부 client가 manifest id 명시로 통과.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Stage 5 — 회귀 + 가드 + 문서

## Task 14: CI grep 가드 스크립트

**Files:**
- Create: `scripts/check-no-wildcard-acl.sh` (또는 .ps1 — 환경별 이중)
- Modify: `.github/workflows/*.yml` 또는 기존 CI 설정 (필요 시)

- [ ] **Step 14.1: 가드 스크립트 작성**

Create `scripts/check-no-wildcard-acl.sh`:

```bash
#!/usr/bin/env bash
# M11 — apps/ + compositor/ + ai-bridge/ + server-host/ 의 src/ 안에
# ActorPattern::Wildcard / MethodPattern::Wildcard 사용을 *0건*으로 강제.
# tests/ 디렉터리는 회귀 테스트가 의도적으로 wildcard를 사용하므로 제외.
# core/src/object/acl.rs의 enum definition은 제외 (정의 자체는 유지).

set -e

VIOLATIONS=$(grep -rn \
    --include='*.rs' \
    --exclude-dir='tests' \
    'ActorPattern::Wildcard\|MethodPattern::Wildcard' \
    apps/ compositor/ ai-bridge/ server-host/ \
    | grep -v 'core/src/object/acl.rs' \
    || true)

if [ -n "$VIOLATIONS" ]; then
    echo "❌ M11 회귀: wildcard ACL 사용 발견"
    echo "$VIOLATIONS"
    exit 1
fi

echo "✅ wildcard ACL 사용 0건 (M11 KI-001/016 가드 통과)"
```

Windows용:
Create `scripts/check-no-wildcard-acl.ps1`:

```powershell
# M11 - wildcard ACL 사용 grep 가드 (Windows)
$violations = Select-String -Path "apps/**/*.rs", "compositor/**/*.rs", "ai-bridge/**/*.rs", "server-host/**/*.rs" `
    -Pattern "ActorPattern::Wildcard|MethodPattern::Wildcard" `
    -Exclude "**/tests/**", "**/object/acl.rs"

if ($violations) {
    Write-Host "M11 회귀: wildcard ACL 사용 발견" -ForegroundColor Red
    $violations | ForEach-Object { Write-Host $_ }
    exit 1
}
Write-Host "wildcard ACL 사용 0건 (M11 가드 통과)" -ForegroundColor Green
```

- [ ] **Step 14.2: 로컬 실행 확인**

```
bash scripts/check-no-wildcard-acl.sh
```

Expected: `✅ wildcard ACL 사용 0건`.

- [ ] **Step 14.3: CI 통합 (있으면)**

`.github/workflows/ci.yml` 또는 기존 CI 정의에 step 추가:
```yaml
- name: M11 wildcard ACL guard
  run: bash scripts/check-no-wildcard-acl.sh
```

CI 정의가 없으면 step skip — 사용자가 수동 실행 정책.

- [ ] **Step 14.4: commit**

```
git add scripts/check-no-wildcard-acl.sh scripts/check-no-wildcard-acl.ps1 .github/workflows/
git commit -m "$(cat <<'EOF'
chore(scripts): M11 T14 — wildcard ACL grep 가드 스크립트

apps/compositor/ai-bridge/server-host의 src/ 안에 ActorPattern/MethodPattern
::Wildcard 사용이 *재도입*되면 CI fail. tests/와 core acl.rs definition은
제외.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 15: m11-acceptance manual test 문서

**Files:**
- Create: `docs/manual-tests/m11-acceptance.md`

- [ ] **Step 15.1: 시나리오 12개 작성**

Create `docs/manual-tests/m11-acceptance.md`:

```markdown
# M11 Acceptance — 수동 회귀 시나리오

**전제:** geulos.exe (launcher) 빌드 + 실행. AI 시나리오는 ANTHROPIC_API_KEY 또는 `/ai start` awaiting flow로 설정.

## 시나리오 A — compositor 사용자 동작 (회귀)

### A1. Explorer.navigate_to
1. launcher 띄움.
2. 좌측 트리에서 C: 폴더명 클릭.
3. **기대:** 우측 Explorer에 C: 내부가 표시.

### A2. Cli.submit_input
1. CLI 영역 클릭 → "hello" 타이핑 → Enter.
2. **기대:** lines 히스토리에 "> hello" + 응답 표시.

### A3. Window 클릭 close
1. 우측에서 파일 더블클릭 → Window 열림.
2. Window 우상단 X 또는 close 동작.
3. **기대:** Window 사라짐.

## 시나리오 B — AI granted/ungranted 경계

### B1. AI Filesystem@1 (path 무관)
1. `/ai start test1` → "임의 경로의 파일을 읽어달라"는 요청 (cwd 밖).
2. **기대:** Dialog 없이 통과 (Filesystem@1.read_external은 항상 허용).

### B2. AI Folder.create_file (granted dir 안 + Dialog 동의)
1. AI에게 "현재 폴더에 hello.txt 만들어줘" 요청.
2. **기대:** Dialog "AI가 <dir>에서 파일 작업 허용?" 표시 → "허용" → 파일 생성 성공.

### B3. AI 같은 Folder 후속 호출 (grant 캐시)
1. B2 직후 "이번엔 world.txt 만들어줘" 요청.
2. **기대:** *Dialog 없이* 통과 (granted_dirs + server GrantStore 캐시).

### B4. AI 다른 Folder 호출 (ungranted)
1. cwd의 다른 sub-folder를 대상으로 "거기에 파일 만들어줘" 요청.
2. **기대:** *새 Dialog* 표시 (디렉터리별 grant).

## 시나리오 C — KI-001 차단 검증 (핵심)

### C1. 외부 geulosh로 Dialog.respond 시도
1. AI에게 write 요청 → Dialog 표시 상태에서 멈춤.
2. 별 터미널에서 `geulosh --connect <addr>` → `query type aios.builtin/Dialog@1` → 응답으로 Dialog ID 얻음.
3. `geulosh invoke <dialog_id> respond '{"choice":"allow"}'`.
4. **기대:** `PermissionDenied` 응답. Dialog는 *여전히* 사용자 응답 대기.

### C2. 외부 geulosh로 Window.close 시도
1. 어떤 파일 Window 열어둠.
2. `geulosh invoke <window_id> close '{}'`.
3. **기대:** `PermissionDenied`. Window 그대로.

### C3. 외부 geulosh로 set_state Window.title
1. `geulosh invoke <window_id> set_state '{"key":"title","value":"hijacked"}'`.
   (또는 wire 명령 형식 — geulosh 정확 syntax 따라.)
2. **기대:** `PermissionDenied`. title 그대로.

## 시나리오 D — invariants

### D1. desktop-shell SetState 통과
- 스크롤 동작 → scroll_y SetState → 통과 (compositor → SetState).
- 새 파일 외부 생성 → fs_watcher → desktop-shell이 child_count SetState → 통과.

### D2. AI invoke Window/Explorer/Cli/Dialog 거부
- AI prompt: "Window 닫아줘" 또는 "Explorer.navigate_to 호출해줘".
- **기대:** AI 측에서 `PermissionDenied` 메시지 수신 → AI가 사용자에게 "차단됨" 안내.

## 통과 기준

12개 시나리오 모두 기대대로. 하나라도 실패 시 plan T9/T10/T11 디버그.
```

- [ ] **Step 15.2: commit**

```
git add docs/manual-tests/m11-acceptance.md
git commit -m "$(cat <<'EOF'
docs(manual-test): M11 T15 — acceptance 시나리오 12개

A. compositor 회귀 (Explorer/Cli/Window) — 기존 동작 보장
B. AI granted/ungranted Folder 경계 — Dialog 흐름 + grant 캐시
C. KI-001 차단 — 외부 geulosh로 Dialog.respond / Window.close /
   set_state Window.title 시도 → 모두 PermissionDenied
D. invariants — desktop-shell SetState 통과 / AI invoke UI 객체 거부

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 16: ADR-037 + known-issues 갱신 + final

**Files:**
- Create: `docs/adr/037-security-acl-hardening.md`
- Modify: `docs/known-issues.md`

- [ ] **Step 16.1: ADR-037 작성**

Create `docs/adr/037-security-acl-hardening.md`:

```markdown
# ADR-037 — 보안 ACL 강화 (wildcard 제거 + AllowIfGrantedDir)

- **상태:** Accepted
- **결정일:** 2026-05-23
- **부모 spec:** `docs/specs/2026-05-23-geulos-m11-security-acl.md`
- **해소 KI:** KI-001 (M3부터 wildcard ACL), KI-016 (M8 set_state wildcard)

## Context

M9/M10 마감 시점에도 desktop-shell 객체 거의 전부에 `add_wildcard_acl`이
박혀있어 외부 client가 임의 객체 invoke + Dialog.respond 우회 가능. M9/M10
spec에 ACL 교체 task가 포함되지 않아 이월된 부채. M11 단일 목표로 해소.

## Decision

1. **ACL 표현:** 객체별 inline `Vec<AclEntry>` *유지*. typed helper 5개로
   분화 — `add_ui_object_acl/add_fs_object_acl/add_dialog_acl/add_filesystem_acl/
   add_container_acl`. 타입별 policy table 도입은 v2로 미룸 (불필요한 복잡도).

2. **AI invoke path-aware:** 새 `AclEffect::AllowIfGrantedDir` + 객체의
   `props.path`를 runtime에 `GrantContext.is_granted(actor, path)`로 조회.
   AI는 Filesystem@1 (항상) + granted_dirs 안의 Folder/File (조건부) 만 통과.

3. **set_state ACL 일관화:** server의 set_state 핸들러가 별도 wildcard 검사
   하던 임시 로직을 제거하고 invoke와 동일한 `Object::is_allowed(actor, AclOp,
   grants)` 사용. `MethodPattern::SetState` variant로 op 구분.

4. **GrantStore wire 동기화:** desktop-shell의 Dialog 응답으로 grant 추가/철회
   시 `GrantUpdate` wire 메시지로 server-host의 GrantStore에 반영. server는
   sender의 actor가 `app:desktop-shell:*` 일 때만 수락 — 외부 client가 자기에게
   grant를 주는 우회 차단.

5. **Dialog 영구 차단:** `add_dialog_acl`이 *system:compositor의 respond
   invoke만* 허용. AI/외부 app의 respond 호출은 PermissionDenied. 이로써 AI
   동의 우회 영구 차단 — KI-001 해소의 가장 큰 가치.

## 대안

- (A) wildcard 유지하고 intercept만 강화: ACL 명목적 교체로 끝나 보안 모델
  명료성 X. 기각.
- (B) 타입별 policy table (중앙 dispatch): 깔끔하나 invoke/set_state 경로
  변경 큼. v2 후보.
- (C) desktop-shell이 AI invoke를 proxy로 re-invoke: server 변경 없음.
  invoke 이중 round-trip + 응답 source 혼란. 기각.

## Consequences

**Positive:**
- KI-001/016 해소. 외부 client의 Dialog 우회 / 임의 invoke 차단.
- AI 동작 경계가 *명확* — Filesystem@1 + granted dir만.
- set_state ACL 평가 경로 통일 — 미래 권한 모델 확장 base.

**Negative:**
- AllowIfGrantedDir의 동적 평가가 매 invoke마다 path lookup. HashSet O(1)
  + Path prefix 비교라 미미하나 측정값 없음. v2에 prof 검토.
- ActorPattern enum에 5 variant (Exact/Wildcard/SystemCompositor/AiSession/
  App)로 늘어남. Wire 직렬화 형식이 enum tag 의존이라 *동시 client/server
  업그레이드* 필요 (현재는 launcher가 일괄 배포라 무관).

**Neutral:**
- echo-app도 wildcard 제거 — 외부 client press는 SystemCompositor/AiSession/
  App enumeration으로 통과. 다음 외부 앱 추가 시 helper 갱신 필요.
```

- [ ] **Step 16.2: known-issues 갱신**

Edit `docs/known-issues.md`:

KI-001 + KI-016에 해소 마커 추가:
```markdown
### KI-001 — ✅ echo-app + desktop-shell wildcard ACL (해소됨)

- **언제 해소:** 2026-05-23 (M11 정식 마감). 자세한 내역 ADR-037.
- **변경 요약:** 객체별 typed helper 5개 (add_ui_object/fs_object/dialog/
  filesystem/container_acl)로 wildcard 16곳 일괄 교체. Dialog.respond는
  system:compositor 단독 — 외부 우회 영구 차단.
- **검증:** scripts/check-no-wildcard-acl.sh, docs/manual-tests/m11-
  acceptance.md.

### KI-016 — ✅ set_state ACL wildcard (해소됨)

- **언제 해소:** 2026-05-23. KI-001과 함께. set_state ACL 검사가 invoke와
  동일한 Object::is_allowed(AclOp::SetState(_), &grants) 평가 경로로 통일.
```

마지막 마일스톤 메모 섹션에 M11 추가:
```markdown
- **M11 정식 마감 (2026-05-23):** KI-001 / KI-016 해소. desktop-shell의
  wildcard ACL 16곳을 객체 타입별 typed helper로 교체. AllowIfGrantedDir
  새 AclEffect로 AI path-aware grant 도입. GrantUpdate wire 메시지로
  desktop-shell ↔ server GrantStore 동기. set_state ACL이 invoke와 동일
  평가 경로로 통일. ADR-037.
```

정기 검토 시점 갱신:
```markdown
- **M12 entry 시:** KI-002 (매니페스트 권한 강제) + KI-003 (query owner
  ai 매칭) + KI-015 (session 파일 잔존 도구) + granted_dirs 디스크 영구화 +
  AI 감사 로그. M11.5 후보들.
- **6개월 (2026-11-23):** KI-014/017 v2 확인.
```

- [ ] **Step 16.3: full manual acceptance 실행**

`docs/manual-tests/m11-acceptance.md`의 시나리오 A1~D2 모두 실행. 결과 기록:
```
docs/manual-tests/m11-acceptance.md
[2026-05-23] A1 ✓ / A2 ✓ / A3 ✓ / B1 ✓ / B2 ✓ / B3 ✓ / B4 ✓ / C1 ✓ / C2 ✓ / C3 ✓ / D1 ✓ / D2 ✓
```

- [ ] **Step 16.4: final 회귀**

```
cargo test --workspace 2>&1 | tail -10
cargo clippy --workspace --no-deps -- -D warnings 2>&1 | tail -5
cargo fmt --check 2>&1 | tail -5
bash scripts/check-no-wildcard-acl.sh
```

Expected: 모두 클린.

- [ ] **Step 16.5: commit**

```
git add docs/adr/037-security-acl-hardening.md docs/known-issues.md docs/manual-tests/m11-acceptance.md
git commit -m "$(cat <<'EOF'
docs: M11 T16 — ADR-037 + known-issues 갱신 + acceptance 결과

- ADR-037: 보안 ACL 강화 결정 본문 (5 helper + AllowIfGrantedDir +
  GrantUpdate wire + Dialog 영구 차단).
- KI-001/016 해소 마커 + M11 종료 메모.
- m11-acceptance 12 시나리오 통과 기록.

M11 정식 마감.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Plan Self-Review 결과

**Spec coverage:**
- ✓ KI-001 / KI-016 해소 (Task 9-12 + 13)
- ✓ ActorPattern 신규 variants (Task 1)
- ✓ MethodPattern 확장 (Task 1)
- ✓ AclEffect::AllowIfGrantedDir (Task 2)
- ✓ GrantContext trait (Task 2)
- ✓ Object::is_allowed 시그니처 변경 + path() helper (Task 3)
- ✓ invoke + set_state ACL 일관화 (Task 4)
- ✓ GrantUpdate wire 메시지 (Task 5)
- ✓ server-host grant handle + actor 가드 (Task 6)
- ✓ AI invoke path-aware end-to-end (Task 7)
- ✓ 5개 helper (Task 8)
- ✓ 호출 16곳 교체 (Task 9-11)
- ✓ wildcard 제거 (Task 12, 13)
- ✓ grep 가드 (Task 14)
- ✓ manual acceptance (Task 15-16)
- ✓ ADR-037 + KI 갱신 (Task 16)

**Placeholder scan:** 모든 step에 코드 + 정확 경로. "TBD" / "implement later" 없음.

**Type 일관성:**
- `ActorPattern::App(String)` — Task 1 정의, Task 8 helper, Task 9-11 호출, Task 13 echo-app — 일관.
- `AclOp::Invoke(String)` / `SetState(String)` — Task 2 정의, Task 3 평가, Task 4 server 사용, Task 8 test — 일관.
- `GrantContext::is_granted(actor, path)` — Task 2 trait, Task 4 GrantStore 구현, Task 7 end-to-end — 일관.
- `GrantUpdate { actor: String, path: String, op: GrantOp }` — Task 5 정의, Task 6 server, Task 11 desktop-shell — 일관.

---

## 실행 핸드오프

**Plan complete and saved to `docs/plans/2026-05-23-geulos-m11-security-acl.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — controller가 task별 fresh subagent + spec/code review

**2. Inline Execution** — 본 세션에서 batch 실행 + 사용자 checkpoint

**Which approach?**
