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
        {
            let mut fh = std::fs::File::create(&f).unwrap();
            fh.write_all(b"hello").unwrap();
            fh.flush().unwrap();
        } // drop closes and flushes before stat
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
