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
                std::thread::spawn(move || serve_conn(stream));
            }
            Err(e) => eprintln!("[host-bridge] accept 오류: {}", e),
        }
    }
}
