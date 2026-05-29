# GeulOS 호스트 브리지 v1 구현 계획 (Model B — 호스트 C:/D: 읽기 탐색)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Windows 호스트에서 도는 `geulos-host-bridge` 프로세스가 호스트 파일시스템을 읽기 전용 RPC로 제공하고, VM 안의 desktop-shell이 이를 통해 파일관리자에서 호스트 전체 드라이브(C:, D: …)를 탐색·열람한다.

**Architecture:** 호스트 바이너리가 `127.0.0.1:5560`에서 length-prefixed JSON RPC를 listen. VM의 desktop-shell이 slirp 게이트웨이 `10.0.2.2:5560`으로 다이얼해 `list_drives`/`list_dir`/`stat`/`read_file`를 호출하고, 결과를 기존 `std_types::folder`/`file` 객체로 합성해 트리에 mount(B안). 경로가 드라이브 문자(`C:\`)면 브리지, `/`면 기존 `std::fs`로 분기. 브리지 미기동 시 VM 루트만 보이는 graceful fallback.

**Tech Stack:** Rust, `geulos-proto`(프레임 codec 재사용), `serde`/`serde_json`, `base64`, std `TcpStream`(블로킹 — 루프백 소형 페이로드), `winapi`(GetLogicalDrives).

**Spec:** `docs/specs/2026-05-29-geulos-host-bridge.md`

---

## File Structure

**신규 (호스트 바이너리 크레이트):**
- `crates/geulos-host-bridge/Cargo.toml` — 크레이트 정의(proto/serde/serde_json/base64, win은 winapi).
- `crates/geulos-host-bridge/src/protocol.rs` — `Request`/`Response`/`EntryInfo`/`StatInfo` 직렬화 타입 + 프레임 read/write 헬퍼.
- `crates/geulos-host-bridge/src/fs_ops.rs` — `list_drives`/`list_dir`/`stat`/`read_file` + 경로 검증(순수, 테스트 대상).
- `crates/geulos-host-bridge/src/main.rs` — TCP listener + 연결당 요청 루프 + dispatch.

**신규 (VM desktop-shell 클라이언트):**
- `apps/desktop-shell/src/host_bridge_client.rs` — `10.0.2.2:5560` 다이얼, `list_drives`/`list_dir`/`read_file`, `is_host_path`, 전역 lazy 연결.

**수정:**
- `Cargo.toml`(workspace) — members에 `crates/geulos-host-bridge` 추가.
- `apps/desktop-shell/src/lib.rs`(또는 `main.rs`의 `mod` 선언부) — `mod host_bridge_client;`.
- `apps/desktop-shell/src/drives.rs` — Linux 분기: 브리지 드라이브 + VM 루트.
- `apps/desktop-shell/src/lazy_mount.rs` — `expand_folder`: 호스트 경로면 브리지 RPC로 객체 합성.
- `boot/build.ps1` — 호스트 네이티브로 `geulos-host-bridge` 빌드 단계 추가.
- `boot/qemu/launch.ps1` — QEMU와 함께 브리지 기동, 종료 시 정리.

---

## Task 1: 워크스페이스에 크레이트 추가 + 프로토콜 타입 (TDD)

**Files:**
- Create: `crates/geulos-host-bridge/Cargo.toml`
- Create: `crates/geulos-host-bridge/src/protocol.rs`
- Create: `crates/geulos-host-bridge/src/main.rs` (임시 stub)
- Modify: `Cargo.toml` (workspace members)

- [ ] **Step 1: workspace members에 추가**

`Cargo.toml`의 `members` 배열에 한 줄 추가:
```toml
    "crates/geulos-host-bridge",
```

- [ ] **Step 2: 크레이트 Cargo.toml 작성**

`crates/geulos-host-bridge/Cargo.toml`:
```toml
[package]
name = "geulos-host-bridge"
version = "0.0.1"
edition.workspace = true

[[bin]]
name = "geulos-host-bridge"
path = "src/main.rs"

[dependencies]
geulos-proto = { path = "../../proto" }
serde = { workspace = true }
serde_json = "1.0"
base64 = "0.22"

[target.'cfg(windows)'.dependencies]
winapi = { version = "0.3", features = ["fileapi"] }
```
(주: `geulos-proto`의 정확한 package 이름은 `proto/Cargo.toml`의 `[package].name` 확인 후 일치시킬 것. compositor가 `geulos_proto`로 import하므로 package name은 `geulos-proto`.)

- [ ] **Step 3: 프로토콜 타입 실패 테스트 작성**

`crates/geulos-host-bridge/src/protocol.rs`:
```rust
//! 호스트 브리지 RPC 프로토콜 — length-prefixed(geulos-proto) JSON 1건.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    ListDrives,
    ListDir { path: String },
    Stat { path: String },
    ReadFile { path: String, max_bytes: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntryInfo {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatInfo {
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    Drives { drives: Vec<String> },
    Entries { entries: Vec<EntryInfo> },
    Stat { stat: StatInfo },
    File { content_base64: String, truncated: bool },
    Error { error: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_list_dir_roundtrip() {
        let r = Request::ListDir { path: "C:\\Users".into() };
        let bytes = serde_json::to_vec(&r).unwrap();
        let back: Request = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn response_error_roundtrip() {
        let r = Response::Error { error: "권한 거부".into() };
        let bytes = serde_json::to_vec(&r).unwrap();
        let back: Response = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(r, back);
    }
}
```

- [ ] **Step 4: main.rs 임시 stub** (컴파일 통과용)

`crates/geulos-host-bridge/src/main.rs`:
```rust
mod protocol;
mod fs_ops;

fn main() {
    eprintln!("geulos-host-bridge (stub)");
}
```
(주: `fs_ops`는 Task 2에서 작성 — 이 stub 단계에선 `mod fs_ops;`를 빼고 Task 2에서 추가하거나, 빈 `fs_ops.rs`를 먼저 만든다.)

- [ ] **Step 5: 테스트 실행**

Run: `cargo test -p geulos-host-bridge`
Expected: protocol 2개 테스트 PASS.

- [ ] **Step 6: 커밋**
```bash
git add Cargo.toml crates/geulos-host-bridge
git commit -m "feat(host-bridge): 크레이트 골격 + RPC 프로토콜 타입"
```

---

## Task 2: fs_ops — 파일시스템 연산 (TDD)

**Files:**
- Create: `crates/geulos-host-bridge/src/fs_ops.rs`

- [ ] **Step 1: 실패 테스트 작성** (temp dir 기반)

`crates/geulos-host-bridge/src/fs_ops.rs`:
```rust
//! 호스트 파일시스템 읽기 연산 — 읽기 전용, 절대경로만.

use std::path::Path;
use crate::protocol::{EntryInfo, StatInfo};

/// 경로가 절대경로이고 `..` 컴포넌트가 없는지 검증.
pub fn is_safe_absolute(path: &str) -> bool {
    let p = Path::new(path);
    p.is_absolute() && !p.components().any(|c| matches!(c, std::path::Component::ParentDir))
}

/// 시스템 드라이브 목록. Windows=GetLogicalDrives, 그 외=["/"].
pub fn list_drives() -> Vec<String> {
    #[cfg(windows)]
    {
        use winapi::um::fileapi::GetLogicalDrives;
        let mask = unsafe { GetLogicalDrives() };
        if mask == 0 {
            return vec!["C:\\".to_string()];
        }
        let mut out = Vec::new();
        for i in 0..26 {
            if mask & (1 << i) != 0 {
                let letter = (b'A' + i as u8) as char;
                out.push(format!("{}:\\", letter));
            }
        }
        out
    }
    #[cfg(not(windows))]
    {
        vec!["/".to_string()]
    }
}

/// 디렉터리 직계 자식. 권한 거부/오류는 Err(메시지).
pub fn list_dir(path: &str) -> Result<Vec<EntryInfo>, String> {
    if !is_safe_absolute(path) {
        return Err(format!("절대경로 아님 또는 '..' 포함: {}", path));
    }
    let rd = std::fs::read_dir(path).map_err(|e| format!("read_dir 실패: {}", e))?;
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(s) => s,
            Err(_) => continue,
        };
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            out.push(EntryInfo { name, is_dir: true, size: 0 });
        } else if meta.is_file() {
            out.push(EntryInfo { name, is_dir: false, size: meta.len() });
        }
    }
    Ok(out)
}

/// 단일 경로 stat.
pub fn stat(path: &str) -> Result<StatInfo, String> {
    if !is_safe_absolute(path) {
        return Err(format!("절대경로 아님: {}", path));
    }
    let meta = std::fs::metadata(path).map_err(|e| format!("metadata 실패: {}", e))?;
    Ok(StatInfo { is_dir: meta.is_dir(), size: meta.len() })
}

/// 파일 내용 읽기(최대 max_bytes). (bytes, truncated) 반환.
pub fn read_file(path: &str, max_bytes: u64) -> Result<(Vec<u8>, bool), String> {
    if !is_safe_absolute(path) {
        return Err(format!("절대경로 아님: {}", path));
    }
    let data = std::fs::read(path).map_err(|e| format!("read 실패: {}", e))?;
    if data.len() as u64 > max_bytes {
        Ok((data[..max_bytes as usize].to_vec(), true))
    } else {
        Ok((data, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp() -> std::path::PathBuf {
        let mut d = std::env::temp_dir();
        // 테스트 충돌 방지: 고정 하위 폴더(테스트마다 정리).
        d.push("geulos_bridge_test");
        let _ = std::fs::create_dir_all(&d);
        d
    }

    #[test]
    fn safe_absolute_rejects_relative_and_dotdot() {
        assert!(!is_safe_absolute("relative/path"));
        assert!(!is_safe_absolute("/a/../b"));
        #[cfg(windows)]
        assert!(is_safe_absolute("C:\\Users"));
        #[cfg(not(windows))]
        assert!(is_safe_absolute("/usr"));
    }

    #[test]
    fn list_dir_returns_entries() {
        let d = tmp();
        let f = d.join("a.txt");
        let mut fh = std::fs::File::create(&f).unwrap();
        fh.write_all(b"hello").unwrap();
        std::fs::create_dir_all(d.join("sub")).unwrap();
        let entries = list_dir(d.to_str().unwrap()).unwrap();
        assert!(entries.iter().any(|e| e.name == "a.txt" && !e.is_dir && e.size == 5));
        assert!(entries.iter().any(|e| e.name == "sub" && e.is_dir));
    }

    #[test]
    fn list_dir_missing_path_errors() {
        let r = list_dir(tmp().join("does_not_exist_xyz").to_str().unwrap());
        assert!(r.is_err());
    }

    #[test]
    fn read_file_truncates_at_max() {
        let d = tmp();
        let f = d.join("big.txt");
        std::fs::write(&f, b"0123456789").unwrap();
        let (data, truncated) = read_file(f.to_str().unwrap(), 4).unwrap();
        assert_eq!(data, b"0123");
        assert!(truncated);
    }
}
```

- [ ] **Step 2: 테스트 실행 (실패→통과 확인)**

Run: `cargo test -p geulos-host-bridge fs_ops`
Expected: 4개 PASS (구현이 같은 파일에 있으므로 바로 통과).

- [ ] **Step 3: 커밋**
```bash
git add crates/geulos-host-bridge/src/fs_ops.rs crates/geulos-host-bridge/src/main.rs
git commit -m "feat(host-bridge): fs_ops 읽기 연산 + 경로 검증 (TDD)"
```

---

## Task 3: 브리지 서버 main (TCP listener + dispatch)

**Files:**
- Modify: `crates/geulos-host-bridge/src/main.rs`
- Modify: `crates/geulos-host-bridge/src/protocol.rs` (프레임 read/write 헬퍼 추가)

- [ ] **Step 1: protocol.rs에 프레임 헬퍼 추가**

`protocol.rs` 끝(테스트 mod 위)에 추가:
```rust
use std::io::{self, Read, Write};
use geulos_proto::{encode_frame, decode_frame, DecodeError};

/// 스트림에서 프레임 1건 읽어 본문 바이트 반환. EOF면 None.
pub fn read_frame<R: Read>(r: &mut R, buf: &mut Vec<u8>) -> io::Result<Option<Vec<u8>>> {
    loop {
        let mut slice: &[u8] = buf;
        match decode_frame(&mut slice) {
            Ok(body) => {
                let consumed = buf.len() - slice.len();
                buf.drain(..consumed);
                return Ok(Some(body));
            }
            Err(DecodeError::Incomplete) => {
                let mut chunk = [0u8; 8192];
                let n = r.read(&mut chunk)?;
                if n == 0 {
                    return Ok(None); // EOF
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{:?}", e))),
        }
    }
}

/// 본문을 프레임으로 인코딩해 스트림에 write.
pub fn write_frame<W: Write>(w: &mut W, body: &[u8]) -> io::Result<()> {
    w.write_all(&encode_frame(body))
}
```
(주: `geulos-proto`의 `DecodeError` 변형 이름은 `proto/src/codec.rs` 확인 — `Incomplete`/`TooLarge` 존재 확인됨.)

- [ ] **Step 2: main.rs 서버 루프 작성**
```rust
mod protocol;
mod fs_ops;

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use protocol::{read_frame, write_frame, Request, Response};

const ADDR: &str = "127.0.0.1:5560";
const READ_FILE_HARD_CAP: u64 = 8 * 1024 * 1024; // 8MB 안전 상한

fn handle_request(req: Request) -> Response {
    match req {
        Request::ListDrives => Response::Drives { drives: fs_ops::list_drives() },
        Request::ListDir { path } => match fs_ops::list_dir(&path) {
            Ok(entries) => Response::Entries { entries },
            Err(e) => Response::Error { error: e },
        },
        Request::Stat { path } => match fs_ops::stat(&path) {
            Ok(stat) => Response::Stat { stat },
            Err(e) => Response::Error { error: e },
        },
        Request::ReadFile { path, max_bytes } => {
            let cap = max_bytes.min(READ_FILE_HARD_CAP);
            match fs_ops::read_file(&path, cap) {
                Ok((bytes, truncated)) => {
                    use base64::{engine::general_purpose::STANDARD, Engine};
                    Response::File { content_base64: STANDARD.encode(bytes), truncated }
                }
                Err(e) => Response::Error { error: e },
            }
        }
    }
}

fn serve_conn(mut stream: TcpStream) {
    let mut buf = Vec::new();
    loop {
        let body = match read_frame(&mut stream, &mut buf) {
            Ok(Some(b)) => b,
            Ok(None) => break, // EOF
            Err(e) => {
                eprintln!("[host-bridge] read 오류: {}", e);
                break;
            }
        };
        let resp = match serde_json::from_slice::<Request>(&body) {
            Ok(req) => handle_request(req),
            Err(e) => Response::Error { error: format!("요청 파싱 실패: {}", e) },
        };
        let out = serde_json::to_vec(&resp).unwrap_or_default();
        if let Err(e) = write_frame(&mut stream, &out).and_then(|_| stream.flush()) {
            eprintln!("[host-bridge] write 오류: {}", e);
            break;
        }
    }
}

fn main() {
    let listener = match TcpListener::bind(ADDR) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[host-bridge] bind {} 실패: {}", ADDR, e);
            std::process::exit(1);
        }
    };
    eprintln!("[host-bridge] listening on {}", ADDR);
    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                // 연결당 thread — VM의 desktop-shell 1개가 주 클라이언트.
                std::thread::spawn(move || serve_conn(stream));
            }
            Err(e) => eprintln!("[host-bridge] accept 오류: {}", e),
        }
    }
}
```

- [ ] **Step 3: 빌드 + 수동 라운드트립 확인 (호스트 네이티브)**

Run: `cargo build -p geulos-host-bridge`
Expected: 컴파일 성공.

수동 확인(선택): 별도 셸에서 `cargo run -p geulos-host-bridge` 실행 후, 작은 클라이언트 테스트(Task 4의 client로 대체) 또는 PowerShell TcpClient로 `{"op":"list_drives"}` 프레임 전송 → `{"drives":[...]}` 수신.

- [ ] **Step 4: 커밋**
```bash
git add crates/geulos-host-bridge/src/main.rs crates/geulos-host-bridge/src/protocol.rs
git commit -m "feat(host-bridge): TCP 서버 루프 + 프레임 read/write + dispatch"
```

---

## Task 4: desktop-shell host_bridge_client (TDD with mock)

**Files:**
- Create: `apps/desktop-shell/src/host_bridge_client.rs`
- Modify: `apps/desktop-shell/src/lib.rs` (또는 main.rs의 모듈 선언) — `mod host_bridge_client;`

**설계 메모:** 블로킹 `std::net::TcpStream`(루프백 소형 페이로드 — 짧은 블록 허용, v2에서 async 검토). 전역 lazy 연결을 `OnceLock<Mutex<Option<Connection>>>`로 유지. 브리지 미응답이면 모든 함수가 `None` 반환 → 호출자 graceful fallback.

- [ ] **Step 1: is_host_path 실패 테스트 작성**

`apps/desktop-shell/src/host_bridge_client.rs`:
```rust
//! VM desktop-shell → 호스트 브리지(10.0.2.2:5560) 클라이언트.
//!
//! slirp 게이트웨이로 호스트 127.0.0.1:5560 도달. 블로킹 std TcpStream(루프백 소형
//! 페이로드). 브리지 미기동/오류면 None 반환 → 호출자가 VM 루트만 노출하는 폴백.

use std::io::Write;
use std::net::TcpStream;
use std::sync::{Mutex, OnceLock};

// 브리지 프로토콜 타입을 desktop-shell 안에 재선언(크레이트 경계 — 의존 추가 대신 복제).
// host-bridge crate의 protocol.rs와 *동일 wire*. 변경 시 양쪽 동기화.
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
    // Stat은 v1 client에서 미사용 — 무시 허용 위해 #[serde(other)] 대체:
    #[serde(other)]
    Unknown,
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
    // 프레임 write (4바이트 BE 길이 + body) — geulos-proto encode_frame과 동일 포맷.
    let mut framed = Vec::with_capacity(4 + body.len());
    framed.extend_from_slice(&(body.len() as u32).to_be_bytes());
    framed.extend_from_slice(&body);
    if stream.write_all(&framed).and_then(|_| stream.flush()).is_err() {
        *guard = None; // 끊김 → 다음 호출에 재연결
        return None;
    }
    let resp_body = read_one_frame(stream).or_else(|| {
        *guard = None;
        None
    })?;
    serde_json::from_slice::<Response>(&resp_body).ok()
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
```
(주: `apps/desktop-shell/Cargo.toml`에 `base64 = "0.22"` 의존 추가 필요.)

- [ ] **Step 2: 모듈 선언 + base64 의존 추가**

`apps/desktop-shell/src/lib.rs`(없으면 `main.rs`)의 mod 선언부에 `pub mod host_bridge_client;` 추가.
`apps/desktop-shell/Cargo.toml` `[dependencies]`에 `base64 = "0.22"` 추가.

- [ ] **Step 3: 테스트 실행**

Run: `cargo test -p geulos-desktop-shell host_bridge_client`
Expected: `is_host_path_detects_drive_letters` PASS.
(주: 정확한 package 이름은 `apps/desktop-shell/Cargo.toml`의 `[package].name` 확인 — `geulos-desktop-shell`.)

- [ ] **Step 4: 커밋**
```bash
git add apps/desktop-shell/src/host_bridge_client.rs apps/desktop-shell/Cargo.toml apps/desktop-shell/src/lib.rs
git commit -m "feat(desktop-shell): 호스트 브리지 클라이언트 + is_host_path (TDD)"
```

---

## Task 5: drives.rs 통합 — 호스트 드라이브 + VM 루트

**Files:**
- Modify: `apps/desktop-shell/src/drives.rs`

- [ ] **Step 1: Linux 분기 변경**

`drives.rs`의 `list_drives()` 비-Windows 분기를 교체:
```rust
    #[cfg(not(windows))]
    {
        // VM(GeulOS/Linux): 호스트 브리지가 살아있으면 호스트 드라이브 + VM 루트,
        // 없으면 VM 루트만(폴백).
        match crate::host_bridge_client::list_drives() {
            Some(drives) => {
                let mut out: Vec<PathBuf> = drives.into_iter().map(PathBuf::from).collect();
                out.push(PathBuf::from("/")); // GeulOS 자체 fs도 탐색 가능
                out
            }
            None => vec![PathBuf::from("/")],
        }
    }
```

- [ ] **Step 2: 빌드 확인 (musl 크로스컴파일)**

Run: `cargo zigbuild --target x86_64-unknown-linux-musl -p geulos-desktop-shell`
Expected: 컴파일 성공.
(주: `crate::host_bridge_client`가 lib에 노출돼야 함 — Task 4 Step 2의 `pub mod` 확인.)

- [ ] **Step 3: 커밋**
```bash
git add apps/desktop-shell/src/drives.rs
git commit -m "feat(desktop-shell): 파일관리자 최상위에 호스트 드라이브 + VM 루트 노출"
```

---

## Task 6: lazy_mount.rs 통합 — 호스트 경로면 브리지로 객체 합성

**Files:**
- Modify: `apps/desktop-shell/src/lazy_mount.rs`

- [ ] **Step 1: expand_folder에 호스트 분기 추가**

`expand_folder` 함수 본문 맨 앞(`let entries = match std::fs::read_dir...` 위)에 추가:
```rust
    // 호스트 경로(C:\ 등)는 브리지 RPC로, VM 경로(/...)는 기존 std::fs로.
    let path_str = folder_path.to_string_lossy();
    if crate::host_bridge_client::is_host_path(&path_str) {
        let entries = match crate::host_bridge_client::list_dir(&path_str) {
            Some(e) => e,
            None => return Ok(Vec::new()), // 브리지 없음/오류 → 빈 폴더(기존 io 오류와 동일 처리)
        };
        let mut out = Vec::new();
        for e in entries {
            // 호스트 경로 결합: 드라이브 구분자 보존(백슬래시). 부모가 'X:\'면 그대로, 아니면 '\' 추가.
            let sep = if path_str.ends_with('\\') || path_str.ends_with('/') { "" } else { "\\" };
            let child_path = format!("{}{}{}", path_str, sep, e.name);
            let obj = if e.is_dir {
                std_types::folder(owner.clone(), &child_path, &e.name, now_ms)
            } else {
                let mime = guess_mime(&e.name);
                let mut f = std_types::file(owner.clone(), &child_path, &e.name, mime, now_ms);
                f.set_state("size_bytes", serde_json::json!(e.size));
                f
            };
            out.push(obj);
        }
        return Ok(out);
    }
```

- [ ] **Step 2: 빌드 확인**

Run: `cargo zigbuild --target x86_64-unknown-linux-musl -p geulos-desktop-shell`
Expected: 컴파일 성공.

- [ ] **Step 3: (선택) 호스트 파일 열람 경로 통합**

파일 더블클릭 → Window content 로드 경로(예: `file_read` 모듈 또는 open_file 핸들러)에서, 대상 File의 path가 `is_host_path`면 `host_bridge_client::read_file(path, 1<<20)`로 내용을 읽어 Window content에 채운다. 기존 std::fs::read 경로는 VM 파일용으로 유지.
(정확한 위치: `apps/desktop-shell/src/handlers/`에서 파일 내용 읽는 함수를 grep `read_file_for_window` 또는 `std::fs::read` 로 찾아 분기 추가. v1 탐색만 우선이면 이 step은 다음 증분으로 미뤄도 됨 — spec 결정사항 1.)

- [ ] **Step 4: 커밋**
```bash
git add apps/desktop-shell/src/lazy_mount.rs
git commit -m "feat(desktop-shell): expand_folder 호스트 경로는 브리지 RPC로 객체 합성"
```

---

## Task 7: 빌드/기동 배선 — build.ps1 + launch.ps1

**Files:**
- Modify: `boot/build.ps1`
- Modify: `boot/qemu/launch.ps1`

- [ ] **Step 1: build.ps1에 호스트 브리지 네이티브 빌드 추가**

`boot/build.ps1`의 크로스컴파일 단계 *근처*(initrd 조립 전, 단 호스트 타겟)에서 호스트 네이티브 빌드:
```powershell
Write-Host "[host-bridge] building native host binary..."
cargo build --release -p geulos-host-bridge
# 산출물: target/release/geulos-host-bridge.exe (호스트에서 실행, initrd에 넣지 않음)
```
(주: 이건 musl 크로스컴파일이 아니라 *호스트 네이티브* 빌드 — VM이 아니라 Windows에서 돈다.)

- [ ] **Step 2: launch.ps1에서 QEMU와 함께 브리지 기동**

`boot/qemu/launch.ps1`에서 QEMU 실행(`& qemu-system-x86_64 @QemuArgs`) *직전*에 브리지 기동, 이후 정리:
```powershell
# 호스트 브리지 기동 (VM이 10.0.2.2:5560으로 도달). 이미 떠 있으면 bind 실패하고 그 인스턴스 사용.
$BridgeExe = Join-Path $WorkspaceRoot "target/release/geulos-host-bridge.exe"
$bridgeProc = $null
if (Test-Path $BridgeExe) {
    $bridgeProc = Start-Process $BridgeExe -PassThru -WindowStyle Hidden
    Write-Host "host-bridge: 기동 (PID $($bridgeProc.Id), 127.0.0.1:5560)"
} else {
    Write-Host "host-bridge: 바이너리 없음 — 호스트 드라이브 비활성 (pwsh boot/build.ps1로 빌드)"
}

& qemu-system-x86_64 @QemuArgs

# QEMU 종료 후 브리지 정리
if ($bridgeProc -and -not $bridgeProc.HasExited) {
    Stop-Process -Id $bridgeProc.Id -Force -ErrorAction SilentlyContinue
}
```

- [ ] **Step 3: 커밋**
```bash
git add boot/build.ps1 boot/qemu/launch.ps1
git commit -m "build: 호스트 브리지 네이티브 빌드 + launch에서 QEMU와 함께 기동"
```

---

## Task 8: 수용 검증 (사용자 + AI 패리티)

**Files:** 없음 (검증만)

- [ ] **Step 1: 전체 빌드 + 부팅**

Run:
```powershell
& .\boot\build.ps1 -Release
& .\boot\qemu\launch.ps1 -Graphics
```
Expected: launch 로그에 `host-bridge: 기동`. VM 부팅 완료.

- [ ] **Step 2: 파일관리자에서 호스트 드라이브 확인**

VM에서 파일관리자 실행 → 최상위에 `C:\`, `D:\` … + `/` 표시. 호스트 드라이브 펼치기 → 실제 Windows 폴더/파일 표시. 하위 폴더 진입 동작.
Expected: 호스트 파일이 보이고 탐색됨.

- [ ] **Step 3: 폴백 검증**

브리지 없이(또는 종료 후) VM 부팅 → 파일관리자 최상위에 `/`만, 정상 동작(크래시 없음).
Expected: graceful fallback.

- [ ] **Step 4: AI 패리티 확인**

ai-bridge 또는 CLI의 AI 모드에서 동일 FileTree/Explorer 객체를 query → 호스트 폴더/파일이 객체로 보임(캡처 없이 데이터로). 
Expected: AI가 호스트 파일 트리를 객체로 관찰.

- [ ] **Step 5: known-issues / ADR 기록**

`docs/adr/`에 ADR 추가(호스트 브리지 Model B 채택, QEMU fsdev 비활성 제약, read-only v1). `docs/manual-tests/`에 수용 절차 기록.

---

## Self-Review (작성자 체크)

- **Spec 커버리지:** list_drives/list_dir/stat/read_file(Task 2~3), 클라이언트(Task 4), drives+루트(Task 5), 호스트 경로 분기(Task 6), graceful fallback(Task 4 rpc None + Task 5/6 분기), launch 기동(Task 7), 읽기전용·절대경로 검증(Task 2). read_file 열람은 Task 6 Step 3(선택). 모두 커버.
- **타입 일관성:** `Request`/`Response`/`EntryInfo`는 host-bridge protocol.rs와 desktop-shell client에 *동일 wire*로 복제(크레이트 경계). 변경 시 양쪽 동기화 필요 — 주석 명시. `is_host_path`/`list_drives`/`list_dir`/`read_file` 시그니처 Task 4 정의와 Task 5/6 사용 일치.
- **미해결:** `geulos-proto` package명·`geulos-desktop-shell` package명·파일 열람 함수 위치는 구현 시 1줄 grep으로 확정(각 Task 주석에 명시).
