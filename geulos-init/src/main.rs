//! GeulOS init — Linux PID 1 책임자 (ADR-016).
//!
//! Linux 타겟에서만 실제 동작. Windows에서는 친절한 에러 메시지 후 종료.
//!
//! 본격 구현 (mount/network/spawn/signal)은 후속 M6 task에서.

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
    println!("[init] geulos-init scaffold (PID {})", std::process::id());
    println!("[init] Tasks 3~6에서 mount/network/spawn/signal 본격 구현 예정");
    // 지금은 즉시 종료 — kernel이 panic하지만 PoC 빌드 검증 목적이므로 OK.
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("geulos-init only builds for Linux target.");
    eprintln!("Use: cargo build --target x86_64-unknown-linux-musl -p geulos-init");
    std::process::exit(1);
}
