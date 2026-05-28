//! VM 디스플레이 기초 골격 — /dev/fb0에 사각형 + 클릭 자국 + 키 표시.
//! 화면·입력 배관이 VM 게스트 안에서 실제로 동작함을 증명한다.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("geulos-vm-skeleton은 VM(Linux) 전용입니다. 호스트 개발은 geulos-compositor를 쓰세요.");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    use geulos_compositor::layout::Rect;
    use geulos_compositor::render::fill_rect;
    use geulos_compositor::vm_fb::Framebuffer;
    use geulos_compositor::vm_input::{
        scale_abs, EvdevSet, ABS_X, ABS_Y, BTN_LEFT, EV_ABS, EV_KEY, TABLET_LOGICAL_MAX,
    };

    println!("[skeleton] starting — opening /dev/fb0");
    let mut fb = match Framebuffer::open() {
        Ok(fb) => fb,
        Err(e) => {
            eprintln!("[skeleton] framebuffer 실패: {}", e);
            std::process::exit(2);
        }
    };
    println!("[skeleton] fb {}x{} {:?}", fb.xres, fb.yres, fb.format());

    let mut input = match EvdevSet::open_all() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("[skeleton] evdev 실패: {}", e);
            std::process::exit(3);
        }
    };

    let (w, h) = (fb.xres, fb.yres);
    let mut canvas = vec![0u32; w * h];

    let mut pointer = (w as i32 / 2, h as i32 / 2);
    let mut markers: Vec<(i32, i32)> = Vec::new();
    let mut key_count: u32 = 0;

    const BG: u32 = 0xFF_1E_1E_1E; // 어두운 회색
    const TITLE: u32 = 0xFF_2D_8C_FF; // 파랑 바
    const CENTER: u32 = 0xFF_4A_9E_FF; // 밝은 파랑 사각형
    const MARKER: u32 = 0xFF_FF_55_55; // 빨강 클릭 자국

    loop {
        // 입력 처리
        input.poll_events(16, |ev| {
            if ev.type_ == EV_ABS && ev.code == ABS_X {
                pointer.0 = scale_abs(ev.value, TABLET_LOGICAL_MAX, w as u32);
            } else if ev.type_ == EV_ABS && ev.code == ABS_Y {
                pointer.1 = scale_abs(ev.value, TABLET_LOGICAL_MAX, h as u32);
            } else if ev.type_ == EV_KEY && ev.code == BTN_LEFT && ev.value == 1 {
                markers.push(pointer);
                println!("[skeleton] click at ({}, {})", pointer.0, pointer.1);
            } else if ev.type_ == EV_KEY && ev.code != BTN_LEFT && ev.value == 1 {
                key_count = key_count.wrapping_add(1);
                println!("[skeleton] key code={} (count={})", ev.code, key_count);
            }
        });

        // 그리기
        fill_rect(&mut canvas, w, h, &Rect { x: 0, y: 0, w: w as i32, h: h as i32 }, BG);
        fill_rect(&mut canvas, w, h, &Rect { x: 0, y: 0, w: w as i32, h: 40 }, TITLE);
        fill_rect(
            &mut canvas,
            w,
            h,
            &Rect { x: w as i32 / 2 - 120, y: h as i32 / 2 - 60, w: 240, h: 120 },
            CENTER,
        );
        // 키 입력 표시기 — 우상단, 키 누를 때마다 색 변화
        let indicator = 0xFF_00_00_00 | ((key_count.wrapping_mul(40) & 0xFF) << 8) | 0x80;
        fill_rect(&mut canvas, w, h, &Rect { x: w as i32 - 60, y: 50, w: 40, h: 40 }, indicator);
        // 클릭 자국
        for &(mx, my) in &markers {
            fill_rect(&mut canvas, w, h, &Rect { x: mx - 5, y: my - 5, w: 10, h: 10 }, MARKER);
        }

        fb.present(&canvas);
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}
