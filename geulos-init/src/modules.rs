//! 커널 모듈 적재 — `finit_module(2)` syscall로 .ko 파일 로드.
//!
//! ADR-017 참고. 우리 initrd엔 `/lib/modules/<kernel-version>/` 디렉터리가 들어
//! 있고, 그 안의 `.ko` 파일들을 *하드코딩된 의존 순서*로 로드한다.
//!
//! 의존 그래프(modules.dep) 파싱은 후속 작업. 현재 M6.5 범위에선 NIC 한 개
//! (`e1000`)만 필요해 단순 순서로 충분.

use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use nix::libc;

/// `finit_module(fd, "", 0)` 래퍼. 이미 적재된 모듈(EEXIST)은 *성공으로 취급*.
fn finit_module(path: &Path) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open {}: {}", path.display(), e))?;

    let res = unsafe {
        libc::syscall(
            libc::SYS_finit_module,
            file.as_raw_fd() as libc::c_int,
            c"".as_ptr(),
            0 as libc::c_int,
        )
    };

    if res < 0 {
        let err = std::io::Error::last_os_error();
        // EEXIST = 모듈 이미 적재됨. 정상 흐름으로 간주.
        if err.raw_os_error() == Some(libc::EEXIST) {
            return Ok(());
        }
        return Err(format!("finit_module {}: {}", path.display(), err));
    }
    Ok(())
}

/// `/lib/modules` 아래의 *첫 번째 커널 버전 디렉터리*를 찾는다.
fn find_kernel_dir() -> Option<PathBuf> {
    std::fs::read_dir("/lib/modules")
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir())
}

/// 디렉터리의 모든 `.ko` 파일을 *우선순위 → 알파벳 순*으로 적재.
///
/// 우선순위(하드코딩): 의존 그래프 하단부터 위로 — virtio 인프라를 먼저, 그 위에
/// 실제 디바이스 드라이버. M6.5 범위에선 `e1000` 한 개라 순서 무관하지만,
/// 향후 `virtio_net` 추가 시 의존(`virtio_pci` → `virtio_ring` → `virtio`)이 미리
/// 적재돼 있어야 한다.
const LOAD_ORDER: &[&str] = &[
    "virtio.ko",
    "virtio_ring.ko",
    "virtio_pci.ko",
    "virtio_pci_modern_dev.ko",
    "virtio_pci_legacy_dev.ko",
    "e1000.ko",
    "virtio_net.ko",
];

pub fn load_all() -> Result<(), String> {
    let kernel_dir = match find_kernel_dir() {
        Some(d) => d,
        None => {
            println!("[init] no /lib/modules/<kernel>/ — skipping module load");
            return Ok(());
        }
    };

    println!("[init] loading kernel modules from {}", kernel_dir.display());

    let kos: Vec<PathBuf> = std::fs::read_dir(&kernel_dir)
        .map_err(|e| format!("read {}: {}", kernel_dir.display(), e))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "ko"))
        .collect();

    if kos.is_empty() {
        println!("[init] no .ko files in {} — skipping", kernel_dir.display());
        return Ok(());
    }

    // 우선순위 모듈 먼저
    let mut loaded: std::collections::HashSet<String> = std::collections::HashSet::new();
    for &priority_name in LOAD_ORDER {
        for ko in &kos {
            let name = ko.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == priority_name {
                match finit_module(ko) {
                    Ok(()) => {
                        println!("[init]   loaded {}", name);
                        loaded.insert(name.to_string());
                    }
                    Err(e) => eprintln!("[init]   {} load failed: {}", name, e),
                }
            }
        }
    }

    // 나머지 .ko (알파벳 순)
    let mut remaining: Vec<&PathBuf> = kos
        .iter()
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            !loaded.contains(n)
        })
        .collect();
    remaining.sort_by_key(|p| p.file_name().unwrap_or_default().to_os_string());

    for ko in remaining {
        let name = ko.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        match finit_module(ko) {
            Ok(()) => println!("[init]   loaded {}", name),
            Err(e) => eprintln!("[init]   {} load failed: {}", name, e),
        }
    }

    Ok(())
}
