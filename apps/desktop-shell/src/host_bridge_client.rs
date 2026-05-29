//! VM desktop-shell → 호스트 브리지(10.0.2.2:5560) 클라이언트.
//!
//! slirp 게이트웨이로 호스트 127.0.0.1:5560 도달. 블로킹 std TcpStream(루프백 소형
//! 페이로드). 브리지 미기동/오류면 None 반환 → 호출자가 VM 루트만 노출하는 폴백.
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
    ListDrives,
    ListDir { path: String },
    ReadFile { path: String, max_bytes: u64 },
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
    Drives { drives: Vec<String> },
    Entries { entries: Vec<EntryInfo> },
    File { content_base64: String, truncated: bool },
    Error { error: String },
}

/// 경로가 호스트 경로(드라이브 문자로 시작)인지. `C:\`, `d:/` 등.
pub fn is_host_path(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

fn conn() -> &'static Mutex<Option<TcpStream>> {
    static C: OnceLock<Mutex<Option<TcpStream>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

/// 요청 1건 송신 후 응답 1건 수신. 연결 없으면 1회 연결 시도. 실패 시 None.
fn rpc(req: &Request) -> Option<Response> {
    let mut guard = conn().lock().ok()?;
    if guard.is_none() {
        *guard = TcpStream::connect(BRIDGE_ADDR).ok();
    }
    let stream = guard.as_mut()?;
    let body = serde_json::to_vec(req).ok()?;
    let mut framed = Vec::with_capacity(4 + body.len());
    framed.extend_from_slice(&(body.len() as u32).to_be_bytes());
    framed.extend_from_slice(&body);
    if stream.write_all(&framed).and_then(|_| stream.flush()).is_err() {
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
