//! /dev/vda 대기 · 포맷 여부 probe · 포맷(mke2fs ext4) · 마운트.

use std::io::Read;
use std::path::Path;

use nix::mount::{mount, MsFlags};

use crate::superblock;

pub const DISK_DEV: &str = "/dev/vda";
pub const NEWROOT: &str = "/newroot";

/// /dev/vda 등장 대기 (virtio-blk PCI enum 지연 대비, 최대 ~3초).
pub fn wait_for_disk() -> bool {
    for attempt in 0..30 {
        if Path::new(DISK_DEV).exists() {
            if attempt > 0 {
                println!("[bootstrap] {} appeared (attempt {})", DISK_DEV, attempt);
            }
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    eprintln!("[bootstrap] {} did not appear", DISK_DEV);
    false
}

/// 디바이스 앞부분을 읽어 ext 매직이 있으면 true(=이미 포맷).
pub fn is_formatted() -> bool {
    let mut buf = vec![0u8; superblock::EXT_MAGIC_OFFSET + 2];
    match std::fs::File::open(DISK_DEV) {
        Ok(mut f) => match f.read_exact(&mut buf) {
            Ok(()) => superblock::has_ext_magic(&buf),
            Err(e) => {
                eprintln!("[bootstrap] read {} failed: {} — treat as blank", DISK_DEV, e);
                false
            }
        },
        Err(e) => {
            eprintln!("[bootstrap] open {} failed: {} — treat as blank", DISK_DEV, e);
            false
        }
    }
}

/// e2fsprogs mke2fs로 진짜 ext4 파일시스템 생성 (initramfs에 번들된 /sbin/mke2fs + musl).
pub fn format() -> Result<(), String> {
    println!("[bootstrap] formatting {} (mke2fs -t ext4) ...", DISK_DEV);
    let status = std::process::Command::new("/sbin/mke2fs")
        .args(["-F", "-q", "-t", "ext4", DISK_DEV])
        .status()
        .map_err(|e| format!("spawn mke2fs: {}", e))?;
    if !status.success() {
        return Err(format!("mke2fs exit: {:?}", status.code()));
    }
    println!("[bootstrap] format done");
    Ok(())
}

/// /dev/vda를 ext4 드라이버로 /newroot에 마운트.
pub fn mount_disk() -> Result<(), String> {
    std::fs::create_dir_all(NEWROOT).map_err(|e| format!("mkdir {}: {}", NEWROOT, e))?;
    mount(Some(DISK_DEV), NEWROOT, Some("ext4"), MsFlags::empty(), None::<&str>)
        .map_err(|e| format!("mount {} -> {}: {}", DISK_DEV, NEWROOT, e))?;
    println!("[bootstrap] mounted {} on {}", DISK_DEV, NEWROOT);
    Ok(())
}
