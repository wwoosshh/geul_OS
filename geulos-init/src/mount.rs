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

    // stage-1 부트스트랩이 switch_root 전에 /proc·/sys·/dev를 mount --move로 디스크
    // 루트로 옮겨 두므로, stage-2(여기)에서 다시 마운트하면 EBUSY가 난다. 이미
    // 마운트된 타깃은 건너뛴다(idempotent).
    let proc_mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();

    let mut errors: Vec<String> = Vec::new();
    for (source, target, fstype, flags) in mounts {
        // 디렉터리 생성 (initrd에 미리 있어야 하지만 안전망)
        if let Err(e) = std::fs::create_dir_all(target) {
            errors.push(format!("mkdir {}: {}", target, e));
            continue;
        }
        if mountpoint_present(&proc_mounts, target) {
            println!("[init] {} already mounted — skip", target);
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

/// `/proc/mounts` 내용에서 target 마운트포인트가 이미 있으면 true (순수 — 테스트 가능).
/// 각 줄: "<src> <mountpoint> <fstype> <opts> ...". 두 번째 필드 비교.
pub fn mountpoint_present(proc_mounts: &str, target: &str) -> bool {
    proc_mounts
        .lines()
        .any(|line| line.split_whitespace().nth(1).map(|mp| mp == target).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "proc /proc proc rw,nosuid 0 0\n\
                          sysfs /sys sysfs rw 0 0\n\
                          devtmpfs /dev devtmpfs rw 0 0\n";

    #[test]
    fn detects_existing_mountpoints() {
        assert!(mountpoint_present(SAMPLE, "/proc"));
        assert!(mountpoint_present(SAMPLE, "/sys"));
        assert!(mountpoint_present(SAMPLE, "/dev"));
    }

    #[test]
    fn absent_mountpoint_is_false() {
        assert!(!mountpoint_present(SAMPLE, "/newroot"));
        assert!(!mountpoint_present(SAMPLE, "/proc/extra"));
        assert!(!mountpoint_present("", "/proc"));
    }
}
