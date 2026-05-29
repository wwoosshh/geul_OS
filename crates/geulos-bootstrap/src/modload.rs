//! Stage 1 전용 모듈 적재 — 디스크 접근에 필요한 모듈만 의존 순서로 적재.
//! (geulos-init/modules.rs는 "전체 적재" 정책이라 별개. 여기선 최소 부분집합만.)

use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use nix::libc;

/// `/lib/modules` 아래 첫 커널 버전 디렉터리.
pub fn find_kernel_dir() -> Option<PathBuf> {
    std::fs::read_dir("/lib/modules")
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir())
}

/// `finit_module(fd, "", 0)`. 이미 적재(EEXIST)면 성공 취급. 파일 없으면 Ok(스킵).
fn finit_module(path: &Path) -> Result<(), String> {
    if !path.exists() {
        println!("[bootstrap]   (skip, absent) {}", path.display());
        return Ok(());
    }
    let file = std::fs::File::open(path).map_err(|e| format!("open {}: {}", path.display(), e))?;
    let res = unsafe {
        libc::syscall(libc::SYS_finit_module, file.as_raw_fd() as libc::c_int, c"".as_ptr(), 0 as libc::c_int)
    };
    if res < 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EEXIST) {
            return Ok(());
        }
        return Err(format!("finit_module {}: {}", path.display(), err));
    }
    Ok(())
}

/// 지정 이름 목록을 순서대로 적재. 디스크 접근에 필요한 최소 세트.
/// virtio 전송 → virtio_blk → ext4 의존 → ext4.
pub fn load_disk_stack(kernel_dir: &Path) {
    const ORDER: &[&str] = &[
        "virtio.ko",
        "virtio_ring.ko",
        "virtio_pci_legacy_dev.ko",
        "virtio_pci_modern_dev.ko",
        "virtio_pci.ko",
        "virtio_blk.ko",
        "crc16.ko",
        "crc32c_generic.ko",
        "libcrc32c.ko",
        "mbcache.ko",
        "jbd2.ko",
        "ext4.ko",
    ];
    for name in ORDER {
        let p = kernel_dir.join(name);
        match finit_module(&p) {
            Ok(()) => println!("[bootstrap]   loaded {}", name),
            Err(e) => eprintln!("[bootstrap]   {} load failed: {}", name, e),
        }
    }
}
