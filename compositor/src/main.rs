//! GeulOS 컴포지터 메인.

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use geulos_compositor::dispatch::{dispatch_click, find_explorer, find_file_tree};
use geulos_compositor::editor::EditorState;
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
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
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
/// `Window`: 특정 Window가 focused — M8 T8.17부터 PageUp/PageDown으로 scroll_y 조정.
/// `None`: 빈 영역 클릭 후 — 키 무시.
#[derive(Debug, Clone)]
enum KeyboardFocus {
    Cli,
    /// Window가 focused. T8.17부터 ObjectId를 PageUp/Down → scroll_y SetState에 사용.
    Window(geulos_core::ObjectId),
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
    /// T7.10: 현재 modifier key 상태 — `WindowEvent::ModifiersChanged`로 갱신.
    /// Ctrl+V 등 단축키 감지에 사용. 매 KeyboardInput에서 별도 lookup 없이 cache 활용.
    modifiers: ModifiersState,
    /// 터치패드 PixelDelta 누적 — 작은 delta (1-2 px)가 정수 나눗셈으로 0이 되어 무시되던
    /// 문제 (사용자 보고 — 터치패드 스크롤 먹통)를 float 누적으로 해소. 1 라인 = 20px 가정,
    /// 누적이 ±20 이상이면 정수 라인 추출 + accumulator는 remainder 유지.
    scroll_accum_y: f64,
    /// M9 T7: edit_mode Window의 컴포지터 측 editor state. 키 입력마다 즉시 SetState(content+dirty)
    /// 로 server에 푸시 (v1 debounce 없음). edit_mode=false면 None — toggle_edit 응답이 도착하면
    /// 매 redraw 직전 동기화 단계에서 Some/None 전환된다.
    editor_state: Option<EditorState>,
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
            modifiers: ModifiersState::empty(),
            scroll_accum_y: 0.0,
            editor_state: None,
        }
    }

    /// focused Window와 컴포지터 측 editor_state 동기화.
    ///
    /// Window는 *항상 편집 가능* (메모장 UX) — focused Window면 무조건 editor_state 생성.
    /// 다른 Window로 focus 이동 시 새 EditorState (content reload). focus 해제 시 None.
    ///
    /// Window가 destroyed면 editor_state 해제. 이미 active editor라면 content는 컴포지터가
    /// *master* — server SetState echo로 재초기화하지 않는다 (cursor 손실 방지).
    fn sync_editor_state(&mut self) {
        let tree = self.tree.lock().unwrap();
        // 현재 editor가 가리키는 window가 *살아있는 Window*인지 확인.
        if let Some(ed) = &self.editor_state {
            let alive = tree
                .get(ed.window_id)
                .map(|o| {
                    o.type_uri.as_str() == "aios.builtin/Window@1"
                        && !o.state.get("destroyed").and_then(|v| v.as_bool()).unwrap_or(false)
                })
                .unwrap_or(false);
            // focus가 다른 곳으로 옮겨갔거나 destroyed면 editor 해제.
            let focus_match =
                matches!(&self.keyboard_focus, KeyboardFocus::Window(wid) if *wid == ed.window_id);
            if !alive || !focus_match {
                drop(tree);
                self.editor_state = None;
                return;
            }
            // 활성 editor — content는 컴포지터가 master, server echo 무시.
            return;
        }
        // editor_state 없음 — focused Window면 진입.
        if let KeyboardFocus::Window(window_id) = &self.keyboard_focus {
            if let Some(obj) = tree.get(*window_id) {
                if obj.type_uri.as_str() == "aios.builtin/Window@1" {
                    let destroyed =
                        obj.state.get("destroyed").and_then(|v| v.as_bool()).unwrap_or(false);
                    if !destroyed {
                        let content = obj
                            .state
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let wid = *window_id;
                        drop(tree);
                        self.editor_state = Some(EditorState::new(wid, content));
                    }
                }
            }
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
                                    // M9 T8: close_confirm — desktop-shell이 dirty=true Window이면
                                    // Dialog 띄움 (또는 v1 단순화: dirty이면 reject + CLI 안내).
                                    // dirty=false면 기존 close와 동일하게 즉시 destroyed=true.
                                    let _ = self.ui_tx.try_send(UiAction::Invoke {
                                        target,
                                        method: "close_confirm".to_string(),
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
                                    // 본문 클릭 → focus + (편집 가능하므로) cursor 위치 갱신.
                                    let _ = self.ui_tx.try_send(UiAction::Invoke {
                                        target,
                                        method: "focus".to_string(),
                                        args: serde_json::Value::Null,
                                    });
                                    self.keyboard_focus = KeyboardFocus::Window(target);
                                    // content area 산출 — render_window의 content_rect와 동일 식.
                                    let content_x = inner.x + 8;
                                    let content_y = inner.y + WINDOW_TITLE_H + 8;
                                    let content_w = inner.w - 16;
                                    let content_h = inner.h - WINDOW_TITLE_H - 16;
                                    let rel_x = cx - content_x;
                                    let rel_y = cy - content_y;
                                    let in_content = rel_x >= 0
                                        && rel_y >= 0
                                        && rel_x < content_w
                                        && rel_y < content_h;
                                    let scroll_y =
                                        obj.state
                                            .get("scroll_y")
                                            .and_then(|v| v.as_i64())
                                            .unwrap_or(0)
                                            .max(0) as i32;
                                    // tree lock 해제 후 sync_editor_state 호출 (내부에서 다시 lock).
                                    drop(tree);
                                    self.sync_editor_state();
                                    if in_content {
                                        if let Some(ed) = self.editor_state.as_mut() {
                                            if ed.window_id == target {
                                                // render와 *동일한* wrap 폭 사용 (margin 4px —
                                                // render의 wrap_w와 일치해야 click hit가 cursor
                                                // 시각 위치와 정확히 매칭).
                                                let wrap_w = (content_w - 4).max(20);
                                                let lines =
                                                    geulos_compositor::editor::wrap_by_pixel_width(
                                                        &ed.content,
                                                        wrap_w,
                                                    );
                                                let click_line =
                                                    (rel_y / 20 + scroll_y).max(0) as usize;
                                                ed.cursor =
                                                    geulos_compositor::editor::byte_offset_from_pixel(
                                                        &lines, click_line, rel_x,
                                                    );
                                            }
                                        }
                                    }
                                }
                            } else if uri == "aios.builtin/ConsoleWindow@1" {
                                // M13 T9: ConsoleWindow hit_test — Window@1과 동형.
                                // geometry: state.x/y/w/h (ConsoleWindow는 state에 geometry 저장).
                                // render_console_window와 *같은* 좌표 계산식 사용.
                                let _ = role;
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
                                    // X 버튼 → "close" invoke. desktop-shell ConsoleWindow handler가
                                    // T8에서 close → process terminate + destroyed=true 처리.
                                    let _ = self.ui_tx.try_send(UiAction::Invoke {
                                        target,
                                        method: "close".to_string(),
                                        args: serde_json::Value::Null,
                                    });
                                } else if resize_rect.contains(cx, cy) {
                                    // resize handle — drag 시작. focus invoke 없음 (Window@1과 동일).
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
                                    // Title bar drag — move 시작 + focus (z-raise).
                                    // Window@1과 동형: drag 시작 시 focus invoke를 함께 보내
                                    // ConsoleWindow가 z 최상위로 올라오게 한다.
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
                                } else {
                                    // 본문 클릭 — ConsoleWindow는 read-only (편집 없음). noop.
                                    // scroll은 MouseWheel 핸들러에서 처리.
                                }
                            } else if uri == "aios.builtin/Dialog@1" {
                                // M9 T7: Dialog 클릭 — 어느 버튼인지 cx로 산출 → respond invoke.
                                //
                                // layout이 산출한 Dialog rect(400×200 화면 중앙)를 그대로 사용.
                                // render_dialog의 버튼 배치(가운데 정렬, btn 100×32, gap 12)와 정확히 일치
                                // 해야 사용자가 본 버튼이 invoke target과 매칭된다.
                                let _ = role;
                                let dialog_rect =
                                    lay.get(target).unwrap_or(Rect { x: 0, y: 0, w: 0, h: 0 });
                                let inner = Rect {
                                    x: dialog_rect.x + 1,
                                    y: dialog_rect.y + 1,
                                    w: dialog_rect.w - 2,
                                    h: dialog_rect.h - 2,
                                };
                                if let Some(actions) =
                                    obj.props.get("actions").and_then(|v| v.as_array())
                                {
                                    let n = actions.len();
                                    if n > 0 {
                                        let btn_w = 100i32;
                                        let btn_h = 32i32;
                                        let gap = 12i32;
                                        let total_w =
                                            n as i32 * btn_w + (n as i32 - 1).max(0) * gap;
                                        let by = inner.y + inner.h - btn_h - 12;
                                        if cy >= by && cy < by + btn_h {
                                            let bx_start = inner.x + (inner.w - total_w) / 2;
                                            let rel = cx - bx_start;
                                            if rel >= 0 {
                                                let idx = rel / (btn_w + gap);
                                                let idx_usize = idx as usize;
                                                let within_btn = rel < idx * (btn_w + gap) + btn_w;
                                                if idx_usize < n && within_btn {
                                                    let label = actions[idx_usize]
                                                        .as_str()
                                                        .unwrap_or("")
                                                        .to_string();
                                                    let _ = self.ui_tx.try_send(UiAction::Invoke {
                                                        target,
                                                        method: "respond".to_string(),
                                                        args: serde_json::json!({
                                                            "action": label
                                                        }),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            } else if uri == "aios.builtin/Cli@1" {
                                // CLI 클릭 — focus 전환만. invoke 없음 (T7.5 자연).
                                self.keyboard_focus = KeyboardFocus::Cli;
                            } else if uri == "aios.builtin/Explorer@1" {
                                // Explorer 자체 클릭 — role 기반 처리.
                                // ExplorerParentNav: 상단 "/" 행 → navigate_up invoke.
                                // Body: 배경 클릭 → noop (자식 행은 별도 hit rect로 도달).
                                if role == HitRole::ExplorerParentNav {
                                    let _ = self.ui_tx.try_send(UiAction::Invoke {
                                        target,
                                        method: "navigate_up".to_string(),
                                        args: serde_json::Value::Null,
                                    });
                                }
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
            // M8 T8.17: 마우스 휠 → scroll_y SetState (UiAction::SetState 직접).
            //
            // 휠 위로 = lines<0 (scroll_y 감소 = 위로 스크롤), 휠 아래 = lines>0.
            // LineDelta: 1 notch = 3 lines (Windows 표준). PixelDelta: 16px = 1 line (macOS/touchpad).
            //
            // hit target에 따라 분기:
            // - Window: 자기 자신 scroll_y.
            // - Folder/File (FileTree나 Explorer의 자식): cursor X로 부모 영역 결정.
            // - 그 외: 무시.
            WindowEvent::MouseWheel { delta, .. } => {
                let (cx, cy) = (self.cursor.0 as i32, self.cursor.1 as i32);
                // 터치패드는 *연속 작은 PixelDelta*를 보냄 — 정수 나눗셈으로 0이 되어
                // 무시되던 문제 fix. float accumulator로 누적 → 1 라인 (~20px) 도달 시
                // 정수 라인 추출. LineDelta(마우스 휠)는 즉시 변환.
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => {
                        // 1 line wheel notch = 3 라인 스크롤. accumulator 영향 X.
                        -(y as f64) * 3.0
                    }
                    MouseScrollDelta::PixelDelta(p) => {
                        // 픽셀 단위 — accumulator에 누적, 20px당 1 라인.
                        self.scroll_accum_y += -p.y / 20.0;
                        // 정수 부분만 lines로, fractional은 accumulator에 남김.
                        let whole = self.scroll_accum_y.trunc();
                        self.scroll_accum_y -= whole;
                        whole
                    }
                };
                let lines = lines as i32;
                if lines == 0 {
                    return;
                }
                if let Some(window) = &self.window {
                    let size = window.inner_size();
                    let tree = self.tree.lock().unwrap();
                    let lay = layout(&tree, size.width as i32, size.height as i32);
                    // CLI rect 위 휠은 cli_state.scroll_offset 조정 (server SetState 아님 —
                    // local). 다른 영역 (FileTree/Explorer/Window)은 server scroll_y SetState.
                    let cli_id = find_cli_object_id(&tree);
                    let cli_hit = cli_id.and_then(|cid| lay.get(cid)).map(|r| r.contains(cx, cy));
                    if cli_hit == Some(true) {
                        drop(tree);
                        if lines < 0 {
                            self.cli_state.scroll_offset =
                                self.cli_state.scroll_offset.saturating_add((-lines) as usize);
                        } else {
                            self.cli_state.scroll_offset =
                                self.cli_state.scroll_offset.saturating_sub(lines as usize);
                        }
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
                        return;
                    }
                    let scroll_target =
                        hit_test(&tree, &lay, cx, cy).and_then(|(target, _role)| {
                            tree.get(target).and_then(|obj| match obj.type_uri.as_str() {
                                // M13 T9: ConsoleWindow@1도 Window@1과 동일하게 자기 자신 scroll_y.
                                "aios.builtin/Window@1" | "aios.builtin/ConsoleWindow@1" => {
                                    Some(target)
                                }
                                _ => find_scroll_target(&tree, cx, size.width as i32),
                            })
                        });
                    if let Some(target_id) = scroll_target {
                        let cur = tree
                            .get(target_id)
                            .and_then(|o| o.state.get("scroll_y").and_then(|v| v.as_i64()))
                            .unwrap_or(0);
                        // T8.20: max도 clamp — `max_scroll_y_for`가 영역별로 추정.
                        // 무한 누적(휠 계속 굴려도 scroll_y만 증가) 방지.
                        let max = max_scroll_y_for(
                            &tree,
                            target_id,
                            size.width as i32,
                            size.height as i32,
                        );
                        let new_scroll_y = (cur + lines as i64).max(0).min(max);
                        drop(tree);
                        let _ = self.ui_tx.try_send(UiAction::SetState {
                            target: target_id,
                            key: "scroll_y".to_string(),
                            value: serde_json::json!(new_scroll_y),
                        });
                    }
                }
            }
            // T7.10: modifier state 갱신 — Ctrl+V 등 단축키 감지에 사용.
            // winit 0.30: `WindowEvent::ModifiersChanged(Modifiers)` → `.state()` getter.
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }
            // M8 T8.9: 키보드 입력 — `keyboard_focus`에 따라 라우팅.
            // - Cli: T7.5 동작 그대로 (insert/backspace/submit) + T7.10 Ctrl+V paste.
            // - Window(_): read-only 본문 — 키 입력 무시. v2에서 Ctrl+W 등 단축키.
            // - None: 무시.
            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key, state: ElementState::Pressed, text, .. },
                ..
            } => match &self.keyboard_focus {
                KeyboardFocus::Cli => {
                    // T7.10: Ctrl+V — arboard 클립보드에서 텍스트 paste.
                    // 사용자가 긴 API key를 awaiting_api_key 모드에서 일일이 타이핑하지 않도록
                    // 한다. shell/ai/awaiting 모든 mode에서 동작 — keyboard_focus=Cli만 조건.
                    // 실패는 silent (eprintln만) — paste 실패가 다른 입력을 막아선 안 됨.
                    if self.modifiers.control_key()
                        && matches!(&logical_key, Key::Character(c) if c.as_str().eq_ignore_ascii_case("v"))
                    {
                        match arboard::Clipboard::new().and_then(|mut cb| cb.get_text()) {
                            Ok(s) => {
                                self.cli_state.handle_paste(&s);
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                            }
                            Err(e) => eprintln!("[compositor] clipboard paste 실패: {}", e),
                        }
                        return;
                    }
                    // PageUp/PageDown — CLI 출력 라인 스크롤 (5 라인씩, scroll_offset 조정).
                    // 사용자 보고: AI 답변이 길 때 이전 라인 확인 불가 → bottom 기준 위로 이동.
                    if matches!(&logical_key, Key::Named(NamedKey::PageUp)) {
                        self.cli_state.scroll_offset =
                            self.cli_state.scroll_offset.saturating_add(5);
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
                        return;
                    }
                    if matches!(&logical_key, Key::Named(NamedKey::PageDown)) {
                        self.cli_state.scroll_offset =
                            self.cli_state.scroll_offset.saturating_sub(5);
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
                        return;
                    }
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
                KeyboardFocus::Window(window_id) => {
                    let window_id = *window_id;
                    // Window는 *항상 편집 가능* (메모장 UX) — focused Window 키 입력은 모두
                    // editor로 라우팅. PageUp/Down은 그 *전*에 viewer-style scroll로 가로채야
                    // 큰 파일에서 키 한 번에 한 줄 추가가 아니라 페이지 단위로 이동한다.
                    //
                    // 키 입력 전에 sync_editor_state를 *반드시 한 번* 호출 — 사용자가 title
                    // bar 드래그로 focus만 얻은 경우 editor가 아직 없을 수 있으므로 Ctrl+S
                    // 발송이 빈 content로 실패하지 않도록 보장 (사용자 보고 freeze fix).
                    self.sync_editor_state();
                    let is_page_key =
                        matches!(&logical_key, Key::Named(NamedKey::PageUp | NamedKey::PageDown));
                    if !is_page_key {
                        handle_window_edit_key(self, window_id, &logical_key, text.as_deref());
                        return;
                    }

                    // PageUp/Down — 10 라인 scroll_y SetState (M8 T8.17).
                    // visible_lines 정확 계산은 v2 (Window 크기 → 가시 라인 수). v1은 10 고정.
                    // T8.20: max도 clamp — render 자체 clamp가 시각 fallback이지만 SetState
                    // 단계에서 잡아야 무한 누적이 안 된다.
                    let delta_lines = match &logical_key {
                        Key::Named(NamedKey::PageUp) => Some(-10i64),
                        Key::Named(NamedKey::PageDown) => Some(10i64),
                        _ => None,
                    };
                    if let Some(d) = delta_lines {
                        if let Some(w) = &self.window {
                            let size = w.inner_size();
                            let (cur, max) = {
                                let tree = self.tree.lock().unwrap();
                                let cur = tree
                                    .get(window_id)
                                    .and_then(|o| o.state.get("scroll_y").and_then(|v| v.as_i64()))
                                    .unwrap_or(0);
                                let max = max_scroll_y_for(
                                    &tree,
                                    window_id,
                                    size.width as i32,
                                    size.height as i32,
                                );
                                (cur, max)
                            };
                            let new_scroll_y = (cur + d).max(0).min(max);
                            let _ = self.ui_tx.try_send(UiAction::SetState {
                                target: window_id,
                                key: "scroll_y".to_string(),
                                value: serde_json::json!(new_scroll_y),
                            });
                            w.request_redraw();
                        }
                    }
                    // 다른 키 (Ctrl+W 등)는 v2.
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
                // M9 T7: server state(edit_mode toggle 등) 도착 후 editor_state 진입/탈출 동기화.
                // render 전에 호출해 첫 redraw에서 cursor가 즉시 보이도록 한다.
                self.sync_editor_state();
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
                    render_frame(
                        &tree,
                        &lay,
                        &mut buffer,
                        w as usize,
                        h as usize,
                        &self.cli_state,
                        self.editor_state.as_ref(),
                    );
                    buffer.present().expect("present");
                }
            }
            _ => {}
        }
    }
}

/// focused Window에 도착한 키 입력 한 건 처리. Window는 *항상 편집 가능*.
///
/// 우선순위:
/// 1. Ctrl+S → `save_to_file` invoke + args에 content 포함 (wire 한 번에 디스크 commit).
/// 2. NamedKey (Backspace/Enter/Left/Right) → editor_state 직접 mutate.
/// 3. 문자 입력 (Ctrl 없는 단일 char) → insert_char.
///
/// **lag fix**: 매 키마다 content를 SetState로 wire에 push하지 않는다. content는 컴포지터
/// *local-master*; dirty만 wire (작은 boolean). save 시점에만 args.content로 디스크에 commit.
/// 이전 v1은 매 키마다 SetState(content+dirty) 2 메시지를 보내 큰 파일에서 wire backpressure로
/// 입력 freeze 발생 (사용자 보고).
fn handle_window_edit_key(
    app: &mut App,
    window_id: geulos_core::ObjectId,
    logical_key: &Key,
    text: Option<&str>,
) {
    // Ctrl+S — 디스크에 저장. editor가 *이 window의 active editor*일 때만 발송 (안전 가드:
    // editor 없을 때 빈 content로 덮어쓰지 않도록).
    if app.modifiers.control_key()
        && matches!(logical_key, Key::Character(c) if c.as_str().eq_ignore_ascii_case("s"))
    {
        let content = match app.editor_state.as_ref() {
            Some(ed) if ed.window_id == window_id => ed.content.clone(),
            _ => {
                eprintln!(
                    "[compositor] Ctrl+S 무시 — editor_state 미준비 (window_id={}). \
                     Window 본문 클릭 후 다시 시도하세요.",
                    window_id
                );
                return;
            }
        };
        eprintln!("[compositor] Ctrl+S → save_to_file invoke 발송 ({} bytes)", content.len());
        let _ = app.ui_tx.try_send(UiAction::Invoke {
            target: window_id,
            method: "save_to_file".to_string(),
            args: serde_json::json!({ "content": content }),
        });
        // save 성공 후 server에서 dirty=false echo가 오면 sync_editor_state가 reset할 수도
        // 있지만, 안전을 위해 *즉시* dirty_synced=false로 reset해서 다음 키 입력 때 다시
        // dirty SetState 한 번 보낼 수 있게.
        if let Some(ed) = app.editor_state.as_mut() {
            ed.dirty_synced = false;
        }
        if let Some(w) = &app.window {
            w.request_redraw();
        }
        return;
    }

    // 편집 키 — editor_state를 mutate. 컨트롤 modifier 있는 단순 char는 무시.
    let editor = match app.editor_state.as_mut() {
        Some(e) => e,
        None => return,
    };
    let mut mutated = false;
    match logical_key {
        Key::Named(NamedKey::Backspace) => {
            editor.backspace();
            mutated = true;
        }
        Key::Named(NamedKey::Enter) => {
            editor.newline();
            mutated = true;
        }
        Key::Named(NamedKey::ArrowLeft) => {
            editor.cursor_left();
            if let Some(w) = &app.window {
                w.request_redraw();
            }
        }
        Key::Named(NamedKey::ArrowRight) => {
            editor.cursor_right();
            if let Some(w) = &app.window {
                w.request_redraw();
            }
        }
        _ => {
            // Ctrl + (S 외) 단축키는 v1에서 무시. text가 있고 단일 char면 insert.
            if app.modifiers.control_key() {
                return;
            }
            let s = match text {
                Some(s) => s,
                None => return,
            };
            let mut chars = s.chars();
            let c = match chars.next() {
                Some(c) => c,
                None => return,
            };
            if chars.next().is_some() {
                // 다중 char는 IME path (v2) — 현재는 무시.
                return;
            }
            if c.is_control() {
                return;
            }
            editor.insert_char(c);
            mutated = true;
        }
    }
    if mutated {
        // dirty=true SetState는 *한 번만* 보냄 (사용자 보고 freeze fix). 매 키마다 보내면
        // mpsc/wire backpressure로 입력 자체가 막힘. save 시점에 reset되어 다음 키 입력 때
        // 다시 한 번 보낼 수 있음.
        let need_dirty_sync = match app.editor_state.as_ref() {
            Some(ed) => !ed.dirty_synced,
            None => false,
        };
        if need_dirty_sync {
            let _ = app.ui_tx.try_send(UiAction::SetState {
                target: window_id,
                key: "dirty".to_string(),
                value: serde_json::json!(true),
            });
            if let Some(ed) = app.editor_state.as_mut() {
                ed.dirty_synced = true;
            }
        }
        if let Some(w) = &app.window {
            w.request_redraw();
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

/// M8 T8.17: 마우스 X 좌표로 FileTree (좌 < 25%) 또는 Explorer (우 25~100%) ID 반환.
///
/// MouseWheel이 Folder/File를 hit한 경우 *부모 영역*에 스크롤 적용. 좌측 25%는 FileTree
/// 행, 그 외는 Explorer 행 — layout.rs::layout_desktop의 left_w(=window_w*0.25) 경계와 일치.
fn find_scroll_target(tree: &TreeModel, cx: i32, window_w: i32) -> Option<geulos_core::ObjectId> {
    let ft_threshold = (window_w as f32 * 0.25) as i32;
    if cx < ft_threshold {
        find_file_tree(tree).map(|o| o.id)
    } else {
        find_explorer(tree).map(|o| o.id)
    }
}

/// 객체별 max scroll_y 추정 — SetState 시점 clamp용 (T8.20).
///
/// render의 정확한 wrapped/visible 계산과 *완전히 일치하진 않지만* — 추정이 over-estimate
/// 쪽이면 사용자가 조금 더 스크롤할 수 있을 뿐 *무한 누적*은 방지된다 (render의 자체
/// `scroll_y.min(total.saturating_sub(visible))` clamp가 시각적 fallback). under-estimate
/// 쪽이면 끝까지 스크롤 안 됨 — 의도적으로 over-estimate.
///
/// - **Window@1**: render_window의 wrap 폭(14)/LINE_HEIGHT(20)/padding과 동일 가정.
///   wrapped 라인 수 = 각 원본 라인의 ceil(len/max_chars) 합.
/// - **FileTree@1**: 전체 트리의 Folder@1 총 수를 over-estimate (실제로는 expanded
///   Folder의 자손만 보이지만 단순화). 가시 라인 = top_h/28.
/// - **Explorer@1**: active_folder.children 수 (null이면 FileTree.children = 드라이브 일람).
///   가시 라인 = top_h/24.
/// - **그 외**: `i64::MAX` — clamp 없이 통과 (안전 fallback).
///
/// `window_h * 0.70`은 layout_desktop이 has_cli=true일 때의 top_h 가정 — has_cli=false면
/// over-estimate되어 무해. v1 단순화.
fn max_scroll_y_for(
    tree: &TreeModel,
    target_id: geulos_core::ObjectId,
    _window_w: i32,
    window_h: i32,
) -> i64 {
    let obj = match tree.get(target_id) {
        Some(o) => o,
        None => return 0,
    };
    match obj.type_uri.as_str() {
        "aios.builtin/Window@1" => {
            let content = obj.state.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let w = obj.state.get("w").and_then(|v| v.as_i64()).unwrap_or(600) as i32;
            let h = obj.state.get("h").and_then(|v| v.as_i64()).unwrap_or(400) as i32;
            // content_rect 크기 — render_window의 inner padding과 일관:
            // inner = w/h - 2 (border 1px×2), content = inner - title(24) - padding(8×2 = 16).
            let content_w = (w - 2 - 16).max(1);
            let content_h = (h - 2 - WINDOW_TITLE_H - 16).max(1);
            // wrap 폭 14는 render_window와 동일 가정 — 어긋나면 스크롤 범위가 어색해진다.
            let max_chars_per_line = (content_w / 14).max(1) as usize;
            let total_wrapped: usize = content
                .lines()
                .map(|line| {
                    let n = line.chars().count();
                    if n == 0 {
                        1
                    } else {
                        n.div_ceil(max_chars_per_line)
                    }
                })
                .sum();
            // LINE_HEIGHT=20은 render_window와 동일.
            let visible = (content_h / 20).max(1) as usize;
            total_wrapped.saturating_sub(visible) as i64
        }
        // M13 T9: ConsoleWindow@1 — lines 배열 길이 기반 clamp.
        // render_console_window의 CONSOLE_LINE_H=20, content_rect와 동일 패딩 가정.
        "aios.builtin/ConsoleWindow@1" => {
            let line_count =
                obj.state.get("lines").and_then(|v| v.as_array()).map(|arr| arr.len()).unwrap_or(0);
            let h = obj.state.get("h").and_then(|v| v.as_i64()).unwrap_or(400) as i32;
            // content_rect.h = h - 2 (border) - WINDOW_TITLE_H - SPACE_MD*2 (padding top+bottom).
            // T5에서 render_console_window content inset이 SPACE_MD(12)로 바뀜 — 여기 clamp도
            // 동기해야 scroll이 마지막 줄까지 도달 (어긋나면 visible 과대추정 → 끝 줄 미도달).
            let content_h =
                (h - 2 - WINDOW_TITLE_H - geulos_compositor::theme::SPACE_MD * 2).max(1);
            // CONSOLE_LINE_H=20은 render_console_window와 동일.
            let visible = (content_h / 20).max(1) as usize;
            line_count.saturating_sub(visible) as i64
        }
        "aios.builtin/FileTree@1" => {
            // 전체 트리의 Folder@1 총 수로 over-estimate (실제는 expanded subtree만 보임).
            // 정확 계산은 layout 결과를 다시 돌려야 하는데, 사용자 입력 핸들러에서 그 비용은
            // 부담 — over-estimate가 안전 + 무한 누적은 막힌다.
            let folder_type = geulos_core::TypeUri::parse("aios.std/Folder@1")
                .expect("Folder@1 TypeUri 파싱 — 정적 문자열이라 실패 불가");
            let total = tree.objects_of_type(&folder_type).len();
            // top_h = window_h * 0.70 (has_cli=true 가정). item_height 28 (T8.16 follow-up).
            let top_h = (window_h as f32 * 0.70) as i32;
            let visible = (top_h / 28).max(1) as usize;
            total.saturating_sub(visible) as i64
        }
        "aios.builtin/Explorer@1" => {
            // active_folder.children 수, active_folder=null이면 FileTree.children (드라이브 일람).
            let active = obj.state.get("active_folder").and_then(|v| v.as_str());
            let total = match active.and_then(|s| {
                if s.is_empty() {
                    None
                } else {
                    uuid::Uuid::parse_str(s).ok()
                }
            }) {
                Some(u) => {
                    let id = geulos_core::ObjectId::from_uuid(u);
                    tree.get(id).map(|f| f.children.len()).unwrap_or(0)
                }
                None => {
                    // FileTree.children fallback — 드라이브 일람이 Explorer에 그려질 때.
                    tree.ids()
                        .filter_map(|id| tree.get(id))
                        .find(|o| o.type_uri.as_str() == "aios.builtin/FileTree@1")
                        .map(|ft| ft.children.len())
                        .unwrap_or(0)
                }
            };
            let top_h = (window_h as f32 * 0.70) as i32;
            // Explorer item_height = layout::EXPLORER_ROW_H. 두 상수가 어긋나면 scroll clamp 부정확.
            let visible = (top_h / geulos_compositor::layout::EXPLORER_ROW_H).max(1) as usize;
            total.saturating_sub(visible) as i64
        }
        // 알 수 없는 타입 — clamp 없이 통과 (호환).
        _ => i64::MAX,
    }
}
