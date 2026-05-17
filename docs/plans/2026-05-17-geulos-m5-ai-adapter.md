# GeulOS M5 — AI 어댑터 인프라 (재배치된 M5) 실행 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to execute task-by-task. **NEVER push** — controller batches push at end.

**Goal:** *AI가 GeulOS의 1급 사용자가 되는* 인프라. `glue-ai` 크레이트가 LLM API(Claude 우선)와 GeulOS 와이어 프로토콜을 연결하는 어댑터·세션 매니저로 진화. ai-probe Python 프로토타입이 입증한 패턴을 Rust 프로덕션 라이브러리로 승격.

**Why this is different from the original M5 plan:**

원래 M5(설계 §9.2)는 *글 VM 임베드*가 핵심이었음. 그러나 2026-05-17 ai-probe 검증(`docs/review/2026-05-17-probe-results.md`)에서 **LLM이 와이어 프로토콜을 직접 매우 잘 다룬다**는 결정적 증거 도출. 따라서:

- *글 VM*은 *AI 필수 글루가 아니라 사람용 매크로 매체*로 재정의 (ADR-004 본문 보강 필요)
- M5의 본질이 *"AI ↔ GeulOS 어댑터 + 세션 인프라"*로 이동
- 글 VM 임베드(`Glscript` 메시지의 실제 실행)는 **M5.5로 연기** — 글 측 G1~G4 마일스톤 완료 후 추가 plan

**M5 deliverables:**
- `glue-ai` 크레이트가 library + binary로 본격화 (현재 placeholder 7줄)
- LLM 어댑터 trait + Claude REST 어댑터 (공식 Rust SDK 없음 → reqwest 사용)
- GeulOS 와이어 클라이언트 (probe.py의 Rust 버전, subscribe + drain까지)
- AI 세션 매니저 (토큰 scope, opcode/token/wall-clock 예산, 감사 로그)
- 시나리오 파일 형식 + runner
- `geulosd`의 `Glscript` 핸들러는 *지속 stub* (NotImplemented with M5.5 reference)
- E2E 인수 테스트: 실제 또는 모킹된 Claude가 echo-app을 자율 조작

**Architecture:**
```
사용자
  │
  ▼
geulos-glue-ai (바이너리)
  │
  ├─── glue_ai::LlmAdapter (trait)
  │       └── ClaudeAdapter (reqwest + ANTHROPIC_API_KEY)
  │           ⤴ HTTPS to api.anthropic.com
  │
  ├─── glue_ai::WireClient
  │       ⤴ TCP to 127.0.0.1:5550 (server-host)
  │
  ├─── glue_ai::Tools (Claude tool definitions + dispatch)
  │       ↳ list_objects_by_type / get_object / invoke_method
  │       ↳ set_state / subscribe / drain / unsubscribe / report_done
  │
  └─── glue_ai::Session (예산 + 감사 로그 + 대화 이력)
```

ADR-005 ("AI는 GeulOS에 결합되지 않음")의 자연스러운 구현체. glue-ai는 *별 프로세스*이며 server-host의 와이어 프로토콜을 *외부 클라이언트로* 사용. server-host는 AI 존재를 모름.

**Tech Stack:**
- `reqwest` 0.12 (Claude REST API)
- `tokio` (이미 존재)
- `serde` / `serde_json` (이미)
- `anyhow` 또는 `thiserror` (에러)
- `tracing` 또는 `log` (감사)

**Selection criteria (완료 조건):**
- `cargo build --workspace --all-targets` 그린, 경고 0
- `cargo test --workspace` 전체 그린 (M0~M4 + KI 회귀 + 신규 glue-ai 테스트)
- `cargo run -p geulos-glue-ai -- run scenarios/05_create_button.toml` 형식의 시나리오 1개가 실제 Claude API로 동작 (사용자가 ANTHROPIC_API_KEY 제공 시)
- 또는 모킹된 LLM 어댑터로 E2E 통합 테스트 그린
- Glscript 메시지는 server-host에서 *명확한 NotImplemented 응답* (M5.5 가이드 포함)
- CI 그린

---

## ADR 시드

- **ADR-015 — M5 재배치: AI 어댑터 인프라가 1차, 글 VM은 M5.5로 연기.** 근거: ai-probe 결과 (LLM의 와이어 프로토콜 직접 사용 가능성 입증). 글 언어의 역할은 *사람용 매크로 매체*로 재정의.

---

## 파일 구조 (사전 매핑)

```
glue-ai/
├── Cargo.toml                     # 본격 deps (reqwest, anyhow, tracing)
├── src/
│   ├── lib.rs                     # 모듈 노출
│   ├── main.rs                    # geulos-glue-ai 바이너리 진입
│   ├── adapter/
│   │   ├── mod.rs                 # LlmAdapter trait
│   │   ├── claude.rs              # Claude REST 어댑터
│   │   └── mock.rs                # 테스트용 결정론 어댑터
│   ├── wire.rs                    # GeulOS 와이어 클라이언트 (probe의 Rust 버전)
│   ├── tools.rs                   # Claude tool 정의 + dispatch
│   ├── session.rs                 # 세션 매니저 + 예산 + 감사
│   ├── scenario.rs                # 시나리오 파일 형식 + runner
│   └── error.rs                   # 에러 타입
└── tests/
    ├── wire_client_test.rs        # WireClient 단위 (server-host와 통신)
    ├── tools_test.rs              # 도구 dispatch 단위
    ├── session_with_mock_test.rs  # 결정론 mock 어댑터로 세션 회귀
    └── m5_acceptance.rs           # E2E (옵션: ANTHROPIC_API_KEY 있을 때 실제)
scenarios/                         # 새 디렉터리 (glue-ai/scenarios/)
├── 05_create_button.toml
├── 06_count_to_5.toml
└── 07_observe_state.toml          # subscribe + drain 검증
```

---

## Task 1: ADR-015 + glue-ai 크레이트 본격 시작

**Files:**
- Create: `docs/adr/015-m5-rebalanced.md`
- Modify: `glue-ai/Cargo.toml` (deps 본격)
- Modify: 루트 `Cargo.toml` (`[workspace.dependencies]`에 reqwest 추가)
- Create: `glue-ai/src/lib.rs` (현재는 binary-only)
- Modify: `glue-ai/src/main.rs` (lib 모듈 사용하도록 정리)

- [ ] **Step 1: ADR-015 작성**

`docs/adr/015-m5-rebalanced.md`:

```markdown
# ADR-015: M5 재배치 — AI 어댑터 인프라가 1차, 글 VM은 M5.5로 연기

- **상태:** Accepted
- **일자:** 2026-05-17
- **결정자:** wwoosshh

## 맥락

설계 §9.2의 원래 M5 산출물 중심은 *글 바이트코드 VM 임베드*였다. 가정: *"AI는 와이어 프로토콜을 직접 다루기 어렵다. 따라서 글이 자연어 글루 역할을 한다."*

2026-05-17 ai-probe(`tools/ai-probe`) 4개 시나리오 실행으로 이 가정이 *틀렸음*이 입증되었다 (`docs/review/2026-05-17-probe-results.md`):

- Claude Sonnet 4.6이 와이어 프로토콜을 *학습 자료 없이도* 4/4 시나리오 통과
- UUID/타입 URI/메서드 시그니처 처리 0건 실수
- Parallel tool use 자동 활용
- 자유 탐색 시나리오에서 GeulOS 디자인을 *정확히 추상화*해 보고

따라서 M5의 *최우선* 가치 제공은:
1. 와이어 프로토콜을 직접 다루는 AI 어댑터 인프라 (현재 ai-probe Python 프로토타입의 Rust 프로덕션 버전)
2. 다중 LLM 백엔드 지원 (Claude/OpenAI/Ollama)
3. 세션 관리, 토큰 scope, 감사 로그

가 되어야 한다.

글 VM 임베드는 *여전히 가치 있지만* 다음 두 가지 이유로 *분리 가능*:
- AI는 글 없이도 잘 작동 (probe 결과)
- 글 측 G1~G4 (바이트코드 VM, 호스트 함수 ABI, 안전 모드) 진척이 별 프로젝트의 일정에 묶임

## 결정

- **M5** = AI 어댑터 인프라. `glue-ai` 크레이트가 library + binary로 본격화. Claude REST 어댑터, 와이어 클라이언트, 세션 매니저, 시나리오 runner.
- **M5.5** = 글 VM 임베드. 글 G1~G4 완료 시점에 별 plan 작성. Glscript 와이어 메시지의 실제 실행 path.
- M5 동안 `Glscript` 와이어 메시지는 *지속 NotImplemented* (server-host에서 명확한 에러 응답 + M5.5 가이드).

## 결과

### 긍정적

- M5 완료가 *글 G 시리즈에 묶이지 않음* — 독립 진행 가능
- AI 1급 사용성을 *프로덕션 품질*로 보장 (ai-probe는 실험 도구)
- 다중 LLM 지원으로 *AI-agnostic* 약속 (ADR-005) 강화
- 시나리오 파일 형식이 *재현 가능한 회귀 테스트*의 기반

### 부정적

- "글 언어로 작동하는 OS"라는 강력한 *내러티브 일부 약화*
- 글 측 진척이 GeulOS의 *마케팅 메시지*에 미치는 영향 분리 필요

### 중립

- 글 언어의 진짜 가치(*사람-AI 공유 매체*, *AppleScript 후계자*)는 README와 ADR-004에서 이미 강조되어 있음

## 대안 검토

- **원래대로 글 VM 우선:** ai-probe 결과를 무시하는 셈. 8주 작업이 G 시리즈 의존으로 6개월 지연될 위험.
- **글 VM과 어댑터 인프라 병행:** scope이 16주로 부풀어 솔로 일정 비현실적.
- **글 VM만 (어댑터 X):** AI 어댑터가 없으면 GeulOS는 *원격 호출 가능한 객체 서버*에 그침. 가치 제안 약함.

## 참고

- ai-probe 결과 보고서: `docs/review/2026-05-17-probe-results.md`
- 설계 §9.2 (원래 M5)
- ADR-004 (글 언어 역할), ADR-005 (AI 결합 안 함)
```

- [ ] **Step 2: workspace.dependencies에 reqwest 추가**

루트 `Cargo.toml`의 `[workspace.dependencies]`에 추가:

```toml
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
anyhow = "1.0"
```

(`rustls-tls`로 native-tls 빌드 부담 회피.)

- [ ] **Step 3: `glue-ai/Cargo.toml` 본격화**

```toml
[package]
name = "geulos-glue-ai"
version = "0.0.1"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "GeulOS glue-AI driver: AI adapter (Claude/etc.) + wire client + session manager"

[[bin]]
name = "geulos-glue-ai"
path = "src/main.rs"

[lib]
name = "geulos_glue_ai"
path = "src/lib.rs"

[dependencies]
geulos-core = { path = "../core" }
geulos-proto = { path = "../proto" }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = "1.0"
reqwest = { workspace = true }
anyhow = { workspace = true }
thiserror = "1.0"
toml = { workspace = true }
uuid = { workspace = true }

[dev-dependencies]
proptest = { workspace = true }
```

- [ ] **Step 4: `glue-ai/src/lib.rs` 생성**

```rust
//! GeulOS glue-AI 드라이버.
//!
//! AI 어댑터(Claude 등) + GeulOS 와이어 클라이언트 + 세션 매니저.
//! M5 plan §1.0 참고.

pub mod adapter;
pub mod error;
pub mod scenario;
pub mod session;
pub mod tools;
pub mod wire;

pub use adapter::{LlmAdapter, LlmResponse, LlmStop, ToolUse};
pub use error::{GlueError, GlueResult};
pub use scenario::{Scenario, ScenarioResult};
pub use session::{Session, SessionBudget, SessionOutcome};
pub use wire::WireClient;
```

각 모듈은 후속 태스크에서 채움. 지금은 stub.

- [ ] **Step 5: 모듈 stub 생성**

`glue-ai/src/{adapter/mod.rs, wire.rs, tools.rs, session.rs, scenario.rs, error.rs}`:

```rust
//! (Task N에서 구현)
```

`glue-ai/src/adapter/mod.rs`는 placeholder로:

```rust
//! LLM 어댑터 trait + 구현체 (Task 3에서 구현).
pub struct LlmAdapter;
pub struct LlmResponse;
pub enum LlmStop { EndTurn, ToolUse }
pub struct ToolUse;
```

`glue-ai/src/error.rs`:

```rust
//! glue-ai 에러 타입.
use thiserror::Error;
pub type GlueResult<T> = Result<T, GlueError>;
#[derive(Debug, Error)]
pub enum GlueError {
    #[error("not implemented yet")]
    NotImplemented,
}
```

`glue-ai/src/{session.rs, scenario.rs}`도 비슷한 minimal stub (Task에서 채울 type alias):

```rust
// session.rs
pub struct Session;
pub struct SessionBudget;
pub struct SessionOutcome;

// scenario.rs
pub struct Scenario;
pub struct ScenarioResult;

// wire.rs
pub struct WireClient;
```

- [ ] **Step 6: `glue-ai/src/main.rs` 임시 업데이트 (라이브러리 사용 표시)**

```rust
//! geulos-glue-ai 바이너리.
//!
//! Task 7에서 본격 구현. 지금은 빌드 통과용.

fn main() {
    println!("geulos-glue-ai (M5 in progress — Task 7에서 본격 구현)");
}
```

- [ ] **Step 7: 빌드 + 커밋**

```bash
cargo build -p geulos-glue-ai
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "build(glue-ai): M5 재배치 + 크레이트 본격 시작 (ADR-015)"
```

(아직 푸시 안 함 — controller가 마지막에 일괄.)

---

## Task 2: WireClient — probe.py의 Rust 버전

**Files:**
- Modify: `glue-ai/src/wire.rs`
- Create: `glue-ai/tests/wire_client_test.rs`

- [ ] **Step 1: TDD — 실패 테스트 작성**

`glue-ai/tests/wire_client_test.rs`:

```rust
use geulos_core::{std_types, ActorId};
use geulos_glue_ai::WireClient;
use geulos_proto::EventKindFilterWire;
use geulos_server_host::run_listener;

async fn spawn_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));
    addr.to_string()
}

#[tokio::test]
async fn connect_as_ai_returns_session() {
    let addr = spawn_server().await;
    let client = WireClient::connect_as_ai(&addr).await.unwrap();
    assert!(client.actor_id().starts_with("ai:"));
}

#[tokio::test]
async fn query_by_type_returns_object_ids() {
    let addr = spawn_server().await;

    // 별 클라가 mount
    let mut mounter = WireClient::connect_as_ai(&addr).await.unwrap();
    let txt = std_types::text(ActorId::local_user(), "hi");
    let txt_id = txt.id;
    mounter.mount(txt).await.unwrap();

    // ai로 query
    let mut client = WireClient::connect_as_ai(&addr).await.unwrap();
    let ids = client.query_by_type("aios.std/Text@1").await.unwrap();
    assert!(ids.iter().any(|s| s == &txt_id.to_string()));
}

#[tokio::test]
async fn get_object_returns_full_data() {
    let addr = spawn_server().await;

    let mut mounter = WireClient::connect_as_ai(&addr).await.unwrap();
    let btn = std_types::button(ActorId::local_user(), "OK");
    let btn_id = btn.id.to_string();
    mounter.mount(btn).await.unwrap();

    let mut client = WireClient::connect_as_ai(&addr).await.unwrap();
    let val = client.get_object(&btn_id).await.unwrap();
    assert_eq!(val["type_uri"], "aios.std/Button@1");
}

#[tokio::test]
async fn invoke_method_against_wildcard_acl_succeeds() {
    let addr = spawn_server().await;

    // owner가 wildcard ACL 버튼 mount
    let mut mounter = WireClient::connect_as_ai(&addr).await.unwrap();
    let mut btn = std_types::button(ActorId::local_user(), "OK");
    btn.acl.push(geulos_core::AclEntry {
        actor: geulos_core::ActorPattern::Wildcard,
        method: geulos_core::MethodPattern::Wildcard,
        effect: geulos_core::AclEffect::Allow,
    });
    let btn_id = btn.id.to_string();
    mounter.mount(btn).await.unwrap();

    let mut client = WireClient::connect_as_ai(&addr).await.unwrap();
    let event_id =
        client.invoke(&btn_id, "press", serde_json::Value::Null).await.unwrap();
    assert!(event_id.starts_with("ev:"));
}

#[tokio::test]
async fn subscribe_and_drain_receive_event() {
    let addr = spawn_server().await;

    // mount
    let mut mounter = WireClient::connect_as_ai(&addr).await.unwrap();
    let mut btn = std_types::button(ActorId::local_user(), "OK");
    btn.acl.push(geulos_core::AclEntry {
        actor: geulos_core::ActorPattern::Wildcard,
        method: geulos_core::MethodPattern::Wildcard,
        effect: geulos_core::AclEffect::Allow,
    });
    let btn_id = btn.id.to_string();
    mounter.mount(btn).await.unwrap();

    // subscribe
    let mut sub_client = WireClient::connect_as_ai(&addr).await.unwrap();
    let sub_id = sub_client.subscribe(&btn_id, &[EventKindFilterWire::Invoke]).await.unwrap();

    // 별 클라가 press
    let mut invoker = WireClient::connect_as_ai(&addr).await.unwrap();
    invoker.invoke(&btn_id, "press", serde_json::Value::Null).await.unwrap();

    // 100~200ms 대기 후 drain
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    let events = sub_client.drain(&sub_id).await.unwrap();
    assert!(!events.is_empty(), "expected at least one event after press");
}
```

- [ ] **Step 2: `glue-ai/src/wire.rs` 본격 구현**

```rust
//! GeulOS 와이어 클라이언트 (probe.py의 Rust 버전).
//!
//! 길이 접두사 JSON 프레임을 TCP로 송수신. `connect_as_ai`로 핸드셰이크 후
//! query/get/invoke/subscribe/drain/unsubscribe/mount 메서드 사용.

use geulos_core::{Object, ObjectId};
use geulos_proto::{
    decode_frame, encode_frame, EventKindFilterWire, GetMsg, GetResult, Hello, HelloAck,
    InvokeAck, InvokeMsg, MountAck, MountMsg, QueryMsg, QueryPredicate, QueryResult, Role,
    SubscribeAck, SubscribeMsg, UnsubscribeMsg,
};
use serde_json::Value;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use uuid::Uuid;

/// 와이어 클라이언트 에러.
#[derive(Debug, Error)]
pub enum WireError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unexpected response (got {got}, want {want})")]
    UnexpectedKind { want: String, got: String },
    #[error("server error: {kind} — {detail}")]
    ServerError { kind: String, detail: String },
    #[error("connection closed unexpectedly")]
    Closed,
}

pub type WireResult<T> = Result<T, WireError>;

/// GeulOS server-host에 TCP로 접속한 한 클라이언트.
pub struct WireClient {
    stream: TcpStream,
    actor_id: String,
    accum: Vec<u8>,
}

impl WireClient {
    /// `Role::Ai`로 핸드셰이크 + HelloAck 수신.
    pub async fn connect_as_ai(addr: &str) -> WireResult<Self> {
        let mut stream = TcpStream::connect(addr).await?;
        let mut accum: Vec<u8> = Vec::new();
        let hello = Hello {
            version: "0.1".to_string(),
            role: Role::Ai,
            auth: Value::Object(Default::default()),
            client_id: "glue-ai".to_string(),
        };
        let body = serde_json::to_vec(&hello)?;
        stream.write_all(&encode_frame(&body)).await?;

        // HelloAck 수신
        let mut buf = vec![0u8; 4096];
        loop {
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                return Err(WireError::Closed);
            }
            accum.extend_from_slice(&buf[..n]);
            let mut slice = accum.as_slice();
            if let Ok(body) = decode_frame(&mut slice) {
                let consumed = accum.len() - slice.len();
                accum.drain(..consumed);
                let ack: HelloAck = serde_json::from_slice(&body)?;
                return Ok(Self { stream, actor_id: ack.actor_id, accum });
            }
        }
    }

    /// 발급된 actor id.
    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }

    /// 한 프레임 송신 + 한 프레임 수신.
    async fn request(&mut self, msg: &Value) -> WireResult<Value> {
        let body = serde_json::to_vec(msg)?;
        self.stream.write_all(&encode_frame(&body)).await?;
        self.read_frame_json().await
    }

    /// 한 프레임 수신 (대기).
    async fn read_frame_json(&mut self) -> WireResult<Value> {
        let mut buf = vec![0u8; 4096];
        loop {
            let mut slice = self.accum.as_slice();
            if let Ok(body) = decode_frame(&mut slice) {
                let consumed = self.accum.len() - slice.len();
                self.accum.drain(..consumed);
                return Ok(serde_json::from_slice(&body)?);
            }
            let n = self.stream.read(&mut buf).await?;
            if n == 0 {
                return Err(WireError::Closed);
            }
            self.accum.extend_from_slice(&buf[..n]);
        }
    }

    /// Mount 객체.
    pub async fn mount(&mut self, obj: Object) -> WireResult<ObjectId> {
        let msg = MountMsg {
            root_object_id: obj.id.to_string(),
            tree: serde_json::to_value(&obj)?,
        };
        let resp = self.request(&serde_json::to_value(&msg)?).await?;
        match resp.get("kind").and_then(|v| v.as_str()) {
            Some("MountAck") => {
                let _ack: MountAck = serde_json::from_value(resp)?;
                Ok(obj.id)
            }
            Some("MountReject") => Err(WireError::ServerError {
                kind: "mount_reject".to_string(),
                detail: resp.get("detail").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            }),
            other => Err(WireError::UnexpectedKind {
                want: "MountAck".to_string(),
                got: other.unwrap_or("?").to_string(),
            }),
        }
    }

    /// Query by type. 객체 ID 문자열 목록 반환.
    pub async fn query_by_type(&mut self, type_uri: &str) -> WireResult<Vec<String>> {
        let q = QueryMsg {
            request_id: format!("q-{}", Uuid::new_v4()),
            query: QueryPredicate::ByType { type_uri: type_uri.to_string() },
        };
        let resp = self.request(&serde_json::to_value(&q)?).await?;
        let r: QueryResult = serde_json::from_value(resp)?;
        Ok(r.objects)
    }

    /// Get object — JSON value 반환.
    pub async fn get_object(&mut self, object_id: &str) -> WireResult<Value> {
        let g = GetMsg {
            request_id: format!("g-{}", Uuid::new_v4()),
            target: object_id.to_string(),
        };
        let resp = self.request(&serde_json::to_value(&g)?).await?;
        match resp.get("kind").and_then(|v| v.as_str()) {
            Some("GetResult") => {
                let r: GetResult = serde_json::from_value(resp)?;
                Ok(r.object)
            }
            _ => Err(WireError::ServerError {
                kind: resp.get("error_kind").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
                detail: resp.get("detail").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            }),
        }
    }

    /// Invoke. event_id 문자열 반환.
    pub async fn invoke(&mut self, target: &str, method: &str, args: Value) -> WireResult<String> {
        let i = InvokeMsg {
            request_id: format!("i-{}", Uuid::new_v4()),
            target: target.to_string(),
            method: method.to_string(),
            args,
        };
        let resp = self.request(&serde_json::to_value(&i)?).await?;
        match resp.get("kind").and_then(|v| v.as_str()) {
            Some("InvokeAck") => {
                let a: InvokeAck = serde_json::from_value(resp)?;
                Ok(a.event_id)
            }
            _ => Err(WireError::ServerError {
                kind: resp.get("error_kind").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
                detail: resp.get("detail").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            }),
        }
    }

    /// Subscribe. subscription_id 반환.
    pub async fn subscribe(
        &mut self,
        target: &str,
        kinds: &[EventKindFilterWire],
    ) -> WireResult<String> {
        let sid = format!("sub-{}", Uuid::new_v4());
        let s = SubscribeMsg {
            subscription_id: sid.clone(),
            target: target.to_string(),
            kinds: kinds.to_vec(),
            include_initial: false,
        };
        let resp = self.request(&serde_json::to_value(&s)?).await?;
        let _ack: SubscribeAck = serde_json::from_value(resp)?;
        Ok(sid)
    }

    /// Drain — 큐에 쌓인 이벤트가 있다면 *지금* 도착한 것까지 모두 수집. 없으면 빈 vec.
    /// (서버는 이벤트를 push해두므로, 이 호출은 *수신 버퍼 비우기*.)
    pub async fn drain(&mut self, _subscription_id: &str) -> WireResult<Vec<Value>> {
        let mut buf = vec![0u8; 4096];
        let mut events = Vec::new();
        // non-blocking poll: short timeout
        let timeout = std::time::Duration::from_millis(100);
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            // 이미 accum에 있는 메시지 먼저 추출
            loop {
                let mut slice = self.accum.as_slice();
                match decode_frame(&mut slice) {
                    Ok(body) => {
                        let consumed = self.accum.len() - slice.len();
                        self.accum.drain(..consumed);
                        let v: Value = serde_json::from_slice(&body)?;
                        if v.get("kind").and_then(|k| k.as_str()) == Some("Event") {
                            events.push(v);
                        }
                    }
                    Err(_) => break,
                }
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            // 더 받아보기
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let r = tokio::time::timeout(remaining, self.stream.read(&mut buf)).await;
            match r {
                Ok(Ok(0)) => return Err(WireError::Closed),
                Ok(Ok(n)) => self.accum.extend_from_slice(&buf[..n]),
                Ok(Err(e)) => return Err(WireError::Io(e)),
                Err(_) => break, // timeout
            }
        }
        Ok(events)
    }

    /// Unsubscribe.
    pub async fn unsubscribe(&mut self, subscription_id: &str) -> WireResult<()> {
        let u = UnsubscribeMsg { subscription_id: subscription_id.to_string() };
        // Unsubscribe는 응답이 없으므로 send만
        let body = serde_json::to_vec(&u)?;
        self.stream.write_all(&encode_frame(&body)).await?;
        Ok(())
    }
}
```

- [ ] **Step 3: 테스트 통과 확인**

```bash
cargo test -p geulos-glue-ai --test wire_client_test
```

5개 모두 PASS 기대.

- [ ] **Step 4: 커밋**

```bash
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "feat(glue-ai): WireClient — query/get/invoke/subscribe/drain/mount/unsubscribe"
```

---

## Task 3: LlmAdapter trait + Claude REST 어댑터

**Files:**
- Modify: `glue-ai/src/adapter/mod.rs` (trait)
- Create: `glue-ai/src/adapter/claude.rs`
- Create: `glue-ai/src/adapter/mock.rs`

- [ ] **Step 1: `glue-ai/src/adapter/mod.rs` 본격 구현**

```rust
//! LLM 어댑터 추상.
//!
//! 다중 백엔드 (Claude / OpenAI / Ollama) 지원을 위한 trait. 첫 구현은 Claude.

use async_trait::async_trait;
use serde_json::Value;

pub mod claude;
pub mod mock;

pub use claude::ClaudeAdapter;
pub use mock::MockAdapter;

/// LLM의 한 response.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    /// 텍스트 출력 (있다면).
    pub text: Vec<String>,
    /// 도구 호출 요청 (있다면).
    pub tool_uses: Vec<ToolUse>,
    /// 모델이 왜 멈췄나.
    pub stop: LlmStop,
    /// 토큰 사용량 (input, output).
    pub tokens: (u64, u64),
}

/// 도구 호출 요청 한 건.
#[derive(Debug, Clone)]
pub struct ToolUse {
    /// LLM이 발급한 고유 ID (응답 매칭용).
    pub id: String,
    /// 도구 이름.
    pub name: String,
    /// 도구 인자 (JSON).
    pub input: Value,
}

/// 모델 종료 이유.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmStop {
    EndTurn,
    ToolUse,
    MaxTokens,
    Other,
}

/// 대화 메시지 (LLM과 주고받는 한 단위).
#[derive(Debug, Clone)]
pub struct LlmMessage {
    pub role: LlmRole,
    /// 본문 — 텍스트 OR 도구 결과들 OR 모델의 도구 호출들.
    pub content: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmRole {
    User,
    Assistant,
}

/// 도구 정의 (Claude의 tool 형식).
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// LLM 어댑터 trait.
#[async_trait]
pub trait LlmAdapter: Send + Sync {
    /// 한 메시지 round-trip — system + history + tools → response.
    async fn complete(
        &self,
        system: &str,
        history: &[LlmMessage],
        tools: &[ToolDef],
    ) -> Result<LlmResponse, crate::GlueError>;
}
```

- [ ] **Step 2: `glue-ai/src/adapter/claude.rs` 구현**

```rust
//! Claude REST 어댑터.
//!
//! 공식 Rust SDK가 없으므로 reqwest로 직접 호출.
//! API: https://docs.anthropic.com/en/api/messages

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use super::{LlmAdapter, LlmMessage, LlmResponse, LlmRole, LlmStop, ToolDef, ToolUse};
use crate::error::{GlueError, GlueResult};

const CLAUDE_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct ClaudeAdapter {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: u32,
}

impl ClaudeAdapter {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
            model: model.into(),
            max_tokens: 2048,
        }
    }

    pub fn from_env(model: impl Into<String>) -> GlueResult<Self> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| GlueError::Config("ANTHROPIC_API_KEY not set".to_string()))?;
        Ok(Self::new(key, model))
    }

    pub fn with_max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = n;
        self
    }
}

#[async_trait]
impl LlmAdapter for ClaudeAdapter {
    async fn complete(
        &self,
        system: &str,
        history: &[LlmMessage],
        tools: &[ToolDef],
    ) -> GlueResult<LlmResponse> {
        let messages_json: Vec<Value> = history
            .iter()
            .map(|m| {
                let role = match m.role {
                    LlmRole::User => "user",
                    LlmRole::Assistant => "assistant",
                };
                json!({ "role": role, "content": m.content })
            })
            .collect();

        let tools_json: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();

        let body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": system,
            "messages": messages_json,
            "tools": tools_json,
        });

        let resp = self.client
            .post(CLAUDE_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| GlueError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let txt = resp.text().await.unwrap_or_default();
            return Err(GlueError::ApiError {
                status: status.as_u16(),
                detail: txt,
            });
        }

        let json: Value = resp.json().await.map_err(|e| GlueError::Network(e.to_string()))?;
        parse_claude_response(json)
    }
}

fn parse_claude_response(json: Value) -> GlueResult<LlmResponse> {
    let stop_str = json.get("stop_reason").and_then(|v| v.as_str()).unwrap_or("");
    let stop = match stop_str {
        "end_turn" => LlmStop::EndTurn,
        "tool_use" => LlmStop::ToolUse,
        "max_tokens" => LlmStop::MaxTokens,
        _ => LlmStop::Other,
    };

    let usage = json.get("usage");
    let in_tokens = usage.and_then(|u| u.get("input_tokens")).and_then(|v| v.as_u64()).unwrap_or(0);
    let out_tokens = usage.and_then(|u| u.get("output_tokens")).and_then(|v| v.as_u64()).unwrap_or(0);

    let mut text = Vec::new();
    let mut tool_uses = Vec::new();
    if let Some(content) = json.get("content").and_then(|v| v.as_array()) {
        for block in content {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                        text.push(t.to_string());
                    }
                }
                "tool_use" => {
                    tool_uses.push(ToolUse {
                        id: block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        name: block.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        input: block.get("input").cloned().unwrap_or(json!({})),
                    });
                }
                _ => {}
            }
        }
    }

    Ok(LlmResponse { text, tool_uses, stop, tokens: (in_tokens, out_tokens) })
}
```

- [ ] **Step 3: `glue-ai/src/adapter/mock.rs` 구현 (테스트용)**

```rust
//! 결정론 MockAdapter — 테스트 전용.

use async_trait::async_trait;
use std::sync::Mutex;

use super::{LlmAdapter, LlmMessage, LlmResponse, LlmStop, ToolDef};
use crate::error::GlueResult;

/// 미리 정해진 응답을 순서대로 반환하는 어댑터.
pub struct MockAdapter {
    responses: Mutex<std::collections::VecDeque<LlmResponse>>,
}

impl MockAdapter {
    pub fn new(responses: Vec<LlmResponse>) -> Self {
        Self { responses: Mutex::new(responses.into_iter().collect()) }
    }
}

#[async_trait]
impl LlmAdapter for MockAdapter {
    async fn complete(
        &self,
        _system: &str,
        _history: &[LlmMessage],
        _tools: &[ToolDef],
    ) -> GlueResult<LlmResponse> {
        let mut q = self.responses.lock().unwrap();
        q.pop_front()
            .ok_or_else(|| crate::error::GlueError::Config("mock exhausted".to_string()))
    }
}
```

- [ ] **Step 4: `error.rs` 보강**

```rust
use thiserror::Error;
pub type GlueResult<T> = Result<T, GlueError>;

#[derive(Debug, Error)]
pub enum GlueError {
    #[error("config: {0}")]
    Config(String),
    #[error("network: {0}")]
    Network(String),
    #[error("api error {status}: {detail}")]
    ApiError { status: u16, detail: String },
    #[error("wire: {0}")]
    Wire(#[from] crate::wire::WireError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("budget exhausted: {0}")]
    BudgetExhausted(String),
}
```

- [ ] **Step 5: `async_trait` 추가**

`glue-ai/Cargo.toml`의 `[dependencies]`에:

```toml
async-trait = "0.1"
```

- [ ] **Step 6: 빌드 + 커밋**

```bash
cargo build -p geulos-glue-ai
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "feat(glue-ai): LlmAdapter trait + ClaudeAdapter (REST) + MockAdapter (테스트용)"
```

---

## Task 4: Tools dispatch layer

**Files:**
- Modify: `glue-ai/src/tools.rs`
- Create: `glue-ai/tests/tools_test.rs`

- [ ] **Step 1: `glue-ai/src/tools.rs` 구현**

```rust
//! Claude 도구 정의 + dispatch — probe.py의 TOOLS와 동등.

use serde_json::{json, Value};

use crate::adapter::ToolDef;
use crate::error::{GlueError, GlueResult};
use crate::wire::WireClient;

pub fn standard_tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "list_objects_by_type".to_string(),
            description: "List all object IDs matching a type URI. \
                          Standard: aios.std/Container@1, aios.std/Text@1, \
                          aios.std/Button@1, aios.std/Toggle@1."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "type_uri": {"type": "string"}
                },
                "required": ["type_uri"]
            }),
        },
        ToolDef {
            name: "get_object".to_string(),
            description: "Fetch full details (props, state, methods, ACL) of an object by UUID."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "object_id": {"type": "string"}
                },
                "required": ["object_id"]
            }),
        },
        ToolDef {
            name: "invoke_method".to_string(),
            description: "Invoke a method on an object. Returns event_id or error_kind."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string"},
                    "method": {"type": "string"},
                    "args": {}
                },
                "required": ["target", "method"]
            }),
        },
        ToolDef {
            name: "subscribe".to_string(),
            description: "Subscribe to events on an object. \
                          Kinds: Invoke, StateSet, Lifecycle, ChildChange. \
                          Returns subscription_id."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string"},
                    "kinds": {
                        "type": "array",
                        "items": {"type": "string", "enum": ["Invoke", "StateSet", "Lifecycle", "ChildChange"]}
                    }
                },
                "required": ["target", "kinds"]
            }),
        },
        ToolDef {
            name: "drain".to_string(),
            description: "Drain queued events for a subscription (returns up to ~100ms worth)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "subscription_id": {"type": "string"}
                },
                "required": ["subscription_id"]
            }),
        },
        ToolDef {
            name: "report_done".to_string(),
            description: "Call exactly once when finished. Provide a summary."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "summary": {"type": "string"}
                },
                "required": ["summary"]
            }),
        },
    ]
}

/// 한 도구 호출을 dispatch. report_done은 특별 처리 (Ok(None)으로 반환).
pub async fn dispatch_tool(
    wire: &mut WireClient,
    name: &str,
    input: &Value,
) -> GlueResult<DispatchResult> {
    use geulos_proto::EventKindFilterWire;

    match name {
        "list_objects_by_type" => {
            let t = input.get("type_uri").and_then(|v| v.as_str()).unwrap_or("");
            let ids = wire.query_by_type(t).await?;
            Ok(DispatchResult::Output(json!({ "object_ids": ids })))
        }
        "get_object" => {
            let id = input.get("object_id").and_then(|v| v.as_str()).unwrap_or("");
            match wire.get_object(id).await {
                Ok(obj) => Ok(DispatchResult::Output(json!({ "object": obj }))),
                Err(e) => Ok(DispatchResult::Output(json!({ "error": e.to_string() }))),
            }
        }
        "invoke_method" => {
            let target = input.get("target").and_then(|v| v.as_str()).unwrap_or("");
            let method = input.get("method").and_then(|v| v.as_str()).unwrap_or("");
            let args = input.get("args").cloned().unwrap_or(Value::Null);
            match wire.invoke(target, method, args).await {
                Ok(eid) => Ok(DispatchResult::Output(json!({ "ok": true, "event_id": eid }))),
                Err(e) => Ok(DispatchResult::Output(json!({ "ok": false, "error": e.to_string() }))),
            }
        }
        "subscribe" => {
            let target = input.get("target").and_then(|v| v.as_str()).unwrap_or("");
            let kinds_arr = input.get("kinds").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            let mut kinds = Vec::new();
            for k in &kinds_arr {
                if let Some(s) = k.as_str() {
                    let kf = match s {
                        "Invoke" => EventKindFilterWire::Invoke,
                        "StateSet" => EventKindFilterWire::StateSet,
                        "Lifecycle" => EventKindFilterWire::Lifecycle,
                        "ChildChange" => EventKindFilterWire::ChildChange,
                        _ => continue,
                    };
                    kinds.push(kf);
                }
            }
            let sid = wire.subscribe(target, &kinds).await?;
            Ok(DispatchResult::Output(json!({ "subscription_id": sid })))
        }
        "drain" => {
            let sid = input.get("subscription_id").and_then(|v| v.as_str()).unwrap_or("");
            let events = wire.drain(sid).await?;
            Ok(DispatchResult::Output(json!({ "events": events })))
        }
        "report_done" => {
            let summary = input.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Ok(DispatchResult::Done { summary })
        }
        other => Err(GlueError::Config(format!("unknown tool: {}", other))),
    }
}

#[derive(Debug)]
pub enum DispatchResult {
    Output(Value),
    Done { summary: String },
}
```

- [ ] **Step 2: 테스트 (`glue-ai/tests/tools_test.rs`)**

```rust
use geulos_glue_ai::tools::{dispatch_tool, DispatchResult, standard_tools};
use geulos_glue_ai::WireClient;
use geulos_server_host::run_listener;
use serde_json::json;

#[tokio::test]
async fn standard_tools_includes_seven_functions() {
    let tools = standard_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"list_objects_by_type"));
    assert!(names.contains(&"get_object"));
    assert!(names.contains(&"invoke_method"));
    assert!(names.contains(&"subscribe"));
    assert!(names.contains(&"drain"));
    assert!(names.contains(&"report_done"));
    assert_eq!(tools.len(), 6);
}

#[tokio::test]
async fn report_done_returns_done_variant() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));
    let mut wire = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();

    let r = dispatch_tool(&mut wire, "report_done", &json!({"summary": "done"})).await.unwrap();
    assert!(matches!(r, DispatchResult::Done { .. }));
}

#[tokio::test]
async fn list_objects_returns_output() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));
    let mut wire = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();

    let r = dispatch_tool(&mut wire, "list_objects_by_type",
                          &json!({"type_uri": "aios.std/Text@1"})).await.unwrap();
    match r {
        DispatchResult::Output(v) => {
            assert!(v.get("object_ids").is_some());
        }
        _ => panic!("expected Output"),
    }
}
```

- [ ] **Step 3: 통과 + 커밋**

```bash
cargo test -p geulos-glue-ai --test tools_test
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "feat(glue-ai): Tools dispatch (probe.py와 동등 + subscribe/drain 추가)"
```

---

## Task 5: Session 매니저 + 예산 + 감사

**Files:**
- Modify: `glue-ai/src/session.rs`
- Create: `glue-ai/tests/session_with_mock_test.rs`

- [ ] **Step 1: `glue-ai/src/session.rs` 구현**

```rust
//! AI 세션 매니저 — 한 작업의 처음부터 끝까지.

use std::time::Instant;

use chrono::Utc;
use serde_json::{json, Value};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::adapter::{LlmAdapter, LlmMessage, LlmResponse, LlmRole, LlmStop, ToolDef};
use crate::error::{GlueError, GlueResult};
use crate::tools::{dispatch_tool, DispatchResult, standard_tools};
use crate::wire::WireClient;

/// 세션 예산.
#[derive(Debug, Clone)]
pub struct SessionBudget {
    pub max_turns: usize,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_wall_secs: u64,
}

impl Default for SessionBudget {
    fn default() -> Self {
        Self {
            max_turns: 12,
            max_input_tokens: 200_000,
            max_output_tokens: 8_000,
            max_wall_secs: 120,
        }
    }
}

/// 세션 결과.
#[derive(Debug, Clone)]
pub struct SessionOutcome {
    pub summary: Option<String>,
    pub turns_used: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub wall_secs: f64,
    pub completed: bool, // report_done 호출 여부
}

/// 한 세션 = (adapter, wire, budget, history, audit).
pub struct Session<A: LlmAdapter> {
    pub adapter: A,
    pub wire: WireClient,
    pub system: String,
    pub tools: Vec<ToolDef>,
    pub budget: SessionBudget,
    pub audit_path: Option<std::path::PathBuf>,
}

impl<A: LlmAdapter> Session<A> {
    pub fn new(adapter: A, wire: WireClient, system: String) -> Self {
        Self {
            adapter,
            wire,
            system,
            tools: standard_tools(),
            budget: SessionBudget::default(),
            audit_path: None,
        }
    }

    pub fn with_budget(mut self, b: SessionBudget) -> Self {
        self.budget = b;
        self
    }

    pub fn with_audit(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.audit_path = Some(path.into());
        self
    }

    /// 사용자 작업을 수행. report_done 호출 시 종료, 또는 budget 소진 시 종료.
    pub async fn run_task(&mut self, user_prompt: &str) -> GlueResult<SessionOutcome> {
        let started = Instant::now();
        let mut history: Vec<LlmMessage> = vec![LlmMessage {
            role: LlmRole::User,
            content: Value::String(user_prompt.to_string()),
        }];

        let mut summary: Option<String> = None;
        let mut total_in: u64 = 0;
        let mut total_out: u64 = 0;
        let mut turn: usize = 0;

        self.audit(&format!(
            "=== session start ===\n actor: {}\n prompt: {}\n",
            self.wire.actor_id(),
            user_prompt
        ))
        .await;

        loop {
            turn += 1;
            if turn > self.budget.max_turns {
                self.audit(&format!("=== budget: max_turns ({}) ===", self.budget.max_turns)).await;
                break;
            }
            if started.elapsed().as_secs() >= self.budget.max_wall_secs {
                self.audit(&format!("=== budget: max_wall_secs ({}) ===", self.budget.max_wall_secs)).await;
                break;
            }
            if total_in >= self.budget.max_input_tokens {
                self.audit("=== budget: max_input_tokens ===").await;
                break;
            }
            if total_out >= self.budget.max_output_tokens {
                self.audit("=== budget: max_output_tokens ===").await;
                break;
            }

            self.audit(&format!("\n--- turn {} ---", turn)).await;

            let resp: LlmResponse = self.adapter.complete(&self.system, &history, &self.tools).await?;
            total_in += resp.tokens.0;
            total_out += resp.tokens.1;

            for t in &resp.text {
                self.audit(&format!("text: {}", t)).await;
            }
            for tu in &resp.tool_uses {
                self.audit(&format!(
                    "tool_use: {}({})",
                    tu.name,
                    serde_json::to_string(&tu.input).unwrap_or_default()
                )).await;
            }

            // assistant turn 기록
            history.push(LlmMessage {
                role: LlmRole::Assistant,
                content: response_to_assistant_content(&resp),
            });

            if resp.stop == LlmStop::EndTurn && resp.tool_uses.is_empty() {
                self.audit("=== stopped without tools ===").await;
                break;
            }

            // 도구 dispatch
            let mut tool_results: Vec<Value> = Vec::new();
            let mut done = false;
            for tu in &resp.tool_uses {
                let r = dispatch_tool(&mut self.wire, &tu.name, &tu.input).await;
                match r {
                    Ok(DispatchResult::Output(v)) => {
                        self.audit(&format!("  -> {}", trim(&v))).await;
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tu.id,
                            "content": serde_json::to_string(&v).unwrap_or_default(),
                        }));
                    }
                    Ok(DispatchResult::Done { summary: s }) => {
                        self.audit(&format!("  -> report_done: {}", s)).await;
                        summary = Some(s);
                        done = true;
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tu.id,
                            "content": "ok",
                        }));
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        self.audit(&format!("  -> error: {}", msg)).await;
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tu.id,
                            "content": format!("error: {}", msg),
                            "is_error": true,
                        }));
                    }
                }
            }

            if done {
                break;
            }

            // 다음 user turn = 도구 결과들
            history.push(LlmMessage {
                role: LlmRole::User,
                content: Value::Array(tool_results),
            });
        }

        let wall = started.elapsed().as_secs_f64();
        self.audit(&format!(
            "\n=== session end ===\n turns: {}\n tokens (in/out): {}/{}\n wall: {:.1}s",
            turn, total_in, total_out, wall
        )).await;

        Ok(SessionOutcome {
            summary,
            turns_used: turn,
            input_tokens: total_in,
            output_tokens: total_out,
            wall_secs: wall,
            completed: summary_present(&summary),
        })
    }

    async fn audit(&self, line: &str) {
        let stamped = format!("{} {}\n", Utc::now().to_rfc3339(), line);
        if let Some(path) = &self.audit_path {
            if let Ok(mut f) = File::options().create(true).append(true).open(path).await {
                let _ = f.write_all(stamped.as_bytes()).await;
            }
        }
    }
}

fn response_to_assistant_content(resp: &LlmResponse) -> Value {
    let mut blocks = Vec::new();
    for t in &resp.text {
        blocks.push(json!({ "type": "text", "text": t }));
    }
    for tu in &resp.tool_uses {
        blocks.push(json!({
            "type": "tool_use",
            "id": tu.id,
            "name": tu.name,
            "input": tu.input,
        }));
    }
    Value::Array(blocks)
}

fn summary_present(s: &Option<String>) -> bool {
    s.is_some()
}

fn trim(v: &Value) -> String {
    let s = serde_json::to_string(v).unwrap_or_default();
    if s.len() > 200 {
        format!("{}...", &s[..200])
    } else {
        s
    }
}
```

- [ ] **Step 2: `chrono` deps 추가**

```toml
chrono = "0.4"
```

- [ ] **Step 3: 테스트 — Mock adapter로 세션 실행**

`glue-ai/tests/session_with_mock_test.rs`:

```rust
use geulos_glue_ai::adapter::{LlmResponse, LlmStop, MockAdapter, ToolUse};
use geulos_glue_ai::session::{Session, SessionBudget};
use geulos_glue_ai::WireClient;
use geulos_server_host::run_listener;
use serde_json::json;

#[tokio::test]
async fn session_with_mock_calls_report_done_immediately() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let wire = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();

    // mock이 단 1턴에서 바로 report_done 호출
    let mock = MockAdapter::new(vec![LlmResponse {
        text: vec!["I'll report immediately.".to_string()],
        tool_uses: vec![ToolUse {
            id: "tu-1".to_string(),
            name: "report_done".to_string(),
            input: json!({"summary": "test summary"}),
        }],
        stop: LlmStop::ToolUse,
        tokens: (100, 20),
    }]);

    let mut session = Session::new(mock, wire, "You are a test.".to_string())
        .with_budget(SessionBudget { max_turns: 5, ..Default::default() });

    let outcome = session.run_task("just do it").await.unwrap();
    assert!(outcome.completed);
    assert_eq!(outcome.summary.as_deref(), Some("test summary"));
    assert_eq!(outcome.turns_used, 1);
}

#[tokio::test]
async fn session_respects_max_turns_budget() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let wire = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();

    // mock이 *영원히* list_objects 호출만 함 (report_done 없음)
    let mut responses = Vec::new();
    for i in 0..10 {
        responses.push(LlmResponse {
            text: vec![format!("turn {}", i)],
            tool_uses: vec![ToolUse {
                id: format!("tu-{}", i),
                name: "list_objects_by_type".to_string(),
                input: json!({"type_uri": "aios.std/Text@1"}),
            }],
            stop: LlmStop::ToolUse,
            tokens: (50, 10),
        });
    }
    let mock = MockAdapter::new(responses);

    let mut session = Session::new(mock, wire, "test".to_string())
        .with_budget(SessionBudget { max_turns: 3, ..Default::default() });

    let outcome = session.run_task("loop forever").await.unwrap();
    assert!(!outcome.completed);
    assert_eq!(outcome.turns_used, 4); // 3을 *넘어선* 시점에 break — 한 단계 더 카운트됨
}
```

- [ ] **Step 4: 통과 + 커밋**

```bash
cargo test -p geulos-glue-ai --test session_with_mock_test
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "feat(glue-ai): Session 매니저 + 예산 + 감사 로그"
```

---

## Task 6: 시나리오 파일 형식 + runner

**Files:**
- Modify: `glue-ai/src/scenario.rs`
- Create: `glue-ai/scenarios/05_create_button.toml`
- Create: `glue-ai/scenarios/06_count_to_5.toml`
- Create: `glue-ai/scenarios/07_observe_state.toml`

- [ ] **Step 1: 시나리오 파일 형식 정의**

`glue-ai/scenarios/05_create_button.toml`:

```toml
name = "explore_system"
goal = """
Tell me what objects exist on this GeulOS system. Use list_objects_by_type for each
standard type (Container/Text/Button/Toggle), then get_object on each ID. Summarize
what you found via report_done.
"""
[budget]
max_turns = 8
max_wall_secs = 60
```

`glue-ai/scenarios/06_count_to_5.toml`:

```toml
name = "press_button_5_times"
goal = """
There is a Button on this system. Press it 5 times. After each press, fetch the
nearby Text object and report what it shows. Tell me the progression of values.
"""
[budget]
max_turns = 14
max_wall_secs = 60
```

`glue-ai/scenarios/07_observe_state.toml`:

```toml
name = "observe_via_subscribe"
goal = """
Find a Text object on this system. Subscribe to StateSet events on it. Wait a
little, then drain the subscription. Tell me if any StateSet events arrived.
This tests whether the subscription mechanism actually delivers events.
"""
[budget]
max_turns = 6
max_wall_secs = 30
```

- [ ] **Step 2: `glue-ai/src/scenario.rs` 구현**

```rust
//! 시나리오 파일 형식 + runner.

use std::path::Path;

use serde::Deserialize;

use crate::error::{GlueError, GlueResult};
use crate::session::SessionBudget;

#[derive(Debug, Clone, Deserialize)]
pub struct Scenario {
    pub name: String,
    pub goal: String,
    #[serde(default)]
    pub budget: ScenarioBudget,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScenarioBudget {
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
    #[serde(default = "default_max_wall")]
    pub max_wall_secs: u64,
}

fn default_max_turns() -> usize { 12 }
fn default_max_wall() -> u64 { 120 }

impl Scenario {
    pub fn load(path: impl AsRef<Path>) -> GlueResult<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(GlueError::Io)?;
        let s: Self = toml::from_str(&content)
            .map_err(|e| GlueError::Config(format!("scenario TOML: {}", e)))?;
        Ok(s)
    }

    pub fn to_session_budget(&self) -> SessionBudget {
        SessionBudget {
            max_turns: self.budget.max_turns,
            max_wall_secs: self.budget.max_wall_secs,
            ..Default::default()
        }
    }
}

#[derive(Debug)]
pub struct ScenarioResult {
    pub name: String,
    pub outcome: crate::session::SessionOutcome,
}
```

- [ ] **Step 3: 커밋**

```bash
cargo build -p geulos-glue-ai
git add -A
git commit -m "feat(glue-ai): 시나리오 TOML 형식 + 3개 예제"
```

---

## Task 7: glue-ai 바이너리 — run 모드

**Files:**
- Modify: `glue-ai/src/main.rs`

- [ ] **Step 1: main.rs 본격 구현**

```rust
//! geulos-glue-ai: AI 어댑터 드라이버 바이너리.
//!
//! 사용:
//!   geulos-glue-ai run --scenario scenarios/05_create_button.toml \
//!                      --server 127.0.0.1:5550 \
//!                      --model claude-sonnet-4-6

use std::path::PathBuf;
use std::process::ExitCode;

use geulos_glue_ai::adapter::ClaudeAdapter;
use geulos_glue_ai::scenario::Scenario;
use geulos_glue_ai::session::Session;
use geulos_glue_ai::WireClient;

const DEFAULT_SYSTEM_PROMPT: &str = include_str!("../system_prompt.md");

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 || args[1] != "run" {
        eprintln!("Usage: geulos-glue-ai run --scenario <path> [--server <addr>] [--model <id>]");
        return ExitCode::from(2);
    }

    let mut scenario_path: Option<PathBuf> = None;
    let mut server_addr = "127.0.0.1:5550".to_string();
    let mut model = "claude-sonnet-4-6".to_string();
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--scenario" if i + 1 < args.len() => {
                scenario_path = Some(args[i + 1].clone().into());
                i += 2;
            }
            "--server" if i + 1 < args.len() => {
                server_addr = args[i + 1].clone();
                i += 2;
            }
            "--model" if i + 1 < args.len() => {
                model = args[i + 1].clone();
                i += 2;
            }
            other => {
                eprintln!("unknown arg: {}", other);
                return ExitCode::from(2);
            }
        }
    }
    let scenario_path = match scenario_path {
        Some(p) => p,
        None => {
            eprintln!("--scenario required");
            return ExitCode::from(2);
        }
    };

    // 1. 시나리오 로드
    let scenario = match Scenario::load(&scenario_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("scenario load error: {}", e);
            return ExitCode::from(1);
        }
    };

    // 2. Claude 어댑터 — workspace .env 또는 환경 변수에서 키
    if std::env::var("ANTHROPIC_API_KEY").is_err() {
        eprintln!("ANTHROPIC_API_KEY not set");
        return ExitCode::from(1);
    }
    let adapter = match ClaudeAdapter::from_env(&model) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("adapter: {}", e);
            return ExitCode::from(1);
        }
    };

    // 3. 와이어 클라이언트
    let wire = match WireClient::connect_as_ai(&server_addr).await {
        Ok(w) => w,
        Err(e) => {
            eprintln!("connect: {}", e);
            return ExitCode::from(1);
        }
    };

    // 4. 세션 구성
    let audit = std::env::current_dir().unwrap_or_default()
        .join("glue-ai-audit.log");
    let mut session = Session::new(adapter, wire, DEFAULT_SYSTEM_PROMPT.to_string())
        .with_budget(scenario.to_session_budget())
        .with_audit(audit.clone());

    // 5. 실행
    println!("[glue-ai] scenario={} model={} server={}", scenario.name, model, server_addr);
    let outcome = match session.run_task(&scenario.goal).await {
        Ok(o) => o,
        Err(e) => {
            eprintln!("session error: {}", e);
            return ExitCode::from(1);
        }
    };

    println!("\n=== outcome ===");
    println!("turns: {}", outcome.turns_used);
    println!("tokens: in={}, out={}", outcome.input_tokens, outcome.output_tokens);
    println!("wall: {:.1}s", outcome.wall_secs);
    if let Some(s) = &outcome.summary {
        println!("summary: {}", s);
    } else {
        println!("(no summary — see audit log: {})", audit.display());
    }

    if outcome.completed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    }
}
```

- [ ] **Step 2: `system_prompt.md` 추가**

`glue-ai/src/system_prompt.md`:

```markdown
You are a tester driving GeulOS, an AI-native operating system, through its wire protocol.

GeulOS exposes a tree of typed objects rather than pixels. Standard types:
- aios.std/Container@1 — layout container (children only)
- aios.std/Text@1 — read-only label, state.content
- aios.std/Button@1 — pressable, method `press`, state.label
- aios.std/Toggle@1 — on/off, methods `toggle`/`set`, state.on

Tools available:
- list_objects_by_type(type_uri) — discover IDs
- get_object(object_id) — full details
- invoke_method(target, method, args) — call method
- subscribe(target, kinds) — start observing events
- drain(subscription_id) — fetch queued events
- report_done(summary) — END the session with a summary (call this last)

Always pass UUIDs back exactly as received. Use parallel tool calls when steps are
independent. If a method isn't in the object's methods list, calling it returns
unknown_method — don't fabricate methods. When done, ALWAYS call report_done with a
specific, honest summary.
```

- [ ] **Step 3: 빌드 + 커밋**

```bash
cargo build -p geulos-glue-ai
git add -A
git commit -m "feat(glue-ai): main 바이너리 — scenario runner + Claude 어댑터 + audit 로그"
```

---

## Task 8: Glscript stub 확인 + 명확한 NotImplemented 응답

**Files:**
- Modify: `server-host/src/connection.rs` (이미 NotImplemented 반환 — 메시지 보강)
- Create: `server-host/tests/glscript_not_implemented_test.rs`

- [ ] **Step 1: connection.rs의 Glscript 분기 메시지 보강**

기존 `"Glscript"` 처리부의 메시지를 다음으로 교체:

```rust
"Glscript" => {
    let req_id = raw.get("request_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    Some(serde_json::json!({
        "kind": "GlscriptError",
        "request_id": req_id,
        "error_kind": "not_implemented",
        "detail": "Glscript execution is deferred to M5.5 (depends on 글 G1~G4). \
                   Use direct RPC (Invoke/Subscribe/etc.) — Claude/GPT handle wire \
                   protocol directly very well. See ADR-015."
    }))
}
```

- [ ] **Step 2: 회귀 테스트**

`server-host/tests/glscript_not_implemented_test.rs`:

```rust
use geulos_proto::*;
use geulos_server_host::run_listener;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn glscript_returns_not_implemented_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Ai,
        auth: json!({}),
        client_id: "t".to_string(),
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&hello).unwrap())).await.unwrap();
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let _: HelloAck = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();

    // Glscript 전송
    let g = GlscriptMsg {
        request_id: "g-1".to_string(),
        source: "test".to_string(),
        budget: json!({}),
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&g).unwrap())).await.unwrap();
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp = decode_frame(&mut slice).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    assert_eq!(v["kind"], "GlscriptError");
    assert_eq!(v["error_kind"], "not_implemented");
    assert!(v["detail"].as_str().unwrap().contains("M5.5"));
}
```

- [ ] **Step 3: 통과 + 커밋**

```bash
cargo test -p geulos-server-host --test glscript_not_implemented_test
git add -A
git commit -m "chore(server-host): Glscript NotImplemented 메시지 명확화 (M5.5 가이드)"
```

---

## Task 9: M5 acceptance — MockAdapter로 결정론 e2e

**Files:**
- Create: `glue-ai/tests/m5_acceptance.rs`

- [ ] **Step 1: 테스트 작성 (MockAdapter 사용 — 외부 API 호출 없음)**

```rust
//! M5 acceptance — MockAdapter로 결정론 e2e.
//!
//! 실제 Claude API 호출은 사용자가 수동으로:
//!   ANTHROPIC_API_KEY=... cargo run -p geulos-glue-ai -- run \
//!     --scenario glue-ai/scenarios/05_create_button.toml

use geulos_core::{std_types, ActorId};
use geulos_glue_ai::adapter::{LlmResponse, LlmStop, MockAdapter, ToolUse};
use geulos_glue_ai::session::{Session, SessionBudget};
use geulos_glue_ai::WireClient;
use geulos_server_host::run_listener;
use serde_json::json;

#[tokio::test]
async fn m5_acceptance_mock_explores_and_reports() {
    // 1. server-host + mount된 객체 준비
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut mounter = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();
    let mut btn = std_types::button(ActorId::local_user(), "OK");
    btn.acl.push(geulos_core::AclEntry {
        actor: geulos_core::ActorPattern::Wildcard,
        method: geulos_core::MethodPattern::Wildcard,
        effect: geulos_core::AclEffect::Allow,
    });
    let btn_id = btn.id.to_string();
    mounter.mount(btn).await.unwrap();

    // 2. Mock — *probe.py 시나리오 02와 비슷한 행동*을 흉내
    //    turn 1: list_objects_by_type(Button)
    //    turn 2: invoke press
    //    turn 3: report_done
    let mock = MockAdapter::new(vec![
        LlmResponse {
            text: vec!["I'll find the button first.".to_string()],
            tool_uses: vec![ToolUse {
                id: "tu-1".to_string(),
                name: "list_objects_by_type".to_string(),
                input: json!({"type_uri": "aios.std/Button@1"}),
            }],
            stop: LlmStop::ToolUse,
            tokens: (100, 30),
        },
        LlmResponse {
            text: vec!["Pressing the button.".to_string()],
            tool_uses: vec![ToolUse {
                id: "tu-2".to_string(),
                name: "invoke_method".to_string(),
                input: json!({"target": btn_id.clone(), "method": "press", "args": null}),
            }],
            stop: LlmStop::ToolUse,
            tokens: (150, 50),
        },
        LlmResponse {
            text: vec!["Done!".to_string()],
            tool_uses: vec![ToolUse {
                id: "tu-3".to_string(),
                name: "report_done".to_string(),
                input: json!({"summary": "Found and pressed button successfully"}),
            }],
            stop: LlmStop::ToolUse,
            tokens: (200, 50),
        },
    ]);

    // 3. ai 클라로 세션 구성
    let wire = WireClient::connect_as_ai(&addr.to_string()).await.unwrap();
    let mut session = Session::new(mock, wire, "test".to_string())
        .with_budget(SessionBudget { max_turns: 5, ..Default::default() });

    let outcome = session.run_task("Press the button.").await.unwrap();

    assert!(outcome.completed);
    assert_eq!(outcome.summary.as_deref(), Some("Found and pressed button successfully"));
    assert_eq!(outcome.turns_used, 3);
    assert!(outcome.input_tokens > 0);
    assert!(outcome.output_tokens > 0);
}
```

- [ ] **Step 2: 통과 + 커밋**

```bash
cargo test -p geulos-glue-ai --test m5_acceptance
git add -A
git commit -m "test(glue-ai): M5 acceptance — MockAdapter로 결정론 e2e"
```

---

## Task 10: 최종 스모크 (controller가 직접 처리, push)

(이 task는 *subagent에 위임하지 않고* controller가 직접 수행. M4 cascade 교훈.)

- [ ] **Step 1: 전체 검증**

```bash
cargo build --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

모두 그린.

- [ ] **Step 2: 단일 push**

```bash
git push origin main
```

- [ ] **Step 3: CI 그린 확인 (mobile 알림으로)**

- [ ] **Step 4: M5 완료 선언**

- glue-ai 크레이트 본격화 (placeholder 7줄 → ~1500줄)
- LlmAdapter + ClaudeAdapter + MockAdapter
- WireClient (probe.py의 Rust 버전 + subscribe + drain)
- Tools dispatch (7개 도구)
- Session 매니저 + 예산 + 감사
- 시나리오 TOML 형식 + 3개 예제
- geulos-glue-ai 바이너리 — `run --scenario` 모드
- Glscript NotImplemented + M5.5 가이드 메시지
- M5 acceptance (MockAdapter 결정론 e2e)

다음 가능 단계:
- 사용자가 실제 Claude API로 시나리오 실행 → 결과 보고서 (probe.py 첫 실행과 비교)
- M5.5 (글 VM 임베드, G 시리즈 완료 시)
- M6 (VM 부팅 통합)

---

## 자체 점검

**스펙 커버리지:**
- 설계 §9.2 M5 산출물 매핑:
  - AI 소켓 + 세션·토큰 관리 → T5 (Session) + T7 (binary)
  - 단발 RPC 라우터 → T2 (WireClient) + T4 (tools)
  - 글 바이트코드 VM 임베드 → *M5.5로 연기* (ADR-015)
  - 호스트 함수 ABI → M5.5
  - 안전 모드 → M5.5
  - 글 스크립트 예산 → T5 SessionBudget이 *RPC 경로*에 동등 기능 제공
  - **완료 기준 (Claude API가 자율 조작)** → T7 + T9
- ai-probe Python 코드의 *모든 패턴* 포함 + 확장 (subscribe/drain)

**플레이스홀더 스캔:** TBD/TODO 없음. 글 VM 부분은 *명시 연기* (ADR-015 + Task 8 Glscript stub).

**타입 일관성:**
- WireClient API (T2) → tools (T4) → session (T5) → binary (T7) 일관 사용
- LlmAdapter trait (T3) → MockAdapter (T3) + ClaudeAdapter (T3) → Session (T5) 일관 사용
- ScenarioBudget (T6) → SessionBudget (T5) 변환 명시

**알려진 한계 (M5 범위 밖):**
- Glscript 실행 (M5.5)
- 다중 동시 AI 세션 (현재는 1세션/1프로세스)
- OpenAI / Ollama 어댑터 (LlmAdapter trait이 준비되어 있어 추가만 하면 됨)
- AI 세션 *지속성* (대화 히스토리 디스크 저장 — 후속)
- 권한 grant UI (M4 컴포지터 도입 이후 별도 작업)
