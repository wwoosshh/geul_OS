//! /proc, /sys, /dev 마운트 (Linux PID 1의 첫 책임).

use nix::mount::{mount, MsFlags};

/// 표준 가상 파일시스템 세 개를 마운트한다.
///
/// 각 디렉터리가 없으면 만들고 mount syscall 호출. 실패해도 진행 — 부분 성공
/// 으로도 일부 service는 동작 가능.
pub fn mount_essentials() -> Result<(), String> {
    let mounts: &[(&str, &str, &str, MsFlags)] = &[
        ("proc", "/proc", "proc", MsFlags::empty()),
        ("sysfs", "/sys", "sysfs", MsFlags::empty()),
        ("devtmpfs", "/dev", "devtmpfs", MsFlags::empty()),
    ];

    let mut errors: Vec<String> = Vec::new();
    for (source, target, fstype, flags) in mounts {
        // 디렉터리 생성 (initrd에 미리 있어야 하지만 안전망)
        if let Err(e) = std::fs::create_dir_all(target) {
            errors.push(format!("mkdir {}: {}", target, e));
            continue;
        }
        match mount(Some(*source), *target, Some(*fstype), *flags, None::<&str>) {
            Ok(()) => println!("[init] mounted {} on {}", source, target),
            Err(e) => errors.push(format!("mount {} -> {}: {}", source, target, e)),
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
