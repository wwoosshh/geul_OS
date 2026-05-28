# VM 컴포지터 A (실제 트리 렌더 + 클릭 왕복) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** VM 게스트 안에서 컴포지터가 geulosd에 접속해 실제 객체 트리(echo-app UI)를 `render_frame`으로 `/dev/fb0`에 그리고, 마우스 클릭을 `hit_test`→`dispatch_click`→Invoke로 서버까지 왕복시킨다.

**Architecture:** `server_client`의 winit 결합(EventLoopProxy)을 mpsc 채널로 떼어내 호스트·VM 공용으로 만들고, `dispatch_click`을 lib로 옮긴 뒤, skeleton bin을 실제 VM 컴포지터(서버연결 + tree 공유 + fb 렌더 + evdev 클릭 라우팅)로 교체한다. 호스트 컴포지터는 mpsc→winit forwarder로 무회귀 유지.

**Tech Stack:** Rust (musl 크로스 컴파일), tokio mpsc, 기존 vm_fb/vm_input, render_frame/layout/hit_test, QEMU virtio-gpu/virtio-input.

---

## File Structure

| 파일 | 책임 | 변경 |
|---|---|---|
| `compositor/src/dispatch.rs` | 클릭 → UiAction 변환 (dispatch_click + 헬퍼) | Create (main.rs에서 이동) |
| `compositor/src/lib.rs` | 모듈 선언 | Modify — `dispatch` 추가, `server_client` cfg 게이트 해제 |
| `compositor/src/server_client.rs` | 서버 TCP 클라 | Modify — winit proxy → mpsc notify |
| `compositor/src/main.rs` | 호스트 winit 컴포지터 | Modify — dispatch_click 제거(import), mpsc+forwarder |
| `compositor/src/bin/geulos-vm-compositor.rs` | VM 컴포지터 진입점 | Create (skeleton 교체) |
| `compositor/src/bin/geulos-vm-skeleton.rs` | (구) 증명 bin | Delete |
| `compositor/Cargo.toml` | bin 선언 | Modify — skeleton→compositor |
| `geulos-init/src/spawn.rs` | 자식 spawn | Modify — skeleton→compositor |
| `boot/build.ps1` | 크로스 컴파일+initrd | Modify — bin 이름 |

각 태스크는 working tree가 빌드되는 상태로 끝난다. 핵심: 시그니처가 바뀌는 server_client(Task 2)는 그 호출자 main.rs를 같은 태스크에서 고친다.

---

## Task 1: dispatch_click을 lib로 이동

**Files:**
- Create: `compositor/src/dispatch.rs`
- Modify: `compositor/src/lib.rs` (`pub mod dispatch;`)
- Modify: `compositor/src/main.rs` (3 fn 제거 + import)

`dispatch_click`/`find_file_tree`/`find_explorer`는 호스트·VM 공용이라 lib로 옮긴다. (`find_scroll_target`/`max_scroll_y_for`/`find_cli_object_id`는 스크롤·키보드용 — 조각 C 대상, main.rs에 남긴다.)

- [ ] **Step 1: dispatch.rs 생성**

`compositor/src/dispatch.rs`:

```rust
//! 클릭 dispatch — Folder/File/Window 등 타입별로 UiAction 생성.
//! main.rs(winit)와 VM 컴포지터가 공유.

use geulos_core::{Object, ObjectId};

use crate::layout::HitRole;
use crate::messages::UiAction;
use crate::tree_model::TreeModel;

/// 클릭 dispatch — 타입별 UiAction 생성.
///
/// - `aios.std/Folder@1`: ExpandToggle → FileTree expand/collapse, Body → Explorer.navigate_to.
/// - `aios.std/File@1`: Explorer.open_file.
/// - 그 외 (echo-app 호환): 첫 메서드를 args=null로 호출.
pub fn dispatch_click(
    tree: &TreeModel,
    target: ObjectId,
    obj: &Object,
    role: HitRole,
) -> Vec<UiAction> {
    match obj.type_uri.as_str() {
        "aios.std/Folder@1" => {
            let mut actions = Vec::new();
            if role == HitRole::ExpandToggle {
                if let Some(ft) = find_file_tree(tree) {
                    let is_expanded =
                        ft.state.get("expanded").and_then(|v| v.as_array()).is_some_and(|arr| {
                            arr.iter().any(|v| v.as_str() == Some(&target.to_string()))
                        });
                    actions.push(UiAction::Invoke {
                        target: ft.id,
                        method: if is_expanded { "collapse" } else { "expand" }.to_string(),
                        args: serde_json::json!({ "id": target.to_string() }),
                    });
                }
            } else if let Some(explorer) = find_explorer(tree) {
                actions.push(UiAction::Invoke {
                    target: explorer.id,
                    method: "navigate_to".to_string(),
                    args: serde_json::json!({ "folder_id": target.to_string() }),
                });
            }
            actions
        }
        "aios.std/File@1" => {
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
        _ => {
            if let Some(m) = obj.methods.first() {
                vec![UiAction::Invoke {
                    target,
                    method: m.name().to_string(),
                    args: serde_json::Value::Null,
                }]
            } else {
                vec![]
            }
        }
    }
}

pub fn find_file_tree(tree: &TreeModel) -> Option<&Object> {
    for id in tree.ids() {
        if let Some(o) = tree.get(id) {
            if o.type_uri.as_str() == "aios.builtin/FileTree@1" {
                return Some(o);
            }
        }
    }
    None
}

pub fn find_explorer(tree: &TreeModel) -> Option<&Object> {
    for id in tree.ids() {
        if let Some(o) = tree.get(id) {
            if o.type_uri.as_str() == "aios.builtin/Explorer@1" {
                return Some(o);
            }
        }
    }
    None
}
```

- [ ] **Step 2: lib.rs에 모듈 선언**

`compositor/src/lib.rs`의 `pub mod editor;` 위(알파벳 순)나 아무 곳에 추가:

```rust
pub mod dispatch;
```

- [ ] **Step 3: main.rs에서 3개 fn 제거 + import 추가**

`compositor/src/main.rs`에서 `fn dispatch_click(...) { ... }`, `fn find_file_tree(...) { ... }`, `fn find_explorer(...) { ... }` 세 함수 정의를 **삭제**한다. (`find_scroll_target`, `max_scroll_y_for`는 남긴다.)

`main.rs` 상단 use 블록에 추가 (세 개 모두 — `find_scroll_target`이 `find_file_tree`/`find_explorer`를 호출하고, 클릭 핸들러가 `dispatch_click`을 호출):

```rust
use geulos_compositor::dispatch::{dispatch_click, find_explorer, find_file_tree};
```

(import하면 기존 호출부 `dispatch_click(...)`, `find_file_tree(tree)`, `find_explorer(tree)`가 그대로 lib 함수로 해석된다 — 호출부 수정 불필요.)

- [ ] **Step 4: 호스트 빌드 확인**

Run:
```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo build -p geulos-compositor
```
Expected: `Finished` (미사용 import/함수 경고 없이).

- [ ] **Step 5: 커밋**

```powershell
git add compositor/src/dispatch.rs compositor/src/lib.rs compositor/src/main.rs
git commit -m "refactor(compositor): dispatch_click을 lib로 이동 (호스트·VM 공용)"
```

---

## Task 2: server_client winit 분리 + 호스트 forwarder

**Files:**
- Modify: `compositor/src/server_client.rs` (proxy → mpsc)
- Modify: `compositor/src/lib.rs` (cfg 게이트 해제)
- Modify: `compositor/src/main.rs` (mpsc + forwarder)

시그니처가 바뀌므로 호출자 main.rs를 같은 태스크에서 고쳐 빌드를 green으로 유지.

- [ ] **Step 1: server_client.rs 시그니처/본문 교체**

`compositor/src/server_client.rs`:

1. import 교체 — `use winit::event_loop::EventLoopProxy;` 삭제. `use std::sync::Arc;`도 삭제(이제 미사용).
2. `run_server_client` 시그니처:
```rust
pub async fn run_server_client(
    addr: String,
    event_tx: mpsc::Sender<ServerEvent>,
    mut ui_rx: mpsc::Receiver<UiAction>,
    notify: mpsc::UnboundedSender<UserEvent>,
) -> Result<(), String> {
```
3. 본문의 모든 `let _ = proxy.send_event(UserEvent::Redraw);` → `let _ = notify.send(UserEvent::Redraw);` (3곳: HelloAck 직후, Get 루프 후, select! stream 분기 내 handle_server_frame 호출 뒤).
4. `UiAction::Quit` 분기의 `let _ = proxy.send_event(UserEvent::Quit);` → `let _ = notify.send(UserEvent::Quit);`.

(`UserEvent` enum 정의는 그대로 둔다 — winit과 무관한 순수 enum.)

- [ ] **Step 2: lib.rs cfg 게이트 해제**

`compositor/src/lib.rs`:
```rust
pub mod server_client;
```
(`#[cfg(not(target_os = "linux"))]` 줄 삭제 — 이제 모든 타겟에서 컴파일.)

- [ ] **Step 3: 호스트 main.rs 적응 (mpsc + forwarder)**

`compositor/src/main.rs`의 `main()`에서 server_client 호출 부분을 교체한다. 기존:
```rust
    let server_addr = addr.clone();
    let proxy_for_tokio = proxy.clone();
    std::thread::spawn(move || {
        let rt =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        rt.block_on(async move {
            if let Err(e) = run_server_client(server_addr, event_tx, ui_rx, proxy_for_tokio).await {
                eprintln!("[compositor] server_client error: {}", e);
            }
        });
    });
```
교체 후:
```rust
    // server_client는 이제 winit-free — UserEvent를 mpsc로 보낸다. forwarder가 winit proxy로 중계.
    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::unbounded_channel::<UserEvent>();
    let server_addr = addr.clone();
    std::thread::spawn(move || {
        let rt =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        rt.block_on(async move {
            if let Err(e) = run_server_client(server_addr, event_tx, ui_rx, notify_tx).await {
                eprintln!("[compositor] server_client error: {}", e);
            }
        });
    });
    // forwarder: mpsc UserEvent → winit EventLoopProxy.
    let proxy_for_fwd = proxy.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            while let Some(ev) = notify_rx.recv().await {
                let _ = proxy_for_fwd.send_event(ev);
            }
        });
    });
```

(`event_rx` 갱신 스레드는 변경 없음 — 거기서 `proxy_for_events.send_event(UserEvent::Redraw)`는 그대로 winit proxy 직접 사용.)

- [ ] **Step 4: 호스트 빌드 + 워크스페이스 테스트**

Run:
```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo build -p geulos-compositor
cargo test --workspace
```
Expected: 빌드 `Finished`, 모든 `test result: ok` (특히 server_client의 `std_types_query_coverage_smoke`).

- [ ] **Step 5: musl lib 빌드 (server_client가 musl로 컴파일되는지)**

Run:
```powershell
cargo build --target x86_64-unknown-linux-musl -p geulos-compositor --lib
```
Expected: `Finished`.

- [ ] **Step 6: 커밋**

```powershell
git add compositor/src/server_client.rs compositor/src/lib.rs compositor/src/main.rs
git commit -m "refactor(compositor): server_client winit 분리 (proxy→mpsc) + 호스트 forwarder"
```

---

## Task 3: VM 컴포지터 bin (skeleton 교체)

**Files:**
- Create: `compositor/src/bin/geulos-vm-compositor.rs`
- Delete: `compositor/src/bin/geulos-vm-skeleton.rs`
- Modify: `compositor/Cargo.toml` (`[[bin]]` 이름)

- [ ] **Step 1: Cargo.toml bin 이름 교체**

`compositor/Cargo.toml`의 skeleton `[[bin]]`을 교체:
```toml
[[bin]]
name = "geulos-vm-compositor"
path = "src/bin/geulos-vm-compositor.rs"
```

- [ ] **Step 2: VM 컴포지터 작성**

`compositor/src/bin/geulos-vm-compositor.rs`:

```rust
//! VM 컴포지터 — geulosd에 접속해 실제 트리를 /dev/fb0에 render_frame으로 그리고,
//! evdev 좌클릭을 hit_test→dispatch_click→Invoke로 서버까지 왕복.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("geulos-vm-compositor는 VM(Linux) 전용입니다. 호스트는 geulos-compositor를 쓰세요.");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use geulos_compositor::dispatch::dispatch_click;
    use geulos_compositor::hit_test::hit_test;
    use geulos_compositor::keyboard::CliLocalState;
    use geulos_compositor::layout::layout;
    use geulos_compositor::messages::{ServerEvent, UiAction};
    use geulos_compositor::render::render_frame;
    use geulos_compositor::server_client::{run_server_client, UserEvent};
    use geulos_compositor::tree_model::TreeModel;
    use geulos_compositor::vm_fb::Framebuffer;
    use geulos_compositor::vm_input::{
        scale_abs, EvdevSet, ABS_X, ABS_Y, BTN_LEFT, EV_ABS, EV_KEY, TABLET_LOGICAL_MAX,
    };

    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:5550".to_string());
    println!("[vm-compositor] starting, server={}", addr);

    let tree: Arc<Mutex<TreeModel>> = Arc::new(Mutex::new(TreeModel::new()));
    let (ui_tx, ui_rx) = tokio::sync::mpsc::channel::<UiAction>(64);
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<ServerEvent>(64);
    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::unbounded_channel::<UserEvent>();
    let quit = Arc::new(AtomicBool::new(false));

    // 1) server_client (tokio)
    let server_addr = addr.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            if let Err(e) = run_server_client(server_addr, event_tx, ui_rx, notify_tx).await {
                eprintln!("[vm-compositor] server_client error: {}", e);
            }
        });
    });

    // 2) event_rx → tree 갱신
    let tree_for_events = tree.clone();
    let quit_for_events = quit.clone();
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
                        quit_for_events.store(true, Ordering::SeqCst);
                        break;
                    }
                }
            }
        });
    });

    // 3) notify_rx → Quit 시 종료 (Redraw는 always-redraw라 무시)
    let quit_for_notify = quit.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            while let Some(ev) = notify_rx.recv().await {
                if let UserEvent::Quit = ev {
                    quit_for_notify.store(true, Ordering::SeqCst);
                    break;
                }
            }
        });
    });

    // 4) 메인 루프 — fb 렌더 + evdev 클릭
    let mut fb = match Framebuffer::open() {
        Ok(fb) => fb,
        Err(e) => {
            eprintln!("[vm-compositor] framebuffer 실패: {}", e);
            std::process::exit(2);
        }
    };
    println!("[vm-compositor] fb {}x{} {:?}", fb.xres, fb.yres, fb.format());
    let mut input = match EvdevSet::open_all() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("[vm-compositor] evdev 실패: {}", e);
            std::process::exit(3);
        }
    };

    let (w, h) = (fb.xres, fb.yres);
    let mut canvas = vec![0u32; w * h];
    let mut pointer = (w as i32 / 2, h as i32 / 2);
    let cli_state = CliLocalState::default();

    while !quit.load(Ordering::SeqCst) {
        // 입력
        input.poll_events(16, |ev| {
            if ev.type_ == EV_ABS && ev.code == ABS_X {
                pointer.0 = scale_abs(ev.value, TABLET_LOGICAL_MAX, w as u32);
            } else if ev.type_ == EV_ABS && ev.code == ABS_Y {
                pointer.1 = scale_abs(ev.value, TABLET_LOGICAL_MAX, h as u32);
            } else if ev.type_ == EV_KEY && ev.code == BTN_LEFT && ev.value == 1 {
                // 클릭 → hit_test → dispatch_click → Invoke. lock guard 범위 최소화.
                let actions = {
                    let tm = tree.lock().unwrap();
                    let lay = layout(&tm, w as i32, h as i32);
                    if let Some((target, role)) = hit_test(&tm, &lay, pointer.0, pointer.1) {
                        if let Some(obj) = tm.get(target) {
                            dispatch_click(&tm, target, obj, role)
                        } else {
                            vec![]
                        }
                    } else {
                        vec![]
                    }
                };
                for a in actions {
                    println!("[vm-compositor] click@({},{}) → {:?}", pointer.0, pointer.1, a);
                    let _ = ui_tx.try_send(a);
                }
            }
        });

        // 렌더
        {
            let tm = tree.lock().unwrap();
            let lay = layout(&tm, w as i32, h as i32);
            render_frame(&tm, &lay, &mut canvas, w, h, &cli_state, None);
        }
        fb.present(&canvas);
        std::thread::sleep(Duration::from_millis(16));
    }
    println!("[vm-compositor] exit");
}
```

- [ ] **Step 3: skeleton 삭제**

```powershell
Remove-Item compositor/src/bin/geulos-vm-skeleton.rs
```

- [ ] **Step 4: musl --bin 빌드**

Run:
```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo build --target x86_64-unknown-linux-musl --release -p geulos-compositor --bin geulos-vm-compositor
```
Expected: `Finished`. 산출물 `target/x86_64-unknown-linux-musl/release/geulos-vm-compositor`.

- [ ] **Step 5: 호스트 빌드 (stub main) + 커밋**

Run:
```powershell
cargo build -p geulos-compositor --bin geulos-vm-compositor
```
Expected: `Finished`.

```powershell
git add compositor/Cargo.toml compositor/src/bin/geulos-vm-compositor.rs
git rm compositor/src/bin/geulos-vm-skeleton.rs
git commit -m "feat(compositor): geulos-vm-compositor — 실제 트리 fb 렌더 + 클릭 왕복 (skeleton 교체)"
```

---

## Task 4: init/build.ps1 — skeleton → compositor

**Files:**
- Modify: `geulos-init/src/spawn.rs`
- Modify: `boot/build.ps1`

- [ ] **Step 1: spawn.rs 교체**

`geulos-init/src/spawn.rs`에서 skeleton spawn 부분을 교체:

```rust
    println!("[init] spawning /bin/geulos-vm-compositor ...");
    let skeleton = match Command::new("/bin/geulos-vm-compositor").arg("127.0.0.1:5550").spawn() {
        Ok(child) => {
            println!("[init] vm-compositor PID = {}", child.id());
            Some(child)
        }
        Err(e) => {
            eprintln!("[init] vm-compositor spawn failed: {} (continuing)", e);
            None
        }
    };
```

(`SpawnedProcesses.skeleton` 필드 이름은 그대로 둬도 무방 — 또는 `compositor`로 rename. 최소 변경 위해 필드명 유지.)

- [ ] **Step 2: build.ps1 bin 이름 교체**

`boot/build.ps1`에서:
- 크로스 컴파일: `--bin geulos-vm-skeleton` → `--bin geulos-vm-compositor` (2곳: Release/debug 분기). throw 메시지도 갱신.
- `$SkeletonBin = Join-Path $BinDir "geulos-vm-skeleton"` → `"geulos-vm-compositor"`.
- `Write-Host "  built: ... geulos-vm-skeleton"` → `geulos-vm-compositor`.
- `Copy-Item $SkeletonBin (Join-Path $StageDir "bin/geulos-vm-skeleton")` → `"bin/geulos-vm-compositor"`.

- [ ] **Step 3: musl init 빌드 + 전체 이미지 빌드**

Run:
```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;C:\Program Files\qemu;$env:PATH"
cargo build --target x86_64-unknown-linux-musl -p geulos-init
pwsh boot/build.ps1 -Release
```
Expected: init `Finished`; build.ps1 `built: geulos-init, geulosd, geulos-echo-app, geulos-vm-compositor` + initrd 조립 성공.

- [ ] **Step 4: 커밋**

```powershell
git add geulos-init/src/spawn.rs boot/build.ps1
git commit -m "build(boot): init/build를 geulos-vm-compositor로 (skeleton 교체)"
```

---

## Task 5: 통합 부팅 + 시각 확인

**Files:** 없음 (실행/관찰)

- [ ] **Step 1: 그래픽 모드 부팅**

Run (사용자가 직접 — 창을 봐야 함):
```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;C:\Program Files\qemu;$env:PATH"
pwsh boot/qemu/launch.ps1 -Graphics
```
Expected: QEMU 창에 echo-app UI — 컨테이너 박스 + "count: 0" 텍스트 + "+1" 버튼이 `render_frame`으로 그려짐.

- [ ] **Step 2: 버튼 클릭 → count 증가 확인**

echo-app "+1" 버튼을 클릭.
Expected: 화면의 "count: 0" → "count: 1" → "count: 2" … 클릭마다 증가.

- [ ] **Step 3: 직렬 로그 확인**

Run:
```powershell
Get-Content boot/serial.log -Tail 30
```
Expected 핵심:
```
[init] spawning /bin/geulos-vm-compositor ...
[vm-compositor] starting, server=127.0.0.1:5550
[vm-compositor] fb 1280x800 PixelFormat { ... }
[vm-compositor] click@(x,y) → Invoke { ... method: "press" ... }
```

- [ ] **Step 4: 합격 판정 + 종료**

사용자가 (1) echo-app UI가 보이고 (2) 버튼 클릭 시 숫자가 증가하는 것을 확인하면 **합격**. 종료:
```powershell
Get-Process qemu-system-x86_64 -ErrorAction SilentlyContinue | Stop-Process -Force
```

실패 시 systematic-debugging: 텍스트 안 보임 → 폰트 musl embed 확인 / 버튼 클릭 무반응 → 직렬 로그의 click 로그 + Invoke 송신 확인 / 화면 빈 채 → server_client 접속 로그 확인.

---

## Self-Review

**Spec coverage:**
- server_client winit 분리(mpsc) → Task 2 ✓
- dispatch_click lib 이동 → Task 1 ✓
- 호스트 main.rs forwarder 무회귀 → Task 2 ✓
- VM 컴포지터 bin(서버연결+tree+fb렌더+클릭라우팅, skeleton 교체) → Task 3 ✓
- init/build 갱신 → Task 4 ✓
- 성공 기준(echo-app UI + 버튼 클릭 count 증가) → Task 5 ✓
- 호스트 무회귀 검증 → Task 2 Step 4 (워크스페이스 테스트) ✓

**Placeholder scan:** 코드 블록 모두 완전. TBD 없음.

**Type consistency:**
- `run_server_client(addr, event_tx, ui_rx, notify: mpsc::UnboundedSender<UserEvent>)` — Task 2 정의, Task 3 호출 일치 ✓
- `dispatch_click(&TreeModel, ObjectId, &Object, HitRole) -> Vec<UiAction>` — Task 1 정의, Task 3 호출 일치 ✓
- `render_frame(&tm, &lay, &mut canvas, w, h, &cli_state, None)` — 실제 시그니처(`cli_state: &CliLocalState`, `editor: Option<&EditorState>`)와 일치 ✓
- `Framebuffer::open/present/format/xres/yres`, `EvdevSet::open_all/poll_events`, `scale_abs`, 상수 — 기존 vm_fb/vm_input과 일치 ✓
- `UserEvent::{Redraw, Quit}` — server_client 정의, Task 3 매칭 일치 ✓

**알려진 단순화 (비-목표와 일치):** 클릭만(키보드/스크롤 X), always-redraw, CLI/editor None.
