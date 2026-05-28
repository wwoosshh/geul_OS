//! GeulOS init — Linux PID 1 책임자 (ADR-016).
//!
//! 책임:
//! 1. /proc, /sys, /dev mount
//! 2. 네트워크 셋업 (QEMU user-mode 활용)
//! 3. geulosd + geulos-echo-app spawn
//! 4. 자식 좀비 reap + main loop
//!
//! Windows에서는 친절한 에러 메시지 후 종료 (cross-compile 안내).

#[cfg(target_os = "linux")]
mod modules;
#[cfg(target_os = "linux")]
mod mount;
#[cfg(target_os = "linux")]
mod network;
#[cfg(target_os = "linux")]
mod signal;
#[cfg(target_os = "linux")]
mod spawn;

#[cfg(target_os = "linux")]
fn main() {
    println!();
    println!("=== GeulOS init (PID {}) ===", std::process::id());
    println!();

    // 1. /proc, /sys, /dev mount
    if let Err(e) = mount::mount_essentials() {
        eprintln!("[init] mount errors: {}", e);
        // 부분 성공도 OK — server-host가 일부 기능 동작 가능
    }

    // 2. 커널 모듈 적재 (ADR-017). NIC 드라이버 등 필수 모듈을 finit_module로.
    //    네트워크보다 *반드시 먼저* — eth*/enp* 인터페이스가 생기려면 드라이버 필요.
    if let Err(e) = modules::load_all() {
        eprintln!("[init] module load errors: {}", e);
    }

    // 3. 네트워크 (QEMU user-mode가 자동 — no-op)
    if let Err(e) = network::bring_up_loopback_and_eth0() {
        eprintln!("[init] network: {}", e);
    }

    // 3. server-host + echo-app spawn
    let processes = match spawn::spawn_all() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[init] spawn failed catastrophically: {}", e);
            eprintln!("[init] cannot continue without server-host");
            // PID 1이 죽으면 kernel panic — 무한 sleep으로 살아만 있기
            loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
        }
    };

    let server_pid = processes.server.id();
    let shell_pid = processes.desktop_shell.as_ref().map(|c| c.id());
    let echo_pid = processes.echo_app.as_ref().map(|c| c.id());
    let skeleton_pid = processes.skeleton.as_ref().map(|c| c.id());

    println!();
    println!(
        "[init] entering main loop (server PID {}, shell PID {:?}, echo PID {:?}, compositor PID {:?})",
        server_pid, shell_pid, echo_pid, skeleton_pid
    );
    println!("[init] external ai-bridge can connect via host-forwarded TCP");
    println!();

    // 4. main loop — 좀비 reap + 자식 sanity 체크
    loop {
        let reaped = signal::reap_zombies();
        if reaped > 0 {
            eprintln!("[init] {} zombies reaped", reaped);
        }

        // server-host가 죽으면 시스템 사실상 중단. 본 단계에서는 *재시작 안 함* —
        // 첫 부팅 디버깅을 단순하게 유지. 향후 M6.5에서 supervisor 정책 추가.
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("geulos-init only runs on Linux (PID 1 role).");
    eprintln!("On Windows host, cross-compile via:");
    eprintln!("  cargo build --target x86_64-unknown-linux-musl -p geulos-init");
    eprintln!("Or run boot/build.ps1 for full VM image assembly.");
    std::process::exit(1);
}
