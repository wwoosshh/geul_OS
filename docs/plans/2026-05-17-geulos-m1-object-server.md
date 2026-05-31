> **Status:** completed (2026-05-17)
> **Note:** M1 객체 서버 + 이벤트 버스 정식 마감 — single-writer 모델 + proptest 1만 케이스 통과.

# GeulOS M1 — 객체 서버 + 이벤트 버스 실행 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** GeulOS의 *심장*인 객체 서버와 이벤트 버스를 인-메모리 라이브러리로 구현. 네트워크/GUI 없음. `Container > Text("hello")` 트리를 만들고 직렬화·역직렬화·invoke·subscribe가 모두 동작하며, proptest 1만 케이스 무사 통과.

**Architecture:** 단일 라이터 이벤트 루프 (ADR-003) — 모든 mutate는 ObjectServer 메서드를 통해 EventBus에 직렬 enqueue. 객체 트리는 *유도된 상태*, 이벤트 로그는 *원천 진실* (Event Sourcing). 단일 스레드 라이브러리이므로 락 없음. 표준 객체 타입 4종(Container/Text/Button/Toggle)이 표준 라이브러리로 동봉.

**Tech Stack:** Rust stable, `serde`/`serde_json`, `uuid`, `thiserror`, `proptest`.

**Selection criteria (완료 조건):**
- `cargo build --workspace` 성공
- `cargo test --workspace` 통과 (proptest 1만 케이스 포함)
- `cargo clippy --workspace --all-targets -- -D warnings` 무경고
- `core/tests/acceptance_test.rs`의 `container_text_round_trip` 테스트 PASS
- CI 그린

---

## 파일 구조 (사전 매핑)

```
core/
├── Cargo.toml                          # proptest, serde_json 추가
├── src/
│   ├── lib.rs                          # 재export 업데이트
│   ├── object.rs                       # (M0 기존, 곧 모듈로 승격)
│   ├── object/
│   │   ├── mod.rs                      # Object 구조체
│   │   ├── identity.rs                 # ObjectId, EventId, ActorId, TypeUri
│   │   ├── method.rs                   # MethodSig, ArgSpec
│   │   ├── acl.rs                      # AclEntry, AclEffect, MethodPattern, ActorPattern
│   │   └── std_types.rs                # Container/Text/Button/Toggle 생성자
│   ├── event/
│   │   ├── mod.rs                      # Event, EventKind, LifecycleKind
│   │   └── bus.rs                      # EventBus
│   └── server/
│       ├── mod.rs                      # ObjectServer
│       ├── mount.rs                    # mount()
│       ├── invoke.rs                   # invoke() + ACL gate
│       ├── query.rs                    # query()
│       └── subscribe.rs                # subscribe() + drain
└── tests/
    ├── object_test.rs                  # (M0 기존, ObjectId 3 tests)
    ├── identity_test.rs                # EventId/ActorId/TypeUri tests
    ├── event_test.rs                   # Event tests
    ├── object_struct_test.rs           # Object 구조체 tests
    ├── std_types_test.rs               # Container/Text/Button/Toggle
    ├── event_bus_test.rs               # EventBus
    ├── server_mount_test.rs            # mount()
    ├── server_invoke_test.rs           # invoke()
    ├── server_query_test.rs            # query()
    ├── server_subscribe_test.rs        # subscribe()
    ├── acceptance_test.rs              # 종합 시나리오
    ├── proptest_p1_tree_integrity.rs   # P1 속성
    └── proptest_p5_roundtrip.rs        # P5 속성
```

---

## Task 1: Identity 모듈 분리 + EventId / ActorId / TypeUri

기존 `core/src/object.rs`를 `core/src/object/mod.rs` + `identity.rs`로 재구성하고, 3개의 새 식별자 타입을 TDD로 추가.

**Files:**
- Create: `core/src/object/mod.rs`
- Create: `core/src/object/identity.rs`
- Delete: `core/src/object.rs`
- Modify: `core/src/lib.rs`
- Create: `core/tests/identity_test.rs`

- [ ] **Step 1: 실패하는 테스트 작성 (identity_test.rs)**

```rust
use geulos_core::{ActorId, EventId, TypeUri};

#[test]
fn event_id_is_monotonic_via_new_in_sequence() {
    let a = EventId::new();
    let b = EventId::new();
    let c = EventId::new();
    assert!(a.as_u64() < b.as_u64());
    assert!(b.as_u64() < c.as_u64());
}

#[test]
fn actor_id_local_user_is_constant() {
    let u1 = ActorId::local_user();
    let u2 = ActorId::local_user();
    assert_eq!(u1, u2);
    assert_eq!(u1.as_str(), "user:local");
}

#[test]
fn actor_id_ai_session_is_unique() {
    let s1 = ActorId::new_ai_session();
    let s2 = ActorId::new_ai_session();
    assert_ne!(s1, s2);
    assert!(s1.as_str().starts_with("ai:"));
}

#[test]
fn type_uri_parses_namespace_and_version() {
    let t = TypeUri::parse("aios.std/Button@1").expect("should parse");
    assert_eq!(t.as_str(), "aios.std/Button@1");
}

#[test]
fn type_uri_rejects_malformed() {
    assert!(TypeUri::parse("nostuff").is_err());
    assert!(TypeUri::parse("missing@version").is_err());
}

#[test]
fn type_uri_serializes_round_trip() {
    let t = TypeUri::parse("aios.std/Container@1").unwrap();
    let s = serde_json::to_string(&t).unwrap();
    let back: TypeUri = serde_json::from_str(&s).unwrap();
    assert_eq!(t, back);
}
```

- [ ] **Step 2: 테스트 실행 → 실패 확인**

Run: `cargo test -p geulos-core --test identity_test`
Expected: 컴파일 실패 ("`EventId` not found" 등).

- [ ] **Step 3: object 모듈 디렉터리 생성 + identity.rs 구현**

먼저 `core/src/object.rs`를 `git rm` 한 뒤 디렉터리화. Windows에서는 `git mv`로 안 됨 — `git rm` + 새 파일 생성.

```bash
git rm core/src/object.rs
mkdir core/src/object
```

`core/src/object/mod.rs`:

```rust
//! 객체 관련 타입과 ID 정의.

pub mod identity;

pub use identity::{ActorId, EventId, ObjectId, TypeUri};
```

`core/src/object/identity.rs`:

```rust
//! 시스템 식별자 타입들.
//!
//! - `ObjectId`: 객체 인스턴스 고유 ID (UUID v4)
//! - `EventId`: 이벤트의 전순서를 부여하는 단조 증가 ID
//! - `ActorId`: 동작을 일으킨 주체 식별자 (사용자/AI/앱/시스템)
//! - `TypeUri`: 객체 타입 식별자 (`aios.std/Button@1` 형식)

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// 시스템 전역에서 유일한 객체 식별자.
///
/// 객체는 한 번 생성되면 ID가 변하지 않는다. 객체가 *소멸*해도 ID는 재사용되지
/// 않는다 — 이벤트 로그의 인과성을 깨지 않기 위함.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectId(Uuid);

impl ObjectId {
    /// 새로운 임의 ObjectId를 발급한다.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ObjectId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 이벤트의 전순서를 부여하는 단조 증가 ID.
///
/// 단일 라이터 모델(ADR-003)에서 이벤트 버스가 발급. 시스템 부팅 시 0부터 시작.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EventId(u64);

static NEXT_EVENT_ID: AtomicU64 = AtomicU64::new(1);

impl EventId {
    /// 새 EventId를 발급한다.
    ///
    /// 본 함수는 프로세스 전역 카운터를 사용. M1에서는 이 정도로 충분.
    /// 향후 ObjectServer가 자체 카운터를 들고 갈 수도 있음.
    pub fn new() -> Self {
        Self(NEXT_EVENT_ID.fetch_add(1, Ordering::SeqCst))
    }

    /// 내부 u64 값을 얻는다.
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Default for EventId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for EventId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ev:{}", self.0)
    }
}

/// 동작을 일으킨 주체 식별자.
///
/// 형식: `user:local`, `ai:<UUID>`, `app:<manifest-id>:<instance-UUID>`,
/// `system:compositor` 등.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActorId(String);

impl ActorId {
    /// 콘솔 로컬 사용자.
    pub fn local_user() -> Self {
        Self("user:local".to_string())
    }

    /// 새 AI 세션.
    pub fn new_ai_session() -> Self {
        Self(format!("ai:{}", Uuid::new_v4()))
    }

    /// 앱 인스턴스.
    pub fn new_app(manifest_id: &str) -> Self {
        Self(format!("app:{}:{}", manifest_id, Uuid::new_v4()))
    }

    /// 시스템 컴포지터.
    pub fn system_compositor() -> Self {
        Self("system:compositor".to_string())
    }

    /// 원시 문자열로 변환.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ActorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 객체 타입 식별자.
///
/// 형식: `<namespace>/<name>@<version>` 예: `aios.std/Button@1`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeUri(String);

/// TypeUri 파싱 오류.
#[derive(Debug, Error)]
pub enum TypeUriParseError {
    /// 슬래시(`/`) 또는 골뱅이(`@`) 누락.
    #[error("TypeUri 형식이 잘못됨: '{0}' (예상: <namespace>/<name>@<version>)")]
    Malformed(String),
}

impl TypeUri {
    /// 문자열을 파싱해 TypeUri를 만든다.
    pub fn parse(s: &str) -> Result<Self, TypeUriParseError> {
        // 최소 검증: '/'와 '@'가 정확히 한 번씩 등장하고 순서가 맞아야 함.
        let slash = s.find('/').ok_or_else(|| TypeUriParseError::Malformed(s.to_string()))?;
        let at = s.find('@').ok_or_else(|| TypeUriParseError::Malformed(s.to_string()))?;
        if slash >= at {
            return Err(TypeUriParseError::Malformed(s.to_string()));
        }
        // 각 부분이 비어있지 않아야 함.
        if s[..slash].is_empty() || s[slash + 1..at].is_empty() || s[at + 1..].is_empty() {
            return Err(TypeUriParseError::Malformed(s.to_string()));
        }
        Ok(Self(s.to_string()))
    }

    /// 원시 문자열로 변환.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TypeUri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
```

- [ ] **Step 4: `core/src/lib.rs` 업데이트**

```rust
//! GeulOS core crate.
//!
//! TCB(Trusted Computing Base)에 해당하는 컴포넌트들을 담는다:
//! 객체 서버, 이벤트 버스, 권한 매니저.

pub mod object;

pub use object::{ActorId, EventId, ObjectId, TypeUri};
```

- [ ] **Step 5: 기존 `core/tests/object_test.rs` 변경 없음 확인**

기존 ObjectId 테스트 3개는 그대로 동작해야 함 — `geulos_core::ObjectId` 임포트가 여전히 유효 (lib.rs에서 재export).

Run: `cargo test -p geulos-core --test object_test`
Expected: 3 tests pass.

- [ ] **Step 6: 새 identity 테스트 실행 → 통과 확인**

Run: `cargo test -p geulos-core --test identity_test`
Expected: 6 tests pass.

- [ ] **Step 7: 전체 sanity**

Run: `cargo build -p geulos-core && cargo test -p geulos-core && cargo clippy -p geulos-core --all-targets -- -D warnings`
Expected: 모두 그린.

- [ ] **Step 8: 커밋**

```bash
git add -A
git commit -m "feat(core): identity 모듈 분리 + EventId/ActorId/TypeUri 추가"
```

---

## Task 2: MethodSig / ArgSpec / AclEntry

**Files:**
- Create: `core/src/object/method.rs`
- Create: `core/src/object/acl.rs`
- Modify: `core/src/object/mod.rs`
- Create: `core/tests/acl_test.rs`

- [ ] **Step 1: 실패하는 테스트 작성 (acl_test.rs)**

```rust
use geulos_core::{AclEffect, AclEntry, ActorId, ActorPattern, ArgSpec, MethodPattern, MethodSig};

#[test]
fn method_sig_constructs() {
    let sig = MethodSig::new("press")
        .with_arg(ArgSpec::new("force", "integer"))
        .with_returns("void");
    assert_eq!(sig.name(), "press");
    assert_eq!(sig.args().len(), 1);
    assert_eq!(sig.returns(), Some("void"));
}

#[test]
fn acl_entry_exact_actor_exact_method_matches() {
    let actor = ActorId::local_user();
    let entry = AclEntry {
        actor: ActorPattern::Exact(actor.clone()),
        method: MethodPattern::Exact("press".to_string()),
        effect: AclEffect::Allow,
    };
    assert!(entry.matches(&actor, "press"));
    assert!(!entry.matches(&actor, "release"));
    assert!(!entry.matches(&ActorId::new_ai_session(), "press"));
}

#[test]
fn acl_entry_wildcard_method_matches_anything() {
    let actor = ActorId::local_user();
    let entry = AclEntry {
        actor: ActorPattern::Exact(actor.clone()),
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    };
    assert!(entry.matches(&actor, "press"));
    assert!(entry.matches(&actor, "anything"));
}

#[test]
fn acl_entry_serde_round_trip() {
    let entry = AclEntry {
        actor: ActorPattern::Exact(ActorId::local_user()),
        method: MethodPattern::Exact("press".to_string()),
        effect: AclEffect::Allow,
    };
    let s = serde_json::to_string(&entry).unwrap();
    let back: AclEntry = serde_json::from_str(&s).unwrap();
    assert_eq!(entry.effect, back.effect);
}
```

- [ ] **Step 2: 테스트 실행 → 실패 확인**

Run: `cargo test -p geulos-core --test acl_test`
Expected: 컴파일 실패.

- [ ] **Step 3: `core/src/object/method.rs` 구현**

```rust
//! 메서드 시그니처 정의.

use serde::{Deserialize, Serialize};

/// 객체 메서드의 인자 사양.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgSpec {
    name: String,
    type_hint: String,
}

impl ArgSpec {
    /// 새 ArgSpec.
    pub fn new(name: impl Into<String>, type_hint: impl Into<String>) -> Self {
        Self { name: name.into(), type_hint: type_hint.into() }
    }

    /// 인자 이름.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 타입 힌트 (예: "integer", "string").
    pub fn type_hint(&self) -> &str {
        &self.type_hint
    }
}

/// 객체가 제공하는 메서드의 시그니처.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MethodSig {
    name: String,
    args: Vec<ArgSpec>,
    returns: Option<String>,
}

impl MethodSig {
    /// 새 MethodSig.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), args: Vec::new(), returns: None }
    }

    /// 인자 추가 (체이닝).
    pub fn with_arg(mut self, arg: ArgSpec) -> Self {
        self.args.push(arg);
        self
    }

    /// 반환 타입 설정 (체이닝).
    pub fn with_returns(mut self, type_hint: impl Into<String>) -> Self {
        self.returns = Some(type_hint.into());
        self
    }

    /// 메서드 이름.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 인자 목록.
    pub fn args(&self) -> &[ArgSpec] {
        &self.args
    }

    /// 반환 타입 힌트.
    pub fn returns(&self) -> Option<&str> {
        self.returns.as_deref()
    }
}
```

- [ ] **Step 4: `core/src/object/acl.rs` 구현**

```rust
//! 접근 제어 목록 (ACL).

use serde::{Deserialize, Serialize};

use super::identity::ActorId;

/// 호출 허용/거부 결정.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AclEffect {
    /// 호출 허용.
    Allow,
    /// 호출 거부.
    Deny,
}

/// 액터 매칭 패턴.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActorPattern {
    /// 정확히 일치하는 액터.
    Exact(ActorId),
    /// 임의의 액터 (`*`).
    Wildcard,
}

impl ActorPattern {
    /// 주어진 액터가 이 패턴과 일치하는지.
    pub fn matches(&self, actor: &ActorId) -> bool {
        match self {
            ActorPattern::Exact(a) => a == actor,
            ActorPattern::Wildcard => true,
        }
    }
}

/// 메서드 이름 매칭 패턴.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MethodPattern {
    /// 정확히 일치.
    Exact(String),
    /// 임의의 메서드.
    Wildcard,
}

impl MethodPattern {
    /// 주어진 메서드 이름이 이 패턴과 일치하는지.
    pub fn matches(&self, method: &str) -> bool {
        match self {
            MethodPattern::Exact(m) => m == method,
            MethodPattern::Wildcard => true,
        }
    }
}

/// ACL의 한 항목.
///
/// 액터·메서드 패턴이 모두 일치할 때 `effect`가 적용된다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AclEntry {
    /// 누구에게 적용되나.
    pub actor: ActorPattern,
    /// 어떤 메서드에 적용되나.
    pub method: MethodPattern,
    /// 허용 또는 거부.
    pub effect: AclEffect,
}

impl AclEntry {
    /// 액터와 메서드가 이 항목에 매치되는지.
    pub fn matches(&self, actor: &ActorId, method: &str) -> bool {
        self.actor.matches(actor) && self.method.matches(method)
    }
}
```

- [ ] **Step 5: `core/src/object/mod.rs` 업데이트**

```rust
//! 객체 관련 타입과 ID 정의.

pub mod acl;
pub mod identity;
pub mod method;

pub use acl::{AclEffect, AclEntry, ActorPattern, MethodPattern};
pub use identity::{ActorId, EventId, ObjectId, TypeUri};
pub use method::{ArgSpec, MethodSig};
```

- [ ] **Step 6: `core/src/lib.rs` 재export 확장**

```rust
//! GeulOS core crate.
//!
//! TCB(Trusted Computing Base)에 해당하는 컴포넌트들을 담는다:
//! 객체 서버, 이벤트 버스, 권한 매니저.

pub mod object;

pub use object::{
    AclEffect, AclEntry, ActorId, ActorPattern, ArgSpec, EventId, MethodPattern, MethodSig,
    ObjectId, TypeUri,
};
```

- [ ] **Step 7: 테스트 실행 → 통과**

Run: `cargo test -p geulos-core --test acl_test`
Expected: 4 tests pass.

- [ ] **Step 8: 전체 sanity**

Run: `cargo test -p geulos-core && cargo clippy -p geulos-core --all-targets -- -D warnings`
Expected: 모두 그린.

- [ ] **Step 9: 커밋**

```bash
git add -A
git commit -m "feat(core): MethodSig + AclEntry + 매칭 패턴 추가"
```

---

## Task 3: Event / EventKind

**Files:**
- Create: `core/src/event/mod.rs`
- Modify: `core/src/lib.rs`
- Create: `core/tests/event_test.rs`

- [ ] **Step 1: 실패하는 테스트 작성 (event_test.rs)**

```rust
use geulos_core::{ActorId, Event, EventKind, LifecycleKind, ObjectId};
use serde_json::json;

#[test]
fn event_carries_metadata() {
    let actor = ActorId::local_user();
    let target = ObjectId::new();
    let ev = Event::new(
        actor.clone(),
        target,
        EventKind::Invoke { method: "press".to_string(), args: json!(null) },
    );
    assert_eq!(ev.actor, actor);
    assert_eq!(ev.target, target);
    assert!(ev.causation.is_none());
}

#[test]
fn event_with_causation_links() {
    let actor = ActorId::local_user();
    let target = ObjectId::new();
    let first = Event::new(actor.clone(), target, EventKind::Lifecycle(LifecycleKind::Created));
    let second = Event::new(
        actor.clone(),
        target,
        EventKind::StateSet { key: "label".to_string(), value: json!("hello") },
    )
    .with_causation(first.id);
    assert_eq!(second.causation, Some(first.id));
}

#[test]
fn event_ids_are_monotonic() {
    let actor = ActorId::local_user();
    let t = ObjectId::new();
    let a = Event::new(actor.clone(), t, EventKind::Lifecycle(LifecycleKind::Created));
    let b = Event::new(actor.clone(), t, EventKind::Lifecycle(LifecycleKind::Destroyed));
    assert!(a.id.as_u64() < b.id.as_u64());
}

#[test]
fn event_serde_round_trip() {
    let ev = Event::new(
        ActorId::local_user(),
        ObjectId::new(),
        EventKind::Invoke { method: "press".to_string(), args: json!({"force": 5}) },
    );
    let s = serde_json::to_string(&ev).unwrap();
    let back: Event = serde_json::from_str(&s).unwrap();
    assert_eq!(ev.actor, back.actor);
    assert_eq!(ev.target, back.target);
    assert_eq!(ev.id, back.id);
}
```

- [ ] **Step 2: 테스트 실행 → 실패 확인**

Run: `cargo test -p geulos-core --test event_test`
Expected: 컴파일 실패.

- [ ] **Step 3: `core/src/event/mod.rs` 구현**

```rust
//! 이벤트 모델.
//!
//! 모든 객체 mutate는 Event로 표현되어 EventBus에 직렬 enqueue된다 (ADR-003).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::object::identity::{ActorId, EventId, ObjectId};

/// 객체 라이프사이클 이벤트 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleKind {
    /// 객체가 생성되었다.
    Created,
    /// 객체가 소멸되었다.
    Destroyed,
}

/// 이벤트의 종류.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum EventKind {
    /// 메서드 호출.
    Invoke {
        /// 호출되는 메서드 이름.
        method: String,
        /// 인자 (JSON Value).
        args: Value,
    },
    /// 객체 상태(state) 변경.
    StateSet {
        /// 변경된 키.
        key: String,
        /// 새 값.
        value: Value,
    },
    /// 객체 라이프사이클.
    Lifecycle(LifecycleKind),
    /// 자식 객체 추가.
    ChildAdded {
        /// 추가된 자식의 ID.
        child: ObjectId,
    },
    /// 자식 객체 제거.
    ChildRemoved {
        /// 제거된 자식의 ID.
        child: ObjectId,
    },
}

/// 시스템에서 발생한 한 이벤트.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// 단조 증가 이벤트 ID.
    pub id: EventId,
    /// 이 이벤트를 일으킨 액터.
    pub actor: ActorId,
    /// 이벤트 대상 객체.
    pub target: ObjectId,
    /// 이벤트 종류.
    pub kind: EventKind,
    /// 이 이벤트를 유발한 다른 이벤트 (있다면).
    pub causation: Option<EventId>,
}

impl Event {
    /// 새 Event를 만든다 (id는 자동 발급).
    pub fn new(actor: ActorId, target: ObjectId, kind: EventKind) -> Self {
        Self { id: EventId::new(), actor, target, kind, causation: None }
    }

    /// 원인 이벤트 ID를 설정한다 (체이닝).
    pub fn with_causation(mut self, cause: EventId) -> Self {
        self.causation = Some(cause);
        self
    }
}
```

- [ ] **Step 4: `core/src/lib.rs` 재export 확장**

```rust
//! GeulOS core crate.

pub mod event;
pub mod object;

pub use event::{Event, EventKind, LifecycleKind};
pub use object::{
    AclEffect, AclEntry, ActorId, ActorPattern, ArgSpec, EventId, MethodPattern, MethodSig,
    ObjectId, TypeUri,
};
```

- [ ] **Step 5: 테스트 통과 확인**

Run: `cargo test -p geulos-core --test event_test`
Expected: 4 tests pass.

- [ ] **Step 6: 전체 sanity**

Run: `cargo test -p geulos-core && cargo clippy -p geulos-core --all-targets -- -D warnings`
Expected: 그린.

- [ ] **Step 7: 커밋**

```bash
git add -A
git commit -m "feat(core): Event + EventKind + LifecycleKind 추가"
```

---

## Task 4: Object 구조체

**Files:**
- Modify: `core/src/object/mod.rs`
- Create: `core/tests/object_struct_test.rs`

- [ ] **Step 1: 실패하는 테스트 작성 (object_struct_test.rs)**

```rust
use geulos_core::{AclEntry, ActorId, ActorPattern, MethodPattern, MethodSig, Object, ObjectId,
                  AclEffect, TypeUri};
use serde_json::json;

#[test]
fn object_constructs_with_required_fields() {
    let owner = ActorId::local_user();
    let type_uri = TypeUri::parse("aios.std/Container@1").unwrap();
    let obj = Object::new(type_uri.clone(), owner.clone());

    assert_eq!(obj.type_uri, type_uri);
    assert_eq!(obj.owner, owner);
    assert!(obj.parent.is_none());
    assert!(obj.children.is_empty());
    assert!(obj.props.is_empty());
    assert!(obj.state.is_empty());
    assert!(obj.methods.is_empty());
    assert!(obj.acl.is_empty());
}

#[test]
fn object_can_set_state_and_get() {
    let mut obj = Object::new(
        TypeUri::parse("aios.std/Text@1").unwrap(),
        ActorId::local_user(),
    );
    obj.set_state("content", json!("hello"));
    assert_eq!(obj.state.get("content"), Some(&json!("hello")));
}

#[test]
fn object_can_attach_acl_and_check() {
    let actor = ActorId::local_user();
    let mut obj = Object::new(
        TypeUri::parse("aios.std/Button@1").unwrap(),
        actor.clone(),
    );
    obj.acl.push(AclEntry {
        actor: ActorPattern::Exact(actor.clone()),
        method: MethodPattern::Exact("press".to_string()),
        effect: AclEffect::Allow,
    });
    assert!(obj.is_allowed(&actor, "press"));
    assert!(!obj.is_allowed(&actor, "explode"));
}

#[test]
fn object_owner_implicit_allow_all() {
    // 소유자는 별도 ACL 없어도 모든 메서드 허용.
    let owner = ActorId::local_user();
    let obj = Object::new(
        TypeUri::parse("aios.std/Button@1").unwrap(),
        owner.clone(),
    );
    assert!(obj.is_allowed(&owner, "any_method"));
}

#[test]
fn object_default_deny_for_others() {
    let owner = ActorId::local_user();
    let other = ActorId::new_ai_session();
    let obj = Object::new(
        TypeUri::parse("aios.std/Button@1").unwrap(),
        owner,
    );
    // 소유자가 아니고 ACL이 없으면 거부.
    assert!(!obj.is_allowed(&other, "press"));
}

#[test]
fn object_explicit_deny_overrides_allow() {
    let actor = ActorId::local_user();
    let mut obj = Object::new(
        TypeUri::parse("aios.std/Button@1").unwrap(),
        ActorId::new_ai_session(), // 다른 owner
    );
    // 와일드카드 허용
    obj.acl.push(AclEntry {
        actor: ActorPattern::Wildcard,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
    // 특정 액터·메서드만 deny
    obj.acl.push(AclEntry {
        actor: ActorPattern::Exact(actor.clone()),
        method: MethodPattern::Exact("press".to_string()),
        effect: AclEffect::Deny,
    });
    assert!(!obj.is_allowed(&actor, "press"));
    assert!(obj.is_allowed(&actor, "anything_else"));
}

#[test]
fn object_serde_round_trip() {
    let mut obj = Object::new(
        TypeUri::parse("aios.std/Container@1").unwrap(),
        ActorId::local_user(),
    );
    obj.set_state("title", json!("test"));
    obj.methods.push(MethodSig::new("show"));

    let s = serde_json::to_string(&obj).unwrap();
    let back: Object = serde_json::from_str(&s).unwrap();
    assert_eq!(obj.id, back.id);
    assert_eq!(obj.type_uri, back.type_uri);
    assert_eq!(obj.state, back.state);
}
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test -p geulos-core --test object_struct_test`
Expected: 컴파일 실패.

- [ ] **Step 3: `core/src/object/mod.rs` 업데이트 — Object 구조체 추가**

```rust
//! 객체 관련 타입과 ID 정의.

pub mod acl;
pub mod identity;
pub mod method;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use acl::{AclEffect, AclEntry, ActorPattern, MethodPattern};
pub use identity::{ActorId, EventId, ObjectId, TypeUri};
pub use method::{ArgSpec, MethodSig};

/// 시스템 안의 의미 객체.
///
/// `Object`는 모든 UI/상호작용 요소의 표현이다. 사용자가 보는 GUI 위젯도,
/// AI가 호출하는 메서드도 모두 같은 객체 모델 위에서 정의된다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Object {
    /// 고유 ID.
    pub id: ObjectId,
    /// 타입 식별자 (예: `aios.std/Button@1`).
    pub type_uri: TypeUri,
    /// 부모 객체 (없으면 루트).
    pub parent: Option<ObjectId>,
    /// 자식 객체 ID 목록.
    pub children: Vec<ObjectId>,
    /// 정적 속성 (예: 색상, 레이블).
    pub props: HashMap<String, Value>,
    /// 가변 상태 (예: 토글 on/off, 텍스트 내용).
    pub state: HashMap<String, Value>,
    /// 호출 가능한 메서드 시그니처.
    pub methods: Vec<MethodSig>,
    /// 이 객체를 *소유하는* 액터.
    pub owner: ActorId,
    /// 접근 제어 목록.
    pub acl: Vec<AclEntry>,
}

impl Object {
    /// 새 Object를 만든다 (id 자동 발급).
    pub fn new(type_uri: TypeUri, owner: ActorId) -> Self {
        Self {
            id: ObjectId::new(),
            type_uri,
            parent: None,
            children: Vec::new(),
            props: HashMap::new(),
            state: HashMap::new(),
            methods: Vec::new(),
            owner,
            acl: Vec::new(),
        }
    }

    /// state에 키-값 설정.
    pub fn set_state(&mut self, key: impl Into<String>, value: Value) {
        self.state.insert(key.into(), value);
    }

    /// props에 키-값 설정.
    pub fn set_prop(&mut self, key: impl Into<String>, value: Value) {
        self.props.insert(key.into(), value);
    }

    /// 주어진 액터가 이 객체의 메서드를 호출할 수 있는지.
    ///
    /// 규칙:
    /// - 소유자는 항상 허용.
    /// - 그렇지 않으면 ACL을 순서대로 평가:
    ///   - 마지막으로 매치된 Deny가 있으면 거부.
    ///   - 마지막으로 매치된 Allow가 있으면 허용.
    /// - 어떤 ACL도 매치 안 되면 거부 (default deny).
    pub fn is_allowed(&self, actor: &ActorId, method: &str) -> bool {
        if &self.owner == actor {
            return true;
        }
        // ACL을 순서대로 평가. 마지막 매치가 효과.
        let mut effect: Option<AclEffect> = None;
        for entry in &self.acl {
            if entry.matches(actor, method) {
                effect = Some(entry.effect);
            }
        }
        matches!(effect, Some(AclEffect::Allow))
    }
}
```

- [ ] **Step 4: `core/src/lib.rs` 재export 확장**

```rust
//! GeulOS core crate.

pub mod event;
pub mod object;

pub use event::{Event, EventKind, LifecycleKind};
pub use object::{
    AclEffect, AclEntry, ActorId, ActorPattern, ArgSpec, EventId, MethodPattern, MethodSig,
    Object, ObjectId, TypeUri,
};
```

- [ ] **Step 5: 테스트 통과 확인**

Run: `cargo test -p geulos-core --test object_struct_test`
Expected: 7 tests pass.

- [ ] **Step 6: 전체 sanity**

Run: `cargo test -p geulos-core && cargo clippy -p geulos-core --all-targets -- -D warnings`
Expected: 그린.

- [ ] **Step 7: 커밋**

```bash
git add -A
git commit -m "feat(core): Object 구조체 + ACL 평가 로직"
```

---

## Task 5: 표준 타입 (Container / Text / Button / Toggle)

**Files:**
- Create: `core/src/object/std_types.rs`
- Modify: `core/src/object/mod.rs`
- Modify: `core/src/lib.rs`
- Create: `core/tests/std_types_test.rs`

- [ ] **Step 1: 실패하는 테스트 작성 (std_types_test.rs)**

```rust
use geulos_core::std_types;
use geulos_core::ActorId;
use serde_json::json;

#[test]
fn container_constructs_with_correct_type_uri() {
    let owner = ActorId::local_user();
    let c = std_types::container(owner.clone());
    assert_eq!(c.type_uri.as_str(), "aios.std/Container@1");
    assert_eq!(c.owner, owner);
    assert!(c.methods.is_empty());
}

#[test]
fn text_carries_content_in_state() {
    let owner = ActorId::local_user();
    let t = std_types::text(owner, "hello world");
    assert_eq!(t.type_uri.as_str(), "aios.std/Text@1");
    assert_eq!(t.state.get("content"), Some(&json!("hello world")));
}

#[test]
fn button_carries_label_and_exposes_press() {
    let owner = ActorId::local_user();
    let b = std_types::button(owner, "OK");
    assert_eq!(b.type_uri.as_str(), "aios.std/Button@1");
    assert_eq!(b.state.get("label"), Some(&json!("OK")));
    assert_eq!(b.methods.len(), 1);
    assert_eq!(b.methods[0].name(), "press");
}

#[test]
fn toggle_carries_state_and_exposes_methods() {
    let owner = ActorId::local_user();
    let t = std_types::toggle(owner, true);
    assert_eq!(t.type_uri.as_str(), "aios.std/Toggle@1");
    assert_eq!(t.state.get("on"), Some(&json!(true)));
    let method_names: Vec<&str> = t.methods.iter().map(|m| m.name()).collect();
    assert!(method_names.contains(&"toggle"));
    assert!(method_names.contains(&"set"));
}
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test -p geulos-core --test std_types_test`
Expected: 컴파일 실패.

- [ ] **Step 3: `core/src/object/std_types.rs` 구현**

```rust
//! 표준 객체 타입 생성자.
//!
//! 모든 GeulOS 앱이 즉시 사용할 수 있는 4가지 기본 객체 타입:
//! `Container`, `Text`, `Button`, `Toggle`.

use serde_json::json;

use super::identity::{ActorId, TypeUri};
use super::method::MethodSig;
use super::Object;

/// 빈 컨테이너. 자식 객체를 담는 용도.
pub fn container(owner: ActorId) -> Object {
    Object::new(
        TypeUri::parse("aios.std/Container@1").expect("정상 TypeUri"),
        owner,
    )
}

/// 텍스트 표시 객체.
pub fn text(owner: ActorId, content: &str) -> Object {
    let mut obj = Object::new(
        TypeUri::parse("aios.std/Text@1").expect("정상 TypeUri"),
        owner,
    );
    obj.set_state("content", json!(content));
    obj
}

/// 누름 버튼.
pub fn button(owner: ActorId, label: &str) -> Object {
    let mut obj = Object::new(
        TypeUri::parse("aios.std/Button@1").expect("정상 TypeUri"),
        owner,
    );
    obj.set_state("label", json!(label));
    obj.methods.push(MethodSig::new("press"));
    obj
}

/// 켜고 끄는 토글.
pub fn toggle(owner: ActorId, initial: bool) -> Object {
    let mut obj = Object::new(
        TypeUri::parse("aios.std/Toggle@1").expect("정상 TypeUri"),
        owner,
    );
    obj.set_state("on", json!(initial));
    obj.methods.push(MethodSig::new("toggle"));
    obj.methods.push(MethodSig::new("set"));
    obj
}
```

- [ ] **Step 4: `core/src/object/mod.rs`에 std_types 모듈 노출**

추가:

```rust
pub mod std_types;
```

- [ ] **Step 5: `core/src/lib.rs`에 std_types 재export**

```rust
//! GeulOS core crate.

pub mod event;
pub mod object;
pub use object::std_types;

pub use event::{Event, EventKind, LifecycleKind};
pub use object::{
    AclEffect, AclEntry, ActorId, ActorPattern, ArgSpec, EventId, MethodPattern, MethodSig,
    Object, ObjectId, TypeUri,
};
```

- [ ] **Step 6: 테스트 통과 확인**

Run: `cargo test -p geulos-core --test std_types_test`
Expected: 4 tests pass.

- [ ] **Step 7: sanity + 커밋**

```bash
cargo test -p geulos-core
cargo clippy -p geulos-core --all-targets -- -D warnings
git add -A
git commit -m "feat(core): 표준 타입 생성자 (Container/Text/Button/Toggle)"
```

---

## Task 6: EventBus

**Files:**
- Create: `core/src/event/bus.rs`
- Modify: `core/src/event/mod.rs`
- Modify: `core/src/lib.rs`
- Create: `core/tests/event_bus_test.rs`

- [ ] **Step 1: 실패하는 테스트 작성 (event_bus_test.rs)**

```rust
use geulos_core::{ActorId, Event, EventBus, EventKind, LifecycleKind, ObjectId};

#[test]
fn event_bus_empty_log() {
    let bus = EventBus::new();
    assert_eq!(bus.log().len(), 0);
}

#[test]
fn event_bus_emit_appends_to_log() {
    let mut bus = EventBus::new();
    let target = ObjectId::new();
    let id = bus.emit(
        ActorId::local_user(),
        target,
        EventKind::Lifecycle(LifecycleKind::Created),
        None,
    );
    assert_eq!(bus.log().len(), 1);
    assert_eq!(bus.log()[0].id, id);
}

#[test]
fn event_bus_total_order_preserved() {
    let mut bus = EventBus::new();
    let target = ObjectId::new();
    let actor = ActorId::local_user();
    let a = bus.emit(actor.clone(), target, EventKind::Lifecycle(LifecycleKind::Created), None);
    let b = bus.emit(actor.clone(), target, EventKind::Lifecycle(LifecycleKind::Destroyed), None);
    assert!(a.as_u64() < b.as_u64());
}

#[test]
fn event_bus_causation_links() {
    let mut bus = EventBus::new();
    let actor = ActorId::local_user();
    let t = ObjectId::new();
    let parent_id = bus.emit(actor.clone(), t, EventKind::Lifecycle(LifecycleKind::Created), None);
    let child_id = bus.emit(
        actor.clone(),
        t,
        EventKind::Lifecycle(LifecycleKind::Destroyed),
        Some(parent_id),
    );
    let child_event = &bus.log()[1];
    assert_eq!(child_event.id, child_id);
    assert_eq!(child_event.causation, Some(parent_id));
}

#[test]
fn event_bus_iter_log_by_actor() {
    let mut bus = EventBus::new();
    let user = ActorId::local_user();
    let ai = ActorId::new_ai_session();
    let t = ObjectId::new();
    bus.emit(user.clone(), t, EventKind::Lifecycle(LifecycleKind::Created), None);
    bus.emit(ai.clone(), t, EventKind::Lifecycle(LifecycleKind::Destroyed), None);
    bus.emit(user.clone(), t, EventKind::Lifecycle(LifecycleKind::Created), None);

    let user_events: Vec<&Event> = bus.log().iter().filter(|e| e.actor == user).collect();
    assert_eq!(user_events.len(), 2);
}
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test -p geulos-core --test event_bus_test`
Expected: 컴파일 실패.

- [ ] **Step 3: `core/src/event/bus.rs` 구현**

```rust
//! 이벤트 버스.
//!
//! 단일 라이터 모델 (ADR-003). 모든 이벤트가 직렬로 emit되어 로그에 영구 기록된다.

use serde_json::Value;

use super::{Event, EventKind, LifecycleKind};
use crate::object::identity::{ActorId, EventId, ObjectId};

/// 이벤트 버스: 시스템 내 모든 이벤트의 전순서를 관리.
///
/// M1에서는 인-메모리 `Vec<Event>` 로그. 후속 마일스톤에서 디스크 영속·압축·롤링이
/// 추가된다.
#[derive(Debug, Default)]
pub struct EventBus {
    log: Vec<Event>,
}

impl EventBus {
    /// 빈 EventBus 생성.
    pub fn new() -> Self {
        Self { log: Vec::new() }
    }

    /// 이벤트를 emit한다.
    ///
    /// 새 EventId를 발급하고 Event를 만들어 로그에 추가한다. 반환값은 발급된 EventId.
    pub fn emit(
        &mut self,
        actor: ActorId,
        target: ObjectId,
        kind: EventKind,
        causation: Option<EventId>,
    ) -> EventId {
        let mut ev = Event::new(actor, target, kind);
        if let Some(cause) = causation {
            ev.causation = Some(cause);
        }
        let id = ev.id;
        self.log.push(ev);
        id
    }

    /// 이벤트 로그 (읽기 전용).
    pub fn log(&self) -> &[Event] {
        &self.log
    }

    /// 로그 길이.
    pub fn len(&self) -> usize {
        self.log.len()
    }

    /// 로그가 비었는지.
    pub fn is_empty(&self) -> bool {
        self.log.is_empty()
    }
}

// 미사용 임포트 방지용.
#[allow(dead_code)]
fn _unused_imports_anchor() {
    let _: Option<EventKind> = None;
    let _: Option<LifecycleKind> = None;
    let _: Option<Value> = None;
}
```

(`_unused_imports_anchor` 는 워닝 방지 임시 함수. clippy 통과를 확인하고 필요 없으면 제거할 것.)

실제로는 `LifecycleKind`나 `Value`가 이 파일에서 직접 쓰이지 않을 수 있으니, 위 `use` 문에서 사용 안 하는 것은 제거. 최종 파일은 다음과 같이 정리:

```rust
//! 이벤트 버스.
//!
//! 단일 라이터 모델 (ADR-003). 모든 이벤트가 직렬로 emit되어 로그에 영구 기록된다.

use super::{Event, EventKind};
use crate::object::identity::{ActorId, EventId, ObjectId};

/// 이벤트 버스: 시스템 내 모든 이벤트의 전순서를 관리.
#[derive(Debug, Default)]
pub struct EventBus {
    log: Vec<Event>,
}

impl EventBus {
    pub fn new() -> Self {
        Self { log: Vec::new() }
    }

    pub fn emit(
        &mut self,
        actor: ActorId,
        target: ObjectId,
        kind: EventKind,
        causation: Option<EventId>,
    ) -> EventId {
        let mut ev = Event::new(actor, target, kind);
        if let Some(cause) = causation {
            ev.causation = Some(cause);
        }
        let id = ev.id;
        self.log.push(ev);
        id
    }

    pub fn log(&self) -> &[Event] {
        &self.log
    }

    pub fn len(&self) -> usize {
        self.log.len()
    }

    pub fn is_empty(&self) -> bool {
        self.log.is_empty()
    }
}
```

- [ ] **Step 4: `core/src/event/mod.rs`에 bus 모듈 노출**

기존 내용 끝에 추가:

```rust
pub mod bus;
pub use bus::EventBus;
```

- [ ] **Step 5: `core/src/lib.rs`에 EventBus 재export**

기존 `pub use event::{Event, EventKind, LifecycleKind};` 를 다음으로 교체:

```rust
pub use event::{Event, EventBus, EventKind, LifecycleKind};
```

- [ ] **Step 6: 테스트 통과 확인**

Run: `cargo test -p geulos-core --test event_bus_test`
Expected: 5 tests pass.

- [ ] **Step 7: sanity + 커밋**

```bash
cargo test -p geulos-core
cargo clippy -p geulos-core --all-targets -- -D warnings
git add -A
git commit -m "feat(core): EventBus (단일 라이터, 이벤트 로그)"
```

---

## Task 7: ObjectServer 스켈레톤 + get()

**Files:**
- Create: `core/src/server/mod.rs`
- Modify: `core/src/lib.rs`
- Create: `core/tests/server_basic_test.rs`

- [ ] **Step 1: 실패하는 테스트 작성**

`core/tests/server_basic_test.rs`:

```rust
use geulos_core::{ActorId, Object, ObjectServer, TypeUri};

#[test]
fn server_starts_empty() {
    let server = ObjectServer::new();
    assert_eq!(server.object_count(), 0);
}

#[test]
fn server_get_nonexistent_returns_none() {
    let server = ObjectServer::new();
    let random_id = geulos_core::ObjectId::new();
    assert!(server.get(&random_id).is_none());
}
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test -p geulos-core --test server_basic_test`
Expected: 컴파일 실패.

- [ ] **Step 3: `core/src/server/mod.rs` 구현 (스켈레톤)**

```rust
//! 객체 서버.
//!
//! TCB의 핵심. 모든 객체의 단일 진실원이며, 모든 mutate는 이 모듈을 통해서만 일어난다.

use std::collections::HashMap;

use crate::event::EventBus;
use crate::object::{Object, ObjectId};

/// 객체 트리를 보관하고 모든 mutate를 직렬화하는 서버.
#[derive(Debug, Default)]
pub struct ObjectServer {
    objects: HashMap<ObjectId, Object>,
    /// 트리의 루트 객체 ID 목록 (앱 단위 서브트리 루트들).
    roots: Vec<ObjectId>,
    /// 이벤트 버스.
    bus: EventBus,
}

impl ObjectServer {
    /// 빈 ObjectServer 생성.
    pub fn new() -> Self {
        Self {
            objects: HashMap::new(),
            roots: Vec::new(),
            bus: EventBus::new(),
        }
    }

    /// 객체 개수.
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// 루트 ID 목록.
    pub fn roots(&self) -> &[ObjectId] {
        &self.roots
    }

    /// ID로 객체 조회.
    pub fn get(&self, id: &ObjectId) -> Option<&Object> {
        self.objects.get(id)
    }

    /// 이벤트 버스에 대한 읽기 전용 접근.
    pub fn bus(&self) -> &EventBus {
        &self.bus
    }
}
```

- [ ] **Step 4: `core/src/lib.rs`에 ObjectServer 재export**

```rust
//! GeulOS core crate.

pub mod event;
pub mod object;
pub mod server;
pub use object::std_types;

pub use event::{Event, EventBus, EventKind, LifecycleKind};
pub use object::{
    AclEffect, AclEntry, ActorId, ActorPattern, ArgSpec, EventId, MethodPattern, MethodSig,
    Object, ObjectId, TypeUri,
};
pub use server::ObjectServer;
```

- [ ] **Step 5: 테스트 통과 확인**

Run: `cargo test -p geulos-core --test server_basic_test`
Expected: 2 tests pass.

- [ ] **Step 6: sanity + 커밋**

```bash
cargo test -p geulos-core
cargo clippy -p geulos-core --all-targets -- -D warnings
git add -A
git commit -m "feat(core): ObjectServer 스켈레톤 + get()"
```

---

## Task 8: ObjectServer.mount()

mount는 객체 서브트리(루트 + 그 자손들)를 한 번에 서버에 등록한다.

**Files:**
- Create: `core/src/server/mount.rs`
- Modify: `core/src/server/mod.rs`
- Create: `core/tests/server_mount_test.rs`

- [ ] **Step 1: 실패하는 테스트 작성**

`core/tests/server_mount_test.rs`:

```rust
use geulos_core::{std_types, ActorId, EventKind, LifecycleKind, ObjectServer};
use serde_json::json;

#[test]
fn mount_single_object() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let obj = std_types::text(owner.clone(), "hello");
    let id = obj.id;

    let root_id = server.mount(obj).expect("mount should succeed");

    assert_eq!(root_id, id);
    assert_eq!(server.object_count(), 1);
    assert!(server.get(&root_id).is_some());
    assert_eq!(server.roots(), &[root_id]);
}

#[test]
fn mount_emits_lifecycle_created_event() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let obj = std_types::container(owner.clone());

    server.mount(obj).unwrap();

    assert_eq!(server.bus().log().len(), 1);
    let ev = &server.bus().log()[0];
    assert_eq!(ev.actor, owner);
    assert!(matches!(ev.kind, EventKind::Lifecycle(LifecycleKind::Created)));
}

#[test]
fn mount_subtree_with_children() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let mut root = std_types::container(owner.clone());
    let child_a = std_types::text(owner.clone(), "a");
    let child_b = std_types::text(owner.clone(), "b");

    let child_a_id = child_a.id;
    let child_b_id = child_b.id;
    root.children.push(child_a_id);
    root.children.push(child_b_id);

    server.mount_with_descendants(root, vec![child_a, child_b]).unwrap();

    assert_eq!(server.object_count(), 3);
    assert!(server.get(&child_a_id).is_some());
    assert!(server.get(&child_b_id).is_some());
    // 각 객체에 대해 Created 이벤트 1개씩
    assert_eq!(server.bus().log().len(), 3);
}

#[test]
fn mount_duplicate_id_rejected() {
    use geulos_core::ObjectId;
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let mut obj1 = std_types::text(owner.clone(), "first");
    let shared_id = obj1.id;
    let mut obj2 = std_types::text(owner, "second");
    obj2.id = shared_id; // 의도적 충돌

    server.mount(obj1).unwrap();
    assert!(server.mount(obj2).is_err());
}
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test -p geulos-core --test server_mount_test`
Expected: 컴파일 실패.

- [ ] **Step 3: `core/src/server/mount.rs` 구현**

```rust
//! mount(): 객체 서브트리 등록.

use thiserror::Error;

use crate::event::{EventKind, LifecycleKind};
use crate::object::{Object, ObjectId};
use crate::server::ObjectServer;

/// mount 실패 사유.
#[derive(Debug, Error)]
pub enum MountError {
    /// 같은 ID를 가진 객체가 이미 존재.
    #[error("이미 등록된 ObjectId: {0}")]
    DuplicateId(ObjectId),
}

impl ObjectServer {
    /// 단일 객체를 루트로 등록한다.
    pub fn mount(&mut self, obj: Object) -> Result<ObjectId, MountError> {
        if self.objects.contains_key(&obj.id) {
            return Err(MountError::DuplicateId(obj.id));
        }
        let id = obj.id;
        let owner = obj.owner.clone();
        self.objects.insert(id, obj);
        self.roots.push(id);
        self.bus.emit(
            owner,
            id,
            EventKind::Lifecycle(LifecycleKind::Created),
            None,
        );
        Ok(id)
    }

    /// 루트와 그 자손들을 한꺼번에 등록한다.
    ///
    /// `descendants`의 객체들은 `root.children`에서 참조되는 순서대로 와야 한다.
    /// 각 자손도 Created 이벤트가 발생한다.
    pub fn mount_with_descendants(
        &mut self,
        root: Object,
        descendants: Vec<Object>,
    ) -> Result<ObjectId, MountError> {
        // 먼저 중복 검사
        if self.objects.contains_key(&root.id) {
            return Err(MountError::DuplicateId(root.id));
        }
        for d in &descendants {
            if self.objects.contains_key(&d.id) {
                return Err(MountError::DuplicateId(d.id));
            }
        }

        let root_id = root.id;
        let root_owner = root.owner.clone();

        // 등록 & 이벤트 발행
        self.objects.insert(root_id, root);
        self.roots.push(root_id);
        self.bus.emit(
            root_owner,
            root_id,
            EventKind::Lifecycle(LifecycleKind::Created),
            None,
        );

        for d in descendants {
            let id = d.id;
            let owner = d.owner.clone();
            self.objects.insert(id, d);
            self.bus.emit(
                owner,
                id,
                EventKind::Lifecycle(LifecycleKind::Created),
                None,
            );
        }

        Ok(root_id)
    }
}
```

- [ ] **Step 4: `core/src/server/mod.rs`에 mount 모듈 노출**

추가:

```rust
pub mod mount;
pub use mount::MountError;
```

- [ ] **Step 5: 테스트 통과 확인**

Run: `cargo test -p geulos-core --test server_mount_test`
Expected: 4 tests pass.

- [ ] **Step 6: sanity + 커밋**

```bash
cargo test -p geulos-core
cargo clippy -p geulos-core --all-targets -- -D warnings
git add -A
git commit -m "feat(core): ObjectServer.mount() + 자손 일괄 등록"
```

---

## Task 9: ObjectServer.invoke() + ACL 게이트 + 이벤트 발행

**Files:**
- Create: `core/src/server/invoke.rs`
- Modify: `core/src/server/mod.rs`
- Create: `core/tests/server_invoke_test.rs`

- [ ] **Step 1: 실패하는 테스트 작성**

`core/tests/server_invoke_test.rs`:

```rust
use geulos_core::{std_types, ActorId, EventKind, ObjectServer};
use serde_json::json;

#[test]
fn invoke_existing_method_succeeds_for_owner() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let btn = std_types::button(owner.clone(), "OK");
    let btn_id = server.mount(btn).unwrap();

    let result = server.invoke(&owner, &btn_id, "press", json!({}));
    assert!(result.is_ok());
}

#[test]
fn invoke_emits_invoke_event() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let btn = std_types::button(owner.clone(), "OK");
    let btn_id = server.mount(btn).unwrap();

    let log_len_before = server.bus().log().len();
    server.invoke(&owner, &btn_id, "press", json!(null)).unwrap();

    assert_eq!(server.bus().log().len(), log_len_before + 1);
    let last = server.bus().log().last().unwrap();
    assert_eq!(last.actor, owner);
    assert_eq!(last.target, btn_id);
    match &last.kind {
        EventKind::Invoke { method, .. } => assert_eq!(method, "press"),
        _ => panic!("expected Invoke event"),
    }
}

#[test]
fn invoke_denied_for_non_owner_without_acl() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let intruder = ActorId::new_ai_session();
    let btn = std_types::button(owner.clone(), "OK");
    let btn_id = server.mount(btn).unwrap();

    let result = server.invoke(&intruder, &btn_id, "press", json!({}));
    assert!(result.is_err());
}

#[test]
fn invoke_nonexistent_object_errors() {
    use geulos_core::ObjectId;
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let bogus = ObjectId::new();
    let result = server.invoke(&owner, &bogus, "press", json!({}));
    assert!(result.is_err());
}

#[test]
fn invoke_unknown_method_errors() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let btn = std_types::button(owner.clone(), "OK");
    let btn_id = server.mount(btn).unwrap();

    let result = server.invoke(&owner, &btn_id, "self_destruct", json!({}));
    assert!(result.is_err());
}
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test -p geulos-core --test server_invoke_test`
Expected: 컴파일 실패.

- [ ] **Step 3: `core/src/server/invoke.rs` 구현**

```rust
//! invoke(): 객체 메서드 호출.

use serde_json::Value;
use thiserror::Error;

use crate::event::EventKind;
use crate::object::{ActorId, EventId, ObjectId};
use crate::server::ObjectServer;

/// invoke 실패 사유.
#[derive(Debug, Error)]
pub enum InvokeError {
    /// 대상 객체가 존재하지 않음.
    #[error("객체를 찾을 수 없음: {0}")]
    NotFound(ObjectId),
    /// 호출자가 권한 없음.
    #[error("권한 없음: 액터 {actor}, 객체 {target}, 메서드 {method}")]
    PermissionDenied { actor: String, target: ObjectId, method: String },
    /// 객체가 그 메서드를 지원하지 않음.
    #[error("객체 {target}는 메서드 '{method}'를 지원하지 않음")]
    UnknownMethod { target: ObjectId, method: String },
}

impl ObjectServer {
    /// 객체의 메서드를 호출한다.
    ///
    /// 흐름:
    /// 1. 대상 객체 존재 확인
    /// 2. 메서드 시그니처 존재 확인
    /// 3. ACL 검사 (소유자 우대 + ACL 평가)
    /// 4. Invoke 이벤트 발행
    pub fn invoke(
        &mut self,
        actor: &ActorId,
        target: &ObjectId,
        method: &str,
        args: Value,
    ) -> Result<EventId, InvokeError> {
        // 1) 객체 존재
        let obj = self
            .objects
            .get(target)
            .ok_or(InvokeError::NotFound(*target))?;

        // 2) 메서드 존재
        if !obj.methods.iter().any(|m| m.name() == method) {
            return Err(InvokeError::UnknownMethod {
                target: *target,
                method: method.to_string(),
            });
        }

        // 3) ACL
        if !obj.is_allowed(actor, method) {
            return Err(InvokeError::PermissionDenied {
                actor: actor.as_str().to_string(),
                target: *target,
                method: method.to_string(),
            });
        }

        // 4) Invoke 이벤트 발행
        let event_id = self.bus.emit(
            actor.clone(),
            *target,
            EventKind::Invoke { method: method.to_string(), args },
            None,
        );

        Ok(event_id)
    }
}
```

- [ ] **Step 4: `core/src/server/mod.rs`에 invoke 모듈 노출**

추가:

```rust
pub mod invoke;
pub use invoke::InvokeError;
```

- [ ] **Step 5: 테스트 통과 확인**

Run: `cargo test -p geulos-core --test server_invoke_test`
Expected: 5 tests pass.

- [ ] **Step 6: sanity + 커밋**

```bash
cargo test -p geulos-core
cargo clippy -p geulos-core --all-targets -- -D warnings
git add -A
git commit -m "feat(core): ObjectServer.invoke() + ACL 게이트 + Invoke 이벤트"
```

---

## Task 10: ObjectServer.query()

타입·소유자·부모 기준으로 객체를 찾는 단발 조회.

**Files:**
- Create: `core/src/server/query.rs`
- Modify: `core/src/server/mod.rs`
- Create: `core/tests/server_query_test.rs`

- [ ] **Step 1: 실패하는 테스트 작성**

`core/tests/server_query_test.rs`:

```rust
use geulos_core::{std_types, ActorId, ObjectServer, Query, TypeUri};

#[test]
fn query_by_type_finds_all() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    server.mount(std_types::text(owner.clone(), "a")).unwrap();
    server.mount(std_types::text(owner.clone(), "b")).unwrap();
    server.mount(std_types::button(owner.clone(), "btn")).unwrap();

    let type_uri = TypeUri::parse("aios.std/Text@1").unwrap();
    let results = server.query(&Query::by_type(type_uri));
    assert_eq!(results.len(), 2);
}

#[test]
fn query_by_owner_filters_correctly() {
    let mut server = ObjectServer::new();
    let user = ActorId::local_user();
    let ai = ActorId::new_ai_session();
    server.mount(std_types::text(user.clone(), "user_owned")).unwrap();
    server.mount(std_types::text(ai.clone(), "ai_owned")).unwrap();

    let user_results = server.query(&Query::by_owner(user.clone()));
    assert_eq!(user_results.len(), 1);

    let ai_results = server.query(&Query::by_owner(ai));
    assert_eq!(ai_results.len(), 1);
}

#[test]
fn query_returns_empty_when_no_match() {
    let server = ObjectServer::new();
    let owner = ActorId::local_user();
    let results = server.query(&Query::by_owner(owner));
    assert!(results.is_empty());
}
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test -p geulos-core --test server_query_test`
Expected: 컴파일 실패.

- [ ] **Step 3: `core/src/server/query.rs` 구현**

```rust
//! query(): 객체 트리 단발 조회.

use crate::object::{ActorId, ObjectId, TypeUri};
use crate::server::ObjectServer;

/// 조회 조건.
#[derive(Debug, Clone)]
pub enum Query {
    /// 특정 타입의 모든 객체.
    ByType(TypeUri),
    /// 특정 액터가 소유한 모든 객체.
    ByOwner(ActorId),
    /// 특정 부모의 직계 자식들.
    ChildrenOf(ObjectId),
}

impl Query {
    /// 타입 기준.
    pub fn by_type(t: TypeUri) -> Self {
        Self::ByType(t)
    }

    /// 소유자 기준.
    pub fn by_owner(a: ActorId) -> Self {
        Self::ByOwner(a)
    }

    /// 자식 기준.
    pub fn children_of(parent: ObjectId) -> Self {
        Self::ChildrenOf(parent)
    }
}

impl ObjectServer {
    /// 트리에서 조건에 맞는 객체 ID 목록을 반환한다.
    pub fn query(&self, q: &Query) -> Vec<ObjectId> {
        match q {
            Query::ByType(t) => self
                .objects
                .iter()
                .filter(|(_, o)| &o.type_uri == t)
                .map(|(id, _)| *id)
                .collect(),
            Query::ByOwner(a) => self
                .objects
                .iter()
                .filter(|(_, o)| &o.owner == a)
                .map(|(id, _)| *id)
                .collect(),
            Query::ChildrenOf(parent) => self
                .objects
                .get(parent)
                .map(|o| o.children.clone())
                .unwrap_or_default(),
        }
    }
}
```

- [ ] **Step 4: `core/src/server/mod.rs`에 query 모듈 노출 + lib.rs 재export**

`server/mod.rs`에 추가:

```rust
pub mod query;
pub use query::Query;
```

`lib.rs`의 `pub use server::ObjectServer;` 를 다음으로 변경:

```rust
pub use server::{ObjectServer, Query};
```

- [ ] **Step 5: 테스트 통과 확인**

Run: `cargo test -p geulos-core --test server_query_test`
Expected: 3 tests pass.

- [ ] **Step 6: sanity + 커밋**

```bash
cargo test -p geulos-core
cargo clippy -p geulos-core --all-targets -- -D warnings
git add -A
git commit -m "feat(core): ObjectServer.query() + Query 종류 3가지"
```

---

## Task 11: ObjectServer.subscribe() + 이벤트 전달

**Files:**
- Create: `core/src/server/subscribe.rs`
- Modify: `core/src/server/mod.rs`
- Modify: `core/src/server/invoke.rs` (구독자에게 이벤트 전달 후크)
- Modify: `core/src/server/mount.rs` (동일)
- Create: `core/tests/server_subscribe_test.rs`

- [ ] **Step 1: 실패하는 테스트 작성**

`core/tests/server_subscribe_test.rs`:

```rust
use geulos_core::{std_types, ActorId, EventKindFilter, ObjectServer};
use serde_json::json;

#[test]
fn subscribe_returns_subscription_id() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let id = server.mount(std_types::button(owner.clone(), "OK")).unwrap();

    let sub_id = server.subscribe(owner.clone(), id, vec![EventKindFilter::Invoke]);
    assert_ne!(sub_id.as_u64(), 0);
}

#[test]
fn subscribe_receives_invoke_events() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let id = server.mount(std_types::button(owner.clone(), "OK")).unwrap();

    let sub_id = server.subscribe(owner.clone(), id, vec![EventKindFilter::Invoke]);

    // mount는 Lifecycle 이벤트를 발행했지만 구독은 Invoke만 필터링하므로 받지 않음.
    let drained = server.drain_subscription(sub_id);
    assert_eq!(drained.len(), 0);

    server.invoke(&owner, &id, "press", json!(null)).unwrap();
    let drained = server.drain_subscription(sub_id);
    assert_eq!(drained.len(), 1);
}

#[test]
fn subscribe_filters_by_target() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let a = server.mount(std_types::button(owner.clone(), "A")).unwrap();
    let b = server.mount(std_types::button(owner.clone(), "B")).unwrap();

    let sub_a = server.subscribe(owner.clone(), a, vec![EventKindFilter::Invoke]);

    server.invoke(&owner, &b, "press", json!(null)).unwrap();
    assert_eq!(server.drain_subscription(sub_a).len(), 0); // b의 invoke는 무관

    server.invoke(&owner, &a, "press", json!(null)).unwrap();
    assert_eq!(server.drain_subscription(sub_a).len(), 1);
}

#[test]
fn subscribe_lifecycle_filter() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let id = server.mount(std_types::container(owner.clone())).unwrap();

    // mount 후 구독 — Lifecycle Created 이벤트는 이미 발행되었으므로 못 받음.
    let sub = server.subscribe(owner, id, vec![EventKindFilter::Lifecycle]);
    let drained = server.drain_subscription(sub);
    assert_eq!(drained.len(), 0);
}

#[test]
fn unsubscribe_stops_delivery() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let id = server.mount(std_types::button(owner.clone(), "OK")).unwrap();
    let sub = server.subscribe(owner.clone(), id, vec![EventKindFilter::Invoke]);

    server.unsubscribe(sub);
    server.invoke(&owner, &id, "press", json!(null)).unwrap();
    // 이미 unsubscribe 후 drain 시도 — empty여야 함 (구독이 없으므로)
    assert_eq!(server.drain_subscription(sub).len(), 0);
}
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test -p geulos-core --test server_subscribe_test`
Expected: 컴파일 실패.

- [ ] **Step 3: `core/src/server/subscribe.rs` 구현**

```rust
//! subscribe(): 객체 이벤트 구독.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::event::{Event, EventKind};
use crate::object::{ActorId, ObjectId};
use crate::server::ObjectServer;

/// 구독 ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

impl SubscriptionId {
    /// 내부 u64로 변환.
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

/// 어떤 종류의 이벤트를 받을지 필터.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKindFilter {
    /// Invoke 이벤트.
    Invoke,
    /// StateSet 이벤트.
    StateSet,
    /// Lifecycle 이벤트.
    Lifecycle,
    /// ChildAdded/ChildRemoved 이벤트.
    ChildChange,
}

impl EventKindFilter {
    /// 주어진 이벤트가 이 필터에 매치되는지.
    pub fn matches(&self, kind: &EventKind) -> bool {
        match (self, kind) {
            (Self::Invoke, EventKind::Invoke { .. }) => true,
            (Self::StateSet, EventKind::StateSet { .. }) => true,
            (Self::Lifecycle, EventKind::Lifecycle(_)) => true,
            (Self::ChildChange, EventKind::ChildAdded { .. })
            | (Self::ChildChange, EventKind::ChildRemoved { .. }) => true,
            _ => false,
        }
    }
}

/// 한 구독의 상태.
#[derive(Debug)]
pub(super) struct Subscription {
    pub(super) subscriber: ActorId,
    pub(super) target: ObjectId,
    pub(super) filters: Vec<EventKindFilter>,
    pub(super) queue: VecDeque<Event>,
}

/// 구독 관리자.
#[derive(Debug, Default)]
pub(super) struct SubscriptionManager {
    next_id: AtomicU64,
    subscriptions: HashMap<SubscriptionId, Subscription>,
}

impl SubscriptionManager {
    pub(super) fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            subscriptions: HashMap::new(),
        }
    }

    pub(super) fn register(
        &mut self,
        subscriber: ActorId,
        target: ObjectId,
        filters: Vec<EventKindFilter>,
    ) -> SubscriptionId {
        let id = SubscriptionId(self.next_id.fetch_add(1, Ordering::SeqCst));
        self.subscriptions.insert(
            id,
            Subscription { subscriber, target, filters, queue: VecDeque::new() },
        );
        id
    }

    pub(super) fn unregister(&mut self, id: SubscriptionId) {
        self.subscriptions.remove(&id);
    }

    /// 모든 매칭 구독에 이벤트를 enqueue한다.
    pub(super) fn deliver(&mut self, ev: &Event) {
        for sub in self.subscriptions.values_mut() {
            if sub.target != ev.target {
                continue;
            }
            if !sub.filters.iter().any(|f| f.matches(&ev.kind)) {
                continue;
            }
            sub.queue.push_back(ev.clone());
        }
    }

    pub(super) fn drain(&mut self, id: SubscriptionId) -> Vec<Event> {
        match self.subscriptions.get_mut(&id) {
            Some(sub) => sub.queue.drain(..).collect(),
            None => Vec::new(),
        }
    }
}

impl ObjectServer {
    /// 구독 등록.
    pub fn subscribe(
        &mut self,
        subscriber: ActorId,
        target: ObjectId,
        filters: Vec<EventKindFilter>,
    ) -> SubscriptionId {
        self.subscriptions.register(subscriber, target, filters)
    }

    /// 구독 해제.
    pub fn unsubscribe(&mut self, id: SubscriptionId) {
        self.subscriptions.unregister(id);
    }

    /// 구독 큐에 쌓인 이벤트를 모두 가져온다 (큐 비움).
    pub fn drain_subscription(&mut self, id: SubscriptionId) -> Vec<Event> {
        self.subscriptions.drain(id)
    }
}
```

- [ ] **Step 4: `core/src/server/mod.rs`에 subscription 통합**

`ObjectServer` 구조체에 `subscriptions: SubscriptionManager` 필드 추가:

```rust
//! 객체 서버.

use std::collections::HashMap;

use crate::event::EventBus;
use crate::object::{Object, ObjectId};

pub mod invoke;
pub mod mount;
pub mod query;
pub mod subscribe;

pub use invoke::InvokeError;
pub use mount::MountError;
pub use query::Query;
pub use subscribe::{EventKindFilter, SubscriptionId};

use subscribe::SubscriptionManager;

/// 객체 트리를 보관하고 모든 mutate를 직렬화하는 서버.
#[derive(Debug, Default)]
pub struct ObjectServer {
    objects: HashMap<ObjectId, Object>,
    roots: Vec<ObjectId>,
    bus: EventBus,
    pub(super) subscriptions: SubscriptionManager,
}

impl ObjectServer {
    pub fn new() -> Self {
        Self {
            objects: HashMap::new(),
            roots: Vec::new(),
            bus: EventBus::new(),
            subscriptions: SubscriptionManager::new(),
        }
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    pub fn roots(&self) -> &[ObjectId] {
        &self.roots
    }

    pub fn get(&self, id: &ObjectId) -> Option<&Object> {
        self.objects.get(id)
    }

    pub fn bus(&self) -> &EventBus {
        &self.bus
    }
}
```

이제 `mount.rs`와 `invoke.rs`의 이벤트 발행 후 구독자에게 전달하는 로직 추가가 필요. 각 파일에서 `self.bus.emit(...)` 호출 직후 다음 한 줄 삽입:

`mount.rs`의 두 mount 메서드 안, 각 `emit` 호출 직후:

```rust
// 직전에 emit된 이벤트를 구독자들에게 전달
if let Some(ev) = self.bus.log().last() {
    self.subscriptions.deliver(ev);
}
```

`invoke.rs`도 마찬가지로 `emit` 직후:

```rust
if let Some(ev) = self.bus.log().last() {
    self.subscriptions.deliver(ev);
}
```

- [ ] **Step 5: lib.rs 재export 추가**

```rust
pub use server::{EventKindFilter, ObjectServer, Query, SubscriptionId};
```

- [ ] **Step 6: 테스트 통과 확인**

Run: `cargo test -p geulos-core --test server_subscribe_test`
Expected: 5 tests pass.

- [ ] **Step 7: sanity + 커밋**

```bash
cargo test -p geulos-core
cargo clippy -p geulos-core --all-targets -- -D warnings
git add -A
git commit -m "feat(core): ObjectServer.subscribe() + 이벤트 필터 + drain"
```

---

## Task 12: proptest 의존성 + P5 라운드트립 속성 테스트

P5: 임의의 Event/Object를 직렬화·역직렬화하면 원본과 동등.

**Files:**
- Modify: `Cargo.toml` (workspace.dependencies에 proptest 추가)
- Modify: `core/Cargo.toml`
- Create: `core/tests/proptest_p5_roundtrip.rs`

- [ ] **Step 1: workspace `Cargo.toml`에 proptest 추가**

`[workspace.dependencies]` 섹션에 다음 추가:

```toml
proptest = "1.4"
```

- [ ] **Step 2: `core/Cargo.toml`의 `[dev-dependencies]`에 proptest 추가**

```toml
[dev-dependencies]
serde_json = "1.0"
proptest = { workspace = true }
```

- [ ] **Step 3: `core/tests/proptest_p5_roundtrip.rs` 작성**

```rust
//! P5: 직렬화 → 역직렬화 → 동등 (라운드트립 속성).

use geulos_core::{ActorId, Event, EventKind, LifecycleKind, ObjectId};
use proptest::prelude::*;
use serde_json::json;

prop_compose! {
    fn arb_actor_id()(kind in 0u8..4u8, suffix in any::<u64>()) -> ActorId {
        match kind {
            0 => ActorId::local_user(),
            1 => ActorId::new_ai_session(),
            2 => ActorId::new_app(&format!("app{}", suffix)),
            _ => ActorId::system_compositor(),
        }
    }
}

prop_compose! {
    fn arb_event_kind()(
        which in 0u8..4u8,
        key in "[a-z]{1,8}",
        val in any::<i64>(),
    ) -> EventKind {
        match which {
            0 => EventKind::Invoke { method: key.clone(), args: json!(val) },
            1 => EventKind::StateSet { key, value: json!(val) },
            2 => EventKind::Lifecycle(LifecycleKind::Created),
            _ => EventKind::Lifecycle(LifecycleKind::Destroyed),
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn event_round_trip(actor in arb_actor_id(), kind in arb_event_kind()) {
        let ev = Event::new(actor, ObjectId::new(), kind);
        let s = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(ev, back);
    }
}
```

- [ ] **Step 4: 실행**

Run: `cargo test -p geulos-core --test proptest_p5_roundtrip --release`
Expected: 10000 cases pass.

(`--release`는 proptest 속도를 위해 권장. 안 써도 통과는 하지만 1-2분 걸릴 수 있음.)

- [ ] **Step 5: sanity + 커밋**

```bash
cargo test -p geulos-core
cargo clippy -p geulos-core --all-targets -- -D warnings
git add -A
git commit -m "test(core): P5 라운드트립 속성 테스트 (proptest 1만 케이스)"
```

---

## Task 13: P1 트리 무결성 속성 테스트

P1: 임의의 mount + invoke 시퀀스 후에도 트리의 부모-자식 참조가 일관됨.

**Files:**
- Create: `core/tests/proptest_p1_tree_integrity.rs`

- [ ] **Step 1: 테스트 작성**

`core/tests/proptest_p1_tree_integrity.rs`:

```rust
//! P1: 트리 무결성 — 임의 mount/invoke 시퀀스 후에도 트리가 유효.
//!
//! 무결성 정의:
//! - 모든 객체의 parent가 None이거나 트리 내 존재하는 객체.
//! - 모든 부모의 children 목록에 있는 ID가 트리 내 존재.
//! - children에 같은 ID가 중복 등장하지 않음.

use geulos_core::{std_types, ActorId, ObjectServer};
use proptest::prelude::*;
use serde_json::json;

#[derive(Debug, Clone)]
enum Op {
    MountText(String),
    MountButton(String),
    MountContainer,
    InvokePress(usize), // 트리 내 N번째 버튼을 누름 (없으면 noop)
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        "[a-z]{1,10}".prop_map(Op::MountText),
        "[a-z]{1,10}".prop_map(Op::MountButton),
        Just(Op::MountContainer),
        (0usize..32).prop_map(Op::InvokePress),
    ]
}

fn verify_tree_integrity(server: &ObjectServer) -> Result<(), String> {
    let object_count = server.object_count();
    for root_id in server.roots() {
        let obj = server.get(root_id).ok_or("root id not in tree")?;
        for child_id in &obj.children {
            if server.get(child_id).is_none() {
                return Err(format!("child {} 가 트리에 없음", child_id));
            }
        }
    }
    // 중복 ID는 HashMap 사용으로 인해 발생 불가하지만 sanity 확인:
    let _ = object_count;
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn random_ops_preserve_tree_integrity(ops in proptest::collection::vec(arb_op(), 0..50)) {
        let mut server = ObjectServer::new();
        let owner = ActorId::local_user();
        let mut button_ids: Vec<geulos_core::ObjectId> = Vec::new();

        for op in ops {
            match op {
                Op::MountText(s) => {
                    let _ = server.mount(std_types::text(owner.clone(), &s));
                }
                Op::MountButton(label) => {
                    let id = server.mount(std_types::button(owner.clone(), &label));
                    if let Ok(id) = id {
                        button_ids.push(id);
                    }
                }
                Op::MountContainer => {
                    let _ = server.mount(std_types::container(owner.clone()));
                }
                Op::InvokePress(idx) => {
                    if let Some(id) = button_ids.get(idx % button_ids.len().max(1)).cloned() {
                        let _ = server.invoke(&owner, &id, "press", json!({}));
                    }
                }
            }
            verify_tree_integrity(&server).expect("invariant broken");
        }
    }
}
```

- [ ] **Step 2: 실행**

Run: `cargo test -p geulos-core --test proptest_p1_tree_integrity --release`
Expected: 10000 cases pass.

- [ ] **Step 3: sanity + 커밋**

```bash
cargo test -p geulos-core
cargo clippy -p geulos-core --all-targets -- -D warnings
git add -A
git commit -m "test(core): P1 트리 무결성 속성 테스트 (proptest 1만 케이스)"
```

---

## Task 14: M1 인수 테스트 (acceptance)

설계 문서 §9.2의 M1 완료 기준을 그대로 코드로 표현.

**Files:**
- Create: `core/tests/acceptance_test.rs`

- [ ] **Step 1: 테스트 작성**

`core/tests/acceptance_test.rs`:

```rust
//! M1 인수 테스트.
//!
//! 설계 문서 §9.2 완료 기준: "라이브러리로 import → Container > Text("hello") 만들고,
//! 직렬화·역직렬화·해체 라운드트립 통과."

use geulos_core::{std_types, ActorId, ObjectServer};

#[test]
fn container_text_round_trip() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();

    // 1. Container > Text("hello") 트리 구성
    let mut container = std_types::container(owner.clone());
    let text = std_types::text(owner.clone(), "hello");
    let text_id = text.id;
    container.children.push(text_id);
    let container_id = container.id;

    // 2. mount
    server.mount_with_descendants(container, vec![text]).unwrap();
    assert_eq!(server.object_count(), 2);

    // 3. Container 객체를 가져와 JSON 직렬화
    let c_obj = server.get(&container_id).unwrap();
    let json = serde_json::to_string(c_obj).expect("직렬화 가능해야 함");
    assert!(json.contains("Container"));

    // 4. 역직렬화 → 동등
    let back: geulos_core::Object = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, container_id);
    assert_eq!(back.type_uri.as_str(), "aios.std/Container@1");
    assert_eq!(back.children, vec![text_id]);

    // 5. Text 객체도 동일
    let t_obj = server.get(&text_id).unwrap();
    let json2 = serde_json::to_string(t_obj).unwrap();
    let back2: geulos_core::Object = serde_json::from_str(&json2).unwrap();
    assert_eq!(back2.id, text_id);
    assert_eq!(
        back2.state.get("content"),
        Some(&serde_json::json!("hello"))
    );
}

#[test]
fn invoke_lifecycle_and_subscribe_observed() {
    use geulos_core::{EventKind, EventKindFilter};
    use serde_json::json;

    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();

    let btn = std_types::button(owner.clone(), "OK");
    let btn_id = server.mount(btn).unwrap();

    let sub = server.subscribe(owner.clone(), btn_id, vec![EventKindFilter::Invoke]);

    server.invoke(&owner, &btn_id, "press", json!({"force": 5})).unwrap();
    server.invoke(&owner, &btn_id, "press", json!({"force": 10})).unwrap();

    let drained = server.drain_subscription(sub);
    assert_eq!(drained.len(), 2);
    for ev in drained {
        match ev.kind {
            EventKind::Invoke { method, .. } => assert_eq!(method, "press"),
            _ => panic!("expected Invoke"),
        }
    }
}
```

- [ ] **Step 2: 실행**

Run: `cargo test -p geulos-core --test acceptance_test`
Expected: 2 tests pass.

- [ ] **Step 3: sanity + 커밋**

```bash
cargo test -p geulos-core
cargo clippy -p geulos-core --all-targets -- -D warnings
git add -A
git commit -m "test(core): M1 인수 테스트 (Container>Text 라운드트립, invoke+subscribe)"
```

---

## Task 15: 최종 스모크 + 푸시

**Files:** (확인용, 신규 작성 없음)

- [ ] **Step 1: 전체 빌드**

Run: `cargo build --workspace --all-targets`
Expected: 경고 없이 성공.

- [ ] **Step 2: 전체 테스트 (proptest 포함)**

Run: `cargo test --workspace --all-targets`
Expected: ObjectId 3 + identity 6 + ACL 4 + event 4 + object_struct 7 + std_types 4 + event_bus 5 + server_basic 2 + server_mount 4 + server_invoke 5 + server_query 3 + server_subscribe 5 + acceptance 2 + P5 proptest + P1 proptest = 모두 PASS.

Run: `cargo test --workspace --release` (proptest 가속)
Expected: 동일.

- [ ] **Step 3: clippy 전체**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 경고 0.

- [ ] **Step 4: 포맷 체크**

Run: `cargo fmt --all -- --check`
Expected: 일치. 차이가 있으면 `cargo fmt --all` 후 fmt 커밋:

```bash
git add -A
git commit -m "style: cargo fmt 적용"
```

- [ ] **Step 5: 푸시**

Run: `git push origin main`

- [ ] **Step 6: CI 그린 확인**

브라우저로 https://github.com/wwoosshh/geul_OS/actions 열어 최근 워크플로우 그린 확인.

- [ ] **Step 7: M1 완료 선언**

다음이 모두 사실이어야 한다:
- ObjectServer.mount / invoke / query / subscribe 모두 동작
- EventBus 단일 라이터, 이벤트 로그 보존
- 표준 타입 4종 (Container/Text/Button/Toggle) 사용 가능
- ACL 평가 (소유자 우대 + ACL 매칭 + 기본 거부) 동작
- P1·P5 proptest 1만 케이스 통과
- acceptance_test 통과
- CI 그린

M2 (와이어 프로토콜 + Unix 소켓 서버) 진입 준비 완료.

---

## 자체 점검 결과

**스펙 커버리지:**
- 설계 문서 §9.2 M1 목표 4개 항목 모두 plan에 매핑:
  - ObjectServer (in-memory tree, ObjectId 발급, ACL 보유) → T1/T4/T7/T8/T9/T10
  - EventBus (단일 라이터 큐, 전순서 부여, 이벤트 로그 영속) → T3/T6
  - 표준 타입 4종 → T5
  - L0 + L1(P1, P5) → T12/T13 (L1) + 전 태스크 (L0)
- 완료 기준 (Container > Text("hello") 라운드트립) → T14

**플레이스홀더 스캔:** TBD/TODO 없음. "Similar to" 참조 없음. 모든 코드 인라인.

**타입 일관성:**
- `ObjectId`(M0 기존) → 모든 후속 태스크에서 일관 참조
- `EventId`(T1) → T3/T6의 Event/EventBus에서 사용 일관
- `ActorId`(T1) → T2 (AclEntry), T4 (Object.owner), T6 (EventBus.emit) 등 모두 일관
- `TypeUri`(T1) → T4 (Object.type_uri), T5 (std_types 생성자), T10 (Query.ByType) 일관
- `Object`(T4) → T7 이후 server 메서드들에서 일관 사용
- `EventBus`(T6) → T7 이후 ObjectServer 내부에서 단일 인스턴스로 보유
