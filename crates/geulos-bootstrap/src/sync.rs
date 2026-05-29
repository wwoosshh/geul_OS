//! /payload 시스템 트리를 디스크 루트(/newroot)로 복사 (B 모델).
//! syncplan::should_preserve로 /root·/home은 건드리지 않음. .tmp+rename 원자성.

use std::path::Path;

use crate::disk::NEWROOT;
use crate::syncplan;

const PAYLOAD: &str = "/payload";

/// /payload/* → /newroot/* 재귀 복사. 보존 경로는 스킵.
pub fn sync_system_files() {
    println!("[bootstrap] syncing {} -> {} (preserve root/home)", PAYLOAD, NEWROOT);
    copy_dir_rec(Path::new(PAYLOAD), "");
    println!("[bootstrap] sync done");
}

/// `rel`은 PAYLOAD 기준 상대경로(디스크 루트 기준과 동일). 빈 문자열=루트.
fn copy_dir_rec(payload_root: &Path, rel: &str) {
    let src_dir = if rel.is_empty() { payload_root.to_path_buf() } else { payload_root.join(rel) };
    let entries = match std::fs::read_dir(&src_dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[bootstrap]   read_dir {}: {}", src_dir.display(), e);
            return;
        }
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let child_rel = if rel.is_empty() { name.to_string() } else { format!("{}/{}", rel, name) };

        if syncplan::should_preserve(&child_rel) {
            println!("[bootstrap]   preserve {}", child_rel);
            continue;
        }
        let dest = format!("{}/{}", NEWROOT, child_rel);
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_dir() {
            let _ = std::fs::create_dir_all(&dest);
            copy_dir_rec(payload_root, &child_rel);
        } else {
            copy_file_atomic(&entry.path(), &dest);
        }
    }
}

/// .tmp로 쓰고 rename (같은 FS 내 원자적). 실패는 로그 후 계속.
fn copy_file_atomic(src: &Path, dest: &str) {
    if let Some(parent) = Path::new(dest).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = format!("{}.tmp", dest);
    if let Err(e) = std::fs::copy(src, &tmp) {
        eprintln!("[bootstrap]   copy {} -> {}: {}", src.display(), tmp, e);
        return;
    }
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    if let Err(e) = std::fs::rename(&tmp, dest) {
        eprintln!("[bootstrap]   rename {} -> {}: {}", tmp, dest, e);
    }
}
