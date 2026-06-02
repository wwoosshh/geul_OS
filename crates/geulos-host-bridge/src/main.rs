mod protocol;
mod fs_ops;
mod auth;
mod exec;

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
                Ok(bytes) if (bytes.len() as u64) > WRITE_FILE_HARD_CAP => Response::Error {
                    error: format!("쓰기 한도 초과: {} > {}", bytes.len(), WRITE_FILE_HARD_CAP),
                },
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
        Request::Exec { cmd, args, cwd, timeout_ms } => {
            match exec::exec(&cmd, &args, &cwd, timeout_ms) {
                Ok((exit_code, stdout, stderr, duration_ms)) => {
                    Response::ExecResult { exit_code, stdout, stderr, duration_ms }
                }
                Err(e) => Response::Error { error: e },
            }
        }
        Request::ExecStreamStart { cmd, args, cwd } => {
            match exec::exec_stream_start(&cmd, &args, &cwd) {
                Ok((stream_id, pid)) => Response::ExecStreamStarted { stream_id, pid },
                Err(e) => Response::Error { error: e },
            }
        }
        Request::ExecStreamPoll { stream_id } => match exec::exec_stream_poll(&stream_id) {
            Ok((lines, status)) => Response::ExecStreamChunk { lines, status },
            Err(e) => Response::Error { error: e },
        },
        Request::ExecStreamKill { stream_id } => match exec::exec_stream_kill(&stream_id) {
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
            match req {
                Request::Auth { token } => {
                    let ok = auth::verify(&token);
                    let resp = Response::Auth { ok };
                    let _ = write_frame(&mut stream, &serde_json::to_vec(&resp).unwrap_or_default())
                        .and_then(|_| stream.flush());
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
    // KI-029: Ctrl+C / 종료 시 spawn된 자식 프로세스 전부 cascade kill.
    if let Err(e) = ctrlc::set_handler(|| {
        eprintln!("[host-bridge] 종료 신호 — 자식 프로세스 cleanup");
        exec::exec_stream_kill_all();
        std::process::exit(0);
    }) {
        eprintln!("[host-bridge] ctrlc handler 등록 실패: {}", e);
    }
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
