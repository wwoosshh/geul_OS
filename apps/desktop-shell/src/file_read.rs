//! Window mount 시점에 file 본문 read — viewer 용 (M8 part 2, ADR-033).
//!
//! mime 필터 (`text/*`만), UTF-8 검증, 1MB cap, 파일 누락/권한 거부 안전 처리.
//!
//! 호출자 (desktop-shell의 open_file 분기)는 결과의 text/too_large를 그대로
//! Window.state.content/content_too_large에 채워 넣는다. 모든 실패 경로는 *예외 X*
//! — 사용자에게 보일 안내 문자열을 text에 담아 반환 (graceful degrade).

use std::path::Path;

/// Window.state.content + content_too_large 채울 결과.
#[derive(Debug, Clone)]
pub struct FileContent {
    pub text: String,
    pub too_large: bool,
}

const MAX_BYTES: usize = 1024 * 1024;

/// 파일을 *viewer용으로* 읽음. 실패 시 사용자에게 보일 안내 메시지를 text에 담아 반환.
///
/// 흐름:
/// 1. mime이 `text/*`가 아니면 → `"[viewer 미지원: <mime>]"`
/// 2. 파일 read 시도 (raw bytes). io error → `"[읽기 실패: <err>]"`
/// 3. 1MB 초과면 첫 1MB만 남김 + `too_large=true`. UTF-8 char boundary 안전.
/// 4. UTF-8 검증 (invalid → `"[텍스트 파일 아님 — UTF-8 디코딩 실패]"`)
pub fn read_file_for_window(path: &Path, mime: &str) -> FileContent {
    // 호스트 path(드라이브 문자)면 bridge를 통해 읽는다. VM(Linux) 빌드에서만.
    #[cfg(not(windows))]
    {
        let path_str = path.to_string_lossy().to_string();
        if crate::host_bridge_client::is_host_path(&path_str) {
            const MAX: u64 = 1 << 20; // 1MB
            match crate::host_bridge_client::read_file(&path_str, MAX) {
                Some((bytes, truncated)) => {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    return host_bridge_fallback(text, truncated);
                }
                None => {
                    return host_bridge_fallback(
                        format!("(호스트 브리지에서 읽기 실패: {})", path.display()),
                        false,
                    );
                }
            }
        }
    }
    if !mime.starts_with("text/") {
        return FileContent { text: format!("[viewer 미지원: {}]", mime), too_large: false };
    }

    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            return FileContent { text: format!("[읽기 실패: {}]", e), too_large: false };
        }
    };

    let (slice, too_large) = if bytes.len() > MAX_BYTES {
        (utf8_safe_prefix(&bytes, MAX_BYTES), true)
    } else {
        (&bytes[..], false)
    };

    match std::str::from_utf8(slice) {
        Ok(s) => FileContent { text: s.to_string(), too_large },
        Err(_) => FileContent {
            text: "[텍스트 파일 아님 — UTF-8 디코딩 실패]".to_string(),
            too_large: false,
        },
    }
}

/// UTF-8 경계 안전한 prefix.
///
/// `max` 까지 자르되, 마지막 char가 멀티바이트 중간에서 잘리지 않도록 *뒤로* 줄여서
/// 안전한 boundary에 맞춤. `max >= bytes.len()` 이면 그대로 반환.
///
/// 알고리즘: trailing continuation 바이트(`10xxxxxx`)를 모두 제거한 뒤, 마지막
/// 바이트가 leading 바이트(`11xxxxxx`)면 그것도 제거. ASCII (`0xxxxxxx`)는
/// 자르지 않아도 안전하므로 그대로 둔다.
#[cfg(not(windows))]
fn host_bridge_fallback(text: String, truncated: bool) -> FileContent {
    FileContent { text, too_large: truncated }
}

fn utf8_safe_prefix(bytes: &[u8], max: usize) -> &[u8] {
    let mut end = max.min(bytes.len());
    if end == bytes.len() {
        return &bytes[..end];
    }
    while end > 0 && (bytes[end - 1] & 0b1100_0000) == 0b1000_0000 {
        end -= 1;
    }
    if end > 0 && bytes[end - 1] >= 0b1100_0000 {
        end -= 1;
    }
    &bytes[..end]
}
