//! 호스트 브리지 RPC 프로토콜 — length-prefixed(geulos-proto) JSON 1건.

use serde::{Deserialize, Serialize};

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
    /// one-shot 명령 실행 — 결과 한 번에 반환. ShellRunner@1.run 위임.
    Exec {
        cmd: String,
        args: Vec<String>,
        cwd: String,
        timeout_ms: u64,
    },
    /// long-running 프로세스 spawn. stream_id 즉시 반환 + 후속 ExecStreamPoll로 line 수신.
    ExecStreamStart {
        cmd: String,
        args: Vec<String>,
        cwd: String,
    },
    /// 마지막 poll 이후 누적된 stdout/stderr line + status 조회.
    ExecStreamPoll { stream_id: String },
    /// 프로세스 종료. stream_id가 alive면 SIGKILL.
    ExecStreamKill { stream_id: String },
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

/// 한 line of stdout/stderr — kind는 "stdout" 또는 "stderr".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessLine {
    pub kind: String,
    pub text: String,
}

/// 프로세스 상태 — "running" / "exited" / "terminated" / "error".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessStatus {
    pub status: String,
    pub exit_code: Option<i32>,
}

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
    /// one-shot 결과.
    ExecResult { exit_code: i32, stdout: String, stderr: String, duration_ms: u64 },
    /// streaming 시작 시 발급되는 식별자.
    ExecStreamStarted { stream_id: String, pid: u32 },
    /// 마지막 poll 이후 새 line들 + 현재 status.
    ExecStreamChunk { lines: Vec<ProcessLine>, status: ProcessStatus },
}

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
}
