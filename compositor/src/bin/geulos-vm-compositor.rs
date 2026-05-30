//! VM 컴포지터 — geulosd에 접속해 실제 트리를 /dev/fb0에 render_frame으로 그리고,
//! evdev 좌클릭을 hit_test→dispatch_click→Invoke로 서버까지 왕복.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("geulos-vm-compositor는 VM(Linux) 전용입니다. 호스트는 geulos-compositor를 쓰세요.");
    std::process::exit(1);
}

/// 디스플레이 백엔드 — DRM/KMS 우선, 실패 시 fbdev 폴백.
#[cfg(target_os = "linux")]
enum Display {
    Drm(geulos_compositor::vm_drm::DrmDisplay),
    Fb(geulos_compositor::vm_fb::Framebuffer),
}

#[cfg(target_os = "linux")]
impl Display {
    fn dims(&self) -> (usize, usize) {
        match self {
            Display::Drm(d) => (d.xres, d.yres),
            Display::Fb(f) => (f.xres, f.yres),
        }
    }
    fn present(&mut self, buf: &[u32]) {
        match self {
            Display::Drm(d) => d.present(buf),
            Display::Fb(f) => f.present(buf),
        }
    }
}

#[cfg(target_os = "linux")]
fn main() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use geulos_compositor::dispatch::{self, dispatch_click};
    use geulos_compositor::editor::EditorState;
    use geulos_compositor::hit_test::hit_test;
    use geulos_compositor::keyboard::{CliLocalState, KeyAction};
    use geulos_compositor::layout::{
        layout, HitRole, Rect, DOCK_ITEM_H, TOPBAR_H, TOPBAR_ITEM_W,
    };
    use geulos_compositor::messages::{ServerEvent, UiAction};
    use geulos_compositor::render::{cli_input_geometry, fill_rect, render_frame, RenameOverlay};
    use geulos_compositor::window_geom::{
        WINDOW_CLOSE_BTN, WINDOW_MIN_H, WINDOW_MIN_W, WINDOW_RESIZE_HANDLE, WINDOW_TITLE_H,
    };
    use geulos_compositor::server_client::{run_server_client, UserEvent};
    use geulos_compositor::tree_model::TreeModel;
    use geulos_compositor::vm_fb::Framebuffer;
    use geulos_compositor::hangul::{qwerty_to_jamo, HangulComposer};
    use geulos_compositor::vm_input::{
        keycode_to_char, scale_abs, EvdevSet, ABS_X, ABS_Y, BTN_LEFT, EV_ABS, EV_KEY, EV_REL,
        KEY_BACKSPACE, KEY_ENTER, KEY_ESC, KEY_HANGEUL, KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_RIGHTCTRL,
        KEY_RIGHTSHIFT, KEY_TAB, REL_WHEEL, TABLET_LOGICAL_MAX,
    };

    // 트리에서 Cli 객체 id 찾기 (한 개 가정 — ADR-023). &TreeModel 받아 borrow 깔끔.
    fn find_cli(
        tm: &geulos_compositor::tree_model::TreeModel,
    ) -> Option<geulos_core::ObjectId> {
        tm.ids().find(|id| {
            tm.get(*id).map(|o| o.type_uri.as_str() == "aios.builtin/Cli@1").unwrap_or(false)
        })
    }

    /// focused=true인 Window 한 개 찾기 (id, content). Window 편집 라우팅 결정에 사용.
    /// 여러 Window가 focused로 잘못 표시될 경우 첫 번째만. ConsoleWindow/FileManager는 제외.
    fn find_focused_window(
        tm: &geulos_compositor::tree_model::TreeModel,
    ) -> Option<(geulos_core::ObjectId, String)> {
        tm.ids().find_map(|id| {
            let obj = tm.get(id)?;
            if obj.type_uri.as_str() != "aios.builtin/Window@1" {
                return None;
            }
            let focused = obj.state.get("focused").and_then(|v| v.as_bool()).unwrap_or(false);
            if !focused {
                return None;
            }
            let content =
                obj.state.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Some((id, content))
        })
    }

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

    // 4) 메인 루프 — 디스플레이 렌더 + evdev 클릭. DRM/KMS 우선(프레임마다 명시적 flush로
    //    fbdev 지연 플러시 병목 회피), 실패 시 fbdev 폴백.
    let mut fb = match geulos_compositor::vm_drm::DrmDisplay::open() {
        Ok(d) => {
            println!("[vm-compositor] 디스플레이 백엔드 = DRM/KMS");
            Display::Drm(d)
        }
        Err(e) => {
            eprintln!("[vm-compositor] DRM 실패({}) → fbdev 폴백", e);
            match Framebuffer::open() {
                Ok(f) => {
                    println!("[vm-compositor] 디스플레이 백엔드 = fbdev {}x{}", f.xres, f.yres);
                    Display::Fb(f)
                }
                Err(e2) => {
                    eprintln!("[vm-compositor] framebuffer도 실패: {}", e2);
                    std::process::exit(2);
                }
            }
        }
    };
    let mut input = match EvdevSet::open_all() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("[vm-compositor] evdev 실패: {}", e);
            std::process::exit(3);
        }
    };

    let (w, h) = fb.dims();
    let mut canvas = vec![0u32; w * h];
    let mut pointer = (w as i32 / 2, h as i32 / 2);
    let mut cli_state = CliLocalState::default();
    let mut shift = false;
    // SP4: Ctrl modifier (단축키) + 인-VM 클립보드 (복사/붙여넣기).
    let mut ctrl = false;
    let mut clipboard = String::new();
    // SP4: 한글 IME 상태 — Tab / Hangul 키로 토글.
    let mut korean_mode = false;
    let mut hangul = HangulComposer::new();

    // 더블클릭 감지 상태 — 500ms 이내 동일 target 재클릭 시 더블클릭으로 판정.
    let mut last_click: Option<(geulos_core::ObjectId, std::time::Instant)> = None;
    const DOUBLE_CLICK_MS: u128 = 500;

    // CLI 입력 라인에서 클릭 x → input_buffer byte offset. render의 입력 기하(input_x)와
    // editor의 측정 기반 매핑을 그대로 써서 시각·hit 일관성 유지 (단일 라인이라 line 0).
    fn cli_offset_at_x(input_buffer: &str, input_x: i32, click_x: i32) -> usize {
        use geulos_compositor::editor::{byte_offset_from_pixel, wrap_by_pixel_width};
        let lines = wrap_by_pixel_width(input_buffer, i32::MAX);
        byte_offset_from_pixel(&lines, 0, click_x - input_x)
    }

    // 좌클릭 drag 상태 (창 이동/리사이즈 + CLI 텍스트 선택/스크롤). drop 시점에 invoke.
    enum DragState {
        None,
        Moving { id: geulos_core::ObjectId, start_cursor: (i32, i32), start_pos: (i32, i32) },
        Resizing { id: geulos_core::ObjectId, start_cursor: (i32, i32), start_size: (i32, i32) },
        // SP4: CLI 입력 라인 텍스트 선택 드래그. input_x로 이동 중 x→offset 매핑.
        SelectingCli { input_x: i32 },
        // 스크롤 드래그: CLI 히스토리 영역을 누른 채 위/아래 드래그 → scroll_offset 조정.
        ScrollingCli { start_y: i32, start_offset: usize, line_height: i32 },
        // Window editor 텍스트 선택 드래그. content_rect 좌표/wrap_w를 보관해 ABS update마다 재사용.
        SelectingWindow {
            window_id: geulos_core::ObjectId,
            content_x: i32,
            content_y: i32,
            content_w: i32,
            scroll_y: usize,
        },
    }
    let mut drag = DragState::None;

    // F2 인라인 이름변경 상태 — [Rename] 클릭 시 selected_item에서 채우고,
    // Enter로 invoke Explorer.rename_selected, Esc로 취소. None이면 키 입력이 CLI로 라우팅.
    struct RenameInputState {
        explorer_id: geulos_core::ObjectId,
        target_id: geulos_core::ObjectId,
        buffer: String,
    }
    let mut rename_input: Option<RenameInputState> = None;

    // Window 편집 — keyboard_focus가 Window면 그 Window의 content를 editor가 들고 키 입력 라우팅.
    // 부팅 직후/CLI 클릭 시 Cli. Window content 클릭 시 그 Window. host compositor의 KeyboardFocus와 동형.
    enum KeyboardFocus {
        Cli,
        Window(geulos_core::ObjectId),
    }
    let mut keyboard_focus = KeyboardFocus::Cli;
    let mut editor: Option<EditorState> = None;
    // editor 한글 IME — preedit char를 editor.content에 *직접* insert해 in-place 표시하고,
    // 그 char가 차지하는 byte 길이를 추적해 다음 jamo 입력 시 replace한다. 0이면 preedit 없음.
    let mut editor_preedit_len: usize = 0;

    while !quit.load(Ordering::SeqCst) {
        // 입력 — 이벤트를 모아 루프 본문에서 처리(상태 변이가 많아 closure 부적합).
        let frame_start = std::time::Instant::now();

        // editor sync — keyboard_focus 기반. Cli면 editor=None, Window(id)면 그 Window의 content를
        // editor가 들고 있음. Window가 destroyed/사라지면 자동 Cli로 폴백.
        // find_focused_window는 더 이상 안 씀 — focused=true Window가 살아있어도 사용자가 CLI 클릭하면
        // editor 비활성이어야 함 (이전 H1 버그: editor가 영구 활성).
        let _ = find_focused_window; // 미사용 — keyboard_focus로 대체
        {
            let tm = tree.lock().unwrap();
            let target_window = match &keyboard_focus {
                KeyboardFocus::Window(id) => tm
                    .get(*id)
                    .filter(|o| o.type_uri.as_str() == "aios.builtin/Window@1")
                    .map(|o| {
                        let content = o
                            .state
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        (*id, content)
                    }),
                KeyboardFocus::Cli => None,
            };
            drop(tm);
            // Window 사라지면 자동 Cli 폴백.
            if matches!(keyboard_focus, KeyboardFocus::Window(_)) && target_window.is_none() {
                keyboard_focus = KeyboardFocus::Cli;
            }
            match (editor.as_ref(), target_window) {
                (None, Some((id, content))) => {
                    let mut ed = EditorState::new(id, content);
                    ed.cursor = ed.content.len();
                    editor = Some(ed);
                }
                (Some(cur), Some((id, content))) if cur.window_id != id => {
                    let mut ed = EditorState::new(id, content);
                    ed.cursor = ed.content.len();
                    editor = Some(ed);
                }
                (Some(_), None) => {
                    editor = None;
                }
                _ => {}
            }
        }

        let mut events = Vec::new();
        input.poll_events(0, |ev| events.push(ev)); // non-blocking — 프레임 페이싱은 루프 끝
        for ev in events {
            if ev.type_ == EV_ABS && ev.code == ABS_X {
                pointer.0 = scale_abs(ev.value, TABLET_LOGICAL_MAX, w as u32);
                // SP4: CLI 텍스트 선택 드래그 중이면 cursor를 현재 x로 확장.
                if let DragState::SelectingCli { input_x } = &drag {
                    let off = cli_offset_at_x(&cli_state.input_buffer, *input_x, pointer.0);
                    cli_state.extend_selection_to(off);
                }
                // Window editor 텍스트 선택 드래그.
                if let DragState::SelectingWindow {
                    content_x, content_y, content_w, scroll_y, ..
                } = &drag
                {
                    if let Some(ed) = editor.as_mut() {
                        const LINE_HEIGHT: i32 = 20;
                        let wrap_w = (*content_w - 4).max(20);
                        let wrapped = geulos_compositor::editor::wrap_by_pixel_width(
                            &ed.content,
                            wrap_w,
                        );
                        let click_line = (((pointer.1 - *content_y).max(0)) / LINE_HEIGHT)
                            as usize
                            + *scroll_y;
                        let click_x = (pointer.0 - *content_x).max(0);
                        let off = geulos_compositor::editor::byte_offset_from_pixel(
                            &wrapped, click_line, click_x,
                        );
                        ed.extend_cursor_to(off);
                    }
                }
            } else if ev.type_ == EV_ABS && ev.code == ABS_Y {
                pointer.1 = scale_abs(ev.value, TABLET_LOGICAL_MAX, h as u32);
                // CLI 스크롤 드래그 중이면 cursor y로 scroll_offset 갱신.
                if let DragState::ScrollingCli { start_y, start_offset, line_height } = &drag {
                    let dy = pointer.1 - *start_y;
                    let new_off = (*start_offset as i32 + dy / *line_height).max(0);
                    cli_state.scroll_offset = new_off as usize;
                }
                // Window editor selection 드래그 — y 변화도 cursor 갱신.
                if let DragState::SelectingWindow {
                    content_x, content_y, content_w, scroll_y, ..
                } = &drag
                {
                    if let Some(ed) = editor.as_mut() {
                        const LINE_HEIGHT: i32 = 20;
                        let wrap_w = (*content_w - 4).max(20);
                        let wrapped = geulos_compositor::editor::wrap_by_pixel_width(
                            &ed.content,
                            wrap_w,
                        );
                        let click_line = (((pointer.1 - *content_y).max(0)) / LINE_HEIGHT)
                            as usize
                            + *scroll_y;
                        let click_x = (pointer.0 - *content_x).max(0);
                        let off = geulos_compositor::editor::byte_offset_from_pixel(
                            &wrapped, click_line, click_x,
                        );
                        ed.extend_cursor_to(off);
                    }
                }
            } else if ev.type_ == EV_REL && ev.code == REL_WHEEL {
                // 마우스 휠 → 커서를 포함하는 *가장 안쪽* 스크롤 가능 컨테이너로 라우팅.
                // value > 0: wheel up → 위쪽/오래된 내용. value < 0: wheel down → 아래/최신.
                // 1 notch = 3 라인.
                //
                // 시각 컨테이너(layout rect) 기반 — 객체 트리 parent를 따라가지 않음. Explorer가
                // 보여주는 Folder/File는 객체 parent가 원래 마운트 위치(드라이브 → FileTree)라
                // parent 추적은 잘못된 컨테이너로 라우팅됨(우측 휠인데 좌측만 스크롤되던 버그).
                if ev.value != 0 {
                    let (cx, cy) = pointer;
                    let tm = tree.lock().unwrap();
                    let lay = layout(&tm, w as i32, h as i32);
                    let scrollable = lay
                        .iter()
                        .filter(|(id, rect, _)| {
                            rect.contains(cx, cy)
                                && tm
                                    .get(*id)
                                    .map(|o| {
                                        matches!(
                                            o.type_uri.as_str(),
                                            "aios.builtin/Cli@1"
                                                | "aios.builtin/Explorer@1"
                                                | "aios.builtin/FileTree@1"
                                                | "aios.builtin/Window@1"
                                                | "aios.builtin/ConsoleWindow@1"
                                        )
                                    })
                                    .unwrap_or(false)
                        })
                        .last()
                        .map(|(id, _, _)| id);
                    if let Some(sid) = scrollable {
                        if let Some(obj) = tm.get(sid) {
                            let uri = obj.type_uri.as_str();
                            if uri == "aios.builtin/Cli@1" {
                                let delta = (ev.value.abs() * 3) as usize;
                                drop(tm);
                                if ev.value > 0 {
                                    cli_state.scroll_offset =
                                        cli_state.scroll_offset.saturating_add(delta);
                                } else {
                                    cli_state.scroll_offset =
                                        cli_state.scroll_offset.saturating_sub(delta);
                                }
                            } else if uri == "aios.builtin/Explorer@1"
                                || uri == "aios.builtin/FileTree@1"
                                || uri == "aios.builtin/Window@1"
                                || uri == "aios.builtin/ConsoleWindow@1"
                            {
                                let cur = obj
                                    .state
                                    .get("scroll_y")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0);
                                // wheel up(value>0) → scroll_y 감소(위로). down → 증가.
                                let new_y = (cur - (ev.value * 3) as i64).max(0);
                                drop(tm);
                                let _ = ui_tx.try_send(UiAction::SetState {
                                    target: sid,
                                    key: "scroll_y".to_string(),
                                    value: serde_json::json!(new_y),
                                });
                            }
                        }
                    }
                }
            } else if ev.type_ == EV_KEY && ev.code == BTN_LEFT && ev.value == 1 {
                // 좌클릭 press — Window/ConsoleWindow 영역 판정 / Explorer nav / 그 외 dispatch.
                let (cx, cy) = pointer;
                let tm = tree.lock().unwrap();
                let lay = layout(&tm, w as i32, h as i32);
                if let Some((target, role)) = hit_test(&tm, &lay, cx, cy) {
                    if let Some(obj) = tm.get(target) {
                        let uri = obj.type_uri.as_str();
                        // chrome 분기는 toolbar HitRole일 때 skip — 같은 fm_id로 push된
                        // FmToolbar* role의 클릭이 잘못 close/move chrome으로 흡수되던 버그 fix.
                        let is_toolbar_role = matches!(role,
                            HitRole::FmToolbarNewFile
                            | HitRole::FmToolbarNewFolder
                            | HitRole::FmToolbarRename
                            | HitRole::FmToolbarDelete
                        );
                        if uri == "aios.builtin/Dialog@1" {
                            // Dialog 버튼 클릭 — host main.rs와 동일 매핑.
                            // 빼면 dispatch_click fallback이 args=Null로 invoke → desktop-shell이
                            // args.get("action").unwrap_or("거부")로 *항상 거부* 해석하는 회귀 (VM 버그).
                            let dialog_rect =
                                lay.get(target).unwrap_or(Rect { x: 0, y: 0, w: 0, h: 0 });
                            let dlg_inner = Rect {
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
                                    const BTN_W: i32 = 100;
                                    const BTN_H: i32 = 32;
                                    const GAP: i32 = 12;
                                    let total_w =
                                        n as i32 * BTN_W + (n as i32 - 1).max(0) * GAP;
                                    let by = dlg_inner.y + dlg_inner.h - BTN_H - 12;
                                    if cy >= by && cy < by + BTN_H {
                                        let bx_start = dlg_inner.x + (dlg_inner.w - total_w) / 2;
                                        let rel = cx - bx_start;
                                        if rel >= 0 {
                                            let idx = rel / (BTN_W + GAP);
                                            let idx_usize = idx as usize;
                                            let within_btn = rel < idx * (BTN_W + GAP) + BTN_W;
                                            if idx_usize < n && within_btn {
                                                let label = actions[idx_usize]
                                                    .as_str()
                                                    .unwrap_or("")
                                                    .to_string();
                                                let _ = ui_tx.try_send(UiAction::Invoke {
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
                        } else if (uri == "aios.builtin/Window@1"
                            || uri == "aios.builtin/ConsoleWindow@1"
                            || uri == "aios.builtin/FileManager@1")
                            && !is_toolbar_role
                        {
                            // main.rs와 동일한 window_geom 상수로 영역 계산 (동작 일치).
                            let win_rect = lay.get(target).unwrap_or(Rect { x: 0, y: 0, w: 0, h: 0 });
                            let inner = Rect {
                                x: win_rect.x + 1,
                                y: win_rect.y + 1,
                                w: win_rect.w - 2,
                                h: win_rect.h - 2,
                            };
                            let title =
                                Rect { x: inner.x, y: inner.y, w: inner.w, h: WINDOW_TITLE_H };
                            let close = Rect {
                                x: title.x + title.w - WINDOW_CLOSE_BTN - 4,
                                y: title.y + 4,
                                w: WINDOW_CLOSE_BTN,
                                h: WINDOW_CLOSE_BTN,
                            };
                            let resize = Rect {
                                x: inner.x + inner.w - WINDOW_RESIZE_HANDLE,
                                y: inner.y + inner.h - WINDOW_RESIZE_HANDLE,
                                w: WINDOW_RESIZE_HANDLE,
                                h: WINDOW_RESIZE_HANDLE,
                            };
                            // close 버튼 메서드: Window@1만 unsaved-changes 확인을 위해
                            // "close_confirm", ConsoleWindow@1/FileManager@1은 plain "close".
                            let use_plain_close = uri == "aios.builtin/ConsoleWindow@1"
                                || uri == "aios.builtin/FileManager@1";
                            if close.contains(cx, cy) {
                                let method = if use_plain_close { "close" } else { "close_confirm" };
                                let _ = ui_tx.try_send(UiAction::Invoke {
                                    target,
                                    method: method.to_string(),
                                    args: serde_json::Value::Null,
                                });
                            } else if resize.contains(cx, cy) {
                                let sw =
                                    obj.state.get("w").and_then(|v| v.as_i64()).unwrap_or(600) as i32;
                                let sh =
                                    obj.state.get("h").and_then(|v| v.as_i64()).unwrap_or(400) as i32;
                                drag = DragState::Resizing {
                                    id: target,
                                    start_cursor: (cx, cy),
                                    start_size: (sw, sh),
                                };
                            } else if title.contains(cx, cy) {
                                let sx =
                                    obj.state.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                let sy =
                                    obj.state.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                drag = DragState::Moving {
                                    id: target,
                                    start_cursor: (cx, cy),
                                    start_pos: (sx, sy),
                                };
                                let _ = ui_tx.try_send(UiAction::Invoke {
                                    target,
                                    method: "focus".to_string(),
                                    args: serde_json::Value::Null,
                                });
                            } else {
                                let _ = ui_tx.try_send(UiAction::Invoke {
                                    target,
                                    method: "focus".to_string(),
                                    args: serde_json::Value::Null,
                                });
                                // Window content 클릭 → keyboard_focus = Window(target). 즉시 editor 활성해
                                // 이번 클릭의 cursor 이동/selection이 곧바로 반영되게 한다 (다음 frame sync 대기 X).
                                if uri == "aios.builtin/Window@1" {
                                    keyboard_focus = KeyboardFocus::Window(target);
                                    if editor.as_ref().map(|e| e.window_id) != Some(target) {
                                        let content = obj
                                            .state
                                            .get("content")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        let mut ed = EditorState::new(target, content);
                                        ed.cursor = ed.content.len();
                                        editor = Some(ed);
                                    }
                                }
                                if uri == "aios.builtin/Window@1" {
                                    if let Some(ed) = editor.as_mut() {
                                        if target == ed.window_id {
                                            let _ = hangul.flush();
                                            editor_preedit_len = 0;
                                            let content_x = inner.x + 8;
                                            let content_y = inner.y + WINDOW_TITLE_H + 8;
                                            let content_w = inner.w - 16;
                                            const LINE_HEIGHT: i32 = 20;
                                            let wrap_w = (content_w - 4).max(20);
                                            let wrapped =
                                                geulos_compositor::editor::wrap_by_pixel_width(
                                                    &ed.content,
                                                    wrap_w,
                                                );
                                            let scroll_y = obj
                                                .state
                                                .get("scroll_y")
                                                .and_then(|v| v.as_i64())
                                                .unwrap_or(0)
                                                .max(0)
                                                as usize;
                                            let click_line = (((cy - content_y).max(0))
                                                / LINE_HEIGHT)
                                                as usize
                                                + scroll_y;
                                            let click_x = (cx - content_x).max(0);
                                            let off =
                                                geulos_compositor::editor::byte_offset_from_pixel(
                                                    &wrapped, click_line, click_x,
                                                );
                                            ed.set_cursor(off);
                                            ed.begin_selection();
                                            drag = DragState::SelectingWindow {
                                                window_id: ed.window_id,
                                                content_x,
                                                content_y,
                                                content_w,
                                                scroll_y,
                                            };
                                        }
                                    }
                                }
                            }
                        } else if uri == "aios.builtin/Explorer@1" {
                            if role == HitRole::ExplorerParentNav {
                                let _ = ui_tx.try_send(UiAction::Invoke {
                                    target,
                                    method: "navigate_up".to_string(),
                                    args: serde_json::Value::Null,
                                });
                            }
                        } else if uri == "aios.builtin/Cli@1" {
                            // SP4: 입력 라인 클릭 → 텍스트 선택 시작 (드래그로 확장).
                            // 조합 중 음절이 있으면 먼저 확정하고 preedit 비움 (클릭=조합 종료).
                            // editor에서 CLI로 포커스 전환 시 editor가 쓴 hangul preedit는 *editor에 이미*
                            // commit된 상태 → CLI로 가져오면 안 됨. preedit char만 비우고 commit X.
                            let prev_was_editor =
                                matches!(keyboard_focus, KeyboardFocus::Window(_));
                            keyboard_focus = KeyboardFocus::Cli;
                            if prev_was_editor {
                                let _ = hangul.flush();
                                editor_preedit_len = 0;
                            } else {
                                if let Some(c) = hangul.flush() {
                                    cli_state.handle_ime_commit(&c.to_string());
                                }
                                cli_state.handle_ime_preedit(String::new());
                            }
                            let rect = lay.get(target).unwrap_or(Rect { x: 0, y: 0, w: 0, h: 0 });
                            let (input_x, prompt_y, line_h) = cli_input_geometry(&rect, obj);
                            if cy >= prompt_y && cy < prompt_y + line_h {
                                // 입력 라인 → 텍스트 선택 시작.
                                let off = cli_offset_at_x(&cli_state.input_buffer, input_x, cx);
                                cli_state.start_selection_at(off);
                                drag = DragState::SelectingCli { input_x };
                            } else if cy < prompt_y {
                                // 히스토리 영역(입력 라인 위) → 스크롤 드래그 시작.
                                cli_state.clear_selection();
                                drag = DragState::ScrollingCli {
                                    start_y: cy,
                                    start_offset: cli_state.scroll_offset,
                                    line_height: line_h.max(1),
                                };
                            } else {
                                // 입력 라인 아래(드문 케이스) → 선택만 해제.
                                cli_state.clear_selection();
                            }
                        } else if role == HitRole::DesktopIcon {
                            // 바탕화면 아이콘 → open() (서버가 props.app으로 앱 실행).
                            let _ = ui_tx.try_send(UiAction::Invoke {
                                target,
                                method: "open".to_string(),
                                args: serde_json::Value::Null,
                            });
                        } else if role == HitRole::DockItem {
                            // 독 항목 → Dock.launch(item_id). 클릭한 y 위치에서 item index 역산
                            // (layout이 r.dock.y부터 DOCK_ITEM_H 칸으로 배치). item_id = items[idx].id,
                            // 없으면 items[idx].app (Dock items 컨벤션) → desktop-shell이 app 해석.
                            let dock_y = TOPBAR_H; // r.dock.y = TOPBAR_H
                            let idx = (((cy - dock_y).max(0)) / DOCK_ITEM_H) as usize;
                            let item_id = obj
                                .state
                                .get("items")
                                .and_then(|v| v.as_array())
                                .and_then(|items| items.get(idx))
                                .map(|it| {
                                    it.get("id")
                                        .and_then(|v| v.as_str())
                                        .or_else(|| it.get("app").and_then(|v| v.as_str()))
                                        .unwrap_or("")
                                        .to_string()
                                })
                                .unwrap_or_default();
                            let _ = ui_tx.try_send(UiAction::Invoke {
                                target,
                                method: "launch".to_string(),
                                args: serde_json::json!({ "item_id": item_id }),
                            });
                        } else if role == HitRole::TopBarItem {
                            // 네비바 항목 → TopBar.activate(item_id). 클릭 x에서 item index 역산
                            // (layout이 x=0부터 TOPBAR_ITEM_W 칸). item_id = items[idx].id, 없으면 "geulos".
                            let idx = (cx.max(0) / TOPBAR_ITEM_W) as usize;
                            let item_id = obj
                                .state
                                .get("items")
                                .and_then(|v| v.as_array())
                                .and_then(|items| items.get(idx))
                                .and_then(|it| it.get("id").and_then(|v| v.as_str()))
                                .unwrap_or("geulos")
                                .to_string();
                            let _ = ui_tx.try_send(UiAction::Invoke {
                                target,
                                method: "activate".to_string(),
                                args: serde_json::json!({ "item_id": item_id }),
                            });
                        } else if role == HitRole::CliResizeHandle {
                            // M3: CLI 리사이즈 드래그 (set_cli_height). M1은 no-op.
                        } else if matches!(role,
                            HitRole::FmToolbarNewFile
                            | HitRole::FmToolbarNewFolder
                            | HitRole::FmToolbarRename
                            | HitRole::FmToolbarDelete
                        ) {
                            // FM 툴바 버튼 → Explorer 메서드 호출. v1.5 고정 이름 전략.
                            // Rename은 invoke 대신 컴포지터 인라인 편집 모드(F2 스타일) 진입.
                            if let Some(ex) = dispatch::find_explorer(&tm) {
                                if role == HitRole::FmToolbarRename {
                                    // selected_item에서 target_id + 현재 name을 buffer로.
                                    let sel = ex
                                        .state
                                        .get("selected_item")
                                        .and_then(|v| v.as_str())
                                        .and_then(|s| {
                                            uuid::Uuid::parse_str(s)
                                                .ok()
                                                .map(geulos_core::ObjectId::from_uuid)
                                        });
                                    if let Some(tid) = sel {
                                        let name = tm
                                            .get(tid)
                                            .and_then(|o| {
                                                o.props.get("name").and_then(|v| v.as_str())
                                            })
                                            .unwrap_or("")
                                            .to_string();
                                        rename_input = Some(RenameInputState {
                                            explorer_id: ex.id,
                                            target_id: tid,
                                            buffer: name,
                                        });
                                    }
                                } else {
                                    let (method, args) = match role {
                                        HitRole::FmToolbarNewFile =>
                                            ("create_file", serde_json::json!({})),
                                        HitRole::FmToolbarNewFolder =>
                                            ("create_folder", serde_json::json!({})),
                                        HitRole::FmToolbarDelete =>
                                            ("delete_selected", serde_json::Value::Null),
                                        _ => unreachable!(),
                                    };
                                    let _ = ui_tx.try_send(UiAction::Invoke {
                                        target: ex.id,
                                        method: method.to_string(),
                                        args,
                                    });
                                }
                            }
                        } else if obj.type_uri.as_str() == "aios.std/Folder@1"
                            || obj.type_uri.as_str() == "aios.std/File@1"
                        {
                            if role == HitRole::ExpandToggle {
                                // [+]/[-] expand toggle — 즉시 dispatch (기존 동작 그대로).
                                let actions = dispatch_click(&tm, target, obj, role);
                                for a in actions {
                                    let _ = ui_tx.try_send(a);
                                }
                            } else if role == HitRole::Body {
                                let now = std::time::Instant::now();
                                let is_double = matches!(
                                    &last_click,
                                    Some((id, t)) if *id == target && now.duration_since(*t).as_millis() < DOUBLE_CLICK_MS
                                );
                                last_click = Some((target, now));
                                if is_double {
                                    // 더블클릭: 폴더 탐색 또는 파일 열기 (기존 dispatch_click).
                                    let actions = dispatch_click(&tm, target, obj, role);
                                    for a in actions {
                                        let _ = ui_tx.try_send(a);
                                    }
                                } else {
                                    // 단일클릭: 행 선택. Explorer.select(folder_id).
                                    if let Some(ex) = dispatch::find_explorer(&tm) {
                                        let _ = ui_tx.try_send(UiAction::Invoke {
                                            target: ex.id,
                                            method: "select".to_string(),
                                            args: serde_json::json!({ "folder_id": target.to_string() }),
                                        });
                                    }
                                }
                            } else {
                                // 그 외 role — 기존 dispatch_click 폴백.
                                let actions = dispatch_click(&tm, target, obj, role);
                                for a in actions {
                                    let _ = ui_tx.try_send(a);
                                }
                            }
                        } else {
                            let actions = dispatch_click(&tm, target, obj, role);
                            for a in actions {
                                let _ = ui_tx.try_send(a);
                            }
                        }
                    }
                }
            } else if ev.type_ == EV_KEY && ev.code == BTN_LEFT && ev.value == 0 {
                // 좌클릭 release — drag 완료 → move/resize invoke.
                let (cx, cy) = pointer;
                match drag {
                    DragState::Moving { id, start_cursor, start_pos } => {
                        let nx = start_pos.0 + (cx - start_cursor.0);
                        let ny = start_pos.1 + (cy - start_cursor.1);
                        let _ = ui_tx.try_send(UiAction::Invoke {
                            target: id,
                            method: "move".to_string(),
                            args: serde_json::json!({ "x": nx, "y": ny }),
                        });
                    }
                    DragState::Resizing { id, start_cursor, start_size } => {
                        let nw = (start_size.0 + (cx - start_cursor.0)).max(WINDOW_MIN_W);
                        let nh = (start_size.1 + (cy - start_cursor.1)).max(WINDOW_MIN_H);
                        let _ = ui_tx.try_send(UiAction::Invoke {
                            target: id,
                            method: "resize".to_string(),
                            args: serde_json::json!({ "w": nw, "h": nh }),
                        });
                    }
                    DragState::SelectingCli { input_x } => {
                        // 선택 드래그 종료 — 최종 x로 cursor 확정.
                        let off = cli_offset_at_x(&cli_state.input_buffer, input_x, cx);
                        cli_state.extend_selection_to(off);
                    }
                    DragState::ScrollingCli { .. } => {
                        // 스크롤 드래그 종료 — 추가 처리 없음 (scroll_offset은 이미 갱신됨).
                    }
                    DragState::SelectingWindow { .. } => {
                        // Window selection 드래그 종료 — anchor는 유지 (Ctrl+C 등 후속 동작 위해).
                    }
                    DragState::None => {}
                }
                drag = DragState::None;
            } else if ev.type_ == EV_KEY && (ev.code == KEY_LEFTSHIFT || ev.code == KEY_RIGHTSHIFT) {
                shift = ev.value != 0;
            } else if ev.type_ == EV_KEY && (ev.code == KEY_LEFTCTRL || ev.code == KEY_RIGHTCTRL) {
                ctrl = ev.value != 0;
            } else if ev.type_ == EV_KEY && ev.value == 1 && rename_input.is_some() {
                // F2 인라인 rename 모드 — 다른 키 라우팅보다 우선.
                // Esc 취소 / Enter 확정 / Backspace 한 글자 / 일반 키 buffer push.
                // 한글 IME 통합은 후속 — V1은 영문/숫자/문장부호만.
                match ev.code {
                    KEY_ESC => {
                        rename_input = None;
                    }
                    KEY_ENTER => {
                        if let Some(ri) = rename_input.take() {
                            let _ = ui_tx.try_send(UiAction::Invoke {
                                target: ri.explorer_id,
                                method: "rename_selected".to_string(),
                                args: serde_json::json!({ "new_name": ri.buffer }),
                            });
                        }
                    }
                    KEY_BACKSPACE => {
                        if let Some(ri) = rename_input.as_mut() {
                            ri.buffer.pop(); // char-aware (UTF-8)
                        }
                    }
                    _ => {
                        if let Some(ch) = keycode_to_char(ev.code, shift) {
                            if let Some(ri) = rename_input.as_mut() {
                                ri.buffer.push(ch);
                            }
                        }
                    }
                }
            } else if ev.type_ == EV_KEY
                && ev.value == 1
                && editor.is_some()
                && ev.code != KEY_TAB
                && ev.code != KEY_HANGEUL
            {
                // Window 편집 — focused Window가 있으면 키 입력을 editor.content로 라우팅.
                // Tab/Hangul은 *글로벌 한/영 토글*이라 editor 분기를 건너뛰어 토글 분기로 가야 함.
                // Ctrl+S → save_to_file invoke (content 포함, wire 한 번에 디스크 commit).
                // Backspace/Enter/char → editor mutate (local-master, render는 editor.content 직접 표시).
                // 한글 IME — preedit char를 editor.content에 in-place insert해 표시하고
                // editor_preedit_len으로 다음 jamo 입력 시 replace.
                let ed = editor.as_mut().unwrap();
                if ctrl {
                    // Ctrl+S 저장 / +A 전체선택 / +C 복사 / +X 잘라내기 / +V 붙여넣기.
                    // 단축키 처리 전 조합 중 음절은 in-place commit으로 처리됨 (editor.content에 있음).
                    let _ = hangul.flush();
                    editor_preedit_len = 0;
                    match keycode_to_char(ev.code, false) {
                        Some('s') => {
                            let _ = ui_tx.try_send(UiAction::Invoke {
                                target: ed.window_id,
                                method: "save_to_file".to_string(),
                                args: serde_json::json!({ "content": ed.content.clone() }),
                            });
                        }
                        Some('a') => ed.select_all(),
                        Some('c') => {
                            let sel = ed.selected_text();
                            if !sel.is_empty() {
                                clipboard = sel.to_string();
                            }
                        }
                        Some('x') => {
                            let sel = ed.selected_text();
                            if !sel.is_empty() {
                                clipboard = sel.to_string();
                                ed.delete_selection();
                            }
                        }
                        Some('v') => {
                            if !clipboard.is_empty() {
                                ed.insert_str(&clipboard);
                            }
                        }
                        _ => {}
                    }
                } else if korean_mode {
                    // 한글 IME — Hangul 조합기 + editor in-place preedit.
                    if ev.code == KEY_BACKSPACE {
                        let (out, should_delete_committed) = hangul.backspace();
                        if editor_preedit_len > 0 {
                            ed.backspace();
                            editor_preedit_len = 0;
                        }
                        if let Some(p) = out.preedit {
                            ed.insert_char(p);
                            editor_preedit_len = p.len_utf8();
                        }
                        if should_delete_committed {
                            ed.backspace();
                        }
                    } else if ev.code == KEY_ENTER {
                        // 조합 중 음절은 이미 editor.content에 있어 committed로 처리.
                        let _ = hangul.flush();
                        editor_preedit_len = 0;
                        ed.newline();
                    } else if let Some(ch) = keycode_to_char(ev.code, shift) {
                        if let Some(jamo) = qwerty_to_jamo(ch) {
                            // 자모 키 — 기존 preedit 제거 후 새 결과 반영.
                            if editor_preedit_len > 0 {
                                ed.backspace();
                                editor_preedit_len = 0;
                            }
                            let out = hangul.input_jamo(jamo);
                            for c in out.committed.chars() {
                                ed.insert_char(c);
                            }
                            if let Some(p) = out.preedit {
                                ed.insert_char(p);
                                editor_preedit_len = p.len_utf8();
                            }
                        } else {
                            // 비-자모 (숫자, 공백 등): 조합 종료 + ASCII insert.
                            let _ = hangul.flush();
                            editor_preedit_len = 0;
                            ed.insert_char(ch);
                        }
                    }
                } else if ev.code == KEY_BACKSPACE {
                    ed.backspace();
                } else if ev.code == KEY_ENTER {
                    ed.newline();
                } else if let Some(ch) = keycode_to_char(ev.code, shift) {
                    ed.insert_char(ch);
                }
            } else if ev.type_ == EV_KEY
                && ev.value == 1
                && (ev.code == KEY_TAB || ev.code == KEY_HANGEUL)
            {
                // Tab(한/영 토글) 또는 Hangul 키 — 한/영 모드 토글.
                // 우Alt는 Windows IME가 가로채 VM에 안 들어오고, Alt는 단축키용으로 비워둠.
                korean_mode = !korean_mode;
                if editor.is_some() {
                    // editor 활성 — preedit char는 이미 editor.content에 committed로 들어있음.
                    let _ = hangul.flush();
                    editor_preedit_len = 0;
                } else {
                    // CLI — 조합 중 음절을 확정하고 preedit 비움.
                    if let Some(c) = hangul.flush() {
                        cli_state.handle_ime_commit(&c.to_string());
                    }
                    cli_state.handle_ime_preedit(String::new());
                }
                println!("[vm-compositor] 한/영 모드: {}", if korean_mode { "한글" } else { "영문" });
            } else if ev.type_ == EV_KEY && ev.value == 1 && ctrl {
                // ── Ctrl 단축키 (영/한 모드 공통) ───────────────────────────────
                // 조합 중 음절이 있으면 먼저 확정 (단축키가 버퍼를 건드리므로 상태 일관성).
                if let Some(c) = hangul.flush() {
                    cli_state.handle_ime_commit(&c.to_string());
                }
                cli_state.handle_ime_preedit(String::new());
                match keycode_to_char(ev.code, false) {
                    Some('a') => cli_state.select_all(),
                    Some('c') => {
                        if let Some(t) = cli_state.copy_selection() {
                            clipboard = t;
                        }
                    }
                    Some('x') => {
                        if let Some(t) = cli_state.cut_selection() {
                            clipboard = t;
                        }
                    }
                    Some('v') => cli_state.handle_paste(&clipboard),
                    _ => {}
                }
            } else if ev.type_ == EV_KEY && ev.value == 1 {
                // 키보드 입력 → CLI.
                if korean_mode {
                    // ── 한글 모드 ──────────────────────────────────────────────
                    if ev.code == KEY_ENTER {
                        // Enter: 조합 중 음절 확정 후 Submit.
                        if let Some(c) = hangul.flush() {
                            cli_state.handle_ime_commit(&c.to_string());
                        }
                        cli_state.handle_ime_preedit(String::new());
                        if let Some(submitted) = cli_state.handle_key(KeyAction::Submit) {
                            let cli_id = {
                                let tm = tree.lock().unwrap();
                                find_cli(&tm)
                            };
                            if let Some(cli_id) = cli_id {
                                let _ = ui_tx.try_send(UiAction::Invoke {
                                    target: cli_id,
                                    method: "submit_input".to_string(),
                                    args: serde_json::json!({ "text": submitted }),
                                });
                            }
                        }
                    } else if ev.code == KEY_BACKSPACE {
                        // Backspace: 조합 중 음절 분해, 또는 이미 committed된 글자 삭제.
                        let (out, should_delete_committed) = hangul.backspace();
                        // backspace는 committed에 뭔가를 넣지 않음 — 삽입 불필요.
                        // preedit 갱신.
                        cli_state.handle_ime_preedit(
                            out.preedit.map(|c| c.to_string()).unwrap_or_default(),
                        );
                        if should_delete_committed {
                            // 조합 중 음절 없음 → 이미 버퍼에 있는 글자 하나 삭제.
                            cli_state.handle_key(KeyAction::Backspace);
                        }
                    } else if let Some(ch) = keycode_to_char(ev.code, shift) {
                        if let Some(jamo) = qwerty_to_jamo(ch) {
                            // SP4: 선택 위에 한글 입력 시작 → 선택 영역 먼저 교체
                            // (조합 중이 아닐 때만; 조합 중 jamo는 기존 선택과 무관).
                            if hangul.preedit().is_none() {
                                cli_state.delete_selection();
                            }
                            // 자모 키: 조합기에 넣고 결과 반영.
                            let out = hangul.input_jamo(jamo);
                            // committed 문자 삽입 (보통 이전 음절 확정분).
                            for c in out.committed.chars() {
                                cli_state.handle_ime_commit(&c.to_string());
                            }
                            // preedit 갱신 (조합 중 음절).
                            cli_state.handle_ime_preedit(
                                out.preedit.map(|c| c.to_string()).unwrap_or_default(),
                            );
                        } else {
                            // 비-자모 키 (숫자, 공백, 문장부호): 조합 중 음절 확정 후 ASCII 삽입.
                            if let Some(c) = hangul.flush() {
                                cli_state.handle_ime_commit(&c.to_string());
                            }
                            cli_state.handle_ime_preedit(String::new());
                            cli_state.handle_key(KeyAction::InsertChar(ch));
                        }
                    }
                } else {
                    // ── 영문 모드 (기존 로직 그대로) ─────────────────────────
                    let action = if ev.code == KEY_ENTER {
                        Some(KeyAction::Submit)
                    } else if ev.code == KEY_BACKSPACE {
                        Some(KeyAction::Backspace)
                    } else {
                        keycode_to_char(ev.code, shift).map(KeyAction::InsertChar)
                    };
                    if let Some(action) = action {
                        if let Some(submitted) = cli_state.handle_key(action) {
                            // Enter — 현재 입력을 Cli.submit_input으로 commit.
                            let cli_id = {
                                let tm = tree.lock().unwrap();
                                find_cli(&tm)
                            };
                            if let Some(cli_id) = cli_id {
                                let _ = ui_tx.try_send(UiAction::Invoke {
                                    target: cli_id,
                                    method: "submit_input".to_string(),
                                    args: serde_json::json!({ "text": submitted }),
                                });
                            }
                        }
                    }
                }
            }
        }

        // 렌더
        {
            let tm = tree.lock().unwrap();
            let lay = layout(&tm, w as i32, h as i32);
            let rename_ov = rename_input.as_ref().map(|ri| RenameOverlay {
                target_id: ri.target_id,
                buffer: ri.buffer.clone(),
            });
            render_frame(
                &tm,
                &lay,
                &mut canvas,
                w,
                h,
                &cli_state,
                editor.as_ref(),
                rename_ov.as_ref(),
            );
        }
        // 마우스 커서 — VM엔 OS 커서가 없으니 컴포지터가 직접 그린다. 십자선(검은 외곽 +
        // 흰 중심)이라 어떤 배경에서도 보이고 중심이 정확한 클릭 지점.
        {
            let (cx, cy) = pointer;
            let black = 0xFF_00_00_00u32;
            let white = 0xFF_FF_FF_FFu32;
            fill_rect(&mut canvas, w, h, &Rect { x: cx - 1, y: cy - 9, w: 3, h: 19 }, black);
            fill_rect(&mut canvas, w, h, &Rect { x: cx - 9, y: cy - 1, w: 19, h: 3 }, black);
            fill_rect(&mut canvas, w, h, &Rect { x: cx, y: cy - 8, w: 1, h: 17 }, white);
            fill_rect(&mut canvas, w, h, &Rect { x: cx - 8, y: cy, w: 17, h: 1 }, white);
        }
        fb.present(&canvas);

        // 프레임 페이싱 — 목표 ~60fps(16ms/frame). poll이 non-blocking이라 여기서 페이스.
        let elapsed = frame_start.elapsed();
        let target = Duration::from_millis(16);
        if elapsed < target {
            std::thread::sleep(target - elapsed);
        }
    }
    println!("[vm-compositor] exit");
}
