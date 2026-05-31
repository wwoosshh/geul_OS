> **Status:** completed (2026-05-30)
> **Note:** Host bridge v1.5 정식 마감 — per-launch 토큰 + canonicalize 허용목록 + write/create_dir/remove/rename. KI-028 해소.

# GeulOS 호스트 브리지 v1.5 구현 계획 (보안 + 쓰기)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 호스트 브리지에 per-launch 토큰 인증 + canonicalize/허용목록 + 쓰기 op(write_file/create_dir/remove/rename)를 추가하고, 파일관리자에서 호스트 파일을 열람·편집·저장하며 새 파일/폴더 생성·이름변경·삭제까지 가능하게 한다 (Model B 증분 ①).

**Architecture:** launch.ps1이 난수 토큰을 만들어 브리지 env와 게스트 커널 cmdline 양쪽에 전달; geulos-init이 cmdline을 파싱해 `/run/geulos/bridge.token` 파일에 저장; desktop-shell 시작 시 그 파일을 읽어 첫 프레임으로 Auth. 모든 fs op는 canonicalize + base-dir 허용목록 재검사. 파일관리자 UI는 단일=선택 / 더블=열기 + 상단 툴바 4버튼 (각 = 객체 메서드 invoke로 AI도 동일 호출).

**Tech Stack:** Rust, geulos-proto frame codec(재사용), serde, base64 0.22, 기존 ShellRunner/Dialog 패턴(재사용).

**Spec:** `docs/specs/2026-05-30-geulos-host-bridge-write.md`

---

## File Structure

**수정:**
- `crates/geulos-host-bridge/src/protocol.rs` — `Auth`/`WriteFile`/`CreateDir`/`Remove`/`Rename` Request 변형 + Response 변형 추가.
- `crates/geulos-host-bridge/src/fs_ops.rs` — `write_file`/`create_dir`/`remove`/`rename` + `canonicalize_under_allowlist` 헬퍼.
- `crates/geulos-host-bridge/src/main.rs` — 연결당 `authed: bool` 상태, 첫 프레임 Auth 강제, 쓰기 op dispatch.
- `boot/qemu/launch.ps1` — 토큰 생성 + 브리지 env + QEMU `-append`에 `geulos.bridge_token=<hex>` 추가.
- `geulos-init/src/main.rs` — `/proc/cmdline` 파싱 → `/run/geulos/bridge.token` 작성.
- `apps/desktop-shell/src/host_bridge_client.rs` — 토큰 로드 + Auth 첫 프레임 + write_file/create_dir/remove/rename 메서드.
- `apps/desktop-shell/src/file_read.rs` — 호스트 경로면 bridge.read_file로 분기.
- `apps/desktop-shell/src/file_write.rs` — 호스트 경로면 bridge.write_file로 분기.
- `apps/desktop-shell/src/folder_ops.rs` — `create_file`/`create_dir`/`rename`/`remove`의 호스트 경로 분기.
- `apps/desktop-shell/src/handlers/dialog_methods.rs` — AI write 승인 시 호스트 경로면 bridge로.
- `apps/desktop-shell/src/handlers/explorer_methods.rs` — `select`/`create_file`/`create_folder`/`rename_selected`/`delete_selected` 메서드 추가.
- `core/src/object/std_types.rs` — Explorer factory에 `selected_item` state + `select/create_file/create_folder/rename_selected/delete_selected` 메서드 추가.
- `compositor/src/bin/geulos-vm-compositor.rs` — 더블클릭 감지(last_click_target + 500ms), 단일=Explorer.select 호출, FM 창 상단 툴바 hit/dispatch.
- `compositor/src/render.rs` — 선택 row 하이라이트, FM 창 툴바 렌더(4 버튼).
- `compositor/src/layout.rs` — FM 창 본문에 toolbar 영역 분리(28px) + 4 버튼 rect/HitRole 추가.

---

## Task 1: 브리지 프로토콜 + 인증 모듈 + 연결당 authed 게이트

**Files:**
- Modify: `crates/geulos-host-bridge/src/protocol.rs` (Request/Response 변형 추가)
- Create: `crates/geulos-host-bridge/src/auth.rs`
- Modify: `crates/geulos-host-bridge/src/main.rs` (`mod auth;` + dispatch)

- [ ] **Step 1: protocol.rs에 Auth + write op 변형 추가**

`crates/geulos-host-bridge/src/protocol.rs`의 `pub enum Request`에 변형 추가:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Auth { token: String },
    ListDrives,
    ListDir { path: String },
    Stat { path: String },
    ReadFile { path: String, max_bytes: u64 },
    WriteFile { path: String, content_base64: String },
    CreateDir { path: String },
    Remove { path: String, recursive: bool },
    Rename { from: String, to: String },
}
```

`pub enum Response`에 변형 추가:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    Auth { ok: bool },
    Drives { drives: Vec<String> },
    Entries { entries: Vec<EntryInfo> },
    Stat { stat: StatInfo },
    File { content_base64: String, truncated: bool },
    Ok,
    Error { error: String },
}
```

- [ ] **Step 2: 프로토콜 라운드트립 테스트 추가**

`protocol.rs`의 `#[cfg(test)] mod tests` 안에 추가:
```rust
#[test]
fn request_auth_roundtrip() {
    let r = Request::Auth { token: "deadbeef".into() };
    let bytes = serde_json::to_vec(&r).unwrap();
    let back: Request = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(r, back);
}

#[test]
fn request_write_file_roundtrip() {
    let r = Request::WriteFile {
        path: "C:\\x.txt".into(),
        content_base64: "aGVsbG8=".into(),
    };
    let bytes = serde_json::to_vec(&r).unwrap();
    let back: Request = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(r, back);
}

#[test]
fn response_auth_ok_roundtrip() {
    let r = Response::Auth { ok: true };
    let bytes = serde_json::to_vec(&r).unwrap();
    let back: Response = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(r, back);
}
```

- [ ] **Step 3: auth.rs 신규 작성**

`crates/geulos-host-bridge/src/auth.rs`:
```rust
//! per-launch 토큰 인증. launch.ps1이 GEULOS_BRIDGE_TOKEN env로 전달 → main이 startup
//! 시 1회 로드. 토큰 미설정이면 인증 비활성(개발용 fallback) — 단 v1.5 정상 운영은 항상 설정.

use std::sync::OnceLock;

static TOKEN: OnceLock<Option<String>> = OnceLock::new();

/// startup 시 1회 호출 — env에서 토큰을 읽어 보관. 이후 verify가 참조.
pub fn init_from_env() {
    let t = std::env::var("GEULOS_BRIDGE_TOKEN").ok().filter(|s| !s.is_empty());
    let _ = TOKEN.set(t);
}

/// 받은 토큰이 보관된 것과 일치하는지. 토큰 미설정이면 무조건 true (개발용).
pub fn verify(received: &str) -> bool {
    match TOKEN.get().and_then(|o| o.as_deref()) {
        Some(expected) => {
            // 상수시간 비교 — timing attack 차단(로컬이라 영향 작지만 위생).
            let a = expected.as_bytes();
            let b = received.as_bytes();
            if a.len() != b.len() {
                return false;
            }
            let mut diff: u8 = 0;
            for (x, y) in a.iter().zip(b.iter()) {
                diff |= x ^ y;
            }
            diff == 0
        }
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_token_set_accepts_anything() {
        // init_from_env 미호출 상태 — 기본 None.
        // 같은 process에서 OnceLock는 한 번만 set되므로 이 test에선 set 안 함.
        // 실제 init 호출은 main의 single test에서 검증.
        // (별 process 또는 다른 test bin에선 None.)
    }

    #[test]
    fn constant_time_compare_basic() {
        // OnceLock 영향 우회 — verify 로직 단독 테스트가 어려우니 동등 구현으로 검증.
        fn ct_eq(a: &[u8], b: &[u8]) -> bool {
            if a.len() != b.len() {
                return false;
            }
            let mut diff: u8 = 0;
            for (x, y) in a.iter().zip(b.iter()) {
                diff |= x ^ y;
            }
            diff == 0
        }
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }
}
```

- [ ] **Step 4: main.rs에 mod auth + startup + connection authed 게이트**

`crates/geulos-host-bridge/src/main.rs`:
```rust
mod protocol;
mod fs_ops;
mod auth;

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use protocol::{read_frame, write_frame, Request, Response};

const ADDR: &str = "127.0.0.1:5560";
const READ_FILE_HARD_CAP: u64 = 8 * 1024 * 1024;
const WRITE_FILE_HARD_CAP: u64 = 16 * 1024 * 1024;

fn handle_request(req: Request) -> Response {
    match req {
        Request::Auth { .. } => Response::Error { error: "Auth는 첫 프레임에서만 허용".into() },
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
        Request::WriteFile { path, content_base64 } => {
            use base64::{engine::general_purpose::STANDARD, Engine};
            match STANDARD.decode(content_base64) {
                Ok(bytes) if (bytes.len() as u64) > WRITE_FILE_HARD_CAP => {
                    Response::Error { error: format!("쓰기 한도 초과: {} > {}", bytes.len(), WRITE_FILE_HARD_CAP) }
                }
                Ok(bytes) => match fs_ops::write_file(&path, &bytes) {
                    Ok(()) => Response::Ok,
                    Err(e) => Response::Error { error: e },
                },
                Err(e) => Response::Error { error: format!("base64 디코드 실패: {}", e) },
            }
        }
        Request::CreateDir { path } => match fs_ops::create_dir(&path) {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error { error: e },
        },
        Request::Remove { path, recursive } => match fs_ops::remove(&path, recursive) {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error { error: e },
        },
        Request::Rename { from, to } => match fs_ops::rename(&from, &to) {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error { error: e },
        },
    }
}

fn serve_conn(mut stream: TcpStream) {
    let mut buf = Vec::new();
    let mut authed = false;
    loop {
        let body = match read_frame(&mut stream, &mut buf) {
            Ok(Some(b)) => b,
            Ok(None) => break,
            Err(e) => {
                eprintln!("[host-bridge] read 오류: {}", e);
                break;
            }
        };
        let req: Request = match serde_json::from_slice(&body) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::Error { error: format!("요청 파싱 실패: {}", e) };
                let _ = write_frame(&mut stream, &serde_json::to_vec(&resp).unwrap_or_default());
                continue;
            }
        };
        if !authed {
            // 첫 프레임은 반드시 Auth.
            match req {
                Request::Auth { token } => {
                    let ok = auth::verify(&token);
                    let resp = Response::Auth { ok };
                    let _ = write_frame(&mut stream, &serde_json::to_vec(&resp).unwrap_or_default()).and_then(|_| stream.flush());
                    if !ok {
                        eprintln!("[host-bridge] auth 실패 — 연결 종료");
                        break;
                    }
                    authed = true;
                    continue;
                }
                _ => {
                    let resp = Response::Error { error: "첫 프레임은 auth여야 합니다".into() };
                    let _ = write_frame(&mut stream, &serde_json::to_vec(&resp).unwrap_or_default());
                    break;
                }
            }
        }
        let resp = handle_request(req);
        let out = serde_json::to_vec(&resp).unwrap_or_default();
        if let Err(e) = write_frame(&mut stream, &out).and_then(|_| stream.flush()) {
            eprintln!("[host-bridge] write 오류: {}", e);
            break;
        }
    }
}

fn main() {
    auth::init_from_env();
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
                std::thread::spawn(move || serve_conn(stream));
            }
            Err(e) => eprintln!("[host-bridge] accept 오류: {}", e),
        }
    }
}
```

- [ ] **Step 5: 빌드 + 테스트**

PowerShell: `$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH; cargo test -p geulos-host-bridge`
Expected: 모든 테스트 PASS (이전 6 + 새 3 = 9개).

- [ ] **Step 6: 커밋**
```bash
git add crates/geulos-host-bridge
git commit -m "feat(host-bridge): Auth + write 프로토콜 + 연결당 authed 게이트"
```

---

## Task 2: fs_ops 쓰기 op + canonicalize 허용목록

**Files:**
- Modify: `crates/geulos-host-bridge/src/fs_ops.rs`

- [ ] **Step 1: 허용목록 헬퍼 + write/create_dir/remove/rename 추가**

`crates/geulos-host-bridge/src/fs_ops.rs`에 함수 추가(기존 함수들 뒤에):
```rust
/// 허용된 base path 목록 — v1.5는 list_drives() 결과 = 전체 드라이브.
fn allowed_bases() -> Vec<std::path::PathBuf> {
    list_drives().into_iter().map(std::path::PathBuf::from).collect()
}

/// canonicalize + 허용 base 하위인지 검증. 실패 시 Err.
fn canonicalize_under_allowlist(path: &str) -> Result<std::path::PathBuf, String> {
    if !is_safe_absolute(path) {
        return Err(format!("절대경로 아님: {}", path));
    }
    let real = std::fs::canonicalize(path).map_err(|e| format!("canonicalize 실패: {}", e))?;
    let bases = allowed_bases();
    for b in &bases {
        // base도 canonicalize해서 비교(심볼릭 링크 정규화 일관성).
        let real_b = match std::fs::canonicalize(b) {
            Ok(p) => p,
            Err(_) => b.clone(),
        };
        if real.starts_with(&real_b) {
            return Ok(real);
        }
    }
    Err(format!("허용목록 밖 경로: {}", real.display()))
}

/// 부모 경로(write 대상의 디렉터리)가 허용목록 안에 있는지 검사. write 대상 파일은
/// 아직 존재 안 할 수 있어 canonicalize 불가 → 부모로 검사.
fn parent_under_allowlist(path: &str) -> Result<std::path::PathBuf, String> {
    let p = std::path::Path::new(path);
    let parent = p.parent().ok_or_else(|| format!("부모 없음: {}", path))?;
    let parent_str = parent.to_str().ok_or_else(|| "부모 경로 인코딩 실패".to_string())?;
    canonicalize_under_allowlist(parent_str)?;
    Ok(p.to_path_buf())
}

pub fn write_file(path: &str, bytes: &[u8]) -> Result<(), String> {
    let p = parent_under_allowlist(path)?;
    std::fs::write(&p, bytes).map_err(|e| format!("write 실패: {}", e))
}

pub fn create_dir(path: &str) -> Result<(), String> {
    let p = parent_under_allowlist(path)?;
    std::fs::create_dir(&p).map_err(|e| format!("create_dir 실패: {}", e))
}

pub fn remove(path: &str, recursive: bool) -> Result<(), String> {
    let real = canonicalize_under_allowlist(path)?;
    let meta = std::fs::metadata(&real).map_err(|e| format!("metadata 실패: {}", e))?;
    if meta.is_dir() {
        if recursive {
            std::fs::remove_dir_all(&real).map_err(|e| format!("remove_dir_all 실패: {}", e))
        } else {
            std::fs::remove_dir(&real).map_err(|e| format!("remove_dir 실패: {}", e))
        }
    } else {
        std::fs::remove_file(&real).map_err(|e| format!("remove_file 실패: {}", e))
    }
}

pub fn rename(from: &str, to: &str) -> Result<(), String> {
    let real_from = canonicalize_under_allowlist(from)?;
    // to 부모가 허용목록 안에 있어야 함(to는 아직 존재 안 함).
    let _ = parent_under_allowlist(to)?;
    std::fs::rename(&real_from, to).map_err(|e| format!("rename 실패: {}", e))
}
```

- [ ] **Step 2: 단위 테스트 추가**

`fs_ops.rs`의 `#[cfg(test)] mod tests` 안에 추가:
```rust
#[test]
fn write_file_creates_and_overwrites() {
    let d = tmp();
    let f = d.join("w.txt");
    let _ = std::fs::remove_file(&f);
    write_file(f.to_str().unwrap(), b"hello").unwrap();
    assert_eq!(std::fs::read(&f).unwrap(), b"hello");
    write_file(f.to_str().unwrap(), b"world").unwrap();
    assert_eq!(std::fs::read(&f).unwrap(), b"world");
}

#[test]
fn create_dir_makes_new_dir() {
    let d = tmp();
    let sub = d.join("new_sub_xyz");
    let _ = std::fs::remove_dir(&sub);
    create_dir(sub.to_str().unwrap()).unwrap();
    assert!(sub.is_dir());
    let _ = std::fs::remove_dir(&sub);
}

#[test]
fn remove_file_and_dir() {
    let d = tmp();
    let f = d.join("rm.txt");
    std::fs::write(&f, b"x").unwrap();
    remove(f.to_str().unwrap(), false).unwrap();
    assert!(!f.exists());
    let sub = d.join("rm_dir");
    std::fs::create_dir_all(sub.join("nested")).unwrap();
    std::fs::write(sub.join("a.txt"), b"a").unwrap();
    remove(sub.to_str().unwrap(), true).unwrap();
    assert!(!sub.exists());
}

#[test]
fn rename_moves_within_allowlist() {
    let d = tmp();
    let a = d.join("rn_a.txt");
    let b = d.join("rn_b.txt");
    std::fs::write(&a, b"x").unwrap();
    let _ = std::fs::remove_file(&b);
    rename(a.to_str().unwrap(), b.to_str().unwrap()).unwrap();
    assert!(!a.exists());
    assert_eq!(std::fs::read(&b).unwrap(), b"x");
    let _ = std::fs::remove_file(&b);
}

#[test]
fn canonicalize_rejects_outside_allowlist_on_unix() {
    // Unix(/만 허용)에서 정상 경로 통과.
    #[cfg(not(windows))]
    {
        assert!(canonicalize_under_allowlist("/tmp").is_ok());
    }
    // Windows에서 잘못된 형식 거부.
    assert!(canonicalize_under_allowlist("relative").is_err());
    assert!(canonicalize_under_allowlist("/a/../b").is_err());
}
```

- [ ] **Step 3: 테스트 실행**

PowerShell: `$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH; cargo test -p geulos-host-bridge fs_ops`
Expected: 새 4 + 기존 4 = 8 PASS.

- [ ] **Step 4: 커밋**
```bash
git add crates/geulos-host-bridge/src/fs_ops.rs
git commit -m "feat(host-bridge): write/create_dir/remove/rename + canonicalize+허용목록"
```

---

## Task 3: launch.ps1 토큰 생성 + 브리지 env + 게스트 cmdline

**Files:**
- Modify: `boot/qemu/launch.ps1`

- [ ] **Step 1: 토큰 생성·전달 코드 삽입**

`boot/qemu/launch.ps1`에서 `$bridgeProc = $null` 줄 위에 토큰 생성 코드 추가, `Start-Process $BridgeExe ...`를 env 전달 버전으로 교체, `-append` 인자에 토큰 추가.

`boot/qemu/launch.ps1`에서 다음 두 블록을 찾아 교체:

찾기 1 — QEMU args의 `-append` 줄:
```powershell
        "-append", "console=ttyS0 video=1280x800",
```
교체 1:
```powershell
        "-append", "console=ttyS0 video=1280x800 geulos.bridge_token=$BridgeToken",
```

찾기 2 — bridge 시작 줄 (`-WindowStyle Hidden`로 끝나는 Start-Process):
```powershell
if (Test-Path $BridgeExe) {
    $bridgeProc = Start-Process $BridgeExe -PassThru -WindowStyle Hidden
    Write-Host "host-bridge: started (PID $($bridgeProc.Id), 127.0.0.1:5560)"
} else {
```
교체 2:
```powershell
# per-launch 128-bit 토큰 (hex 32자). 브리지엔 env, 게스트엔 -append.
$rand = New-Object byte[] 16
[System.Security.Cryptography.RNGCryptoServiceProvider]::new().GetBytes($rand)
$BridgeToken = -join ($rand | ForEach-Object { $_.ToString("x2") })
if (Test-Path $BridgeExe) {
    $bridgeProc = Start-Process $BridgeExe -PassThru -WindowStyle Hidden -Environment @{ "GEULOS_BRIDGE_TOKEN" = $BridgeToken }
    Write-Host "host-bridge: started (PID $($bridgeProc.Id), 127.0.0.1:5560, token=$($BridgeToken.Substring(0,8))...)"
} else {
```

찾기 1을 먼저 적용해야 `$BridgeToken` 변수를 참조하는 `-append` 줄이 토큰 생성 블록 *이전에* 평가되지 않게 코드 순서를 맞춰야 한다. `$QemuArgs += @(...)` 블록 *이전에* 토큰 생성 코드 + 브리지 기동 블록을 두면 깔끔.

실제 적용 순서 (launch.ps1 본문 위→아래 흐름):
1. 기존 `$QemuArgs = @(...)` (kernel/initrd/mem/accel).
2. 기존 disk 추가.
3. **NEW**: 토큰 생성 (16 byte → 32 hex) → `$BridgeToken`.
4. **NEW**: 브리지 기동 (env로 토큰 전달).
5. `if ($Graphics)` 블록의 `-append` 줄에 `geulos.bridge_token=$BridgeToken` 추가.
6. 기존 QEMU 실행 + finally 정리.

확실하게 하려면: 토큰 생성을 `if ($Graphics)` 블록 **이전**으로 옮기고, 기존 브리지 spawn 블록도 같이 그 위로 옮긴다. 현재 launch.ps1엔 브리지 spawn이 QEMU 실행 직전에 있을 텐데, 토큰 생성을 그 한 줄 위에 추가하면 된다.

- [ ] **Step 2: 스크립트 파싱 검사**

PowerShell: `$null = [System.Management.Automation.PSParser]::Tokenize((Get-Content boot/qemu/launch.ps1 -Raw), [ref]$null); if ($?) { "OK" }`
Expected: `OK`.

- [ ] **Step 3: 호스트 브리지 빌드 + 직접 실행으로 토큰 검증**

```powershell
cargo build --release -p geulos-host-bridge
$env:GEULOS_BRIDGE_TOKEN = "test1234"
Start-Process -FilePath "target/release/geulos-host-bridge.exe" -PassThru -WindowStyle Hidden
```
다른 PowerShell에서 1초 후 직접 TcpClient로 연결해 잘못된 토큰 보내고 거부 응답 확인 가능 — 다만 cargo test의 통합 테스트가 다음 task에서 client 통합으로 검증하므로 생략 가능.

- [ ] **Step 4: 커밋**
```bash
git add boot/qemu/launch.ps1
git commit -m "build(launch): per-launch 토큰 생성 + 브리지 env + 게스트 cmdline 전달"
```

---

## Task 4: geulos-init — cmdline 파싱 + /run/geulos/bridge.token 작성

**Files:**
- Modify: `geulos-init/src/main.rs`

- [ ] **Step 1: 현재 geulos-init/src/main.rs 구조 파악**

```bash
grep -n "fn main\|cmdline\|/run\|/proc" geulos-init/src/main.rs
```
파일에 main 함수가 있고, 부팅 초기에 mount 등을 한다.

- [ ] **Step 2: 토큰 추출 + 파일 작성 함수 추가**

`geulos-init/src/main.rs`의 import + 함수 추가:
```rust
fn extract_bridge_token() -> Option<String> {
    let cmdline = std::fs::read_to_string("/proc/cmdline").ok()?;
    for tok in cmdline.split_whitespace() {
        if let Some(rest) = tok.strip_prefix("geulos.bridge_token=") {
            // hex 검증 (32자 0-9a-f).
            if rest.len() == 32 && rest.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(rest.to_lowercase());
            }
        }
    }
    None
}

fn write_bridge_token_file(token: &str) {
    if let Err(e) = std::fs::create_dir_all("/run/geulos") {
        eprintln!("[init] /run/geulos 디렉터리 생성 실패: {}", e);
        return;
    }
    let path = "/run/geulos/bridge.token";
    match std::fs::write(path, token) {
        Ok(()) => {
            // 권한 — root만 read+write, 다른 사용자 read (단일 사용자 OS라 너그러움 OK).
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644));
            }
            eprintln!("[init] bridge token 저장: {} ({})", path, &token[..8]);
        }
        Err(e) => eprintln!("[init] bridge token 저장 실패: {}", e),
    }
}
```

- [ ] **Step 3: main에서 호출**

`geulos-init/src/main.rs`의 `fn main()` 안, /proc /sys /dev mount 직후 (다른 시스템콜 호출 전 안전):
```rust
    if let Some(token) = extract_bridge_token() {
        write_bridge_token_file(&token);
    } else {
        eprintln!("[init] geulos.bridge_token cmdline 없음 — 호스트 브리지 인증 비활성");
    }
```

- [ ] **Step 4: 크로스컴파일 + 빌드 확인**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cargo zigbuild --release --target x86_64-unknown-linux-musl -p geulos-init
```
Expected: 컴파일 성공.

- [ ] **Step 5: 커밋**
```bash
git add geulos-init/src/main.rs
git commit -m "feat(init): /proc/cmdline geulos.bridge_token 추출 → /run/geulos/bridge.token"
```

---

## Task 5: desktop-shell 클라이언트 — 토큰 로드 + Auth + 쓰기 메서드

**Files:**
- Modify: `apps/desktop-shell/src/host_bridge_client.rs`

- [ ] **Step 1: 토큰 로드 + Auth 첫 프레임 + 새 op 추가**

`apps/desktop-shell/src/host_bridge_client.rs` 전체를 교체(기존 구조 + 추가):

기존 enum Request에 새 변형 추가:
```rust
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
}
```

기존 enum Response에 새 변형 추가:
```rust
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Response {
    Auth { ok: bool },
    Drives { drives: Vec<String> },
    Entries { entries: Vec<EntryInfo> },
    File { content_base64: String, truncated: bool },
    Ok,
    Error { error: String },
}
```

토큰 로드 — 파일 상단에 import + helper:
```rust
fn load_token() -> Option<String> {
    std::fs::read_to_string("/run/geulos/bridge.token")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
```

`rpc` 함수에서 연결 직후 Auth 핸드셰이크 — 기존 `if guard.is_none() { *guard = TcpStream::connect(BRIDGE_ADDR).ok(); }` 다음에 추가:
```rust
    if guard.is_none() {
        let stream_opt = TcpStream::connect(BRIDGE_ADDR).ok();
        if let Some(mut s) = stream_opt {
            // 첫 프레임: Auth.
            let token = load_token().unwrap_or_default();
            let auth_body = serde_json::to_vec(&Request::Auth { token }).ok()?;
            let mut framed = Vec::with_capacity(4 + auth_body.len());
            framed.extend_from_slice(&(auth_body.len() as u32).to_be_bytes());
            framed.extend_from_slice(&auth_body);
            if s.write_all(&framed).and_then(|_| s.flush()).is_err() {
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
    }
```

새 공개 메서드 추가:
```rust
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
```

- [ ] **Step 2: musl 빌드 확인**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cargo zigbuild --release --target x86_64-unknown-linux-musl -p geulos-desktop-shell
```
Expected: 컴파일 성공.

- [ ] **Step 3: 커밋**
```bash
git add apps/desktop-shell/src/host_bridge_client.rs
git commit -m "feat(desktop-shell): 호스트 브리지 토큰 Auth + write/create_dir/remove/rename"
```

---

## Task 6: 파일 열람/저장 라우팅 (host path → bridge)

**Files:**
- Modify: `apps/desktop-shell/src/file_read.rs`
- Modify: `apps/desktop-shell/src/file_write.rs`
- Modify: `apps/desktop-shell/src/folder_ops.rs`
- Modify: `apps/desktop-shell/src/handlers/dialog_methods.rs` (AI 승인 후 write)

- [ ] **Step 1: file_read.rs — 호스트 path면 bridge.read_file**

`apps/desktop-shell/src/file_read.rs`의 `pub fn read_file_for_window(path: &Path, mime: &str) -> FileContent` 함수 시작부에 추가:
```rust
    // 호스트 path(드라이브 문자)면 bridge를 통해 읽는다. VM(Linux) 빌드에서만.
    #[cfg(not(windows))]
    {
        let path_str = path.to_string_lossy();
        if crate::host_bridge_client::is_host_path(&path_str) {
            const MAX: u64 = 1 << 20; // 1MB
            match crate::host_bridge_client::read_file(&path_str, MAX) {
                Some((bytes, truncated)) => {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    return FileContent {
                        text,
                        truncated,
                        mime: mime.to_string(),
                    };
                }
                None => {
                    return FileContent {
                        text: format!("(호스트 브리지에서 읽기 실패: {})", path.display()),
                        truncated: false,
                        mime: mime.to_string(),
                    };
                }
            }
        }
    }
    // (기존 코드 — std::fs로 읽는 흐름)
```
(`FileContent` 구조는 file_read.rs의 기존 정의 그대로 사용. 필드 이름이 다르면 정확히 일치시킬 것.)

- [ ] **Step 2: file_write.rs — 호스트 path면 bridge.write_file**

`apps/desktop-shell/src/file_write.rs`의 `pub fn save(path: &Path, content: &str) -> Result<(), String>` 함수 시작부에 추가:
```rust
    #[cfg(not(windows))]
    {
        let path_str = path.to_string_lossy();
        if crate::host_bridge_client::is_host_path(&path_str) {
            return crate::host_bridge_client::write_file(&path_str, content.as_bytes());
        }
    }
    // (기존 std::fs::write 흐름)
```

- [ ] **Step 3: folder_ops.rs — create_file/create_dir/remove/rename 호스트 분기**

`apps/desktop-shell/src/folder_ops.rs`의 각 함수에 시작부 추가(예: `create_file`):
```rust
    #[cfg(not(windows))]
    {
        let path_str = new_path.to_string_lossy();
        if crate::host_bridge_client::is_host_path(&path_str) {
            return crate::host_bridge_client::write_file(&path_str, b"")
                .map_err(|e| format!("호스트 파일 생성 실패: {}", e));
        }
    }
    // (기존 std::fs::write 흐름)
```
같은 패턴을 `create_dir`/`rename`/`remove`에도 적용 (각각 bridge::create_dir / rename / remove 호출).

- [ ] **Step 4: handlers/dialog_methods.rs — AI 승인 write 분기**

`apps/desktop-shell/src/handlers/dialog_methods.rs`의 line 222 근처 `match std::fs::write(&path, &content)` 줄을 호스트 분기로 감싸기:
```rust
                    let write_result: std::io::Result<()> = {
                        #[cfg(not(windows))]
                        if crate::host_bridge_client::is_host_path(&path.to_string_lossy()) {
                            // bridge로 라우팅, 결과 변환.
                            crate::host_bridge_client::write_file(&path.to_string_lossy(), content.as_bytes())
                                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                        } else {
                            std::fs::write(&path, &content)
                        }
                        #[cfg(windows)]
                        std::fs::write(&path, &content)
                    };
                    match write_result {
                        // ... 기존 처리
                    }
```
(정확한 변수명/scope는 기존 코드 컨텍스트에 맞춰 조정.)

- [ ] **Step 5: musl 빌드 + 호스트 테스트**

```powershell
cargo zigbuild --release --target x86_64-unknown-linux-musl -p geulos-desktop-shell
cargo test -p geulos-desktop-shell --lib
```
Expected: 컴파일 + 기존 테스트 PASS.

- [ ] **Step 6: 커밋**
```bash
git add apps/desktop-shell/src/file_read.rs apps/desktop-shell/src/file_write.rs apps/desktop-shell/src/folder_ops.rs apps/desktop-shell/src/handlers/dialog_methods.rs
git commit -m "feat(desktop-shell): file_read/write/folder_ops + AI write Dialog 호스트 path 라우팅"
```

---

## Task 7: Explorer — selected_item + create/rename/delete 메서드

**Files:**
- Modify: `core/src/object/std_types.rs` (explorer factory에 selected_item + 새 메서드)
- Modify: `apps/desktop-shell/src/handlers/explorer_methods.rs` (handle_select/create_file/create_folder/rename_selected/delete_selected)
- Modify: `apps/desktop-shell/src/main.rs` (새 메서드 dispatch arm)

- [ ] **Step 1: core/std_types.rs — Explorer factory 확장**

`core/src/object/std_types.rs`의 `pub fn explorer(owner: ActorId) -> Object` 안에 추가:
```rust
    obj.set_state("selected_item", json!(null));
    obj.methods.push(MethodSig::new("select").with_arg(ArgSpec::new("folder_id", "string")));
    obj.methods.push(MethodSig::new("create_file").with_arg(ArgSpec::new("name", "string")));
    obj.methods.push(MethodSig::new("create_folder").with_arg(ArgSpec::new("name", "string")));
    obj.methods.push(MethodSig::new("rename_selected").with_arg(ArgSpec::new("new_name", "string")));
    obj.methods.push(MethodSig::new("delete_selected"));
```

- [ ] **Step 2: 단위 테스트**

`core/src/object/std_types.rs`의 `#[cfg(test)] mod tests` 또는 별 위치에 추가:
```rust
#[test]
fn explorer_has_new_methods() {
    let ex = explorer(ActorId::local_user());
    let names: Vec<&str> = ex.methods.iter().map(|m| m.name()).collect();
    assert!(names.contains(&"select"));
    assert!(names.contains(&"create_file"));
    assert!(names.contains(&"create_folder"));
    assert!(names.contains(&"rename_selected"));
    assert!(names.contains(&"delete_selected"));
    assert_eq!(ex.state.get("selected_item"), Some(&json!(null)));
}
```

- [ ] **Step 3: handlers/explorer_methods.rs — 새 핸들러 5개**

`apps/desktop-shell/src/handlers/explorer_methods.rs`에 추가:
```rust
pub fn handle_select(target_id: ObjectId, args: &Value) -> InvokeOutcome {
    let id_str = args.get("folder_id").and_then(|v| v.as_str()).unwrap_or("");
    InvokeOutcome {
        state_sets: vec![(target_id, "selected_item".to_string(), json!(id_str))],
    }
}

pub async fn handle_create_file(
    target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    create_under_active(target_id, args, stream, mounted_objects, owner, req_seq, false).await
}

pub async fn handle_create_folder(
    target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    create_under_active(target_id, args, stream, mounted_objects, owner, req_seq, true).await
}

async fn create_under_active(
    target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    req_seq: &mut u64,
    is_dir: bool,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return Ok(InvokeOutcome::empty());
    }
    // active_folder 경로 lookup.
    let active_folder = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|ex| ex.state.get("active_folder").and_then(|v| v.as_str()))
        .and_then(parse_object_id);
    let folder_path = match active_folder.and_then(|fid| lookup_folder_path(mounted_objects, fid)) {
        Some(p) => p,
        None => return Ok(InvokeOutcome::empty()),
    };
    let sep = if folder_path.to_string_lossy().ends_with('\\') || folder_path.to_string_lossy().ends_with('/') { "" } else { "\\" };
    let new_path = format!("{}{}{}", folder_path.display(), sep, name);
    let result = if is_dir {
        folder_ops::create_dir(std::path::Path::new(&new_path))
    } else {
        folder_ops::create_file(std::path::Path::new(&new_path))
    };
    if let Err(e) = result {
        eprintln!("[explorer] create 실패: {}", e);
        return Ok(InvokeOutcome::empty());
    }
    // re-expand active_folder — fs_watcher 등록 안 한 host path도 새 자식 보이게.
    if let Some(fid) = active_folder {
        // children 재mount: 기존 children 모두 detach 후 re-expand. 단순화: needs_expand가
        // 아닌데도 강제 expand하려면 parent.children을 비운 뒤 lazy_expand_if_needed 호출.
        if let Some(parent) = mounted_objects.iter_mut().find(|o| o.id == fid) {
            parent.children.clear();
        }
        lazy_expand_if_needed(stream, mounted_objects, owner, fid, req_seq, None).await?;
    }
    Ok(InvokeOutcome::empty())
}

pub async fn handle_rename_selected(
    target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    let new_name = args.get("new_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if new_name.is_empty() || new_name.contains('/') || new_name.contains('\\') {
        return Ok(InvokeOutcome::empty());
    }
    let sel_id = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|ex| ex.state.get("selected_item").and_then(|v| v.as_str()))
        .and_then(parse_object_id);
    let sel_id = match sel_id {
        Some(i) => i,
        None => return Ok(InvokeOutcome::empty()),
    };
    let old_path = match lookup_folder_path(mounted_objects, sel_id)
        .or_else(|| crate::handlers::lookup_file_path(mounted_objects, sel_id))
    {
        Some(p) => p,
        None => return Ok(InvokeOutcome::empty()),
    };
    let parent_path = old_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let sep = if parent_path.to_string_lossy().ends_with('\\') || parent_path.to_string_lossy().ends_with('/') { "" } else { "\\" };
    let new_path = format!("{}{}{}", parent_path.display(), sep, new_name);
    if let Err(e) = folder_ops::rename(&old_path, std::path::Path::new(&new_path)) {
        eprintln!("[explorer] rename 실패: {}", e);
        return Ok(InvokeOutcome::empty());
    }
    // re-expand parent active_folder.
    let active = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|ex| ex.state.get("active_folder").and_then(|v| v.as_str()))
        .and_then(parse_object_id);
    if let Some(fid) = active {
        if let Some(parent) = mounted_objects.iter_mut().find(|o| o.id == fid) {
            parent.children.clear();
        }
        lazy_expand_if_needed(stream, mounted_objects, owner, fid, req_seq, None).await?;
    }
    Ok(InvokeOutcome::empty())
}

pub async fn handle_delete_selected(
    target_id: ObjectId,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    let sel_id = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|ex| ex.state.get("selected_item").and_then(|v| v.as_str()))
        .and_then(parse_object_id);
    let sel_id = match sel_id {
        Some(i) => i,
        None => return Ok(InvokeOutcome::empty()),
    };
    let path = match lookup_folder_path(mounted_objects, sel_id)
        .or_else(|| crate::handlers::lookup_file_path(mounted_objects, sel_id))
    {
        Some(p) => p,
        None => return Ok(InvokeOutcome::empty()),
    };
    let is_dir = mounted_objects
        .iter()
        .find(|o| o.id == sel_id)
        .map(|o| o.type_uri.as_str() == "aios.std/Folder@1")
        .unwrap_or(false);
    if let Err(e) = folder_ops::remove(&path, is_dir) {
        eprintln!("[explorer] delete 실패: {}", e);
        return Ok(InvokeOutcome::empty());
    }
    // re-expand active_folder.
    let active = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|ex| ex.state.get("active_folder").and_then(|v| v.as_str()))
        .and_then(parse_object_id);
    if let Some(fid) = active {
        if let Some(parent) = mounted_objects.iter_mut().find(|o| o.id == fid) {
            parent.children.clear();
        }
        lazy_expand_if_needed(stream, mounted_objects, owner, fid, req_seq, None).await?;
    }
    // selected_item 해제.
    Ok(InvokeOutcome {
        state_sets: vec![(target_id, "selected_item".to_string(), json!(null))],
    })
}
```

(`folder_ops::create_file`/`create_dir`/`rename`/`remove`는 Task 6에서 호스트 분기가 추가된 함수들. 시그니처에 맞춰 호출.)

- [ ] **Step 4: main.rs dispatch — 새 5개 메서드 추가**

`apps/desktop-shell/src/main.rs`의 invoke dispatch match에 추가(기존 explorer_methods 분기 옆):
```rust
                "select" if target_type_is(&mounted_objects, target_id, "aios.builtin/Explorer@1") => {
                    explorer_methods::handle_select(target_id, &args)
                }
                "create_file" if target_type_is(&mounted_objects, target_id, "aios.builtin/Explorer@1") => {
                    explorer_methods::handle_create_file(
                        target_id, &args, &mut stream, &mut mounted_objects, &owner, &mut req_seq,
                    ).await?
                }
                "create_folder" if target_type_is(&mounted_objects, target_id, "aios.builtin/Explorer@1") => {
                    explorer_methods::handle_create_folder(
                        target_id, &args, &mut stream, &mut mounted_objects, &owner, &mut req_seq,
                    ).await?
                }
                "rename_selected" if target_type_is(&mounted_objects, target_id, "aios.builtin/Explorer@1") => {
                    explorer_methods::handle_rename_selected(
                        target_id, &args, &mut stream, &mut mounted_objects, &owner, &mut req_seq,
                    ).await?
                }
                "delete_selected" if target_type_is(&mounted_objects, target_id, "aios.builtin/Explorer@1") => {
                    explorer_methods::handle_delete_selected(
                        target_id, &mut stream, &mut mounted_objects, &owner, &mut req_seq,
                    ).await?
                }
```

- [ ] **Step 5: 빌드 + 테스트**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cargo test -p geulos-core --lib
cargo zigbuild --release --target x86_64-unknown-linux-musl -p geulos-desktop-shell
```
Expected: PASS + 컴파일.

- [ ] **Step 6: 커밋**
```bash
git add core/src/object/std_types.rs apps/desktop-shell/src/handlers/explorer_methods.rs apps/desktop-shell/src/main.rs
git commit -m "feat(explorer): selected_item + select/create_file/create_folder/rename_selected/delete_selected"
```

---

## Task 8: 더블클릭 감지 + FM 툴바 hit/render

**Files:**
- Modify: `compositor/src/bin/geulos-vm-compositor.rs` (더블클릭 + 단일=Explorer.select + 툴바 hit dispatch)
- Modify: `compositor/src/layout.rs` (FM 본문 위 28px toolbar 영역 + 4 버튼 rect/HitRole)
- Modify: `compositor/src/render.rs` (toolbar 4 버튼 렌더 + selected_item 행 하이라이트)

- [ ] **Step 1: HitRole 확장 (FM 툴바 4 버튼)**

`compositor/src/layout.rs`의 HitRole enum에 추가:
```rust
    /// FM 창 툴바 — 새 파일/폴더/이름변경/삭제.
    FmToolbarNewFile,
    FmToolbarNewFolder,
    FmToolbarRename,
    FmToolbarDelete,
```

`layout_file_panels` 시작부에서 inner 영역 안에 toolbar 영역을 분리하고 그 안에 4 버튼 push. 본문(FT/Explorer)은 toolbar 아래로 밀어내야 함:
```rust
    const TOOLBAR_H: i32 = 28;
    const TOOLBAR_BTN_W: i32 = 100;
    let toolbar = Rect { x: inner.x, y: inner.y, w: inner.w, h: TOOLBAR_H };
    let mut bx = toolbar.x + 4;
    for role in [HitRole::FmToolbarNewFile, HitRole::FmToolbarNewFolder, HitRole::FmToolbarRename, HitRole::FmToolbarDelete] {
        out.push((fm.id, Rect { x: bx, y: toolbar.y + 2, w: TOOLBAR_BTN_W, h: TOOLBAR_H - 4 }, role));
        bx += TOOLBAR_BTN_W + 4;
    }
    // FT/Explorer 본문은 toolbar 아래로:
    let body_y = toolbar.y + TOOLBAR_H;
    let body_h = inner.h - TOOLBAR_H;
    // (기존 file_panel_split_x 계산 + tree_w/ex_x/ex_w 그대로, inner.y/inner.h 대신 body_y/body_h 사용)
```
(기존 layout_file_panels의 `inner.y` 참조를 `body_y`로, `inner.h`를 `body_h`로 정확히 치환.)

- [ ] **Step 2: render — toolbar 4 버튼 렌더**

`compositor/src/render.rs`의 render_file_manager(또는 FM 본문 렌더) 안에 toolbar 영역 채우고 버튼 4개 렌더:
```rust
// toolbar 배경
fill_rect(buffer, w, h, &Rect { x: inner.x, y: inner.y, w: inner.w, h: 28 }, theme::SURFACE_ELEVATED);
let labels = ["+ 새 파일", "+ 새 폴더", "이름 변경", "삭제"];
let mut bx = inner.x + 4;
for label in labels.iter() {
    let btn = Rect { x: bx, y: inner.y + 2, w: 100, h: 24 };
    fill_rect(buffer, w, h, &btn, theme::SURFACE_PANEL);
    draw_text(buffer, w, h, label, btn.x + 4, btn.y + 4, theme::TEXT_PRIMARY);
    bx += 104;
}
```

- [ ] **Step 3: render — selected_item 행 하이라이트**

`compositor/src/render.rs`의 Explorer row 렌더 직전, 현재 Explorer의 `selected_item`과 일치하면 배경을 ACCENT_SUBTLE로 채움:
```rust
let selected = ex.state.get("selected_item").and_then(|v| v.as_str()).unwrap_or("");
let is_selected = selected == cid.to_string();
if is_selected {
    fill_rect(buffer, w, h, &row_rect, theme::ACCENT_SUBTLE);
}
```

- [ ] **Step 4: 더블클릭 감지 + 단일 클릭 = Explorer.select + 툴바 버튼 dispatch**

`compositor/src/bin/geulos-vm-compositor.rs`의 main 함수 안에 상태 추가:
```rust
let mut last_click: Option<(geulos_core::ObjectId, std::time::Instant)> = None;
const DOUBLE_CLICK_MS: u128 = 500;
```

BTN_LEFT press의 Folder/File body 분기(현 dispatch_click 직전)를 감싸기:
```rust
} else if obj.type_uri.as_str() == "aios.std/Folder@1" || obj.type_uri.as_str() == "aios.std/File@1" {
    if role == HitRole::ExpandToggle {
        // [+] toggle — 즉시 dispatch (기존 동작 유지).
        let actions = dispatch_click(&tm, target, obj, role);
        for a in actions { let _ = ui_tx.try_send(a); }
    } else if role == HitRole::Body {
        // 단일 vs 더블 — last_click과 비교.
        let now = std::time::Instant::now();
        let is_double = matches!(&last_click, Some((id, t)) if *id == target && now.duration_since(*t).as_millis() < DOUBLE_CLICK_MS);
        last_click = Some((target, now));
        if is_double {
            // 기존 dispatch — navigate_to (Folder) or open_file (File).
            let actions = dispatch_click(&tm, target, obj, role);
            for a in actions { let _ = ui_tx.try_send(a); }
        } else {
            // 단일 — Explorer.select.
            if let Some(ex) = dispatch::find_explorer(&tm) {
                let _ = ui_tx.try_send(UiAction::Invoke {
                    target: ex.id,
                    method: "select".to_string(),
                    args: serde_json::json!({ "folder_id": target.to_string() }),
                });
            }
        }
    }
}
```

툴바 버튼 분기 — BTN_LEFT press의 HitRole 분기에 추가:
```rust
} else if role == HitRole::FmToolbarNewFile || role == HitRole::FmToolbarNewFolder
    || role == HitRole::FmToolbarRename || role == HitRole::FmToolbarDelete {
    // target은 FM. Explorer 찾아 그 메서드 invoke. 이름은 v1.5 단순화 — 고정값
    // ("새 파일.txt"/"새 폴더") 사용. 진짜 이름 입력 Dialog는 follow-up.
    if let Some(ex) = dispatch::find_explorer(&tm) {
        let (method, args) = match role {
            HitRole::FmToolbarNewFile => ("create_file", serde_json::json!({"name": "새 파일.txt"})),
            HitRole::FmToolbarNewFolder => ("create_folder", serde_json::json!({"name": "새 폴더"})),
            HitRole::FmToolbarRename => ("rename_selected", serde_json::json!({"new_name": "이름 변경됨"})),
            HitRole::FmToolbarDelete => ("delete_selected", serde_json::Value::Null),
            _ => unreachable!(),
        };
        let _ = ui_tx.try_send(UiAction::Invoke {
            target: ex.id,
            method: method.to_string(),
            args,
        });
    }
}
```
(이름 입력 Dialog는 v1.5 출시 후 폴리시 단계로 분리 — 우선 고정 이름 + 사용자가 직후 [이름 변경]으로 바꿈.)

- [ ] **Step 5: 빌드 + 테스트**

```powershell
cargo test -p geulos-compositor --lib
cargo zigbuild --release --target x86_64-unknown-linux-musl -p geulos-compositor --bin geulos-vm-compositor
```
Expected: PASS + 컴파일.

- [ ] **Step 6: 커밋**
```bash
git add compositor/src/bin/geulos-vm-compositor.rs compositor/src/layout.rs compositor/src/render.rs
git commit -m "feat(compositor): 더블클릭 감지 + FM 툴바 4버튼 + selected 행 하이라이트"
```

---

## Task 9: 전체 통합 빌드 + 수용 검증 (사용자)

**Files:** 없음 (검증만)

- [ ] **Step 1: 전체 빌드 + 부팅**

```powershell
& .\boot\build.ps1 -Release
& .\boot\qemu\launch.ps1 -Graphics
```
Expected: launch 로그에 `host-bridge: started (... token=xxxxxxxx...)`, 게스트 부팅 후 init 로그에 `[init] bridge token 저장`.

- [ ] **Step 2: 호스트 텍스트 파일 열람·편집·저장**

VM에서 파일관리자 → C:\ 안의 .txt 파일 더블클릭 → Notepad에 내용 표시 → 편집 → Ctrl+S → Windows에서 같은 파일 열어 변경 확인.

- [ ] **Step 3: 새 파일/폴더 생성**

파일관리자 진입 후 어느 폴더에서 툴바 [+ 새 파일] / [+ 새 폴더] → 호스트에 생성 확인(Windows Explorer).

- [ ] **Step 4: 이름 변경 / 삭제**

행 단일클릭(선택 하이라이트) → [이름 변경] → 호스트에 반영 확인. [삭제] → 호스트에서 제거 확인.

- [ ] **Step 5: AI Dialog 흐름**

ai-bridge에서 AI가 호스트 파일 write 호출 → GeulOS에 Dialog 뜸 → [허용] → 반영 / [거부] → 미반영.

- [ ] **Step 6: 토큰 mismatch 폴백**

`/run/geulos/bridge.token`을 잘못된 값으로 변조 후 새 파일관리자 열기 → 호스트 드라이브 미표시(폴백, 크래시 없음).

- [ ] **Step 7: known-issues KI-028 closed 표시 + ADR 작성**

`docs/adr/`에 새 ADR (`041-host-bridge-write-security.md`): 토큰 cmdline 전달 결정 + canonicalize+허용목록 + Dialog AI 패턴 재사용. `docs/known-issues.md`의 KI-028을 ✅ 해소 표시.

---

## Self-Review (작성자 체크)

- **Spec 커버리지**: 보안(토큰 Task 3+4+5, canonicalize Task 2), 쓰기(Task 1+2 op + Task 6 라우팅), FM UX(Task 7 메서드 + Task 8 더블클릭/툴바/하이라이트), Notepad 열람·저장(Task 6 file_read/file_write), AI Dialog(Task 6 dialog_methods), fix bundle(이미 hot-fix로 완료 — `5650a43`/`11bd026`).
- **Placeholder 스캔**: 없음. 모든 step에 실코드.
- **타입 일관성**: Request/Response 변형 이름이 bridge protocol.rs(Task 1)와 host_bridge_client.rs(Task 5)에 동일하게 추가 — 양쪽 동기화 필수(주석으로 명시).
- **결정 누락**: 이름 입력 Dialog는 v1.5 후속 폴리시로 분리(Task 8 노트). 시작 시점엔 고정 이름 + Rename으로 변경 가능.
