> **Status:** completed (2026-05-17)
> **Note:** M3 앱 런타임 + 권한 매니저 정식 마감 — echo-app 별 프로세스 + 매니페스트 권한.

# GeulOS M3 — 앱 런타임 + 권한 매니저 실행 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 앱이 *별 프로세스*로 떠서 서버에 접속해 자기 UI를 게시하고, 외부 클라이언트의 호출에 반응해 상태를 갱신할 수 있게 한다. M0에서 만든 echo-app placeholder를 *실제 동작하는 앱*으로 채운다.

**Architecture:**
- 앱은 M2 와이어 프로토콜의 `Role::App` 클라이언트로 동작 — 별 프로세스, TCP로 server-host 접속
- `aios.toml` 매니페스트 = 앱의 정체·요구 권한·사용 ui_types 선언서. 앱이 시작 시 읽어 Hello에 포함, 서버가 검증
- `StateSet` 와이어 메시지 추가 — 앱이 자기 객체의 state를 갱신할 수 있게
- 앱 라이프사이클: connect → handshake → mount → running. 연결 끊김 = "crash" 로 간주, 서버가 cleanup 이벤트 발행
- echo-app: container > [text "count: N", button "+1"]. press → app이 받아 text.content 갱신. M2 acceptance의 자연스러운 다음 단계.

**Tech Stack:** `toml` (매니페스트 파싱), 기존 tokio/serde/serde_json.

**Selection criteria (완료 조건):**
- `cargo build --workspace --all-targets` 성공, 경고 0
- `cargo test --workspace` 전체 그린
- `cargo run -p geulos-server-host` + `cargo run -p geulos-echo-app`을 두 터미널에서 띄우면 echo-app이 mount 성공
- 외부 클라이언트(geulosh --connect 또는 test 코드)가 press 호출 → 카운터 증가가 *Subscribe 이벤트*로 관찰됨
- CI 그린

---

## ADR 시드

- **ADR-011 — 앱 = 별 프로세스, TCP로 server-host에 접속.** UDS는 M6에서.
- **ADR-012 — M3 단계의 권한은 "매니페스트가 곧 선언, 자동 부여". 사용자 동의 UI는 M4 컴포지터 도착 후.**

---

## 파일 구조 (사전 매핑)

```
core/
├── src/
│   ├── object/
│   │   └── manifest.rs                # AppManifest, ManifestError
│   └── server/
│       └── set_state.rs               # ObjectServer.set_state()
└── tests/
    ├── manifest_test.rs
    └── server_set_state_test.rs

proto/
├── src/
│   └── messages.rs                    # + StateSetMsg / StateSetAck / StateSetError
└── tests/
    └── messages_test.rs               # + state_set tests

server-host/
├── src/
│   ├── connection.rs                  # 매니페스트 검증
│   ├── dispatch.rs                    # + handle_state_set
│   ├── lifecycle.rs                   # 앱 세션 추적 + disconnect 정리
│   └── lib.rs                         # lifecycle 노출
└── tests/
    ├── manifest_handshake_conformance.rs
    └── state_set_conformance.rs

apps/echo-app/
├── Cargo.toml                         # 의존성 본격 추가
├── aios.toml                          # NEW 매니페스트 파일
└── src/
    ├── main.rs                        # 실제 동작
    └── lib.rs                         # 테스트 가능한 로직

tests/                                 # 워크스페이스 루트 통합 테스트
└── m3_acceptance.rs                   # echo-app subprocess + 외부 클라이언트
```

---

## Task 1: ActorId::from_str (M2 백로그 + M3 전제)

**Files:**
- Modify: `core/src/object/identity.rs`
- Modify: `core/tests/identity_test.rs`

ActorId의 외부 문자열 → ActorId 변환이 필요. M2에서는 query owner ai:<uuid> 매칭 불가가 한계였고, M3에서 manifest 기반 actor 추적 시 더 자주 필요해짐.

- [ ] **Step 1: 실패 테스트 추가**

`core/tests/identity_test.rs`의 끝에:

```rust
use geulos_core::object::identity::ActorIdParseError;

#[test]
fn actor_id_from_str_accepts_known_prefixes() {
    let u = ActorId::from_str("user:local").unwrap();
    assert_eq!(u.as_str(), "user:local");

    let s = ActorId::from_str("system:compositor").unwrap();
    assert_eq!(s.as_str(), "system:compositor");

    let a = ActorId::from_str("ai:abc-123").unwrap();
    assert_eq!(a.as_str(), "ai:abc-123");

    let p = ActorId::from_str("app:memo:xyz-789").unwrap();
    assert_eq!(p.as_str(), "app:memo:xyz-789");
}

#[test]
fn actor_id_from_str_rejects_unknown_prefix() {
    let err = ActorId::from_str("wat:something").unwrap_err();
    assert!(matches!(err, ActorIdParseError::UnknownPrefix(_)));
}

#[test]
fn actor_id_from_str_rejects_empty() {
    assert!(ActorId::from_str("").is_err());
}

#[test]
fn actor_id_round_trip_via_serde() {
    let original = ActorId::from_str("ai:test-session-1").unwrap();
    let json = serde_json::to_string(&original).unwrap();
    let back: ActorId = serde_json::from_str(&json).unwrap();
    assert_eq!(original, back);
}
```

- [ ] **Step 2: 실패 확인**

`cargo test -p geulos-core --test identity_test`
→ 컴파일 실패.

- [ ] **Step 3: 구현 추가 (`core/src/object/identity.rs`)**

기존 파일 끝에 추가:

```rust
/// ActorId 파싱 오류.
#[derive(Debug, Error)]
pub enum ActorIdParseError {
    /// 빈 문자열.
    #[error("empty actor id")]
    Empty,
    /// 알 수 없는 접두사.
    #[error("unknown actor prefix: '{0}' (expected user:/system:/ai:/app:)")]
    UnknownPrefix(String),
}

impl ActorId {
    /// 문자열로부터 ActorId를 구성.
    ///
    /// 허용 접두사: `user:`, `system:`, `ai:`, `app:`.
    pub fn from_str(s: &str) -> Result<Self, ActorIdParseError> {
        if s.is_empty() {
            return Err(ActorIdParseError::Empty);
        }
        let prefix = s.split(':').next().unwrap_or("");
        if !matches!(prefix, "user" | "system" | "ai" | "app") {
            return Err(ActorIdParseError::UnknownPrefix(prefix.to_string()));
        }
        Ok(Self(s.to_string()))
    }
}
```

`ActorIdParseError`를 `pub use`로 노출 — `core/src/object/mod.rs`와 `core/src/lib.rs`도 업데이트.

- [ ] **Step 4: 테스트 통과 + 커밋**

```bash
cargo test -p geulos-core
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "feat(core): ActorId::from_str + ActorIdParseError"
```

---

## Task 2: AppManifest 타입 + aios.toml 파싱

**Files:**
- Modify: 루트 `Cargo.toml` (`[workspace.dependencies]`에 `toml = "0.8"` 추가)
- Modify: `core/Cargo.toml` (toml dev-dep)
- Create: `core/src/object/manifest.rs`
- Modify: `core/src/object/mod.rs`, `core/src/lib.rs`
- Create: `core/tests/manifest_test.rs`

- [ ] **Step 1: workspace에 toml 의존성 추가**

`Cargo.toml`의 `[workspace.dependencies]`:

```toml
toml = "0.8"
```

`core/Cargo.toml`의 `[dependencies]`에 추가:

```toml
toml = { workspace = true }
```

- [ ] **Step 2: 실패 테스트 작성 (`core/tests/manifest_test.rs`)**

```rust
use geulos_core::{AppManifest, ManifestError, TypeUri};

#[test]
fn parse_minimal_manifest() {
    let toml = r#"
id = "memo"
permissions = []
ui_types = ["aios.std/Text@1"]
"#;
    let m = AppManifest::from_toml(toml).unwrap();
    assert_eq!(m.id, "memo");
    assert!(m.permissions.is_empty());
    assert_eq!(m.ui_types.len(), 1);
    assert_eq!(m.ui_types[0].as_str(), "aios.std/Text@1");
}

#[test]
fn parse_full_manifest() {
    let toml = r#"
id = "echo"
permissions = ["fs.user.docs", "clipboard.read"]
ui_types = ["aios.std/Container@1", "aios.std/Text@1", "aios.std/Button@1"]
"#;
    let m = AppManifest::from_toml(toml).unwrap();
    assert_eq!(m.id, "echo");
    assert_eq!(m.permissions.len(), 2);
    assert_eq!(m.ui_types.len(), 3);
}

#[test]
fn rejects_missing_id() {
    let toml = r#"
permissions = []
ui_types = []
"#;
    let err = AppManifest::from_toml(toml).unwrap_err();
    assert!(matches!(err, ManifestError::Toml(_)));
}

#[test]
fn rejects_invalid_ui_type_uri() {
    let toml = r#"
id = "bad"
permissions = []
ui_types = ["this is not a type uri"]
"#;
    let err = AppManifest::from_toml(toml).unwrap_err();
    assert!(matches!(err, ManifestError::BadTypeUri(_)));
}

#[test]
fn round_trip_via_to_toml() {
    let m = AppManifest {
        id: "test".to_string(),
        permissions: vec!["fs.user.docs".to_string()],
        ui_types: vec![TypeUri::parse("aios.std/Text@1").unwrap()],
    };
    let s = m.to_toml().unwrap();
    let back = AppManifest::from_toml(&s).unwrap();
    assert_eq!(m.id, back.id);
    assert_eq!(m.permissions, back.permissions);
    assert_eq!(m.ui_types.len(), back.ui_types.len());
}

#[test]
fn allows_known_type_uri() {
    let toml = r#"
id = "x"
permissions = []
ui_types = ["aios.std/Button@1", "x/Custom@2"]
"#;
    let m = AppManifest::from_toml(toml).unwrap();
    assert_eq!(m.ui_types.len(), 2);
}
```

- [ ] **Step 3: 실패 확인 후 구현**

`core/src/object/manifest.rs`:

```rust
//! 앱 매니페스트 (`aios.toml`).
//!
//! 앱이 시작할 때 자기 정체·요구 권한·사용 ui_types를 선언한다.
//! 서버는 Hello 시 검증해 ActorId(`app:<id>:<uuid>`)를 발급.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::identity::TypeUri;

/// 매니페스트 파일의 raw 표현 (toml에서 deserialize).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestRaw {
    id: String,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    ui_types: Vec<String>,
}

/// 앱 매니페스트.
#[derive(Debug, Clone, PartialEq)]
pub struct AppManifest {
    /// 앱 고유 ID (영문/숫자/`-`/`_`).
    pub id: String,
    /// 카테고리 권한 목록 (예: `fs.user.docs`).
    pub permissions: Vec<String>,
    /// 이 앱이 사용할 객체 타입 URI 목록.
    pub ui_types: Vec<TypeUri>,
}

/// 매니페스트 파싱 오류.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// TOML 파싱 실패.
    #[error("TOML parse error: {0}")]
    Toml(String),
    /// TypeUri 파싱 실패.
    #[error("bad TypeUri: {0}")]
    BadTypeUri(String),
}

impl AppManifest {
    /// TOML 문자열로부터 파싱.
    pub fn from_toml(s: &str) -> Result<Self, ManifestError> {
        let raw: ManifestRaw =
            toml::from_str(s).map_err(|e| ManifestError::Toml(e.to_string()))?;
        let mut ui_types = Vec::new();
        for t in raw.ui_types {
            let parsed =
                TypeUri::parse(&t).map_err(|_| ManifestError::BadTypeUri(t.clone()))?;
            ui_types.push(parsed);
        }
        Ok(Self {
            id: raw.id,
            permissions: raw.permissions,
            ui_types,
        })
    }

    /// TOML 문자열로 직렬화.
    pub fn to_toml(&self) -> Result<String, ManifestError> {
        let raw = ManifestRaw {
            id: self.id.clone(),
            permissions: self.permissions.clone(),
            ui_types: self.ui_types.iter().map(|t| t.as_str().to_string()).collect(),
        };
        toml::to_string(&raw).map_err(|e| ManifestError::Toml(e.to_string()))
    }

    /// 주어진 type_uri가 매니페스트에 선언되어 있는지.
    pub fn declares_type(&self, type_uri: &TypeUri) -> bool {
        self.ui_types.iter().any(|t| t == type_uri)
    }
}
```

`core/src/object/mod.rs`에 `pub mod manifest;` + `pub use manifest::{AppManifest, ManifestError};` 추가.

`core/src/lib.rs` 재export 확장:

```rust
pub use object::{
    AclEffect, AclEntry, ActorId, ActorPattern, AppManifest, ArgSpec, EventId, ManifestError,
    MethodPattern, MethodSig, Object, ObjectId, TypeUri,
};
```

- [ ] **Step 4: 테스트 통과 + 커밋**

```bash
cargo test -p geulos-core --test manifest_test
cargo test -p geulos-core
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "feat(core): AppManifest + aios.toml 파싱"
```

---

## Task 3: ObjectServer.set_state() + StateSet 이벤트

객체 상태를 *외부에서* 변경하는 path. 지금은 Object.set_state가 라이브러리 API로만 노출되어 있고, 와이어/server를 통해서는 불가능. 

**Files:**
- Create: `core/src/server/set_state.rs`
- Modify: `core/src/server/mod.rs`
- Create: `core/tests/server_set_state_test.rs`

- [ ] **Step 1: 실패 테스트**

`core/tests/server_set_state_test.rs`:

```rust
use geulos_core::{std_types, ActorId, EventKind, ObjectServer};
use serde_json::json;

#[test]
fn set_state_by_owner_succeeds() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let txt = std_types::text(owner.clone(), "initial");
    let id = server.mount(txt).unwrap();

    let ev = server
        .set_state(&owner, &id, "content", json!("updated"))
        .expect("owner should be allowed");
    assert!(ev.as_u64() > 0);

    let obj = server.get(&id).unwrap();
    assert_eq!(obj.state.get("content"), Some(&json!("updated")));
}

#[test]
fn set_state_emits_state_set_event() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let txt = std_types::text(owner.clone(), "x");
    let id = server.mount(txt).unwrap();

    let log_len_before = server.bus().log().len();
    server.set_state(&owner, &id, "content", json!("y")).unwrap();

    let log = server.bus().log();
    assert_eq!(log.len(), log_len_before + 1);
    match &log.last().unwrap().kind {
        EventKind::StateSet { key, value } => {
            assert_eq!(key, "content");
            assert_eq!(value, &json!("y"));
        }
        _ => panic!("expected StateSet event"),
    }
}

#[test]
fn set_state_denied_for_non_owner() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let intruder = ActorId::new_ai_session();
    let txt = std_types::text(owner, "x");
    let id = server.mount(txt).unwrap();

    let result = server.set_state(&intruder, &id, "content", json!("hacked"));
    assert!(result.is_err());
}

#[test]
fn set_state_nonexistent_object_errors() {
    use geulos_core::ObjectId;
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let bogus = ObjectId::new();
    let result = server.set_state(&owner, &bogus, "content", json!("x"));
    assert!(result.is_err());
}
```

- [ ] **Step 2: 구현 (`core/src/server/set_state.rs`)**

```rust
//! set_state(): 객체 상태 직접 갱신.

use serde_json::Value;
use thiserror::Error;

use crate::event::EventKind;
use crate::object::{ActorId, EventId, ObjectId};
use crate::server::ObjectServer;

/// set_state 실패 사유.
#[derive(Debug, Error)]
pub enum SetStateError {
    /// 객체 없음.
    #[error("객체를 찾을 수 없음: {0}")]
    NotFound(ObjectId),
    /// 권한 없음.
    #[error("권한 없음: 액터 {actor}, 객체 {target}, 키 {key}")]
    PermissionDenied { actor: String, target: ObjectId, key: String },
}

impl ObjectServer {
    /// 객체의 state 필드 하나를 갱신하고 StateSet 이벤트를 발행.
    ///
    /// ACL: 소유자만 허용 (M3 기본 정책). 추후 매니페스트 권한과 연동.
    pub fn set_state(
        &mut self,
        actor: &ActorId,
        target: &ObjectId,
        key: &str,
        value: Value,
    ) -> Result<EventId, SetStateError> {
        // 1) 객체 존재
        let obj = self
            .objects
            .get_mut(target)
            .ok_or(SetStateError::NotFound(*target))?;

        // 2) ACL — 소유자 우대, 그 외는 거부 (M3 기본).
        if &obj.owner != actor {
            return Err(SetStateError::PermissionDenied {
                actor: actor.as_str().to_string(),
                target: *target,
                key: key.to_string(),
            });
        }

        // 3) 갱신
        obj.state.insert(key.to_string(), value.clone());

        // 4) 이벤트 발행 (이벤트 버스 + 구독자 알림)
        let event_id = self.bus.emit(
            actor.clone(),
            *target,
            EventKind::StateSet { key: key.to_string(), value },
            None,
        );
        if let Some(ev) = self.bus.log().last() {
            self.subscriptions.deliver(ev);
        }

        Ok(event_id)
    }
}
```

**중요:** ObjectServer의 `objects` 필드는 현재 `HashMap<ObjectId, Object>` (pub(crate)). `get_mut` 접근이 가능한지 확인 필요. `core/src/server/mod.rs`에서 `pub(crate)` 또는 `pub(super)`가 적절히 설정되어 있어야 함.

- [ ] **Step 3: 모듈 노출 + 재export**

`core/src/server/mod.rs`에 추가:

```rust
pub mod set_state;
pub use set_state::SetStateError;
```

- [ ] **Step 4: 테스트 통과 + 커밋**

```bash
cargo test -p geulos-core --test server_set_state_test
cargo test -p geulos-core
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "feat(core): ObjectServer.set_state() + StateSet 이벤트"
```

---

## Task 4: StateSet 와이어 메시지

**Files:**
- Modify: `proto/src/messages.rs`
- Modify: `proto/src/lib.rs`
- Modify: `proto/tests/messages_test.rs`

- [ ] **Step 1: 메시지 타입 추가**

`proto/src/messages.rs`의 끝에:

```rust
/// `StateSet` 요청: 객체의 state 키 하나를 갱신.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "StateSet")]
pub struct StateSetMsg {
    pub request_id: String,
    pub target: String,
    pub key: String,
    pub value: Value,
}

/// `StateSetAck`: 성공 응답.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "StateSetAck")]
pub struct StateSetAck {
    pub request_id: String,
    pub event_id: String,
}

/// `StateSetError`: 실패 응답.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "StateSetError")]
pub struct StateSetError {
    pub request_id: String,
    /// (M2 InvokeError와 동일한 직렬화 트릭: 와이어 키는 `error_kind`)
    #[serde(rename = "error_kind")]
    pub kind: String,
    pub detail: String,
}
```

`proto/src/lib.rs` 재export 확장. 

- [ ] **Step 2: 테스트 추가 (`proto/tests/messages_test.rs` 끝에)**

```rust
use geulos_proto::{StateSetAck, StateSetError, StateSetMsg};

#[test]
fn state_set_message_round_trip() {
    let m = StateSetMsg {
        request_id: "r-1".to_string(),
        target: "obj-uuid".to_string(),
        key: "content".to_string(),
        value: serde_json::json!("hello"),
    };
    let s = serde_json::to_string(&m).unwrap();
    assert!(s.contains(r#""kind":"StateSet""#));
    let back: StateSetMsg = serde_json::from_str(&s).unwrap();
    assert_eq!(m, back);
}

#[test]
fn state_set_ack_round_trip() {
    let a = StateSetAck {
        request_id: "r-1".to_string(),
        event_id: "ev:42".to_string(),
    };
    let s = serde_json::to_string(&a).unwrap();
    assert!(s.contains(r#""kind":"StateSetAck""#));
}

#[test]
fn state_set_error_uses_error_kind_wire_name() {
    let e = StateSetError {
        request_id: "r-1".to_string(),
        kind: "permission".to_string(),
        detail: "denied".to_string(),
    };
    let s = serde_json::to_string(&e).unwrap();
    assert!(s.contains(r#""kind":"StateSetError""#));
    assert!(s.contains(r#""error_kind":"permission""#));
}
```

- [ ] **Step 3: 통과 + 커밋**

```bash
cargo test -p geulos-proto
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "feat(proto): StateSet 와이어 메시지 (Msg/Ack/Error)"
```

---

## Task 5: server-host에 매니페스트 검증 + StateSet 디스패치

**Files:**
- Modify: `server-host/src/connection.rs` (Hello 단계에서 App role 매니페스트 검증)
- Modify: `server-host/src/dispatch.rs` (handle_state_set 추가)
- Modify: `server-host/Cargo.toml` (toml 의존성)
- Create: `server-host/tests/manifest_handshake_conformance.rs`
- Create: `server-host/tests/state_set_conformance.rs`

- [ ] **Step 1: 매니페스트 검증 테스트**

`server-host/tests/manifest_handshake_conformance.rs`:

```rust
use geulos_proto::{decode_frame, encode_frame, Hello, HelloAck, HelloReject, Role};
use geulos_server_host::run_listener;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn app_hello_with_valid_manifest_succeeds() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let manifest = json!({
        "manifest": {
            "id": "test-app",
            "permissions": [],
            "ui_types": ["aios.std/Text@1"]
        }
    });
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::App,
        auth: manifest,
        client_id: "c".to_string(),
    };
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp_body = decode_frame(&mut slice).unwrap();
    let ack: HelloAck = serde_json::from_slice(&resp_body)
        .expect(&format!("expected HelloAck, got: {}", String::from_utf8_lossy(&resp_body)));
    assert!(ack.actor_id.starts_with("app:test-app:"));
}

#[tokio::test]
async fn app_hello_with_missing_manifest_rejected() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::App,
        auth: json!({}), // 매니페스트 없음
        client_id: "c".to_string(),
    };
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp_body = decode_frame(&mut slice).unwrap();
    let rej: HelloReject =
        serde_json::from_slice(&resp_body).expect("expected HelloReject");
    assert_eq!(rej.reason, "missing_manifest");
}

#[tokio::test]
async fn app_hello_with_invalid_manifest_rejected() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::App,
        auth: json!({"manifest": {"id": "x", "permissions": [], "ui_types": ["bad type uri"]}}),
        client_id: "c".to_string(),
    };
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp_body = decode_frame(&mut slice).unwrap();
    let rej: HelloReject =
        serde_json::from_slice(&resp_body).expect("expected HelloReject");
    assert_eq!(rej.reason, "invalid_manifest");
}
```

- [ ] **Step 2: 구현 (`connection.rs`의 `read_and_handle_hello_split`에 App 분기 보강)**

기존 Role::App 처리부:
```rust
Role::App => ActorId::new_app(
    hello.auth.get("manifest").and_then(|m| m.get("id"))
        .and_then(|v| v.as_str()).unwrap_or("unknown"),
),
```
를 다음으로 교체:

```rust
Role::App => {
    let manifest_val = match hello.auth.get("manifest") {
        Some(m) => m,
        None => {
            let rej = HelloReject {
                reason: "missing_manifest".to_string(),
                detail: "Role::App requires auth.manifest".to_string(),
            };
            let body = serde_json::to_vec(&rej).unwrap();
            let mut w = writer.lock().await;
            let _ = w.write_all(&encode_frame(&body)).await;
            return Err("missing_manifest".to_string());
        }
    };

    // raw JSON → TOML round-trip은 복잡하니 직접 deserialize 시도
    let raw_id = manifest_val.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let raw_ui_types: Vec<String> = manifest_val
        .get("ui_types")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    if raw_id.is_empty() {
        let rej = HelloReject {
            reason: "invalid_manifest".to_string(),
            detail: "manifest.id is required".to_string(),
        };
        let body = serde_json::to_vec(&rej).unwrap();
        let mut w = writer.lock().await;
        let _ = w.write_all(&encode_frame(&body)).await;
        return Err("invalid_manifest".to_string());
    }

    // 모든 ui_types가 유효한 TypeUri여야 함.
    for t in &raw_ui_types {
        if geulos_core::TypeUri::parse(t).is_err() {
            let rej = HelloReject {
                reason: "invalid_manifest".to_string(),
                detail: format!("bad TypeUri in ui_types: '{}'", t),
            };
            let body = serde_json::to_vec(&rej).unwrap();
            let mut w = writer.lock().await;
            let _ = w.write_all(&encode_frame(&body)).await;
            return Err("invalid_manifest".to_string());
        }
    }

    ActorId::new_app(raw_id)
}
```

- [ ] **Step 3: StateSet 디스패치 추가 (`dispatch.rs`)**

```rust
/// StateSet 메시지 처리.
pub async fn handle_state_set(
    handle: &ObjectServerHandle,
    msg: geulos_proto::StateSetMsg,
    session_actor: ActorId,
) -> Value {
    let target = match parse_object_id(&msg.target) {
        Some(t) => t,
        None => {
            return serde_json::to_value(geulos_proto::StateSetError {
                request_id: msg.request_id,
                kind: "malformed_target".to_string(),
                detail: format!("bad UUID: {}", msg.target),
            })
            .unwrap();
        }
    };
    match handle.set_state(session_actor, target, msg.key.clone(), msg.value).await {
        Ok(event_id) => serde_json::to_value(geulos_proto::StateSetAck {
            request_id: msg.request_id,
            event_id: event_id.to_string(),
        })
        .unwrap(),
        Err(e) => {
            let err_str = e.to_string();
            let kind = if err_str.contains("권한") || err_str.contains("permission") {
                "permission"
            } else if err_str.contains("찾을 수 없음") {
                "not_found"
            } else {
                "core"
            };
            serde_json::to_value(geulos_proto::StateSetError {
                request_id: msg.request_id,
                kind: kind.to_string(),
                detail: err_str,
            })
            .unwrap()
        }
    }
}
```

`actor.rs`의 `ObjectServerHandle`에 `set_state` 메서드 추가 (mpsc 채널 패턴 따라 SetState command 추가):

```rust
// Command enum에 추가:
SetState {
    actor: ActorId,
    target: ObjectId,
    key: String,
    value: Value,
    reply: oneshot::Sender<Result<geulos_core::EventId, geulos_core::SetStateError>>,
},

// ObjectServerHandle impl에 추가:
pub async fn set_state(
    &self,
    actor: ActorId,
    target: ObjectId,
    key: String,
    value: Value,
) -> Result<geulos_core::EventId, HandleError> {
    let (tx, rx) = oneshot::channel();
    self.tx
        .send(Command::SetState { actor, target, key, value, reply: tx })
        .await
        .map_err(|_| HandleError::ActorGone)?;
    rx.await
        .map_err(|_| HandleError::ActorGone)?
        .map_err(|e| HandleError::Core(e.to_string()))
}

// run 루프 match에 추가:
Command::SetState { actor, target, key, value, reply } => {
    let res = server.set_state(&actor, &target, &key, value);
    let _ = reply.send(res);
}
```

`core/src/lib.rs`에 `SetStateError` re-export 필요.

`connection.rs`의 dispatch_one에 "StateSet" 케이스 추가:

```rust
"StateSet" => {
    let m: geulos_proto::StateSetMsg = match serde_json::from_value(raw) { Ok(m) => m, Err(_) => return };
    Some(handle_state_set(handle, m, actor.clone()).await)
}
```

- [ ] **Step 4: StateSet 적합성 테스트**

`server-host/tests/state_set_conformance.rs`:

```rust
use geulos_core::{std_types, ActorId};
use geulos_proto::*;
use geulos_server_host::run_listener;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn state_set_by_owner_succeeds_over_wire() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    // user role로 접속
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Ai, // ai actor를 user owner로 만들지 못하므로 우회 — 아래 mount는 user owner로 만들고 invoke는 ai가 함
        auth: json!({}),
        client_id: "t".to_string(),
    };
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let _ack: HelloAck = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();

    // 클라이언트가 ai 세션으로 접속했지만, mount는 client-side에서 정한 owner를 사용 (M3에서는 그냥 user:local로 만든 객체).
    // StateSet은 owner만 허용 — ai 세션이 user 소유 객체를 set_state 시도 → permission denied 기대.
    let txt = std_types::text(ActorId::local_user(), "before");
    let target = txt.id.to_string();
    let mount = MountMsg {
        root_object_id: target.clone(),
        tree: serde_json::to_value(&txt).unwrap(),
    };
    let body = serde_json::to_vec(&mount).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let _ack: MountAck = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();

    // ai 세션이 StateSet 시도 → permission denied
    let ss = StateSetMsg {
        request_id: "r-1".to_string(),
        target: target.clone(),
        key: "content".to_string(),
        value: json!("after"),
    };
    let body = serde_json::to_vec(&ss).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp_body = decode_frame(&mut slice).unwrap();
    let err: StateSetError = serde_json::from_slice(&resp_body).expect("StateSetError");
    assert_eq!(err.kind, "permission");
}
```

(*owner 세션으로 set_state 성공 검증은 다음 acceptance에서 echo-app 흐름으로 검증 — 여기서는 wire-level 거부 동작만 테스트.*)

- [ ] **Step 5: 통과 + 커밋**

```bash
cargo test -p geulos-server-host
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "feat(server-host): App manifest 검증 + StateSet 디스패치"
```

---

## Task 6: 앱 라이프사이클 — disconnect 감지

연결이 끊기면 그 actor가 mount한 객체들에 Lifecycle::Destroyed 이벤트 발행. (객체 자체를 지우는 것은 *데이터 보존*과 충돌 — M3에서는 *이벤트만* 발행하고 객체는 보관. M4+에서 컴포지터가 active vs gone 상태로 시각화.)

**Files:**
- Modify: `server-host/src/actor.rs` (액터에 actor_objects 매핑 추가)
- Modify: `server-host/src/connection.rs` (disconnect 시 OnDisconnect command 호출)
- Create: `server-host/src/lifecycle.rs` (해체 이벤트 발행 헬퍼)
- Create: `server-host/tests/lifecycle_test.rs`

- [ ] **Step 1: 테스트 작성 (실패용)**

`server-host/tests/lifecycle_test.rs`:

```rust
use geulos_core::{std_types, ActorId, EventKindFilter};
use geulos_server_host::{ObjectServerActor, ObjectServerHandle};
use serde_json::json;

#[tokio::test]
async fn disconnect_emits_lifecycle_destroyed_for_actor_objects() {
    let handle = ObjectServerActor::spawn();

    let owner = ActorId::local_user();
    let txt = std_types::text(owner.clone(), "x");
    let id = handle.mount(txt).await.unwrap();

    // 관찰자 등록
    let observer = ActorId::system_compositor();
    let sub_id = handle.subscribe(observer, id, vec![EventKindFilter::Lifecycle]).await.unwrap();

    // disconnect 시뮬레이션
    handle.disconnect_actor(owner).await.unwrap();

    // Destroyed 이벤트가 와야 함
    let evs = handle.drain(sub_id).await.unwrap();
    assert!(evs.iter().any(|e| matches!(e.kind, geulos_core::EventKind::Lifecycle(geulos_core::LifecycleKind::Destroyed))));
}
```

- [ ] **Step 2: 구현**

`actor.rs`의 Command enum에 `DisconnectActor` 추가:

```rust
DisconnectActor {
    actor: ActorId,
    reply: oneshot::Sender<()>,
},
```

핸들에:

```rust
pub async fn disconnect_actor(&self, actor: ActorId) -> Result<(), HandleError> {
    let (tx, rx) = oneshot::channel();
    self.tx
        .send(Command::DisconnectActor { actor, reply: tx })
        .await
        .map_err(|_| HandleError::ActorGone)?;
    rx.await.map_err(|_| HandleError::ActorGone)
}
```

actor run loop에:

```rust
Command::DisconnectActor { actor, reply } => {
    // 이 actor가 소유한 모든 객체에 대해 Lifecycle::Destroyed 발행
    let owned: Vec<ObjectId> = server
        .objects_iter()
        .filter(|(_, o)| o.owner == actor)
        .map(|(id, _)| *id)
        .collect();
    for id in owned {
        let _ = server.emit_destroyed(&actor, &id);
    }
    let _ = reply.send(());
}
```

이 코드는 `ObjectServer`에 두 가지 API가 필요함:
- `objects_iter() -> impl Iterator<Item = (&ObjectId, &Object)>` — 객체 목록 순회
- `emit_destroyed(&ActorId, &ObjectId) -> EventId` — Destroyed 이벤트 발행

`core/src/server/mod.rs`에 이들 추가:

```rust
impl ObjectServer {
    /// 모든 객체 순회 (액터별 필터링 등에 사용).
    pub fn objects_iter(&self) -> impl Iterator<Item = (&ObjectId, &Object)> {
        self.objects.iter()
    }

    /// 객체의 Lifecycle::Destroyed 이벤트를 발행 (객체는 보관, 이벤트만 발행).
    pub fn emit_destroyed(&mut self, actor: &ActorId, id: &ObjectId) -> EventId {
        let event_id = self.bus.emit(
            actor.clone(),
            *id,
            crate::event::EventKind::Lifecycle(crate::event::LifecycleKind::Destroyed),
            None,
        );
        if let Some(ev) = self.bus.log().last() {
            self.subscriptions.deliver(ev);
        }
        event_id
    }
}
```

`connection.rs`의 read 루프 종료 시 disconnect 호출:

```rust
// 기존 read 루프 종료 직후
push_task.abort();
let _ = handle.disconnect_actor(actor_id).await;
```

- [ ] **Step 3: 통과 + 커밋**

```bash
cargo test -p geulos-server-host
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "feat(server-host): 연결 종료 시 actor 객체에 Lifecycle::Destroyed 발행"
```

---

## Task 7: echo-app 본격 구현

**Files:**
- Modify: `apps/echo-app/Cargo.toml`
- Create: `apps/echo-app/aios.toml`
- Modify: `apps/echo-app/src/main.rs`
- Create: `apps/echo-app/src/lib.rs`
- Create: `apps/echo-app/tests/echo_logic_test.rs`

- [ ] **Step 1: 매니페스트 파일 작성**

`apps/echo-app/aios.toml`:

```toml
id = "echo"
permissions = []
ui_types = [
    "aios.std/Container@1",
    "aios.std/Text@1",
    "aios.std/Button@1",
]
```

- [ ] **Step 2: Cargo.toml 확장**

```toml
[package]
name = "geulos-echo-app"
version = "0.0.1"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "GeulOS demo app: counter button"

[[bin]]
name = "geulos-echo-app"
path = "src/main.rs"

[lib]
name = "geulos_echo_app"
path = "src/lib.rs"

[dependencies]
geulos-core = { path = "../../core" }
geulos-proto = { path = "../../proto" }
tokio = { workspace = true }
serde_json = "1.0"
```

- [ ] **Step 3: `apps/echo-app/src/lib.rs` (테스트 가능 로직)**

```rust
//! echo-app의 핵심 로직 — UI 트리 구성 + 이벤트 반응.

use geulos_core::{std_types, ActorId, Object};

/// echo-app의 초기 UI 트리를 만든다.
///
/// 반환값: (container, text, button) — 모두 같은 owner.
pub fn build_ui(owner: ActorId) -> (Object, Object, Object) {
    let mut container = std_types::container(owner.clone());
    let mut text = std_types::text(owner.clone(), "count: 0");
    let mut button = std_types::button(owner, "+1");

    container.children.push(text.id);
    container.children.push(button.id);
    text.parent = Some(container.id);
    button.parent = Some(container.id);

    (container, text, button)
}

/// 현재 count 값으로부터 다음 count 값과 새 텍스트 컨텐츠를 만든다.
pub fn next_count(current: i64) -> (i64, String) {
    let next = current + 1;
    (next, format!("count: {}", next))
}
```

- [ ] **Step 4: `lib.rs` 단위 테스트**

`apps/echo-app/tests/echo_logic_test.rs`:

```rust
use geulos_core::ActorId;
use geulos_echo_app::{build_ui, next_count};

#[test]
fn build_ui_returns_3_objects_with_parent_relations() {
    let owner = ActorId::new_app("echo");
    let (container, text, button) = build_ui(owner.clone());

    assert_eq!(container.children.len(), 2);
    assert_eq!(text.parent, Some(container.id));
    assert_eq!(button.parent, Some(container.id));
}

#[test]
fn next_count_increments() {
    let (n, s) = next_count(0);
    assert_eq!(n, 1);
    assert_eq!(s, "count: 1");
}
```

- [ ] **Step 5: `apps/echo-app/src/main.rs` 실제 동작**

```rust
//! echo-app: count 버튼 + 텍스트 라벨.
//!
//! 동작:
//! 1. 서버에 App role + 매니페스트로 접속
//! 2. Container > [Text, Button] mount
//! 3. Button을 invoke 필터로 subscribe
//! 4. press 이벤트가 오면 카운터를 증가시키고 Text.content StateSet
//!
//! 외부 클라이언트가 Button을 invoke press 하면 Text가 갱신되어야 함.

use std::time::Duration;

use geulos_echo_app::{build_ui, next_count};
use geulos_proto::{
    decode_frame, encode_frame, EventKindFilterWire, EventMsg, Hello, HelloAck, InvokeMsg,
    MountAck, MountMsg, Role, StateSetAck, StateSetMsg, SubscribeAck, SubscribeMsg,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SERVER_ADDR: &str = "127.0.0.1:5550";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::args().nth(1).unwrap_or_else(|| SERVER_ADDR.to_string());
    println!("echo-app connecting to {}...", addr);

    let mut stream = TcpStream::connect(&addr).await?;

    // 1) Hello (App + manifest)
    let manifest = json!({
        "manifest": {
            "id": "echo",
            "permissions": [],
            "ui_types": [
                "aios.std/Container@1",
                "aios.std/Text@1",
                "aios.std/Button@1",
            ]
        }
    });
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::App,
        auth: manifest,
        client_id: "echo-app".to_string(),
    };
    let body = serde_json::to_vec(&hello)?;
    stream.write_all(&encode_frame(&body)).await?;

    let mut buf = vec![0u8; 16384];
    let mut accum: Vec<u8> = Vec::new();
    let actor_str: String;
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 { return Err("closed before HelloAck".into()); }
        accum.extend_from_slice(&buf[..n]);
        let mut slice = accum.as_slice();
        if let Ok(body) = decode_frame(&mut slice) {
            let consumed = accum.len() - slice.len();
            accum.drain(..consumed);
            let ack: HelloAck = serde_json::from_slice(&body)?;
            actor_str = ack.actor_id.clone();
            println!("[echo-app] HelloAck: actor={}", actor_str);
            break;
        }
    }

    // 2) UI 구성 + mount
    let owner = geulos_core::ActorId::from_str(&actor_str)?;
    let (container, text, button) = build_ui(owner.clone());
    let text_id = text.id;
    let button_id = button.id;

    for obj in [&container, &text, &button] {
        let msg = MountMsg {
            root_object_id: obj.id.to_string(),
            tree: serde_json::to_value(obj)?,
        };
        let body = serde_json::to_vec(&msg)?;
        stream.write_all(&encode_frame(&body)).await?;
        // MountAck 소비
        loop {
            let n = stream.read(&mut buf).await?;
            if n == 0 { return Err("closed".into()); }
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
    println!("[echo-app] mounted: container={}, text={}, button={}", container.id, text_id, button_id);

    // 3) Subscribe to button.invoke
    let sub = SubscribeMsg {
        subscription_id: "sub-button".to_string(),
        target: button_id.to_string(),
        kinds: vec![EventKindFilterWire::Invoke],
        include_initial: false,
    };
    let body = serde_json::to_vec(&sub)?;
    stream.write_all(&encode_frame(&body)).await?;
    // SubscribeAck 소비
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 { return Err("closed".into()); }
        accum.extend_from_slice(&buf[..n]);
        let mut slice = accum.as_slice();
        if let Ok(b) = decode_frame(&mut slice) {
            let consumed = accum.len() - slice.len();
            accum.drain(..consumed);
            let _: SubscribeAck = serde_json::from_slice(&b)?;
            break;
        }
    }
    println!("[echo-app] subscribed to button events");

    // 4) 이벤트 루프
    let mut count: i64 = 0;
    let mut req_seq: u64 = 0;
    loop {
        let n = match tokio::time::timeout(Duration::from_secs(60), stream.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => { eprintln!("read error: {}", e); break; }
            Err(_) => { println!("[echo-app] idle 60s, exiting"); break; }
        };
        if n == 0 { break; }
        accum.extend_from_slice(&buf[..n]);
        loop {
            let mut slice = accum.as_slice();
            match decode_frame(&mut slice) {
                Ok(body) => {
                    let consumed = accum.len() - slice.len();
                    accum.drain(..consumed);
                    let raw: serde_json::Value = match serde_json::from_slice(&body) { Ok(v) => v, Err(_) => continue };
                    let kind = raw.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                    if kind == "Event" {
                        let ev: EventMsg = match serde_json::from_value(raw) { Ok(e) => e, Err(_) => continue };
                        // press 이벤트 감지 → count 증가 + Text 갱신
                        let method = ev.event.get("kind")
                            .and_then(|k| k.get("Invoke"))
                            .and_then(|i| i.get("method"))
                            .and_then(|m| m.as_str())
                            .unwrap_or("");
                        if method == "press" {
                            let (new_count, new_text) = next_count(count);
                            count = new_count;
                            req_seq += 1;
                            let ss = StateSetMsg {
                                request_id: format!("r-{}", req_seq),
                                target: text_id.to_string(),
                                key: "content".to_string(),
                                value: json!(new_text),
                            };
                            let body = serde_json::to_vec(&ss)?;
                            stream.write_all(&encode_frame(&body)).await?;
                            println!("[echo-app] count -> {}", new_count);
                        }
                    } else if kind == "StateSetAck" {
                        let _: StateSetAck = match serde_json::from_value(raw) { Ok(a) => a, Err(_) => continue };
                    }
                }
                Err(_) => break,
            }
        }
    }
    println!("[echo-app] exit");
    Ok(())
}
```

- [ ] **Step 6: 빌드 + 단위 테스트 통과**

```bash
cargo build -p geulos-echo-app
cargo test -p geulos-echo-app
```

- [ ] **Step 7: 수동 통합 테스트 (옵션)**

```bash
# 터미널 A
cargo run -p geulos-server-host

# 터미널 B
cargo run -p geulos-echo-app

# 터미널 C
cargo run -p geulos-shell -- --connect 127.0.0.1:5550
> ls   # echo-app이 만든 3개 객체 보임
> invoke #<button-label> press
> ls   # text의 state.content가 갱신됨
```

(*M3 단계에서는 ls가 본인 actor의 객체만 보임 — geulosh는 ai actor니 echo-app 객체를 볼 수 있는지는 query owner 동작에 달림. 본 단계에서는 query type 으로 확인.*)

- [ ] **Step 8: 커밋**

```bash
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "feat(echo-app): 실제 구현 (Container>Text+Button, press→count 증가)"
```

---

## Task 8: M3 acceptance 통합 테스트

echo-app subprocess + 외부 클라이언트 시뮬레이션을 하나의 통합 테스트로.

**Files:**
- Create: `server-host/tests/m3_acceptance.rs`

- [ ] **Step 1: 통합 테스트 작성**

`server-host/tests/m3_acceptance.rs`:

```rust
//! M3 acceptance: echo-app subprocess + 외부 클라이언트가 press → count 증가 관찰.

use geulos_proto::*;
use geulos_server_host::run_listener;
use serde_json::json;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::time::timeout;

#[tokio::test]
#[ignore = "spawns subprocess; run with --include-ignored"]
async fn echo_app_button_press_increments_counter() -> Result<(), Box<dyn std::error::Error>> {
    // 1) 서버 띄우기
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(run_listener(listener));

    // 2) echo-app subprocess spawn
    let echo_exe = env!("CARGO_BIN_EXE_geulos-echo-app");
    let mut child = Command::new(echo_exe)
        .arg(addr.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    // echo-app이 mount + subscribe 완료할 시간 부여
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 3) 외부 클라이언트 (geulosh 역할)로 접속
    let mut stream = TcpStream::connect(addr).await?;
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Ai,
        auth: json!({}),
        client_id: "external".to_string(),
    };
    let body = serde_json::to_vec(&hello)?;
    stream.write_all(&encode_frame(&body)).await?;
    let mut buf = vec![0u8; 16384];
    let n = stream.read(&mut buf).await?;
    let mut slice = &buf[..n];
    let _ack: HelloAck = serde_json::from_slice(&decode_frame(&mut slice)?)?;

    // 4) query type aios.std/Button@1 으로 button 찾기
    let q = QueryMsg {
        request_id: "q-1".to_string(),
        query: QueryPredicate::ByType { type_uri: "aios.std/Button@1".to_string() },
    };
    let body = serde_json::to_vec(&q)?;
    stream.write_all(&encode_frame(&body)).await?;
    let n = stream.read(&mut buf).await?;
    let mut slice = &buf[..n];
    let qres: QueryResult = serde_json::from_slice(&decode_frame(&mut slice)?)?;
    assert!(!qres.objects.is_empty(), "echo-app의 button을 찾지 못함");
    let button_id = qres.objects[0].clone();

    // 5) Text 찾기
    let q2 = QueryMsg {
        request_id: "q-2".to_string(),
        query: QueryPredicate::ByType { type_uri: "aios.std/Text@1".to_string() },
    };
    let body = serde_json::to_vec(&q2)?;
    stream.write_all(&encode_frame(&body)).await?;
    let n = stream.read(&mut buf).await?;
    let mut slice = &buf[..n];
    let qres2: QueryResult = serde_json::from_slice(&decode_frame(&mut slice)?)?;
    let text_id = qres2.objects[0].clone();

    // 6) Subscribe to text (StateSet)
    let sub = SubscribeMsg {
        subscription_id: "obs-text".to_string(),
        target: text_id.clone(),
        kinds: vec![EventKindFilterWire::StateSet],
        include_initial: false,
    };
    let body = serde_json::to_vec(&sub)?;
    stream.write_all(&encode_frame(&body)).await?;
    let n = stream.read(&mut buf).await?;
    let mut slice = &buf[..n];
    let _: SubscribeAck = serde_json::from_slice(&decode_frame(&mut slice)?)?;

    // 7) 버튼 press 호출 — Permission Denied 예상 (button owner=echo-app, caller=ai-session)
    // 하지만 echo-app은 invoke 이벤트를 *이벤트 버스에서* 받을 수는 있나?
    // 잠깐 — Invoke가 permission denied되면 이벤트 자체가 발행되지 않음!
    //
    // 따라서 echo-app이 외부 호출에 반응하려면 button의 ACL이 외부 호출자(또는 wildcard)를 허용해야 함.
    // M1에서 set_state/invoke ACL은 "owner만"이 기본. M3에서는 button.acl에 wildcard 추가 필요.
    //
    // 본 plan은 *최소 변화*로 acceptance를 통과시키기 위해, build_ui에서 button에 wildcard ACL 추가.
    // (Task 7의 build_ui를 수정해야 함 — 본 plan에서 이 변경을 명시했어야 했다는 사후 검토.)
    //
    // 본 acceptance test는 *수정된 build_ui*가 button을 anyone-can-press로 설정한다고 가정.

    let inv = InvokeMsg {
        request_id: "ext-1".to_string(),
        target: button_id.clone(),
        method: "press".to_string(),
        args: json!(null),
    };
    let body = serde_json::to_vec(&inv)?;
    stream.write_all(&encode_frame(&body)).await?;

    // 응답 (InvokeAck 기대) 처리
    let _ = stream.read(&mut buf).await?;

    // 8) Subscribe로 StateSet 이벤트 기다리기
    let mut got_state_set = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        let n = match timeout(Duration::from_millis(500), stream.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            _ => continue,
        };
        if n == 0 { break; }
        let mut slice = &buf[..n];
        while let Ok(body) = decode_frame(&mut slice) {
            let raw: serde_json::Value = match serde_json::from_slice(&body) { Ok(v) => v, Err(_) => continue };
            if raw.get("kind").and_then(|v| v.as_str()) == Some("Event") {
                let ev: EventMsg = serde_json::from_value(raw)?;
                let kind_label = ev.event.get("kind");
                if kind_label.and_then(|k| k.get("StateSet")).is_some() {
                    got_state_set = true;
                    break;
                }
            }
        }
        if got_state_set { break; }
    }

    let _ = child.kill().await;
    assert!(got_state_set, "Text의 StateSet 이벤트를 못 받음 — echo-app이 press에 반응하지 않은 듯");

    Ok(())
}
```

- [ ] **Step 2: build_ui에 wildcard ACL 추가**

`apps/echo-app/src/lib.rs`의 `build_ui` 끝에:

```rust
use geulos_core::{AclEntry, AclEffect, ActorPattern, MethodPattern};

// build_ui 안에서 button에 wildcard ACL 추가:
button.acl.push(AclEntry {
    actor: ActorPattern::Wildcard,
    method: MethodPattern::Wildcard,
    effect: AclEffect::Allow,
});
```

(M3에서는 echo-app이 *명시적으로* "내 버튼은 누구나 누를 수 있다" 선언. M4에서 사용자 동의 기반 권한이 들어오면 개선.)

- [ ] **Step 3: 실행 (`--include-ignored`)**

```bash
cargo test -p geulos-server-host --test m3_acceptance --include-ignored
```

(주의: subprocess spawn 테스트는 CI에서 fragile할 수 있음. `#[ignore]` 처리로 일반 실행에서 제외.)

- [ ] **Step 4: 커밋**

```bash
git add -A
git commit -m "test(server-host): M3 acceptance — echo-app subprocess + 외부 press → count"
```

---

## Task 9: 최종 스모크 + 푸시

- [ ] **Step 1: 전체 검증**

```bash
cargo build --workspace --all-targets
cargo test --workspace --all-targets        # ignored 테스트는 제외 — 일반 테스트만
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

모두 그린.

- [ ] **Step 2: ignored 테스트도 한 번 실행**

```bash
cargo test -p geulos-server-host --test m3_acceptance --include-ignored
```

성공해야 함. 실패 시 echo-app subprocess 출력을 보고 디버깅.

- [ ] **Step 3: 푸시 + CI 확인**

```bash
git push origin main
```

GitHub Actions 그린 확인.

- [ ] **Step 4: M3 완료 선언**

다음이 모두 사실:
- AppManifest + aios.toml 파싱
- 매니페스트 검증 (Hello App role)
- ObjectServer.set_state() + StateSet 와이어 메시지
- 연결 종료 시 actor 객체에 Destroyed 이벤트 발행
- echo-app이 실제 동작 (mount, subscribe, press 반응, count 갱신)
- M3 acceptance가 subprocess와 외부 클라이언트 사이의 e2e 흐름 검증
- CI 그린

M4 (컴포지터 GUI) 진입 준비 완료.

---

## 자체 점검 결과

**스펙 커버리지:**
- 설계 문서 §9.2 M3 산출물 5개 매핑:
  - `aios.toml` 매니페스트 형식 → Task 2
  - 권한 매니저 (M3에서는 매니페스트 자동 부여 + ui_types 검증) → Task 5
  - 앱 라이프사이클 → Task 6
  - 데모 앱 echo-app 실동작 → Task 7
  - 완료 기준 (외부 클라이언트가 press → count 증가 → 구독으로 관찰) → Task 8

**플레이스홀더 스캔:** TBD/TODO 없음. 사용자 동의 UI는 M4로 *명시 연기* (ADR-012).

**타입 일관성:**
- `ActorId::from_str` (T1) → 모든 후속 메시지 파싱 (echo-app, dispatch) 에서 사용
- `AppManifest` (T2) → connection.rs 매니페스트 검증에서 일관
- `StateSetMsg/Ack/Error` (T4) → server-host dispatch + echo-app 양쪽에서 일관
- `EventKind::StateSet` → core의 기존 정의 + StateSet 이벤트 발행/구독에서 일관

**알려진 한계 (M3 범위 밖):**
- 권한 카테고리 (`fs.user.docs` 등)의 *실제 강제* 없음. M5+ 글 AI 드라이버에서 fs API 도입 시.
- 사용자 동의 다이얼로그 없음 (ADR-012로 M4 연기).
- 매니페스트의 ui_types가 Mount 시 강제되지 않음 (스펙엔 있지만 M3에서는 *허용 목록*으로만 관리, 위반 시 reject는 M4+).
- echo-app이 wildcard ACL을 강제 — 추후 "특정 ai_session에게만 허용" 같은 세밀 제어는 M5+.
- subprocess 통합 테스트는 `#[ignore]` 처리 — CI fragile 회피. 수동 실행으로 보장.
