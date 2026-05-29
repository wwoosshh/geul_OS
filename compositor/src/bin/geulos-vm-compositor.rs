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

    use geulos_compositor::dispatch::dispatch_click;
    use geulos_compositor::hit_test::hit_test;
    use geulos_compositor::keyboard::{CliLocalState, KeyAction};
    use geulos_compositor::layout::{
        layout, HitRole, Rect, DOCK_ITEM_H, TOPBAR_H, TOPBAR_ITEM_W,
    };
    use geulos_compositor::messages::{ServerEvent, UiAction};
    use geulos_compositor::render::{fill_rect, render_frame};
    use geulos_compositor::window_geom::{
        WINDOW_CLOSE_BTN, WINDOW_MIN_H, WINDOW_MIN_W, WINDOW_RESIZE_HANDLE, WINDOW_TITLE_H,
    };
    use geulos_compositor::server_client::{run_server_client, UserEvent};
    use geulos_compositor::tree_model::TreeModel;
    use geulos_compositor::vm_fb::Framebuffer;
    use geulos_compositor::vm_input::{
        keycode_to_char, scale_abs, EvdevSet, ABS_X, ABS_Y, BTN_LEFT, EV_ABS, EV_KEY,
        KEY_BACKSPACE, KEY_ENTER, KEY_LEFTSHIFT, KEY_RIGHTSHIFT, TABLET_LOGICAL_MAX,
    };

    // 트리에서 Cli 객체 id 찾기 (한 개 가정 — ADR-023). &TreeModel 받아 borrow 깔끔.
    fn find_cli(
        tm: &geulos_compositor::tree_model::TreeModel,
    ) -> Option<geulos_core::ObjectId> {
        tm.ids().find(|id| {
            tm.get(*id).map(|o| o.type_uri.as_str() == "aios.builtin/Cli@1").unwrap_or(false)
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

    // 좌클릭 drag 상태 (창 이동/리사이즈). drop 시점에 한 번 invoke (main.rs와 동형).
    enum DragState {
        None,
        Moving { id: geulos_core::ObjectId, start_cursor: (i32, i32), start_pos: (i32, i32) },
        Resizing { id: geulos_core::ObjectId, start_cursor: (i32, i32), start_size: (i32, i32) },
    }
    let mut drag = DragState::None;

    while !quit.load(Ordering::SeqCst) {
        // 입력 — 이벤트를 모아 루프 본문에서 처리(상태 변이가 많아 closure 부적합).
        let frame_start = std::time::Instant::now();
        let mut events = Vec::new();
        input.poll_events(0, |ev| events.push(ev)); // non-blocking — 프레임 페이싱은 루프 끝
        for ev in events {
            if ev.type_ == EV_ABS && ev.code == ABS_X {
                pointer.0 = scale_abs(ev.value, TABLET_LOGICAL_MAX, w as u32);
            } else if ev.type_ == EV_ABS && ev.code == ABS_Y {
                pointer.1 = scale_abs(ev.value, TABLET_LOGICAL_MAX, h as u32);
            } else if ev.type_ == EV_KEY && ev.code == BTN_LEFT && ev.value == 1 {
                // 좌클릭 press — Window/ConsoleWindow 영역 판정 / Explorer nav / 그 외 dispatch.
                let (cx, cy) = pointer;
                let tm = tree.lock().unwrap();
                let lay = layout(&tm, w as i32, h as i32);
                if let Some((target, role)) = hit_test(&tm, &lay, cx, cy) {
                    if let Some(obj) = tm.get(target) {
                        let uri = obj.type_uri.as_str();
                        if uri == "aios.builtin/Window@1" || uri == "aios.builtin/ConsoleWindow@1" {
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
                            let is_console = uri == "aios.builtin/ConsoleWindow@1";
                            if close.contains(cx, cy) {
                                let method = if is_console { "close" } else { "close_confirm" };
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
                            // CLI focus — 키보드 입력은 C2에서.
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
                    DragState::None => {}
                }
                drag = DragState::None;
            } else if ev.type_ == EV_KEY && (ev.code == KEY_LEFTSHIFT || ev.code == KEY_RIGHTSHIFT) {
                shift = ev.value != 0;
            } else if ev.type_ == EV_KEY && ev.value == 1 {
                // 키보드 입력 → CLI (현재 모든 키를 CLI로; window 편집/한글 IME는 후속).
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

        // 렌더
        {
            let tm = tree.lock().unwrap();
            let lay = layout(&tm, w as i32, h as i32);
            render_frame(&tm, &lay, &mut canvas, w, h, &cli_state, None);
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
