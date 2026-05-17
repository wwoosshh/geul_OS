//! SIGCHLD 처리 + 좀비 reaping.
//!
//! PID 1은 모든 *고아 프로세스의 부모*이므로 좀비를 reap해줘야 함.
//! 매 루프마다 non-blocking waitpid로 sweep.

use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;

/// 종료된 자식 프로세스들을 reap한다. 반환값은 reap한 개수.
pub fn reap_zombies() -> usize {
    let mut reaped = 0;
    loop {
        match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => break,
            Ok(WaitStatus::Exited(pid, code)) => {
                eprintln!("[init] reaped PID {} (exit {})", pid, code);
                reaped += 1;
            }
            Ok(WaitStatus::Signaled(pid, sig, _)) => {
                eprintln!("[init] reaped PID {} (signal {:?})", pid, sig);
                reaped += 1;
            }
            Ok(_) => reaped += 1,
            Err(_) => break, // 더 reap할 자식 없음 또는 ECHILD
        }
    }
    reaped
}
