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

/// 대상 path가 이미 심볼릭 링크면 거부 (write/create가 symlink target을 따라 외부로
/// 새는 것을 방지). 존재하지 않으면 OK.
fn reject_existing_symlink(p: &std::path::Path) -> Result<(), String> {
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.file_type().is_symlink() => {
            Err(format!("심볼릭 링크 대상 거부: {}", p.display()))
        }
        _ => Ok(()),
    }
}

/// 연산 후 *실제 path가 허용목록 안*인지 재검증. 외부면 가능한 정리 + Err.
/// symlink TOCTOU 등 pre-check를 우회한 케이스 차단(defense-in-depth).
fn post_op_verify(p: &std::path::Path, cleanup_if_outside: bool) -> Result<(), String> {
    let real = match std::fs::canonicalize(p) {
        Ok(r) => r,
        Err(_) => return Ok(()), // 연산 후 path 없거나 access 불가 — OS 이슈, 통과.
    };
    let bases = allowed_bases();
    for b in &bases {
        let real_b = std::fs::canonicalize(b).unwrap_or_else(|_| b.clone());
        if real.starts_with(&real_b) {
            return Ok(());
        }
    }
    if cleanup_if_outside {
        let _ = std::fs::remove_file(p);
    }
    Err(format!("post-op 검증 실패 (허용목록 밖): {}", real.display()))
}

pub fn write_file(path: &str, bytes: &[u8]) -> Result<(), String> {
    let p = parent_under_allowlist(path)?;
    reject_existing_symlink(&p)?;
    std::fs::write(&p, bytes).map_err(|e| format!("write 실패: {}", e))?;
    post_op_verify(&p, true)
}

pub fn create_dir(path: &str) -> Result<(), String> {
    let p = parent_under_allowlist(path)?;
    reject_existing_symlink(&p)?;
    std::fs::create_dir(&p).map_err(|e| format!("create_dir 실패: {}", e))?;
    post_op_verify(&p, false)
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
    let to_path = parent_under_allowlist(to)?;
    reject_existing_symlink(&to_path)?;
    std::fs::rename(&real_from, &to_path).map_err(|e| format!("rename 실패: {}", e))?;
    post_op_verify(&to_path, false)
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
    fn canonicalize_basic_rejects() {
        assert!(canonicalize_under_allowlist("relative").is_err());
        assert!(canonicalize_under_allowlist("/a/../b").is_err());
    }
}
