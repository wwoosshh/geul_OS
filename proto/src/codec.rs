//! 길이 접두사 프레임 codec.
//!
//! 형식: `[u32 big-endian length][body bytes]`. body는 UTF-8 JSON.
//!
//! 동기 API. tokio 측에서 `AsyncRead`/`AsyncWrite` 래핑은 server-host에서.

use thiserror::Error;

/// 디코딩 오류.
#[derive(Debug, Error)]
pub enum DecodeError {
    /// 데이터가 부족함. 더 받아서 재시도.
    #[error("incomplete frame — need more bytes")]
    Incomplete,
    /// 너무 큰 프레임.
    #[error("frame too large: {0} bytes (max 16 MB)")]
    TooLarge(u32),
}

/// 최대 프레임 크기: 16 MB.
pub const MAX_FRAME_SIZE: u32 = 16 * 1024 * 1024;

/// 본문을 프레임으로 인코딩.
pub fn encode_frame(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + body.len());
    let len = body.len() as u32;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// 입력 슬라이스에서 한 프레임을 디코딩하고 슬라이스를 진행.
///
/// 디코딩 성공: 본문 바이트 반환, `*buf`는 다음 프레임 시작으로 이동.
/// 부족: `Incomplete` 반환, `*buf`는 변경 없음.
pub fn decode_frame(buf: &mut &[u8]) -> Result<Vec<u8>, DecodeError> {
    if buf.len() < 4 {
        return Err(DecodeError::Incomplete);
    }
    let len_bytes: [u8; 4] = buf[0..4].try_into().expect("이미 길이 검증됨");
    let len = u32::from_be_bytes(len_bytes);
    if len > MAX_FRAME_SIZE {
        return Err(DecodeError::TooLarge(len));
    }
    let total = 4usize + len as usize;
    if buf.len() < total {
        return Err(DecodeError::Incomplete);
    }
    let body = buf[4..total].to_vec();
    *buf = &buf[total..];
    Ok(body)
}
