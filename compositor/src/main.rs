//! GeulOS 컴포지터 메인.

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use geulos_compositor::hit_test::hit_test;
use geulos_compositor::keyboard::{CliLocalState, KeyAction};
use geulos_compositor::layout::{layout, HitRole, Rect};
use geulos_compositor::messages::{ServerEvent, UiAction};
use geulos_compositor::render::render_frame;
use geulos_compositor::server_client::{run_server_client, UserEvent};
use geulos_compositor::tree_model::TreeModel;
use geulos_compositor::window_geom::{
    WINDOW_CLOSE_BTN, WINDOW_MIN_H, WINDOW_MIN_W, WINDOW_RESIZE_HANDLE, WINDOW_TITLE_H,
};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

/// 좌클릭 drag 상태 (M8 T8.9).
///
/// `MovingWindow`: title bar drag — cursor delta를 window의 (x, y)에 더해 `move` invoke.
/// `ResizingWindow`: 우하 resize handle drag — cursor delta를 (w, h)에 더해 `resize` invoke.
///
/// Drag 중에는 *시각 피드백 없음* (plan §13). drop 시점에 한 번 invoke → server → state_set →
/// natural redraw. v2에서 ghost rect 등 개선.
#[derive(Debug, Clone)]
enum DragState {
    None,
    MovingWindow {
        window_id: geulos_core::ObjectId,
        start_cursor: (i32, i32),
        start_pos: (i32, i32),
    },
    ResizingWindow {
        window_id: geulos_core::ObjectId,
        start_cursor: (i32, i32),
        start_size: (i32, i32),
    },
}

/// 키보드 입력을 라우팅할 대상 (M8 T8.9).
///
/// `Cli`: T7.5 동작 — 모든 키가 CLI 버퍼로 (default).
/// `Window`: 특정 Window가 focused — 본문은 read-only이라 v1은 키 무시 (Ctrl+W 등 단축키는 v2).
/// `None`: 빈 영역 클릭 후 — 키 무시.
#[derive(Debug, Clone)]
enum KeyboardFocus {
    Cli,
    /// Window가 focused. ObjectId는 v2에서 Ctrl+W 등 단축키 라우팅에 사용 예정.
    /// v1은 본문이 read-only라 ID를 읽지 않지만, 미리 보유해 두면 v2 시 시그니처 변경 없음.
    Window(#[allow(dead_code)] geulos_core::ObjectId),
    None,
}

struct App {
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    tree: Arc<Mutex<TreeModel>>,
    ui_tx: tokio::sync::mpsc::Sender<UiAction>,
    cursor: (f64, f64),
    /// T7.5: 컴포지터-사이드 CLI 입력 버퍼/커서. server tree와 분리.
    cli_state: CliLocalState,
    /// M8 T8.9: 좌클릭 drag 상태 — title bar 이동 / corner 리사이즈.
    drag: DragState,
    /// M8 T8.9: 키보드 입력 라우팅 대상. T7.5 호환을 위해 default = Cli.
    keyboard_focus: KeyboardFocus,
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
            drag: DragState::None,
            keyboard_focus: KeyboardFocus::Cli,
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title("GeulOS Compositor (M4)")
            .with_inner_size(PhysicalSize::new(800u32, 600u32));
        let window = Arc::new(event_loop.create_window(attrs).expect("create_window"));
        // T7.6 (ADR-029): 한글 IME 활성화 — Windows TSF가 winit 통해 Preedit/Commit emit.
        window.set_ime_allowed(true);
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
                    if let Some((target, role)) = hit_test(&tree, &lay, cx, cy) {
                        if let Some(obj) = tree.get(target) {
                            let uri = obj.type_uri.as_str();
                            if uri == "aios.builtin/Window@1" {
                                // Window 분기는 role 무시 — 영역 분석은 자체 좌표 계산 (T8.9).
                                let _ = role;
                                // Window 영역 분석: close / title bar / resize handle / content.
                                // render.rs와 *같은* 좌표 계산식 사용 — window_geom 상수 공유.
                                let win_rect =
                                    lay.get(target).unwrap_or(Rect { x: 0, y: 0, w: 0, h: 0 });
                                let inner = Rect {
                                    x: win_rect.x + 1,
                                    y: win_rect.y + 1,
                                    w: win_rect.w - 2,
                                    h: win_rect.h - 2,
                                };
                                let title_rect =
                                    Rect { x: inner.x, y: inner.y, w: inner.w, h: WINDOW_TITLE_H };
                                let close_rect = Rect {
                                    x: title_rect.x + title_rect.w - WINDOW_CLOSE_BTN - 4,
                                    y: title_rect.y + 4,
                                    w: WINDOW_CLOSE_BTN,
                                    h: WINDOW_CLOSE_BTN,
                                };
                                let resize_rect = Rect {
                                    x: inner.x + inner.w - WINDOW_RESIZE_HANDLE,
                                    y: inner.y + inner.h - WINDOW_RESIZE_HANDLE,
                                    w: WINDOW_RESIZE_HANDLE,
                                    h: WINDOW_RESIZE_HANDLE,
                                };
                                if close_rect.contains(cx, cy) {
                                    let _ = self.ui_tx.try_send(UiAction::Invoke {
                                        target,
                                        method: "close".to_string(),
                                        args: serde_json::Value::Null,
                                    });
                                } else if resize_rect.contains(cx, cy) {
                                    // resize handle은 close보다 우선순위 낮음 (close가 우상 corner).
                                    // plan §13 spec: resize는 focus invoke 안 함 (drag end만).
                                    let start_size = (
                                        obj.state.get("w").and_then(|v| v.as_i64()).unwrap_or(600)
                                            as i32,
                                        obj.state.get("h").and_then(|v| v.as_i64()).unwrap_or(400)
                                            as i32,
                                    );
                                    self.drag = DragState::ResizingWindow {
                                        window_id: target,
                                        start_cursor: (cx, cy),
                                        start_size,
                                    };
                                } else if title_rect.contains(cx, cy) {
                                    // Title bar drag — move 시작 + focus.
                                    let start_pos = (
                                        obj.state.get("x").and_then(|v| v.as_i64()).unwrap_or(0)
                                            as i32,
                                        obj.state.get("y").and_then(|v| v.as_i64()).unwrap_or(0)
                                            as i32,
                                    );
                                    self.drag = DragState::MovingWindow {
                                        window_id: target,
                                        start_cursor: (cx, cy),
                                        start_pos,
                                    };
                                    let _ = self.ui_tx.try_send(UiAction::Invoke {
                                        target,
                                        method: "focus".to_string(),
                                        args: serde_json::Value::Null,
                                    });
                                    self.keyboard_focus = KeyboardFocus::Window(target);
                                } else {
                                    // 본문 클릭 → focus only (drag 없음, read-only).
                                    let _ = self.ui_tx.try_send(UiAction::Invoke {
                                        target,
                                        method: "focus".to_string(),
                                        args: serde_json::Value::Null,
                                    });
                                    self.keyboard_focus = KeyboardFocus::Window(target);
                                }
                            } else if uri == "aios.builtin/Cli@1" {
                                // CLI 클릭 — focus 전환만. invoke 없음 (T7.5 자연).
                                self.keyboard_focus = KeyboardFocus::Cli;
                            } else {
                                // Folder/File/echo-app 등 — 기존 dispatch_click + role.
                                let actions = dispatch_click(&tree, target, obj, role);
                                for action in actions {
                                    let _ = self.ui_tx.try_send(action);
                                }
                            }
                        }
                    } else {
                        // 빈 영역 클릭 → focus 해제. 다음 키 입력은 무시된다.
                        self.keyboard_focus = KeyboardFocus::None;
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                let (cx, cy) = (self.cursor.0 as i32, self.cursor.1 as i32);
                match self.drag.clone() {
                    DragState::MovingWindow { window_id, start_cursor, start_pos } => {
                        let dx = cx - start_cursor.0;
                        let dy = cy - start_cursor.1;
                        let new_x = start_pos.0 + dx;
                        let new_y = start_pos.1 + dy;
                        let _ = self.ui_tx.try_send(UiAction::Invoke {
                            target: window_id,
                            method: "move".to_string(),
                            args: serde_json::json!({ "x": new_x, "y": new_y }),
                        });
                    }
                    DragState::ResizingWindow { window_id, start_cursor, start_size } => {
                        let dw = cx - start_cursor.0;
                        let dh = cy - start_cursor.1;
                        let new_w = (start_size.0 + dw).max(WINDOW_MIN_W);
                        let new_h = (start_size.1 + dh).max(WINDOW_MIN_H);
                        let _ = self.ui_tx.try_send(UiAction::Invoke {
                            target: window_id,
                            method: "resize".to_string(),
                            args: serde_json::json!({ "w": new_w, "h": new_h }),
                        });
                    }
                    DragState::None => {}
                }
                self.drag = DragState::None;
            }
            WindowEvent::Resized(_) => {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            // M8 T8.9: 키보드 입력 — `keyboard_focus`에 따라 라우팅.
            // - Cli: T7.5 동작 그대로 (insert/backspace/submit).
            // - Window(_): read-only 본문 — 키 입력 무시. v2에서 Ctrl+W 등 단축키.
            // - None: 무시.
            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key, state: ElementState::Pressed, text, .. },
                ..
            } => match &self.keyboard_focus {
                KeyboardFocus::Cli => {
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
                KeyboardFocus::Window(_) => {
                    // v1: 본문은 read-only. 단축키는 v2.
                }
                KeyboardFocus::None => {
                    // 무시.
                }
            },
            // T7.6 (ADR-029): 한글 IME 이벤트 — `keyboard_focus=Cli`일 때만 cli_state에 반영.
            // Window/None focus에서는 *완전 무시* — M8 read-only Window 본문과 일관.
            // v2에서 TextArea 등 editable Window 도입 시 라우팅 확장.
            WindowEvent::Ime(ime) => {
                if let KeyboardFocus::Cli = self.keyboard_focus {
                    match ime {
                        Ime::Preedit(text, _cursor_range) => {
                            self.cli_state.handle_ime_preedit(text);
                            if let Some(w) = &self.window {
                                w.request_redraw();
                            }
                        }
                        Ime::Commit(text) => {
                            self.cli_state.handle_ime_commit(&text);
                            if let Some(w) = &self.window {
                                w.request_redraw();
                            }
                        }
                        Ime::Enabled | Ime::Disabled => {
                            // 상태 전환 — 무시. winit 명세상 Disabled 직전에 빈 Preedit가
                            // 도착하므로 preedit_text는 그 경로로 자연스럽게 비워진다.
                        }
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

/// winit KeyEvent → keyboard::KeyAction 변환 (T7.5 ASCII v1 + T7.6 IME 공존).
///
/// 우선순위: NamedKey(Enter/Backspace) → text (단일 문자 입력). 한글 multi-char는
/// `WindowEvent::Ime` 채널이 별도로 처리한다 (ADR-029).
fn key_event_to_action(logical_key: &Key, text: Option<&str>) -> Option<KeyAction> {
    if let Key::Named(named) = logical_key {
        match named {
            NamedKey::Enter => return Some(KeyAction::Submit),
            NamedKey::Backspace => return Some(KeyAction::Backspace),
            _ => {}
        }
    }
    // 문자 입력 — winit이 제공한 text 사용 (단일 char만).
    let s = text?;
    let mut chars = s.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        // 다중 문자는 IME Preedit/Commit 이벤트로 처리됨 (T7.6 ADR-029).
        // KeyboardInput에서 받은 multi-char text는 IME path와 중복일 수 있어 무시.
        return None;
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
/// `role`은 hit_test가 반환한 HitRole — Folder 분기에서 expand vs navigate 의미 분리에 사용
/// (M8 회귀 fix #2). Window 분기는 main의 자체 영역 분석으로 처리되므로 dispatch_click에는
/// 도달하지 않는다.
///
/// - `aios.std/Folder@1`:
///   - `role == ExpandToggle`: FileTree expand/collapse만 — `[+]`/`[-]` 표식 클릭.
///   - `role == Body`: Explorer.navigate_to만 — 폴더명 영역 클릭 (좌측 트리든 우측 Explorer든).
/// - `aios.std/File@1`: Explorer.open_file (M8 T8.7에서 새 Window mount).
/// - 그 외 (echo-app 호환): 첫 메서드를 args=null로 호출.
fn dispatch_click(
    tree: &TreeModel,
    target: geulos_core::ObjectId,
    obj: &geulos_core::Object,
    role: HitRole,
) -> Vec<UiAction> {
    match obj.type_uri.as_str() {
        "aios.std/Folder@1" => {
            let mut actions = Vec::new();
            if role == HitRole::ExpandToggle {
                // [+]/[-] 영역 클릭 — expand/collapse만, navigate 안 함.
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
            } else {
                // Body 클릭 — navigate_to만 (좌측 트리 폴더명이든 우측 Explorer든).
                if let Some(explorer) = find_explorer(tree) {
                    actions.push(UiAction::Invoke {
                        target: explorer.id,
                        method: "navigate_to".to_string(),
                        args: serde_json::json!({ "folder_id": target.to_string() }),
                    });
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
