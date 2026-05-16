# GeulOS M4 — 컴포지터 (사용자 GUI) 실행 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to execute task-by-task.

**Goal:** *처음으로 사람이 본다.* 컴포지터가 server-host에 접속해 객체 트리를 가져와 host OS 윈도우에 그린다. 사용자의 마우스 클릭이 객체 ID로 변환되어 Invoke 이벤트가 발행되고, 외부 클라이언트와 동일 결과를 일으킨다 (시나리오 C 대칭성 증명).

**Architecture:**
- **softbuffer**(CPU 픽셀 버퍼) + **winit**(윈도우/이벤트 루프) + **fontdue**(폰트 래스터화) 조합
- wgpu 대신 softbuffer 선택 — ADR-007의 "M4 시점 재검토" 옵션 발동. 이유: M4 범위가 좁고 (echo-app의 4개 객체 그리기), GPU 셰이더 도입은 학습 비용 대비 이득이 작음. M6 베어메탈 가까워질 때 wgpu/virtio-gpu 재검토.
- 컴포지터는 *별 프로세스*. server-host에 `Role::Compositor`로 접속. 동기 GUI 스레드 (winit) + 비동기 TCP 스레드 (tokio)의 두 스레드 모델. Arc<Mutex<TreeModel>>로 공유.
- 입력 → 좌표 → ObjectId hit-test → `InvokeMsg` 와이어 전송

**Tech Stack:** `winit = "0.30"`, `softbuffer = "0.4"`, `fontdue = "0.9"`, 기존 tokio/serde/serde_json.

**Selection criteria (완료 조건):**
- `cargo run -p geulos-compositor` 으로 호스트 OS 윈도우 열림
- 별 터미널에서 `cargo run -p geulos-server-host` + `cargo run -p geulos-echo-app` 띄우면 컴포지터 창에 echo-app의 UI (Container > [Text "count: 0", Button "+1"]) 가 보임
- 사용자가 마우스로 버튼 클릭 → text 표시가 "count: 1"으로 갱신됨
- 이때 별 클라이언트(geulosh --connect)가 text를 subscribe하고 있으면 동일 StateSet 이벤트를 관찰
- CI 그린 (단, headless 테스트만; 실제 윈도우 띄우는 테스트는 `#[ignore]`)

---

## ADR 시드

- **ADR-013 — M4 컴포지터는 softbuffer (CPU)**, wgpu는 M6 또는 후속 PR로 연기. 근거: scope 작음, 학습 곡선 낮음, virtio-gpu와의 연계가 명확해질 때까지 보류.

---

## 파일 구조 (사전 매핑)

```
proto/
└── src/messages.rs                  # + GetMsg / GetResult / GetError

compositor/
├── Cargo.toml                       # winit, softbuffer, fontdue, tokio 의존
├── src/
│   ├── main.rs                      # 바이너리 진입 — winit event loop
│   ├── lib.rs                       # 모듈 노출
│   ├── tree_model.rs                # 로컬 객체 트리 미러
│   ├── server_client.rs             # 별 스레드의 tokio TCP 클라이언트
│   ├── render.rs                    # softbuffer 픽셀 그리기
│   ├── text.rs                      # fontdue 텍스트 래스터화
│   ├── layout.rs                    # 레이아웃 계산 (Container=vstack)
│   ├── hit_test.rs                  # 좌표 → ObjectId
│   └── messages.rs                  # 두 스레드 간 명령/이벤트
└── tests/
    └── layout_test.rs               # 레이아웃 단위 테스트
```

---

## Task 1: GetMsg 와이어 프로토콜 확장 + ADR-013

컴포지터가 객체 ID 목록을 받은 뒤 *각 객체의 전체 내용*을 얻을 수 있어야 함. M2의 Query는 ID만 반환. 새 Get 메시지 필요.

**Files:**
- Create: `docs/adr/013-softbuffer-for-m4.md`
- Modify: `proto/src/messages.rs`
- Modify: `proto/src/lib.rs`
- Modify: `proto/tests/messages_test.rs`
- Modify: `server-host/src/actor.rs` (handle.get 이미 존재, 단지 wire 디스패치만)
- Modify: `server-host/src/dispatch.rs` (handle_get)
- Modify: `server-host/src/connection.rs` (dispatch_one에 "Get" 케이스)

- [ ] **Step 1: ADR-013 작성**

`docs/adr/013-softbuffer-for-m4.md`:

```markdown
# ADR-013: M4 컴포지터 렌더링 백엔드로 softbuffer 채택, wgpu는 M6에서 재검토

- **상태:** Accepted (ADR-007 잠정 결정의 후속)
- **일자:** 2026-05-17

## 맥락

ADR-007에서 컴포지터 백엔드로 wgpu를 잠정 선택했으나 *"M4 완료 시 재검토"* 라고 명시. M4 진입 시점에 평가한 결과:

- M4의 시각적 범위는 echo-app의 4개 객체 (Container/Text/Button × 1, 또는 Toggle 한 종류 더). GPU 가속이 필요한 정도가 아님.
- wgpu의 셰이더/파이프라인 학습 곡선이 가파름. 1주 학습 스파이크 후에도 사용자 GUI 구현이 늦어질 가능성.
- softbuffer는 CPU 픽셀 버퍼 단순 API. 즉시 시작 가능.
- M6 시점에 virtio-gpu와 wgpu의 통합이 자연스러워지면 그때 백엔드 교체. 객체 트리 ↔ 렌더 경계가 명확하므로 국소적 교체 가능.

## 결정

M4 컴포지터는 **softbuffer + winit + fontdue** 조합으로 구현. wgpu는 M6 또는 후속 PR로 연기.

## 결과

### 긍정적
- M4 시작 즉시 픽셀 그리기 가능
- 의존성 트리 가벼움 (wgpu는 ~50개, softbuffer 스택은 ~10개)
- 디버깅 쉬움 (CPU 픽셀 = 직접 검사 가능)

### 부정적
- 큰 트리 (수백 객체)에서 성능 떨어짐. M4 범위에선 문제 없음.
- 폰트 안티에일리어싱, GPU 가속 효과 등은 후일.

### 중립
- ADR-007의 "재검토 시점"은 *M6 완료 시*로 재설정. 그때 virtio-gpu 호환성과 함께 wgpu 도입 가치를 재평가.

## 참고

- ADR-007 — wgpu 잠정 선택 (M4 시점 재검토)
- M4 plan §1.0 — softbuffer 도입 동기
```

- [ ] **Step 2: 실패 테스트 (`proto/tests/messages_test.rs` 끝에)**

```rust
use geulos_proto::{GetError, GetMsg, GetResult};

#[test]
fn get_msg_round_trip() {
    let m = GetMsg {
        request_id: "g-1".to_string(),
        target: "obj-uuid".to_string(),
    };
    let s = serde_json::to_string(&m).unwrap();
    assert!(s.contains(r#""kind":"Get""#));
    let back: GetMsg = serde_json::from_str(&s).unwrap();
    assert_eq!(m, back);
}

#[test]
fn get_result_carries_object() {
    let r = GetResult {
        request_id: "g-1".to_string(),
        object: serde_json::json!({"id": "x"}),
    };
    let s = serde_json::to_string(&r).unwrap();
    assert!(s.contains(r#""kind":"GetResult""#));
}

#[test]
fn get_error_uses_error_kind_wire_name() {
    let e = GetError {
        request_id: "g-1".to_string(),
        kind: "not_found".to_string(),
        detail: "missing".to_string(),
    };
    let s = serde_json::to_string(&e).unwrap();
    assert!(s.contains(r#""kind":"GetError""#));
    assert!(s.contains(r#""error_kind":"not_found""#));
}
```

- [ ] **Step 3: `proto/src/messages.rs` 끝에 추가**

```rust
/// `Get` 요청: 객체의 전체 내용 조회.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "Get")]
pub struct GetMsg {
    pub request_id: String,
    pub target: String,
}

/// `GetResult` 응답.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "GetResult")]
pub struct GetResult {
    pub request_id: String,
    /// core::Object를 JSON으로 직렬화한 값.
    pub object: Value,
}

/// `GetError` 응답.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "GetError")]
pub struct GetError {
    pub request_id: String,
    #[serde(rename = "error_kind")]
    pub kind: String,
    pub detail: String,
}
```

`proto/src/lib.rs` 재export 확장.

- [ ] **Step 4: server-host에 Get 디스패치 추가**

`server-host/src/dispatch.rs`에 추가:

```rust
/// Get 메시지 처리.
pub async fn handle_get(handle: &ObjectServerHandle, msg: geulos_proto::GetMsg) -> Value {
    let target = match parse_object_id(&msg.target) {
        Some(t) => t,
        None => {
            return serde_json::to_value(geulos_proto::GetError {
                request_id: msg.request_id,
                kind: "malformed_target".to_string(),
                detail: format!("bad UUID: {}", msg.target),
            })
            .unwrap();
        }
    };
    match handle.get(target).await {
        Ok(Some(obj)) => serde_json::to_value(geulos_proto::GetResult {
            request_id: msg.request_id,
            object: serde_json::to_value(&obj).unwrap_or(Value::Null),
        })
        .unwrap(),
        Ok(None) => serde_json::to_value(geulos_proto::GetError {
            request_id: msg.request_id,
            kind: "not_found".to_string(),
            detail: format!("no object with id {}", msg.target),
        })
        .unwrap(),
        Err(e) => serde_json::to_value(geulos_proto::GetError {
            request_id: msg.request_id,
            kind: "core".to_string(),
            detail: e.to_string(),
        })
        .unwrap(),
    }
}
```

`server-host/src/connection.rs`의 dispatch_one match에 추가:

```rust
"Get" => {
    let m: geulos_proto::GetMsg = match serde_json::from_value(raw) { Ok(m) => m, Err(_) => return };
    Some(handle_get(handle, m).await)
}
```

- [ ] **Step 5: 통합 적합성 테스트 (`server-host/tests/get_conformance.rs` 신규)**

```rust
use geulos_core::{std_types, ActorId};
use geulos_proto::*;
use geulos_server_host::run_listener;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn get_returns_full_object() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Ai,
        auth: json!({}),
        client_id: "c".to_string(),
    };
    let body = serde_json::to_vec(&hello).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let _: HelloAck = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();

    // 객체 mount
    let txt = std_types::text(ActorId::local_user(), "hi");
    let target = txt.id.to_string();
    let mount = MountMsg {
        root_object_id: target.clone(),
        tree: serde_json::to_value(&txt).unwrap(),
    };
    let body = serde_json::to_vec(&mount).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let _: MountAck = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();

    // Get
    let g = GetMsg { request_id: "g-1".to_string(), target: target.clone() };
    let body = serde_json::to_vec(&g).unwrap();
    stream.write_all(&encode_frame(&body)).await.unwrap();
    let n = stream.read(&mut buf).await.unwrap();
    let mut slice = &buf[..n];
    let resp: GetResult = serde_json::from_slice(&decode_frame(&mut slice).unwrap()).unwrap();
    assert_eq!(resp.request_id, "g-1");
    assert!(resp.object.get("id").is_some());
}
```

- [ ] **Step 6: 빌드/테스트/커밋**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "feat(proto+server-host): Get 메시지 + ADR-013 (softbuffer 채택)"
```

---

## Task 2: compositor 크레이트 스캐폴드 (winit + softbuffer "hello window")

**Files:**
- Modify: 루트 `Cargo.toml` (winit/softbuffer/fontdue/raw-window-handle 추가)
- Modify: `compositor/Cargo.toml`
- Modify: `compositor/src/main.rs`
- Create: `compositor/src/lib.rs` (lib target)

- [ ] **Step 1: workspace.dependencies 확장**

```toml
winit = "0.30"
softbuffer = "0.4"
fontdue = "0.9"
```

- [ ] **Step 2: `compositor/Cargo.toml` 확장**

```toml
[package]
name = "geulos-compositor"
version = "0.0.1"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "GeulOS user-facing GUI compositor (M4: softbuffer + winit + fontdue)"

[[bin]]
name = "geulos-compositor"
path = "src/main.rs"

[lib]
name = "geulos_compositor"
path = "src/lib.rs"

[dependencies]
geulos-core = { path = "../core" }
geulos-proto = { path = "../proto" }
tokio = { workspace = true }
winit = { workspace = true }
softbuffer = { workspace = true }
fontdue = { workspace = true }
serde_json = "1.0"
```

- [ ] **Step 3: `compositor/src/lib.rs` 신규 (모듈 노출)**

```rust
//! GeulOS compositor library.

pub mod hit_test;
pub mod layout;
pub mod messages;
pub mod render;
pub mod server_client;
pub mod text;
pub mod tree_model;
```

(각 모듈은 후속 태스크에서 채움. 지금은 빈 stub.)

- [ ] **Step 4: 빈 stub 파일 6개 + `src/main.rs` 갱신**

빈 stub들 (`compositor/src/{hit_test,layout,messages,render,server_client,text,tree_model}.rs`):
```rust
//! (Task N에서 구현)
```

`compositor/src/main.rs`:

```rust
//! GeulOS 컴포지터: 객체 트리를 host OS 윈도우에 그린다.

use std::num::NonZeroU32;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

struct App {
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title("GeulOS Compositor (M4)")
            .with_inner_size(PhysicalSize::new(800u32, 600u32));
        let window = Arc::new(event_loop.create_window(attrs).expect("create_window"));
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer Context");
        let surface = softbuffer::Surface::new(&context, window.clone()).expect("softbuffer Surface");
        self.window = Some(window);
        self.surface = Some(surface);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let (Some(window), Some(surface)) = (&self.window, &mut self.surface) {
                    let size = window.inner_size();
                    let (w, h) = (size.width, size.height);
                    if w == 0 || h == 0 {
                        return;
                    }
                    surface
                        .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
                        .expect("resize");
                    let mut buffer = surface.buffer_mut().expect("buffer_mut");
                    // 흰 배경
                    for px in buffer.iter_mut() {
                        *px = 0xFF_FF_FF_FF; // 0xAARRGGBB on softbuffer (top byte ignored)
                    }
                    buffer.present().expect("present");
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("EventLoop");
    let mut app = App { window: None, surface: None };
    event_loop.run_app(&mut app).expect("run_app");
}
```

- [ ] **Step 5: 빌드 + 수동 sanity**

```bash
cargo build -p geulos-compositor
cargo run -p geulos-compositor
```

흰 800x600 윈도우가 떠야 함. X 버튼 누르면 닫힘.

(이건 *대화형* 검증. 자동 테스트는 아님 — 다음 task부터 logic은 lib에서 테스트.)

- [ ] **Step 6: 커밋**

```bash
git add -A
git commit -m "feat(compositor): winit + softbuffer 'hello window' 스캐폴드"
```

---

## Task 3: TreeModel + 두 스레드 간 메시지 정의

**Files:**
- Modify: `compositor/src/tree_model.rs`
- Modify: `compositor/src/messages.rs`
- Create: `compositor/tests/tree_model_test.rs`

- [ ] **Step 1: `compositor/src/tree_model.rs` 구현**

```rust
//! 컴포지터가 보유하는 로컬 객체 트리 미러.
//!
//! server-host에서 받은 Object들의 *복사본*을 ObjectId로 인덱스. 입력 hit-test와
//! 렌더링은 이 미러를 읽음. 서버 이벤트가 도착하면 미러 업데이트.

use std::collections::HashMap;

use geulos_core::{Object, ObjectId, TypeUri};

/// 로컬 트리 모델.
#[derive(Debug, Default)]
pub struct TreeModel {
    objects: HashMap<ObjectId, Object>,
    /// 컴포지터가 처음 query로 발견한 루트 후보 (parent가 None인 것).
    roots: Vec<ObjectId>,
}

impl TreeModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// 객체 한 개 삽입 또는 덮어쓰기.
    pub fn upsert(&mut self, obj: Object) {
        let id = obj.id;
        let is_root = obj.parent.is_none();
        self.objects.insert(id, obj);
        if is_root && !self.roots.contains(&id) {
            self.roots.push(id);
        }
    }

    /// 객체 제거 (Lifecycle Destroyed에 대응).
    pub fn remove(&mut self, id: ObjectId) {
        self.objects.remove(&id);
        self.roots.retain(|r| *r != id);
    }

    /// 객체 조회.
    pub fn get(&self, id: ObjectId) -> Option<&Object> {
        self.objects.get(&id)
    }

    /// 모든 ID 순회.
    pub fn ids(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.objects.keys().copied()
    }

    /// 루트 목록.
    pub fn roots(&self) -> &[ObjectId] {
        &self.roots
    }

    /// 객체 개수.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// 비어있는지.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// state 키 갱신 (StateSet 이벤트 처리용).
    pub fn set_state(&mut self, id: ObjectId, key: String, value: serde_json::Value) {
        if let Some(obj) = self.objects.get_mut(&id) {
            obj.state.insert(key, value);
        }
    }

    /// 특정 타입 URI의 객체만 추리기.
    pub fn objects_of_type(&self, type_uri: &TypeUri) -> Vec<ObjectId> {
        self.objects
            .iter()
            .filter(|(_, o)| &o.type_uri == type_uri)
            .map(|(id, _)| *id)
            .collect()
    }
}
```

- [ ] **Step 2: `compositor/src/messages.rs` 구현 (두 스레드 간 IPC)**

```rust
//! winit 메인 스레드와 tokio TCP 스레드 사이의 메시지 정의.

use geulos_core::{Object, ObjectId};
use serde_json::Value;

/// winit → tokio: 클릭/입력 등에 의해 발생한 액션.
#[derive(Debug, Clone)]
pub enum UiAction {
    /// 객체의 메서드 호출 요청.
    Invoke {
        target: ObjectId,
        method: String,
        args: Value,
    },
    /// 종료 요청.
    Quit,
}

/// tokio → winit: 서버에서 받은 변화.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    /// 객체가 (재)등록됨 — TreeModel.upsert.
    ObjectUpserted(Object),
    /// 객체가 사라짐.
    ObjectRemoved(ObjectId),
    /// 객체의 state 키 갱신됨.
    StateSet {
        id: ObjectId,
        key: String,
        value: Value,
    },
    /// 연결 손실.
    Disconnected,
}
```

- [ ] **Step 3: `compositor/tests/tree_model_test.rs`**

```rust
use geulos_compositor::tree_model::TreeModel;
use geulos_core::{std_types, ActorId};

#[test]
fn upsert_adds_to_objects_and_roots_if_no_parent() {
    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    let t = std_types::text(owner, "hi");
    let id = t.id;
    tm.upsert(t);
    assert_eq!(tm.len(), 1);
    assert!(tm.roots().contains(&id));
}

#[test]
fn upsert_with_parent_not_added_to_roots() {
    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    let mut t = std_types::text(owner, "child");
    t.parent = Some(geulos_core::ObjectId::new());
    let id = t.id;
    tm.upsert(t);
    assert_eq!(tm.len(), 1);
    assert!(!tm.roots().contains(&id));
}

#[test]
fn remove_takes_object_and_root_out() {
    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    let t = std_types::text(owner, "x");
    let id = t.id;
    tm.upsert(t);
    tm.remove(id);
    assert_eq!(tm.len(), 0);
    assert!(!tm.roots().contains(&id));
}

#[test]
fn set_state_updates_object_state() {
    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    let t = std_types::text(owner, "before");
    let id = t.id;
    tm.upsert(t);
    tm.set_state(id, "content".to_string(), serde_json::json!("after"));
    let obj = tm.get(id).unwrap();
    assert_eq!(obj.state.get("content"), Some(&serde_json::json!("after")));
}

#[test]
fn objects_of_type_filters() {
    use geulos_core::TypeUri;
    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    tm.upsert(std_types::text(owner.clone(), "a"));
    tm.upsert(std_types::button(owner.clone(), "b"));
    tm.upsert(std_types::text(owner, "c"));

    let txt_type = TypeUri::parse("aios.std/Text@1").unwrap();
    let texts = tm.objects_of_type(&txt_type);
    assert_eq!(texts.len(), 2);
}
```

- [ ] **Step 4: 통과 + 커밋**

```bash
cargo test -p geulos-compositor
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "feat(compositor): TreeModel + 두 스레드 간 메시지 정의"
```

---

## Task 4: server_client — 별 tokio 스레드에서 서버 연결

**Files:**
- Modify: `compositor/src/server_client.rs`

- [ ] **Step 1: 구현**

```rust
//! 별 tokio 스레드에서 server-host와 TCP로 대화.
//!
//! winit 메인 스레드는 mpsc 채널로:
//! - 입력 → UiAction 송신
//! - ServerEvent 수신 → 트리 갱신 + 윈도우 redraw 요청

use std::sync::Arc;
use std::time::Duration;

use geulos_core::{Object, ObjectId, TypeUri};
use geulos_proto::{
    decode_frame, encode_frame, EventKindFilterWire, EventMsg, GetMsg, GetResult, Hello, HelloAck,
    InvokeMsg, QueryMsg, QueryPredicate, QueryResult, Role, SubscribeMsg,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use winit::event_loop::EventLoopProxy;

use crate::messages::{ServerEvent, UiAction};

/// 표준 타입 URI 목록 — M4에서 컴포지터가 처음 query 할 것들.
const STD_TYPES: &[&str] = &[
    "aios.std/Container@1",
    "aios.std/Text@1",
    "aios.std/Button@1",
    "aios.std/Toggle@1",
];

/// 컴포지터의 redraw/quit 신호를 winit에 보내는 user_event 타입.
#[derive(Debug, Clone)]
pub enum UserEvent {
    Redraw,
    Quit,
}

/// 컴포지터 측 server-host 클라이언트 실행.
///
/// 별 tokio 스레드에서 호출. server에 접속, query+get으로 초기 트리를 가져옴,
/// 모든 객체 subscribe, 이벤트 받아서 event_tx로 전달.
/// 같이 ui_rx로 UiAction(예: 클릭에 의한 Invoke)을 받아 wire로 전송.
pub async fn run_server_client(
    addr: String,
    event_tx: mpsc::Sender<ServerEvent>,
    mut ui_rx: mpsc::Receiver<UiAction>,
    proxy: Arc<EventLoopProxy<UserEvent>>,
) -> Result<(), String> {
    let mut stream = TcpStream::connect(&addr).await.map_err(|e| e.to_string())?;

    // 1) Hello as Compositor
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Compositor,
        auth: json!({}),
        client_id: "compositor".to_string(),
    };
    let body = serde_json::to_vec(&hello).map_err(|e| e.to_string())?;
    stream
        .write_all(&encode_frame(&body))
        .await
        .map_err(|e| e.to_string())?;

    let mut accum: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 16384];
    // HelloAck 수신
    let _ack: HelloAck = read_typed(&mut stream, &mut accum, &mut buf).await?;
    let _ = proxy.send_event(UserEvent::Redraw);

    // 2) 표준 타입별 Query → 객체 ID 모으기
    let mut all_ids: Vec<String> = Vec::new();
    for (i, t) in STD_TYPES.iter().enumerate() {
        let q = QueryMsg {
            request_id: format!("q-{}", i),
            query: QueryPredicate::ByType { type_uri: t.to_string() },
        };
        write_msg(&mut stream, &q).await?;
        let qr: QueryResult = read_typed(&mut stream, &mut accum, &mut buf).await?;
        all_ids.extend(qr.objects);
    }

    // 3) 각 ID에 대해 Get 후 ServerEvent::ObjectUpserted 전송
    for (i, id_str) in all_ids.iter().enumerate() {
        let g = GetMsg {
            request_id: format!("g-{}", i),
            target: id_str.clone(),
        };
        write_msg(&mut stream, &g).await?;
        let gr: GetResult = read_typed(&mut stream, &mut accum, &mut buf).await?;
        if let Ok(obj) = serde_json::from_value::<Object>(gr.object) {
            let _ = event_tx.send(ServerEvent::ObjectUpserted(obj)).await;
        }
    }
    let _ = proxy.send_event(UserEvent::Redraw);

    // 4) 각 객체에 Subscribe (Invoke + StateSet + Lifecycle)
    for (i, id_str) in all_ids.iter().enumerate() {
        let s = SubscribeMsg {
            subscription_id: format!("sub-{}", i),
            target: id_str.clone(),
            kinds: vec![
                EventKindFilterWire::Invoke,
                EventKindFilterWire::StateSet,
                EventKindFilterWire::Lifecycle,
            ],
            include_initial: false,
        };
        write_msg(&mut stream, &s).await?;
        let _ack = read_response_body(&mut stream, &mut accum, &mut buf).await?;
    }

    // 5) 동시 루프: 서버 → 클라 event 수신 + UI → 서버 Invoke 송신
    loop {
        tokio::select! {
            r = stream.read(&mut buf) => {
                let n = match r {
                    Ok(0) => { let _ = event_tx.send(ServerEvent::Disconnected).await; return Ok(()); }
                    Ok(n) => n,
                    Err(_) => { let _ = event_tx.send(ServerEvent::Disconnected).await; return Ok(()); }
                };
                accum.extend_from_slice(&buf[..n]);
                loop {
                    let mut slice = accum.as_slice();
                    match decode_frame(&mut slice) {
                        Ok(body) => {
                            let consumed = accum.len() - slice.len();
                            accum.drain(..consumed);
                            handle_server_frame(&body, &event_tx).await;
                            let _ = proxy.send_event(UserEvent::Redraw);
                        }
                        Err(_) => break,
                    }
                }
            }
            Some(action) = ui_rx.recv() => {
                match action {
                    UiAction::Invoke { target, method, args } => {
                        let req_id = format!("inv-{}", target);
                        let m = InvokeMsg {
                            request_id: req_id,
                            target: target.to_string(),
                            method,
                            args,
                        };
                        let _ = write_msg(&mut stream, &m).await;
                    }
                    UiAction::Quit => {
                        let _ = proxy.send_event(UserEvent::Quit);
                        return Ok(());
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                // 살아있음 — 별 작업 없음
            }
        }
    }
}

async fn handle_server_frame(body: &[u8], event_tx: &mpsc::Sender<ServerEvent>) {
    let raw: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return,
    };
    let kind = raw.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    if kind == "Event" {
        let ev: EventMsg = match serde_json::from_value(raw) {
            Ok(e) => e,
            Err(_) => return,
        };
        // 이벤트 종류별 분석
        let target_str = ev.event.get("target").and_then(|v| v.as_str()).unwrap_or("");
        let target_id: ObjectId = match serde_json::from_str(&format!("\"{}\"", target_str)) {
            Ok(t) => t,
            Err(_) => return,
        };
        let kind_str = ev.event.get("kind").and_then(|k| k.get("kind"))
            .and_then(|v| v.as_str()).unwrap_or("");
        match kind_str {
            "StateSet" => {
                let kind_obj = ev.event.get("kind").unwrap();
                let key = kind_obj.get("key").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let value = kind_obj.get("value").cloned().unwrap_or(serde_json::Value::Null);
                let _ = event_tx.send(ServerEvent::StateSet { id: target_id, key, value }).await;
            }
            "Lifecycle" => {
                // Destroyed → 제거
                let lifecycle = ev.event.get("kind").and_then(|k| k.get("Lifecycle"))
                    .and_then(|v| v.as_str()).unwrap_or("");
                if lifecycle == "Destroyed" {
                    let _ = event_tx.send(ServerEvent::ObjectRemoved(target_id)).await;
                }
            }
            _ => {}
        }
    }
}

async fn write_msg<T: serde::Serialize>(
    stream: &mut TcpStream,
    msg: &T,
) -> Result<(), String> {
    let body = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    stream
        .write_all(&encode_frame(&body))
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn read_response_body(
    stream: &mut TcpStream,
    accum: &mut Vec<u8>,
    buf: &mut [u8],
) -> Result<Vec<u8>, String> {
    loop {
        let mut slice = accum.as_slice();
        if let Ok(body) = decode_frame(&mut slice) {
            let consumed = accum.len() - slice.len();
            accum.drain(..consumed);
            return Ok(body);
        }
        let n = stream.read(buf).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("closed".to_string());
        }
        accum.extend_from_slice(&buf[..n]);
    }
}

async fn read_typed<T: serde::de::DeserializeOwned>(
    stream: &mut TcpStream,
    accum: &mut Vec<u8>,
    buf: &mut [u8],
) -> Result<T, String> {
    let body = read_response_body(stream, accum, buf).await?;
    serde_json::from_slice(&body).map_err(|e| format!("decode: {}", e))
}
```

- [ ] **Step 2: 컴파일 sanity**

```bash
cargo build -p geulos-compositor
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 3: 커밋**

```bash
git add -A
git commit -m "feat(compositor): server_client (별 tokio 스레드 + Query+Get+Subscribe 흐름)"
```

---

## Task 5: 레이아웃 엔진 (Container = vstack, Text/Button = box)

**Files:**
- Modify: `compositor/src/layout.rs`
- Create: `compositor/tests/layout_test.rs`

- [ ] **Step 1: 구현**

```rust
//! 단순 레이아웃 엔진.
//!
//! Container = 세로 stack (vstack). Text/Button/Toggle = 자식 없는 직사각형 box.
//! 루트 컨테이너가 윈도우 전체를 채움.

use geulos_core::{Object, ObjectId, TypeUri};

use crate::tree_model::TreeModel;

/// 한 객체의 화면상 사각형.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

/// 레이아웃 결과: ObjectId → Rect 매핑.
#[derive(Debug, Default)]
pub struct LayoutResult {
    pub rects: Vec<(ObjectId, Rect)>,
}

impl LayoutResult {
    pub fn get(&self, id: ObjectId) -> Option<Rect> {
        self.rects.iter().find(|(i, _)| *i == id).map(|(_, r)| *r)
    }

    pub fn iter(&self) -> impl Iterator<Item = (ObjectId, Rect)> + '_ {
        self.rects.iter().copied()
    }
}

/// 객체 타입별 정해진 높이 (단순 모델).
fn item_height(type_uri: &TypeUri) -> i32 {
    match type_uri.as_str() {
        "aios.std/Text@1" => 40,
        "aios.std/Button@1" => 60,
        "aios.std/Toggle@1" => 40,
        _ => 0, // Container는 자체 크기 없음 (자식의 합으로 계산)
    }
}

const PADDING: i32 = 16;
const SPACING: i32 = 8;

/// 한 객체와 그 자손을 레이아웃해서 사각형 목록을 반환.
fn layout_object(
    tree: &TreeModel,
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
    if obj.type_uri.as_str() == "aios.std/Container@1" {
        // vstack: 자식들을 세로로 배치, 자기 높이는 자식 합 + padding
        let mut cur_y = y + PADDING;
        let inner_x = x + PADDING;
        let inner_w = avail_w - 2 * PADDING;
        let mut content_h = 0i32;
        for &child_id in &obj.children {
            let used = layout_object(tree, child_id, inner_x, cur_y, inner_w, out);
            cur_y += used + SPACING;
            content_h += used + SPACING;
        }
        // SPACING 마지막 제거
        if content_h > 0 {
            content_h -= SPACING;
        }
        let total_h = content_h + 2 * PADDING;
        out.push((id, Rect { x, y, w: avail_w, h: total_h }));
        total_h
    } else {
        let h = item_height(&obj.type_uri);
        out.push((id, Rect { x, y, w: avail_w, h }));
        h
    }
}

/// 전체 트리를 레이아웃. roots의 첫 객체가 윈도우 채움. 나머지 roots는 그 아래로.
pub fn layout(tree: &TreeModel, win_w: i32, win_h: i32) -> LayoutResult {
    let mut out = Vec::new();
    let mut y = 0i32;
    for &root in tree.roots() {
        let used = layout_object(tree, root, 0, y, win_w, &mut out);
        y += used;
        if y >= win_h {
            break;
        }
    }
    LayoutResult { rects: out }
}
```

- [ ] **Step 2: 테스트**

`compositor/tests/layout_test.rs`:

```rust
use geulos_compositor::layout::layout;
use geulos_compositor::tree_model::TreeModel;
use geulos_core::{std_types, ActorId};

#[test]
fn empty_tree_yields_empty_layout() {
    let tm = TreeModel::new();
    let r = layout(&tm, 800, 600);
    assert_eq!(r.rects.len(), 0);
}

#[test]
fn single_text_assigned_height_40() {
    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    let t = std_types::text(owner, "hi");
    let id = t.id;
    tm.upsert(t);
    let r = layout(&tm, 800, 600);
    let rect = r.get(id).unwrap();
    assert_eq!(rect.h, 40);
}

#[test]
fn container_with_text_and_button_vstacks() {
    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    let mut c = std_types::container(owner.clone());
    let mut text = std_types::text(owner.clone(), "count: 0");
    let mut button = std_types::button(owner, "+1");

    let c_id = c.id;
    let text_id = text.id;
    let button_id = button.id;

    text.parent = Some(c_id);
    button.parent = Some(c_id);
    c.children.push(text_id);
    c.children.push(button_id);

    tm.upsert(c);
    tm.upsert(text);
    tm.upsert(button);

    let r = layout(&tm, 800, 600);
    let trect = r.get(text_id).unwrap();
    let brect = r.get(button_id).unwrap();
    // text는 button 위에 있어야 함
    assert!(trect.y < brect.y);
    // padding/spacing 적용된 만큼 x도 16 이상
    assert!(trect.x >= 16);
}

#[test]
fn click_hit_test_via_rect_contains() {
    use geulos_compositor::layout::Rect;
    let r = Rect { x: 10, y: 20, w: 30, h: 40 };
    assert!(r.contains(15, 25));
    assert!(!r.contains(5, 25));
    assert!(!r.contains(15, 65));
}
```

- [ ] **Step 3: 통과 + 커밋**

```bash
cargo test -p geulos-compositor
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "feat(compositor): 레이아웃 엔진 (Container vstack)"
```

---

## Task 6: 텍스트 래스터화 (fontdue) + 렌더 (softbuffer 픽셀 그리기)

**Files:**
- Modify: `compositor/src/text.rs`
- Modify: `compositor/src/render.rs`

- [ ] **Step 1: 폰트 임베드 — 시스템 폰트 또는 빌트인**

가장 단순: `include_bytes!`로 작은 폰트를 임베드. 예: Roboto Mono Regular 또는 Noto Sans CJK Korean (한글 표시용). 라이선스 호환 폰트.

실용적: `compositor/fonts/` 디렉터리에 `NotoSansKR-Regular.ttf` 파일을 두고 `include_bytes!`. 매니페스트에 라이선스 동봉.

대안 (이 plan이 가정): **시스템 기본 폰트 사용** — Windows의 `C:\Windows\Fonts\segoeui.ttf`, Linux의 `/usr/share/fonts/...`. 런타임 검출.

복잡도를 줄이기 위해 **fontdue의 demo에서 제공하는 임베드 폰트 사용** 또는 사용자 시스템에 항상 있을 만한 폰트 (Windows: `arial.ttf`):

```rust
// Windows 기준
const FONT_PATH: &str = r"C:\Windows\Fonts\arial.ttf";
```

(Linux/Mac 호환을 위해 향후 확장. M4에선 Windows-only 가정 OK.)

- [ ] **Step 2: `compositor/src/text.rs` 구현**

```rust
//! 텍스트 래스터화 (fontdue 기반).

use std::sync::OnceLock;

use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::Font;

const FONT_BYTES: &[u8] = include_bytes!("../fonts/font.ttf");
const FONT_SIZE: f32 = 18.0;

static FONT: OnceLock<Font> = OnceLock::new();

fn font() -> &'static Font {
    FONT.get_or_init(|| {
        Font::from_bytes(FONT_BYTES, fontdue::FontSettings::default())
            .expect("font load")
    })
}

/// 텍스트를 ARGB 픽셀 버퍼에 그리는 유틸.
///
/// `buffer`: ARGB u32 픽셀 버퍼 (softbuffer 호환). `stride`는 한 행의 픽셀 수.
/// `(x, y)`는 텍스트 left-top 위치.
/// `color`: ARGB u32 (예: 0xFF_00_00_00 검정).
pub fn draw_text(
    buffer: &mut [u32],
    stride: usize,
    height: usize,
    text: &str,
    x: i32,
    y: i32,
    color: u32,
) {
    let f = font();
    let fonts = [f];
    let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
    layout.reset(&LayoutSettings::default());
    layout.append(&fonts, &TextStyle::new(text, FONT_SIZE, 0));
    for glyph in layout.glyphs() {
        let (metrics, bitmap) = f.rasterize(glyph.parent, FONT_SIZE);
        let gx = x + glyph.x as i32;
        let gy = y + glyph.y as i32 + (FONT_SIZE as i32);
        for row in 0..metrics.height {
            for col in 0..metrics.width {
                let px = gx + col as i32;
                let py = gy + row as i32;
                if px < 0 || py < 0 || px >= stride as i32 || py >= height as i32 {
                    continue;
                }
                let alpha = bitmap[row * metrics.width + col];
                if alpha == 0 {
                    continue;
                }
                let idx = (py as usize) * stride + (px as usize);
                let bg = buffer[idx];
                buffer[idx] = blend_argb(bg, color, alpha);
            }
        }
    }
}

fn blend_argb(bg: u32, fg: u32, alpha: u8) -> u32 {
    let a = alpha as u32;
    let inv = 255 - a;
    let bg_r = (bg >> 16) & 0xFF;
    let bg_g = (bg >> 8) & 0xFF;
    let bg_b = bg & 0xFF;
    let fg_r = (fg >> 16) & 0xFF;
    let fg_g = (fg >> 8) & 0xFF;
    let fg_b = fg & 0xFF;
    let r = (bg_r * inv + fg_r * a) / 255;
    let g = (bg_g * inv + fg_g * a) / 255;
    let b = (bg_b * inv + fg_b * a) / 255;
    0xFF_00_00_00 | (r << 16) | (g << 8) | b
}
```

**중요:** `compositor/fonts/font.ttf` 파일을 두어야 컴파일 가능. *implementer가 OS의 폰트 하나를 그쪽으로 복사하거나, fontdue의 시연용 폰트를 download해서 둘 것.*

대안: 동적 로드 (`std::fs::read`). 본 plan에서는 단순화를 위해 임베드.

- [ ] **Step 3: `compositor/src/render.rs` 구현**

```rust
//! softbuffer 픽셀 버퍼에 객체 트리 그리기.

use geulos_core::{Object, ObjectId, TypeUri};

use crate::layout::{LayoutResult, Rect};
use crate::text::draw_text;
use crate::tree_model::TreeModel;

const COLOR_BG: u32 = 0xFF_F5_F5_F5;
const COLOR_CONTAINER: u32 = 0xFF_E0_E0_E0;
const COLOR_BUTTON: u32 = 0xFF_42_75_E0;
const COLOR_TEXT: u32 = 0xFF_22_22_22;
const COLOR_BUTTON_TEXT: u32 = 0xFF_FF_FF_FF;

/// 한 프레임을 그린다.
pub fn render_frame(
    tree: &TreeModel,
    layout: &LayoutResult,
    buffer: &mut [u32],
    width: usize,
    height: usize,
) {
    // 배경
    fill_rect(buffer, width, height, &Rect { x: 0, y: 0, w: width as i32, h: height as i32 }, COLOR_BG);

    for (id, rect) in layout.iter() {
        let obj = match tree.get(id) {
            Some(o) => o,
            None => continue,
        };
        match obj.type_uri.as_str() {
            "aios.std/Container@1" => {
                fill_rect(buffer, width, height, &rect, COLOR_CONTAINER);
            }
            "aios.std/Text@1" => {
                fill_rect(buffer, width, height, &rect, COLOR_BG);
                let content = obj.state.get("content")
                    .and_then(|v| v.as_str()).unwrap_or("(empty)");
                draw_text(buffer, width, height, content, rect.x + 8, rect.y + 8, COLOR_TEXT);
            }
            "aios.std/Button@1" => {
                fill_rect(buffer, width, height, &rect, COLOR_BUTTON);
                let label = obj.state.get("label")
                    .and_then(|v| v.as_str()).unwrap_or("(button)");
                draw_text(buffer, width, height, label, rect.x + 16, rect.y + 16, COLOR_BUTTON_TEXT);
            }
            "aios.std/Toggle@1" => {
                let on = obj.state.get("on").and_then(|v| v.as_bool()).unwrap_or(false);
                let color = if on { 0xFF_4C_AF_50 } else { 0xFF_9E_9E_9E };
                fill_rect(buffer, width, height, &rect, color);
                draw_text(buffer, width, height,
                    if on { "ON" } else { "OFF" },
                    rect.x + 16, rect.y + 8, COLOR_BUTTON_TEXT);
            }
            _ => {}
        }
    }
}

fn fill_rect(buffer: &mut [u32], w: usize, h: usize, r: &Rect, color: u32) {
    let x0 = r.x.max(0) as usize;
    let y0 = r.y.max(0) as usize;
    let x1 = ((r.x + r.w).max(0) as usize).min(w);
    let y1 = ((r.y + r.h).max(0) as usize).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            buffer[y * w + x] = color;
        }
    }
}
```

- [ ] **Step 4: 빌드 sanity**

```bash
cargo build -p geulos-compositor
```

폰트 파일이 없으면 컴파일 실패. *implementer는 적절한 .ttf 파일을 `compositor/fonts/font.ttf`에 두어야 함.* 예: Windows의 `C:\Windows\Fonts\arial.ttf` 복사. 또는 nice cross-platform 옵션: `JetBrainsMono-Regular.ttf` (오픈 라이선스).

- [ ] **Step 5: 커밋**

```bash
git add -A
git commit -m "feat(compositor): fontdue 텍스트 + softbuffer 렌더링"
```

---

## Task 7: hit-test (마우스 클릭 → ObjectId)

**Files:**
- Modify: `compositor/src/hit_test.rs`

- [ ] **Step 1: 구현**

```rust
//! 좌표 → 클릭 가능한 ObjectId (가장 안쪽).

use geulos_core::ObjectId;

use crate::layout::LayoutResult;
use crate::tree_model::TreeModel;

/// 주어진 좌표에 있는 클릭 가능한 객체를 반환.
///
/// "클릭 가능"의 정의: methods가 비어있지 않은 객체 (즉 Button 등).
/// Container는 클릭 통과시킴.
pub fn hit_test(tree: &TreeModel, layout: &LayoutResult, px: i32, py: i32) -> Option<ObjectId> {
    // layout.rects는 부모-자식 순서로 출력되므로 *뒤에서부터* 검사 (자식이 위)
    let mut candidates: Vec<_> = layout.iter().collect();
    candidates.reverse();
    for (id, rect) in candidates {
        if rect.contains(px, py) {
            if let Some(obj) = tree.get(id) {
                if !obj.methods.is_empty() {
                    return Some(id);
                }
            }
        }
    }
    None
}
```

- [ ] **Step 2: 테스트 (`compositor/tests/layout_test.rs` 끝에 추가)**

```rust
use geulos_compositor::hit_test::hit_test;

#[test]
fn hit_test_finds_button_not_container() {
    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    let mut c = std_types::container(owner.clone());
    let mut text = std_types::text(owner.clone(), "x");
    let mut button = std_types::button(owner, "press me");

    let c_id = c.id;
    let text_id = text.id;
    let button_id = button.id;

    text.parent = Some(c_id);
    button.parent = Some(c_id);
    c.children.push(text_id);
    c.children.push(button_id);
    tm.upsert(c);
    tm.upsert(text);
    tm.upsert(button);

    let r = layout(&tm, 800, 600);
    let btn_rect = r.get(button_id).unwrap();
    let cx = btn_rect.x + 10;
    let cy = btn_rect.y + 10;
    let hit = hit_test(&tm, &r, cx, cy);
    assert_eq!(hit, Some(button_id), "Button을 hit해야 함, Container를 지나가야 함");
}
```

- [ ] **Step 3: 통과 + 커밋**

```bash
cargo test -p geulos-compositor
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -m "feat(compositor): hit-test (좌표 → 클릭 가능 ObjectId)"
```

---

## Task 8: main.rs 통합 — 윈도우 + 트리 + 렌더 + 입력 + 서버 스레드

이 태스크는 *모든 모듈을 묶는다*. 비교적 분량 큰 코드.

**Files:**
- Modify: `compositor/src/main.rs`

- [ ] **Step 1: 구현**

```rust
//! GeulOS 컴포지터 메인.

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use geulos_compositor::hit_test::hit_test;
use geulos_compositor::layout::layout;
use geulos_compositor::messages::{ServerEvent, UiAction};
use geulos_compositor::render::render_frame;
use geulos_compositor::server_client::{run_server_client, UserEvent};
use geulos_compositor::tree_model::TreeModel;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

struct App {
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    tree: Arc<Mutex<TreeModel>>,
    ui_tx: tokio::sync::mpsc::Sender<UiAction>,
    cursor: (f64, f64),
}

impl App {
    fn new(tree: Arc<Mutex<TreeModel>>, ui_tx: tokio::sync::mpsc::Sender<UiAction>) -> Self {
        Self {
            window: None,
            surface: None,
            tree,
            ui_tx,
            cursor: (0.0, 0.0),
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title("GeulOS Compositor (M4)")
            .with_inner_size(PhysicalSize::new(800u32, 600u32));
        let window = Arc::new(event_loop.create_window(attrs).expect("create_window"));
        let context = softbuffer::Context::new(window.clone()).expect("Context");
        let surface = softbuffer::Surface::new(&context, window.clone()).expect("Surface");
        self.window = Some(window);
        self.surface = Some(surface);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Redraw => {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            UserEvent::Quit => _event_loop.exit(),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                let _ = self.ui_tx.try_send(UiAction::Quit);
                event_loop.exit();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                let (cx, cy) = (self.cursor.0 as i32, self.cursor.1 as i32);
                if let Some(window) = &self.window {
                    let size = window.inner_size();
                    let tree = self.tree.lock().unwrap();
                    let lay = layout(&tree, size.width as i32, size.height as i32);
                    if let Some(target) = hit_test(&tree, &lay, cx, cy) {
                        if let Some(obj) = tree.get(target) {
                            // 첫 번째 메서드를 호출 (간단)
                            if let Some(m) = obj.methods.first() {
                                let _ = self.ui_tx.try_send(UiAction::Invoke {
                                    target,
                                    method: m.name().to_string(),
                                    args: serde_json::Value::Null,
                                });
                            }
                        }
                    }
                }
            }
            WindowEvent::Resized(_) => {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let (Some(window), Some(surface)) = (&self.window, &mut self.surface) {
                    let size = window.inner_size();
                    let (w, h) = (size.width, size.height);
                    if w == 0 || h == 0 { return; }
                    surface.resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
                        .expect("resize");
                    let mut buffer = surface.buffer_mut().expect("buffer_mut");
                    let tree = self.tree.lock().unwrap();
                    let lay = layout(&tree, w as i32, h as i32);
                    render_frame(&tree, &lay, &mut buffer, w as usize, h as usize);
                    buffer.present().expect("present");
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:5550".to_string());

    let event_loop: EventLoop<UserEvent> = EventLoop::with_user_event().build().expect("EventLoop");
    let proxy = Arc::new(event_loop.create_proxy());

    let tree: Arc<Mutex<TreeModel>> = Arc::new(Mutex::new(TreeModel::new()));
    let (ui_tx, ui_rx) = tokio::sync::mpsc::channel::<UiAction>(64);
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<ServerEvent>(64);

    // tokio 런타임 스레드
    let server_addr = addr.clone();
    let proxy_for_tokio = proxy.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async move {
            if let Err(e) = run_server_client(server_addr, event_tx, ui_rx, proxy_for_tokio).await {
                eprintln!("[compositor] server_client error: {}", e);
            }
        });
    });

    // event_rx → tree 갱신 스레드
    let tree_for_events = tree.clone();
    let proxy_for_events = proxy.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            while let Some(ev) = event_rx.recv().await {
                let mut tm = tree_for_events.lock().unwrap();
                match ev {
                    ServerEvent::ObjectUpserted(o) => tm.upsert(o),
                    ServerEvent::ObjectRemoved(id) => tm.remove(id),
                    ServerEvent::StateSet { id, key, value } => tm.set_state(id, key, value),
                    ServerEvent::Disconnected => {
                        let _ = proxy_for_events.send_event(UserEvent::Quit);
                        break;
                    }
                }
                drop(tm);
                let _ = proxy_for_events.send_event(UserEvent::Redraw);
            }
        });
    });

    let mut app = App::new(tree, ui_tx);
    event_loop.run_app(&mut app).expect("run_app");
}
```

- [ ] **Step 2: 빌드 + 수동 sanity**

```bash
cargo build -p geulos-compositor
```

빌드만 확인. 실행은 *서버 + echo-app + 컴포지터* 3개 동시 실행이 필요해 Task 9 acceptance에서.

- [ ] **Step 3: 커밋**

```bash
git add -A
git commit -m "feat(compositor): main.rs 통합 — winit + 서버 스레드 + 입력 처리"
```

---

## Task 9: M4 수동 acceptance

**Files:**
- 코드 변경 없음. 수동 시나리오만.

- [ ] **Step 1: 3터미널 시나리오**

터미널 A:
```powershell
cd C:\AiOS
cargo run -p geulos-server-host
```

터미널 B:
```powershell
cd C:\AiOS
cargo run -p geulos-echo-app
```

터미널 C:
```powershell
cd C:\AiOS
cargo run -p geulos-compositor
```

- [ ] **Step 2: 검증 (사람이 시각적으로 확인)**

컴포지터 윈도우에 다음이 나타나야 함:
- 회색 컨테이너 배경
- 그 안에 "count: 0" 텍스트
- 그 아래에 파란 "+1" 버튼

마우스로 파란 버튼을 클릭 → 텍스트가 "count: 1"으로 갱신.
연달아 클릭 → "count: 2", "count: 3", ...

이게 *시나리오 C 대칭성*의 시각적 증명. 외부 클라이언트가 보내는 Invoke와 사용자 클릭이 같은 결과를 일으킴.

- [ ] **Step 3: 추가 검증 (4번째 터미널, 선택)**

```powershell
cargo run -p geulos-shell -- --connect 127.0.0.1:5550
> query type aios.std/Button@1
# button ID 받아서
> invoke <button-id> press
```

→ 컴포지터 창의 카운트도 함께 증가해야 함. 외부 호출과 마우스 클릭이 진짜로 같은 경로.

- [ ] **Step 4: 결과 기록**

```bash
# 스크린샷이 있다면 docs/screenshots/m4_acceptance.png에 저장 (선택)
# 또는 콘솔 로그를 capture해서 PR description에 포함

# 적어도 *언어로 기록*
git add -A
git commit -m "docs: M4 acceptance 수동 검증 완료 표시" --allow-empty
```

---

## Task 10: 최종 스모크 + 푸시

- [ ] **Step 1: 검증**

```bash
cargo build --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

- [ ] **Step 2: 푸시**

```bash
git push origin main
```

- [ ] **Step 3: CI 그린 확인**

CI에서 컴포지터 실행 테스트는 *없음* (window 없음). 단위/통합 테스트만.

- [ ] **Step 4: M4 완료 선언**

다음이 모두 사실:
- 호스트 OS 윈도우에 객체 트리가 그려짐
- 마우스 클릭이 hit-test → Invoke로 변환되어 서버에 전달됨
- echo-app의 카운터가 사람 클릭으로 증가하고 화면에 반영됨
- 외부 클라이언트의 동일 호출과 결과 일치 (시나리오 C 대칭성)
- 56+ 단위/통합 테스트 + 신규 컴포지터 테스트 모두 PASS
- CI 그린

**원래의 GeulOS 비전: AI에게 점자 설명서를 주는 OS — 이제 점자뿐 아니라 사람의 눈에도 보인다.**

M5 (글 AI I/O 드라이버) 진입 준비 완료.

---

## 자체 점검 결과

**스펙 커버리지:**
- 설계 §9.2 M4 산출물 5개 매핑:
  - 컴포지터 별 프로세스 (Task 2)
  - 객체 트리 → 그래픽 변환 (Task 6 render.rs)
  - 입력 → 이벤트 변환 (Task 7, 8)
  - 가장 기본적인 레이아웃 (Task 5)
  - 데모 시나리오 (Task 9)

**플레이스홀더 스캔:** TBD/TODO 없음. 폰트 파일 두는 위치는 implementer 결정사항으로 *명시*.

**알려진 한계 (M4 범위 밖):**
- 한글 표시: 폰트가 한글 자모를 포함해야 함. 임베드 폰트 선택 시 주의.
- 다중 모니터/DPI 스케일링 미고려.
- 컴포지터가 죽으면 echo-app도 별 메시지 없이 disconnect됨 — 향후 graceful shutdown.
- wgpu/GPU 가속 안 함 (ADR-013로 의도적 연기).
- 컴포지터의 모든 객체 *poll-and-get* 방식 — server에서 새 객체가 mount되면 컴포지터는 알아차리지 못함. M4 후속 PR로 "all-objects subscribe" 추가 필요.
