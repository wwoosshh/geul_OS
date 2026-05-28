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
    use geulos_compositor::layout::{layout, HitRole, Rect};
    use geulos_compositor::messages::{ServerEvent, UiAction};
    use geulos_compositor::render::{fill_rect, render_frame};
    use geulos_compositor::window_geom::{
        WINDOW_CLOSE_BTN, WINDOW_MIN_H, WINDOW_MIN_W, WINDOW_RESIZE_HANDLE, WINDOW_TITLE_H,
    };
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

    // 좌클릭 drag 상태 (창 이동/리사이즈). drop 시점에 한 번 invoke (main.rs와 동형).
    enum DragState {
        None,
        Moving { id: geulos_core::ObjectId, start_cursor: (i32, i32), start_pos: (i32, i32) },
        Resizing { id: geulos_core::ObjectId, start_cursor: (i32, i32), start_size: (i32, i32) },
    }
    let mut drag = DragState::None;

    while !quit.load(Ordering::SeqCst) {
        // 입력 — 이벤트를 모아 루프 본문에서 처리(상태 변이가 많아 closure 부적합).
        let mut events = Vec::new();
        input.poll_events(16, |ev| events.push(ev));
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
        std::thread::sleep(Duration::from_millis(16));
    }
    println!("[vm-compositor] exit");
}
