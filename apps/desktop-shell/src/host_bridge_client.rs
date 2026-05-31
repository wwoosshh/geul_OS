//! VM desktop-shell → 호스트 브리지(10.0.2.2:5560) 클라이언트.
//!
//! slirp 게이트웨이로 호스트 127.0.0.1:5560 도달. 블로킹 std TcpStream(루프백 소형
//! 페이로드). 브리지 미기동/오류면 None 반환 → 호출자가 VM 루트만 노출하는 폴백.
//!
//! v1.5: 연결 직후 첫 프레임 Auth{token} 송신, ok면 사용. 토큰은 /run/geulos/bridge.token
//! (geulos-init이 /proc/cmdline 파싱해 작성). 토큰 없거나 auth 실패 시 None.
//!
//! wire 포맷(4바이트 BE 길이 + JSON)은 geulos-host-bridge 크레이트의 protocol.rs와 동일.
//! Request/Response 타입은 크레이트 경계라 여기 복제 — 변경 시 양쪽 동기화.

use std::io::Write;
use std::net::TcpStream;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

const BRIDGE_ADDR: &str = "10.0.2.2:5560";

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Auth { token: String },
    ListDrives,
    ListDir { path: String },
    ReadFile { path: String, max_bytes: u64 },
    WriteFile { path: String, content_base64: String },
    CreateDir { path: String },
    Remove { path: String, recursive: bool },
    Rename { from: String, to: String },
    Exec { cmd: String, args: Vec<String>, cwd: String, timeout_ms: u64 },
    ExecStreamStart { cmd: String, args: Vec<String>, cwd: String },
    ExecStreamPoll { stream_id: String },
    ExecStreamKill { stream_id: String },
}

#[derive(Deserialize, Clone)]
pub struct ProcessLine {
    pub kind: String,
    pub text: String,
}

#[derive(Deserialize, Clone)]
pub struct ProcessStatus {
    pub status: String,
    pub exit_code: Option<i32>,
}

#[derive(Deserialize)]
pub struct EntryInfo {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Response {
    Auth { ok: bool },
    Drives { drives: Vec<String> },
    Entries { entries: Vec<EntryInfo> },
    File { content_base64: String, truncated: bool },
    Ok,
    #[allow(dead_code)]
    Error { error: String },
    ExecResult { exit_code: i32, stdout: String, stderr: String, duration_ms: u64 },
    ExecStreamStarted { stream_id: String, pid: u32 },
    ExecStreamChunk { lines: Vec<ProcessLine>, status: ProcessStatus },
}

/// 경로가 호스트 경로(드라이브 문자로 시작)인지. `C:\`, `d:/` 등.
pub fn is_host_path(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

fn load_token() -> Option<String> {
    std::fs::read_to_string("/run/geulos/bridge.token")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn conn() -> &'static Mutex<Option<TcpStream>> {
    static C: OnceLock<Mutex<Option<TcpStream>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

fn write_one_frame(stream: &mut TcpStream, body: &[u8]) -> std::io::Result<()> {
    let mut framed = Vec::with_capacity(4 + body.len());
    framed.extend_from_slice(&(body.len() as u32).to_be_bytes());
    framed.extend_from_slice(body);
    stream.write_all(&framed)?;
    stream.flush()
}

fn read_one_frame(stream: &mut TcpStream) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).ok()?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 16 * 1024 * 1024 {
        return None;
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).ok()?;
    Some(body)
}

/// 요청 1건 송신 후 응답 1건 수신. 연결 없으면 1회 연결+Auth 시도. 실패 시 None.
fn rpc(req: &Request) -> Option<Response> {
    let mut guard = conn().lock().ok()?;
    if guard.is_none() {
        let mut s = TcpStream::connect(BRIDGE_ADDR).ok()?;
        // 첫 프레임: Auth.
        let token = load_token().unwrap_or_default();
        let auth_body = serde_json::to_vec(&Request::Auth { token }).ok()?;
        if write_one_frame(&mut s, &auth_body).is_err() {
            return None;
        }
        let resp_body = read_one_frame(&mut s)?;
        match serde_json::from_slice::<Response>(&resp_body).ok()? {
            Response::Auth { ok: true } => {
                *guard = Some(s);
            }
            _ => {
                eprintln!("[host_bridge_client] auth 실패 — 토큰 mismatch 또는 미설정");
                return None;
            }
        }
    }
    let stream = guard.as_mut()?;
    let body = serde_json::to_vec(req).ok()?;
    if write_one_frame(stream, &body).is_err() {
        *guard = None;
        return None;
    }
    match read_one_frame(stream) {
        Some(resp_body) => serde_json::from_slice::<Response>(&resp_body).ok(),
        None => {
            *guard = None;
            None
        }
    }
}

/// 호스트 드라이브 목록(`C:\`, `D:\` …). 브리지 없으면 None.
pub fn list_drives() -> Option<Vec<String>> {
    match rpc(&Request::ListDrives)? {
        Response::Drives { drives } => Some(drives),
        _ => None,
    }
}

/// 호스트 디렉터리 자식 목록. 브리지 없거나 오류면 None.
pub fn list_dir(path: &str) -> Option<Vec<EntryInfo>> {
    match rpc(&Request::ListDir { path: path.to_string() })? {
        Response::Entries { entries } => Some(entries),
        _ => None,
    }
}

/// 호스트 파일 내용(최대 max_bytes). (bytes, truncated). 오류면 None.
pub fn read_file(path: &str, max_bytes: u64) -> Option<(Vec<u8>, bool)> {
    match rpc(&Request::ReadFile { path: path.to_string(), max_bytes })? {
        Response::File { content_base64, truncated } => {
            use base64::{engine::general_purpose::STANDARD, Engine};
            let bytes = STANDARD.decode(content_base64).ok()?;
            Some((bytes, truncated))
        }
        _ => None,
    }
}

/// 호스트 파일 쓰기 (overwrite). 오류면 Err(메시지).
pub fn write_file(path: &str, bytes: &[u8]) -> Result<(), String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let req = Request::WriteFile {
        path: path.to_string(),
        content_base64: STANDARD.encode(bytes),
    };
    match rpc(&req) {
        Some(Response::Ok) => Ok(()),
        Some(Response::Error { error }) => Err(error),
        _ => Err("브리지 없음/응답 불일치".into()),
    }
}

pub fn create_dir(path: &str) -> Result<(), String> {
    match rpc(&Request::CreateDir { path: path.to_string() }) {
        Some(Response::Ok) => Ok(()),
        Some(Response::Error { error }) => Err(error),
        _ => Err("브리지 없음/응답 불일치".into()),
    }
}

pub fn remove(path: &str, recursive: bool) -> Result<(), String> {
    match rpc(&Request::Remove { path: path.to_string(), recursive }) {
        Some(Response::Ok) => Ok(()),
        Some(Response::Error { error }) => Err(error),
        _ => Err("브리지 없음/응답 불일치".into()),
    }
}

pub fn rename(from: &str, to: &str) -> Result<(), String> {
    match rpc(&Request::Rename { from: from.to_string(), to: to.to_string() }) {
        Some(Response::Ok) => Ok(()),
        Some(Response::Error { error }) => Err(error),
        _ => Err("브리지 없음/응답 불일치".into()),
    }
}

/// 호스트 측 one-shot 명령 실행. ShellRunner@1.run 위임.
/// 반환: (exit_code, stdout, stderr, duration_ms). 오류면 Err.
pub fn exec(
    cmd: &str,
    args: &[String],
    cwd: &str,
    timeout_ms: u64,
) -> Result<(i32, String, String, u64), String> {
    let req = Request::Exec {
        cmd: cmd.to_string(),
        args: args.to_vec(),
        cwd: cwd.to_string(),
        timeout_ms,
    };
    match rpc(&req) {
        Some(Response::ExecResult { exit_code, stdout, stderr, duration_ms }) => {
            Ok((exit_code, stdout, stderr, duration_ms))
        }
        Some(Response::Error { error }) => Err(error),
        _ => Err("브리지 없음/응답 불일치".into()),
    }
}

/// streaming 시작 — stream_id + pid 반환. ShellRunner@1.run_streamed 위임.
pub fn exec_stream_start(
    cmd: &str,
    args: &[String],
    cwd: &str,
) -> Result<(String, u32), String> {
    let req = Request::ExecStreamStart {
        cmd: cmd.to_string(),
        args: args.to_vec(),
        cwd: cwd.to_string(),
    };
    match rpc(&req) {
        Some(Response::ExecStreamStarted { stream_id, pid }) => Ok((stream_id, pid)),
        Some(Response::Error { error }) => Err(error),
        _ => Err("브리지 없음/응답 불일치".into()),
    }
}

/// 마지막 poll 이후 누적된 line + status 조회.
pub fn exec_stream_poll(
    stream_id: &str,
) -> Result<(Vec<ProcessLine>, ProcessStatus), String> {
    let req = Request::ExecStreamPoll { stream_id: stream_id.to_string() };
    match rpc(&req) {
        Some(Response::ExecStreamChunk { lines, status }) => Ok((lines, status)),
        Some(Response::Error { error }) => Err(error),
        _ => Err("브리지 없음/응답 불일치".into()),
    }
}

pub fn exec_stream_kill(stream_id: &str) -> Result<(), String> {
    let req = Request::ExecStreamKill { stream_id: stream_id.to_string() };
    match rpc(&req) {
        Some(Response::Ok) => Ok(()),
        Some(Response::Error { error }) => Err(error),
        _ => Err("브리지 없음/응답 불일치".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_host_path_detects_drive_letters() {
        assert!(is_host_path("C:\\Users"));
        assert!(is_host_path("d:/work"));
        assert!(!is_host_path("/usr/bin"));
        assert!(!is_host_path("relative"));
        assert!(!is_host_path("/"));
    }
}
