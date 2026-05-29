//! GeulOS stage-1 부트스트랩 (initramfs /init = PID 1).
//!
//! 책임: virtio_blk+ext4 모듈 적재 → /dev/vda 포맷(빈 경우) → 마운트 →
//! 시스템 파일 동기화(M1) → switch_root로 디스크 루트 진입.
//! 모든 실패는 램디스크 폴백으로 degrade — PID 1은 절대 그냥 종료하지 않는다.

// 순수 모듈 — 모든 타겟에서 컴파일/테스트.
mod superblock;
mod syncplan;

// 시스템콜 모듈 — Linux 타겟에서만 컴파일.
#[cfg(target_os = "linux")]
mod modload;
#[cfg(target_os = "linux")]
mod disk;
#[cfg(target_os = "linux")]
mod switchroot;

#[cfg(target_os = "linux")]
fn main() {
    // M0/M1에서 구현 채움.
    println!("[bootstrap] geulos-bootstrap stage 1 (PID {})", std::process::id());
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("geulos-bootstrap only runs on Linux (initramfs PID 1 role).");
    eprintln!("Cross-compile: cargo zigbuild --target x86_64-unknown-linux-musl -p geulos-bootstrap");
    std::process::exit(1);
}
