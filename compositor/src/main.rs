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
        Self { window: None, surface: None, tree, ui_tx, cursor: (0.0, 0.0) }
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
                            let actions = dispatch_click(&tree, target, obj);
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

/// 타입별 클릭 디스패치.
///
/// - `aios.std/Folder@1`: FileTree.expand 또는 collapse (현재 expanded 상태로 결정)
/// - `aios.std/File@1`: FileTree.select + Canvas.set_file
/// - 그 외 (echo-app 호환): 첫 메서드를 args=null로 호출
fn dispatch_click(
    tree: &TreeModel,
    target: geulos_core::ObjectId,
    obj: &geulos_core::Object,
) -> Vec<UiAction> {
    match obj.type_uri.as_str() {
        "aios.std/Folder@1" => {
            let ft = find_file_tree(tree);
            let is_expanded = ft.is_some_and(|f| {
                f.state
                    .get("expanded")
                    .and_then(|v| v.as_array())
                    .is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some(&target.to_string())))
            });
            let method = if is_expanded { "collapse" } else { "expand" };
            if let Some(ft) = ft {
                vec![UiAction::Invoke {
                    target: ft.id,
                    method: method.to_string(),
                    args: serde_json::json!({ "id": target.to_string() }),
                }]
            } else {
                vec![]
            }
        }
        "aios.std/File@1" => {
            let mut out = Vec::new();
            if let Some(ft) = find_file_tree(tree) {
                out.push(UiAction::Invoke {
                    target: ft.id,
                    method: "select".to_string(),
                    args: serde_json::json!({ "id": target.to_string() }),
                });
            }
            if let Some(cv) = find_canvas(tree) {
                out.push(UiAction::Invoke {
                    target: cv.id,
                    method: "set_file".to_string(),
                    args: serde_json::json!({ "id": target.to_string() }),
                });
            }
            out
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

fn find_canvas(tree: &TreeModel) -> Option<&geulos_core::Object> {
    for id in tree.ids() {
        if let Some(o) = tree.get(id) {
            if o.type_uri.as_str() == "aios.builtin/Canvas@1" {
                return Some(o);
            }
        }
    }
    None
}
