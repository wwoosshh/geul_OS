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
                    println!("[vm-compositor] click@({},{}) -> {:?}", pointer.0, pointer.1, a);
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
