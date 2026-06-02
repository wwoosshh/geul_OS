//! 호스트 측 명령 실행 — ShellRunner@1 위임 (one-shot + streaming).
//!
//! 화이트리스트 binary만 통과 (desktop-shell 측에서도 검사하지만 defense in depth).
//! one-shot은 동기 wait, streaming은 spawn 후 process registry로 polling 인터페이스 제공.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::protocol::{ProcessLine, ProcessStatus};

/// 호스트 측 화이트리스트 — desktop-shell ShellRunner.props.allowed_binaries와 일치 유지.
const ALLOWED_BINARIES: &[&str] = &[
    "git", "npm", "yarn", "pnpm", "npx", "cargo", "rustc", "docker", "node", "python", "pip",
];

/// 단일 streaming 프로세스 상태. lines는 ring (max LINE_RING) — old line drop.
struct StreamEntry {
    child: Child,
    /// 누적 line — Poll 호출 시 drain 후 caller로 보냄. 다음 polling까지 비어 있음.
    pending: Arc<Mutex<Vec<ProcessLine>>>,
    /// 최종 exit_code (Wait 후). None이면 아직 running.
    finished: Arc<Mutex<Option<i32>>>,
}

/// stream_id → StreamEntry. process registry. UUID로 id 발급.
static REGISTRY: once_cell::sync::Lazy<Mutex<HashMap<String, StreamEntry>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));

fn allowed(cmd: &str) -> bool {
    ALLOWED_BINARIES.contains(&cmd)
}

fn validate_cwd(cwd: &str) -> Result<std::path::PathBuf, String> {
    // 빈 cwd면 호스트의 USERPROFILE을 default — cargo --version 같은 cwd-무관 명령에서
    // AI가 cwd 생략한 경우 친절하게 처리. 사용자가 명시한 cwd만 검증.
    if cwd.is_empty() {
        if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            let p = std::path::PathBuf::from(home);
            if p.exists() {
                return Ok(p);
            }
        }
        return Err("cwd 빈 문자열 + USERPROFILE/HOME 없음".into());
    }
    let p = std::path::PathBuf::from(cwd);
    if !p.is_absolute() {
        return Err(format!("cwd는 절대경로여야: {}", cwd));
    }
    if !p.exists() {
        return Err(format!("cwd 존재 안 함: {}", cwd));
    }
    Ok(p)
}

/// Windows .cmd/.bat shim fallback — npx/npm/yarn/pnpm은 Node.js가 .cmd로 설치.
/// `Command::new("npx")` 는 단순 PATH 검색이라 못 찾음 → ".cmd" 붙여 재시도.
fn spawn_with_fallback(
    cmd: &str,
    args: &[String],
    cwd_path: &std::path::Path,
) -> std::io::Result<std::process::Child> {
    let try_spawn = |c: &str| -> std::io::Result<std::process::Child> {
        Command::new(c)
            .args(args)
            .current_dir(cwd_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    };
    match try_spawn(cmd) {
        Ok(c) => Ok(c),
        Err(e) if cfg!(windows) && e.kind() == std::io::ErrorKind::NotFound => {
            for ext in &[".cmd", ".bat", ".exe"] {
                let with_ext = format!("{}{}", cmd, ext);
                if let Ok(c) = try_spawn(&with_ext) {
                    return Ok(c);
                }
            }
            Err(e)
        }
        Err(e) => Err(e),
    }
}

/// one-shot 실행 — 동기 wait_with_output. timeout 초과 시 kill + error.
pub fn exec(
    cmd: &str,
    args: &[String],
    cwd: &str,
    timeout_ms: u64,
) -> Result<(i32, String, String, u64), String> {
    if !allowed(cmd) {
        return Err(format!("화이트리스트 외: {}. 허용: {:?}", cmd, ALLOWED_BINARIES));
    }
    let cwd_path = validate_cwd(cwd)?;
    let start = Instant::now();
    let mut child = spawn_with_fallback(cmd, args, &cwd_path)
        .map_err(|e| format!("spawn 실패 ({}): {}", cmd, e))?;

    let deadline = start + Duration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut s) = child.stdout.take() {
                    use std::io::Read;
                    let _ = s.read_to_string(&mut stdout);
                }
                if let Some(mut s) = child.stderr.take() {
                    use std::io::Read;
                    let _ = s.read_to_string(&mut stderr);
                }
                let code = status.code().unwrap_or(-1);
                return Ok((code, stdout, stderr, start.elapsed().as_millis() as u64));
            }
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("timeout 초과: {}ms", timeout_ms));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("wait 실패: {}", e)),
        }
    }
}

/// streaming 시작 — spawn child + reader thread가 stdout/stderr line을 pending에 push.
/// 반환: (stream_id, pid).
pub fn exec_stream_start(
    cmd: &str,
    args: &[String],
    cwd: &str,
) -> Result<(String, u32), String> {
    if !allowed(cmd) {
        return Err(format!("화이트리스트 외: {}. 허용: {:?}", cmd, ALLOWED_BINARIES));
    }
    let cwd_path = validate_cwd(cwd)?;
    let mut child = spawn_with_fallback(cmd, args, &cwd_path)
        .map_err(|e| format!("spawn 실패 ({}): {}", cmd, e))?;
    let pid = child.id();
    let stream_id = format!("exec-{}", uuid::Uuid::new_v4());
    let pending: Arc<Mutex<Vec<ProcessLine>>> = Arc::new(Mutex::new(Vec::new()));
    let finished: Arc<Mutex<Option<i32>>> = Arc::new(Mutex::new(None));

    // stdout reader thread.
    if let Some(stdout) = child.stdout.take() {
        let p = Arc::clone(&pending);
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                if let Ok(mut g) = p.lock() {
                    g.push(ProcessLine { kind: "stdout".into(), text: line });
                }
            }
        });
    }
    // stderr reader thread.
    if let Some(stderr) = child.stderr.take() {
        let p = Arc::clone(&pending);
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                if let Ok(mut g) = p.lock() {
                    g.push(ProcessLine { kind: "stderr".into(), text: line });
                }
            }
        });
    }

    REGISTRY
        .lock()
        .map_err(|e| format!("registry lock: {}", e))?
        .insert(stream_id.clone(), StreamEntry { child, pending, finished });
    Ok((stream_id, pid))
}

/// 마지막 poll 이후 누적된 line을 drain + 현재 상태 반환.
/// 프로세스가 종료됐으면 finished에 exit_code 채워서 status = "exited" 반환.
pub fn exec_stream_poll(stream_id: &str) -> Result<(Vec<ProcessLine>, ProcessStatus), String> {
    let mut reg = REGISTRY.lock().map_err(|e| format!("registry lock: {}", e))?;
    let entry = reg.get_mut(stream_id).ok_or_else(|| format!("stream_id 없음: {}", stream_id))?;
    // try_wait — 종료됐으면 exit_code 캐시.
    if entry.finished.lock().map(|g| g.is_none()).unwrap_or(true) {
        match entry.child.try_wait() {
            Ok(Some(status)) => {
                if let Ok(mut g) = entry.finished.lock() {
                    *g = Some(status.code().unwrap_or(-1));
                }
            }
            Ok(None) => {}
            Err(e) => return Err(format!("try_wait: {}", e)),
        }
    }
    let lines = std::mem::take(
        &mut *entry.pending.lock().map_err(|e| format!("pending lock: {}", e))?,
    );
    let exit_code = entry.finished.lock().ok().and_then(|g| *g);
    let status = ProcessStatus {
        status: if exit_code.is_some() { "exited".into() } else { "running".into() },
        exit_code,
    };
    Ok((lines, status))
}

/// 프로세스 kill. exit_code 셋 + 다음 poll에서 status="terminated".
///
/// **Windows process tree kill**: `child.kill()`은 *직계 child*만 죽인다. `npm.cmd` 같은
/// shim이 손주(node.exe, vite 등)를 spawn한 경우 손주는 orphan으로 살아남아 VM 종료해도
/// 계속 동작. `taskkill /F /T /PID <pid>`는 process tree 전체를 cascade kill한다.
pub fn exec_stream_kill(stream_id: &str) -> Result<(), String> {
    let mut reg = REGISTRY.lock().map_err(|e| format!("registry lock: {}", e))?;
    let entry = reg.remove(stream_id).ok_or_else(|| format!("stream_id 없음: {}", stream_id))?;
    let pid = entry.child.id();
    drop(reg); // taskkill 호출 동안 lock 풀기

    #[cfg(windows)]
    {
        taskkill_pid(pid);
    }
    #[cfg(not(windows))]
    {
        let mut child = entry.child;
        let _ = child.kill();
        let _ = child.wait();
    }
    // finished 캐시.
    if let Ok(mut g) = entry.finished.lock() {
        if g.is_none() {
            *g = Some(-1);
        }
    }
    Ok(())
}

/// 단일 pid의 process tree를 cascade kill. Windows `taskkill /F /T`.
#[cfg(windows)]
fn taskkill_pid(pid: u32) {
    match Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(out) if !out.status.success() => {
            let err = String::from_utf8_lossy(&out.stderr).into_owned();
            eprintln!("[exec] taskkill /T /PID {} 부분 실패: {}", pid, err);
        }
        Ok(_) => {}
        Err(e) => eprintln!("[exec] taskkill spawn 실패 (pid {}): {}", pid, e),
    }
}

/// REGISTRY의 *모든* 스트림 프로세스를 cascade kill. 호스트 브리지 종료 훅에서 호출
/// (KI-029) — VM 종료 시 npm/node 손주가 orphan으로 잔존하던 문제 해소.
pub fn exec_stream_kill_all() {
    let entries: Vec<(String, u32)> = match REGISTRY.lock() {
        Ok(mut reg) => reg.drain().map(|(id, e)| (id, e.child.id())).collect(),
        Err(e) => {
            eprintln!("[exec] kill_all registry lock 실패: {}", e);
            return;
        }
    };
    for (id, pid) in entries {
        eprintln!("[exec] 종료 cleanup: stream {} pid {} kill", id, pid);
        #[cfg(windows)]
        taskkill_pid(pid);
        #[cfg(not(windows))]
        {
            let _ = pid;
        }
    }
}

#[cfg(test)]
mod kill_all_tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn kill_all_drains_registry() {
        // 장수 프로세스 spawn — node가 ~100초 동안 살아 있도록 setTimeout.
        // exec_stream_start 시그니처: (cmd: &str, args: &[String], cwd: &str).
        let args = vec!["-e".to_string(), "setTimeout(function(){}, 100000)".to_string()];
        let (sid, _pid) = exec_stream_start("node", &args, "").expect("spawn");
        assert!(REGISTRY.lock().unwrap().contains_key(&sid));
        exec_stream_kill_all();
        assert!(REGISTRY.lock().unwrap().is_empty(), "kill_all 후 REGISTRY 비어야 함");
    }
}
