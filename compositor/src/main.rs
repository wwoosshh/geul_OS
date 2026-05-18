//! GeulOS 컴포지터 메인.

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use geulos_compositor::hit_test::hit_test;
use geulos_compositor::keyboard::{CliLocalState, KeyAction};
use geulos_compositor::layout::{layout, LayoutResult};
use geulos_compositor::messages::{ServerEvent, UiAction};
use geulos_compositor::render::render_frame;
use geulos_compositor::server_client::{run_server_client, UserEvent};
use geulos_compositor::tree_model::TreeModel;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

struct App {
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    tree: Arc<Mutex<TreeModel>>,
    ui_tx: tokio::sync::mpsc::Sender<UiAction>,
    cursor: (f64, f64),
    /// T7.5: 컴포지터-사이드 CLI 입력 버퍼/커서. server tree와 분리.
    cli_state: CliLocalState,
}

impl App {
    fn new(tree: Arc<Mutex<TreeModel>>, ui_tx: tokio::sync::mpsc::Sender<UiAction>) -> Self {
        Self {
            window: None,
            surface: None,
            tree,
            ui_tx,
            cursor: (0.0, 0.0),
            cli_state: CliLocalState::default(),
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

    fn user_event(&mut self, event_loop: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Redraw => {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            UserEvent::Quit => event_loop.exit(),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                let _ = self.ui_tx.try_send(UiAction::Quit);
                event_loop.exit();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let (cx, cy) = (self.cursor.0 as i32, self.cursor.1 as i32);
                if let Some(window) = &self.window {
                    let size = window.inner_size();
                    let tree = self.tree.lock().unwrap();
                    let lay = layout(&tree, size.width as i32, size.height as i32);
                    if let Some(target) = hit_test(&tree, &lay, cx, cy) {
                        if let Some(obj) = tree.get(target) {
                            let actions =
                                dispatch_click(&tree, &lay, target, obj, size.width as i32);
                            for action in actions {
                                let _ = self.ui_tx.try_send(action);
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
            // T7.5: 키보드 입력 — focused 객체 개념 부재. CLI만 받는다고 가정.
            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key, state: ElementState::Pressed, text, .. },
                ..
            } => {
                let action = key_event_to_action(&logical_key, text.as_deref());
                if let Some(action) = action {
                    let submitted = self.cli_state.handle_key(action);
                    if let Some(submitted_text) = submitted {
                        // Enter — submit_input invoke로 commit.
                        // lock guard 보유 시간을 최소화하기 위해 ID만 추출 후 즉시 drop.
                        let cli_id = {
                            let tree = self.tree.lock().unwrap();
                            find_cli_object_id(&tree)
                        };
                        if let Some(cli_id) = cli_id {
                            let _ = self.ui_tx.try_send(UiAction::Invoke {
                                target: cli_id,
                                method: "submit_input".to_string(),
                                args: serde_json::json!({ "text": submitted_text }),
                            });
                        }
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
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
                    let tree = self.tree.lock().unwrap();
                    let lay = layout(&tree, w as i32, h as i32);
                    render_frame(&tree, &lay, &mut buffer, w as usize, h as usize, &self.cli_state);
                    buffer.present().expect("present");
                }
            }
            _ => {}
        }
    }
}

/// winit KeyEvent → keyboard::KeyAction 변환 (T7.5 ASCII v1).
///
/// 우선순위: NamedKey(Enter/Backspace) → text (문자 입력). 한글/IME는 T7.6.
fn key_event_to_action(logical_key: &Key, text: Option<&str>) -> Option<KeyAction> {
    if let Key::Named(named) = logical_key {
        match named {
            NamedKey::Enter => return Some(KeyAction::Submit),
            NamedKey::Backspace => return Some(KeyAction::Backspace),
            _ => {}
        }
    }
    // 문자 입력 — winit이 제공한 text 사용 (단일 char만). 다중 문자(IME pre-edit 등)는
    // T7.6에서 처리.
    let s = text?;
    let mut chars = s.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        // TODO(T7.6): IME pre-edit 다중 문자 처리 — 현재는 한글 무반응.
        return None; // 다중 문자는 일단 무시 (안전).
    }
    Some(KeyAction::InsertChar(c))
}

/// 트리에서 Cli 객체의 ID만 찾아 반환 (한 개만 존재 가정 — ADR-023).
///
/// 전체 `Object`를 clone하지 않기 위해 ID만 반환 — `state.lines`가 1000라인까지
/// 자라므로 clone 비용을 매 Enter마다 지불할 이유가 없다.
fn find_cli_object_id(tree: &TreeModel) -> Option<geulos_core::ObjectId> {
    tree.ids().find(|id| {
        tree.get(*id).map(|o| o.type_uri.as_str() == "aios.builtin/Cli@1").unwrap_or(false)
    })
}

fn main() {
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:5550".to_string());

    let event_loop: EventLoop<UserEvent> = EventLoop::with_user_event().build().expect("EventLoop");
    let proxy = Arc::new(event_loop.create_proxy());

    let tree: Arc<Mutex<TreeModel>> = Arc::new(Mutex::new(TreeModel::new()));
    let (ui_tx, ui_rx) = tokio::sync::mpsc::channel::<UiAction>(64);
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<ServerEvent>(64);

    // tokio 런타임 스레드 — server-host와 TCP로 대화
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

/// 클릭 dispatch — Folder/File/Window 등 타입별로 UiAction 생성.
///
/// `layout`은 rect 위치로 좌측 FileTree 영역(좌 25%)인지 우측 Explorer 영역인지 판정에 사용.
/// `window_w`는 좌측 25% 경계 계산.
///
/// - `aios.std/Folder@1`: Explorer.navigate_to 무조건 + 좌측 클릭이면 FileTree expand/collapse 추가.
/// - `aios.std/File@1`: Explorer.open_file (M8 T8.7에서 새 Window mount).
/// - 그 외 (echo-app 호환): 첫 메서드를 args=null로 호출.
fn dispatch_click(
    tree: &TreeModel,
    layout: &LayoutResult,
    target: geulos_core::ObjectId,
    obj: &geulos_core::Object,
    window_w: i32,
) -> Vec<UiAction> {
    match obj.type_uri.as_str() {
        "aios.std/Folder@1" => {
            let mut actions = Vec::new();
            // Explorer.navigate_to 무조건 호출 (좌·우 어느 영역 클릭이든).
            if let Some(explorer) = find_explorer(tree) {
                actions.push(UiAction::Invoke {
                    target: explorer.id,
                    method: "navigate_to".to_string(),
                    args: serde_json::json!({ "folder_id": target.to_string() }),
                });
            }
            // 좌측 FileTree 영역(좌 25%)이면 expand/collapse 토글 추가.
            if let Some(rect) = layout.get(target) {
                let ft_threshold = (window_w as f32 * 0.25) as i32;
                if rect.x < ft_threshold {
                    if let Some(ft) = find_file_tree(tree) {
                        let is_expanded =
                            ft.state.get("expanded").and_then(|v| v.as_array()).is_some_and(
                                |arr| arr.iter().any(|v| v.as_str() == Some(&target.to_string())),
                            );
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
            // M8: 파일 클릭 → Explorer.open_file (새 Window mount, T8.7).
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
            // 기존 echo-app 호환: 첫 메서드 호출.
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

fn find_file_tree(tree: &TreeModel) -> Option<&geulos_core::Object> {
    for id in tree.ids() {
        if let Some(o) = tree.get(id) {
            if o.type_uri.as_str() == "aios.builtin/FileTree@1" {
                return Some(o);
            }
        }
    }
    None
}

fn find_explorer(tree: &TreeModel) -> Option<&geulos_core::Object> {
    for id in tree.ids() {
        if let Some(o) = tree.get(id) {
            if o.type_uri.as_str() == "aios.builtin/Explorer@1" {
                return Some(o);
            }
        }
    }
    None
}
