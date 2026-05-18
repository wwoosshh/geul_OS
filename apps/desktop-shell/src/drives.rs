//! Windows 드라이브 열거. 비-Windows는 단일 root("/") fallback.
//!
//! M8 ADR-028 — 시작 시 모든 드라이브 자동 mount. winapi `GetLogicalDrives`
//! 비트마스크 → 알파벳 letter. cfg gate로 Windows 전용 dependency 격리.

use std::path::PathBuf;

/// 시스템의 모든 root 경로를 반환.
///
/// Windows: `GetLogicalDrives` Win32 API로 비트마스크 → 알파벳별 드라이브 letter.
/// 비-Windows: `["/"]` 단일 fallback (테스트/디자인 단순성 목적, 실제 Linux/macOS 지원은 후속).
pub fn list_drives() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        list_drives_windows()
    }
    #[cfg(not(windows))]
    {
        vec![PathBuf::from("/")]
    }
}

#[cfg(windows)]
fn list_drives_windows() -> Vec<PathBuf> {
    use winapi::um::fileapi::GetLogicalDrives;
    let mask = unsafe { GetLogicalDrives() };
    if mask == 0 {
        // API 실패 시 fallback — 적어도 C 시도.
        return vec![PathBuf::from("C:\\")];
    }
    let mut out = Vec::new();
    for i in 0..26 {
        if mask & (1 << i) != 0 {
            let letter = (b'A' + i as u8) as char;
            out.push(PathBuf::from(format!("{}:\\", letter)));
        }
    }
    out
}
