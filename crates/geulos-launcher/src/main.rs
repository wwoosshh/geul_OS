//! GeulOS 단일 진입점 launcher.
//!
//! 매번 server-host → desktop-shell → compositor를 *수동으로* 3 cmd로 spawn하는 것은
//! 일반 사용자에게 비실용적 (개발 디버그 용도). 이 launcher가 셋 모두 자동으로 spawn +
//! 순서 보장 + 로그 파일에 forward + compositor 종료 시 cleanup 한다.
//!
//! **사용**: `cargo run -p geulos-launcher` 또는 `geulos.exe` 직접 실행.
//!
//! **디버그**: 기존 `cargo run -p geulos-server-host` 등 분리 실행은 그대로 동작 — 이
//! launcher는 *통합 모드*만 제공하며 기존 워크플로를 깨지 않는다.
//!
//! **NOTE**: server-host crate(`geulos-server-host`)의 실제 binary 이름은 `geulosd`
//! (Cargo.toml `[[bin]] name = "geulosd"`). desktop-shell / compositor는 crate 이름
//! 그대로(`geulos-desktop-shell`, `geulos-compositor`). locate_bin은 이 실제 binary
//! 이름을 사용한다.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};

const SERVER_ADDR: &str = "127.0.0.1:5550";
const SERVER_READY_TIMEOUT_SECS: u64 = 10;
const SHELL_READY_TIMEOUT_SECS: u64 = 15;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("[geulos] GeulOS launcher 시작");

    // 로그 디렉터리 — ~/.geulos/logs/.
    let log_dir = dirs_log_dir()?;
    std::fs::create_dir_all(&log_dir)?;
    eprintln!("[geulos] 로그 디렉터리: {}", log_dir.display());

    // 1) server-host spawn — 실제 binary 이름은 `geulosd`.
    let server_bin = locate_bin("geulosd")?;
    eprintln!("[geulos] server-host spawn: {}", server_bin.display());
    let mut server = spawn_with_log(&server_bin, &log_dir.join("server.log")).await?;

    // server TCP listen 대기.
    if !wait_for_tcp(SERVER_ADDR, SERVER_READY_TIMEOUT_SECS).await {
        eprintln!("[geulos] server-host {}초 안에 listen 안 함 — 종료", SERVER_READY_TIMEOUT_SECS);
        let _ = server.kill().await;
        std::process::exit(1);
    }
    eprintln!("[geulos] server-host ready");

    // 2) desktop-shell spawn.
    let shell_bin = locate_bin("geulos-desktop-shell")?;
    eprintln!("[geulos] desktop-shell spawn: {}", shell_bin.display());
    let mut shell = spawn_with_log(&shell_bin, &log_dir.join("shell.log")).await?;

    // desktop-shell이 ready 메시지 찍을 때까지 대기 — 로그 파일 polling.
    if !wait_for_log_line(&log_dir.join("shell.log"), "subscribed to", SHELL_READY_TIMEOUT_SECS)
        .await
    {
        eprintln!("[geulos] desktop-shell {}초 안에 ready 안 함 — 종료", SHELL_READY_TIMEOUT_SECS);
        let _ = shell.kill().await;
        let _ = server.kill().await;
        std::process::exit(1);
    }
    eprintln!("[geulos] desktop-shell ready");

    // 3) compositor spawn — foreground attach (사용자가 GUI 본다).
    let compositor_bin = locate_bin("geulos-compositor")?;
    eprintln!("[geulos] compositor spawn (GUI): {}", compositor_bin.display());
    let mut compositor = spawn_with_log(&compositor_bin, &log_dir.join("compositor.log")).await?;

    // Ctrl+C 핸들러 — 모든 자식 정리.
    tokio::select! {
        status = compositor.wait() => {
            eprintln!("[geulos] compositor 종료 (status: {:?})", status);
        }
        _ = tokio::signal::ctrl_c() => {
            eprintln!("[geulos] Ctrl+C — cleanup 시작");
            let _ = compositor.kill().await;
        }
    }

    // 자식 cleanup — compositor가 종료(또는 kill)된 후 server/shell도 모두 정리.
    eprintln!("[geulos] desktop-shell cleanup");
    let _ = shell.kill().await;
    eprintln!("[geulos] server-host cleanup");
    let _ = server.kill().await;

    eprintln!("[geulos] GeulOS launcher 종료");
    Ok(())
}

/// `~/.geulos/logs/` 디렉터리 경로.
fn dirs_log_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| "USERPROFILE/HOME 환경변수 없음")?;
    Ok(PathBuf::from(home).join(".geulos").join("logs"))
}

/// 바이너리 위치 결정.
///
/// 1순위: launcher 같은 디렉터리 (release 배포 시 — `target/release/`)
/// 2순위: `target/debug/` (개발 시 — `cargo run -p geulos-launcher`)
/// 3순위: PATH (`which`)
fn locate_bin(name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().ok_or("exe parent 없음")?;
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let candidate = dir.join(format!("{}{}", name, suffix));
    if candidate.exists() {
        return Ok(candidate);
    }
    // 개발 fallback — 같은 target/debug에 다른 bin이 있을 가능성 (cargo workspace).
    let dev_candidate = dir.join(format!("{}{}", name, suffix));
    if dev_candidate.exists() {
        return Ok(dev_candidate);
    }
    Err(format!("바이너리 못 찾음: {} (검색: {})", name, candidate.display()).into())
}

/// 자식 프로세스 spawn — stdout/stderr를 로그 파일에 forward.
///
/// 자식의 stdout/stderr를 *동시에* 캡처하고 line 단위로 로그 파일에 append. forward는
/// 별도 tokio task에서 — main이 wait/kill 호출 가능.
async fn spawn_with_log(bin: &Path, log_path: &Path) -> Result<Child, Box<dyn std::error::Error>> {
    let mut child = Command::new(bin).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

    let log_path_clone = log_path.to_path_buf();
    let stdout = child.stdout.take().ok_or("stdout pipe 없음")?;
    let stderr = child.stderr.take().ok_or("stderr pipe 없음")?;

    // 별도 task에서 stdout/stderr를 로그 파일 + 콘솔 둘 다에 forward.
    tokio::spawn(async move {
        let _ = forward_streams(stdout, stderr, log_path_clone).await;
    });

    Ok(child)
}

async fn forward_streams(
    stdout: tokio::process::ChildStdout,
    stderr: tokio::process::ChildStderr,
    log_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = tokio::fs::OpenOptions::new().create(true).append(true).open(&log_path).await?;

    let stdout_reader = BufReader::new(stdout);
    let stderr_reader = BufReader::new(stderr);

    let mut stdout_lines = stdout_reader.lines();
    let mut stderr_lines = stderr_reader.lines();

    loop {
        tokio::select! {
            line = stdout_lines.next_line() => {
                match line {
                    Ok(Some(l)) => {
                        let _ = file.write_all(l.as_bytes()).await;
                        let _ = file.write_all(b"\n").await;
                    }
                    _ => break,
                }
            }
            line = stderr_lines.next_line() => {
                match line {
                    Ok(Some(l)) => {
                        let _ = file.write_all(l.as_bytes()).await;
                        let _ = file.write_all(b"\n").await;
                    }
                    _ => break,
                }
            }
        }
    }
    Ok(())
}

/// 주어진 TCP 주소가 listen 중일 때까지 대기. 200ms 간격 폴링, timeout_secs 후 false.
async fn wait_for_tcp(addr: &str, timeout_secs: u64) -> bool {
    let start = std::time::Instant::now();
    let deadline = Duration::from_secs(timeout_secs);
    while start.elapsed() < deadline {
        if TcpStream::connect(addr).await.is_ok() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

/// 로그 파일에 특정 substring이 등장할 때까지 대기. 200ms 간격, timeout_secs 후 false.
async fn wait_for_log_line(log_path: &Path, needle: &str, timeout_secs: u64) -> bool {
    let start = std::time::Instant::now();
    let deadline = Duration::from_secs(timeout_secs);
    while start.elapsed() < deadline {
        if let Ok(contents) = tokio::fs::read_to_string(log_path).await {
            if contents.contains(needle) {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}
