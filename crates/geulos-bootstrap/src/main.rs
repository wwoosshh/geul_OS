//! GeulOS stage-1 부트스트랩 (initramfs /init = PID 1).

mod superblock;
mod syncplan;

#[cfg(target_os = "linux")]
mod modload;
#[cfg(target_os = "linux")]
mod disk;
#[cfg(target_os = "linux")]
mod switchroot;

#[cfg(target_os = "linux")]
mod mountvfs {
    use nix::mount::{mount, MsFlags};
    /// /proc·/sys·/dev 마운트 (stage 1 진입 직후).
    pub fn mount_essentials() {
        let m: &[(&str, &str, &str)] = &[
            ("proc", "/proc", "proc"),
            ("sysfs", "/sys", "sysfs"),
            ("devtmpfs", "/dev", "devtmpfs"),
        ];
        for (src, tgt, fstype) in m {
            let _ = std::fs::create_dir_all(tgt);
            match mount(Some(*src), *tgt, Some(*fstype), MsFlags::empty(), None::<&str>) {
                Ok(()) => println!("[bootstrap] mounted {} on {}", src, tgt),
                Err(e) => eprintln!("[bootstrap] mount {} failed: {}", tgt, e),
            }
        }
    }
}

/// 디스크 단계 실패 시 폴백: initramfs의 stage2(/payload/sbin/init)를 직접 exec.
/// (= 현재의 비영속 램디스크 동작.) PID 1 유지를 위해 exec.
#[cfg(target_os = "linux")]
fn fallback_ramdisk() -> ! {
    use std::ffi::CString;
    use nix::unistd::execv;
    eprintln!("[bootstrap] FALLBACK → ramdisk boot (/payload/sbin/init, 비영속)");
    let init = CString::new("/payload/sbin/init").unwrap();
    if let Err(e) = execv(&init, &[init.clone()]) {
        eprintln!("[bootstrap] fallback execv failed: {} — PID 1 sleep loop", e);
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

#[cfg(target_os = "linux")]
fn main() {
    // M0 stub stage-2: switch_root가 `/sbin/init stage2-stub`로 재진입하면 여기로.
    if std::env::args().nth(1).as_deref() == Some("stage2-stub") {
        println!("[stage2-stub] ===== GEULOS STAGE2 REACHED ON DISK ROOT =====");
        println!("[stage2-stub] cwd={:?} /sbin/init exists={}",
            std::env::current_dir(), std::path::Path::new("/sbin/init").exists());
        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    }

    println!();
    println!("=== GeulOS bootstrap (stage 1, PID {}) ===", std::process::id());

    mountvfs::mount_essentials();

    let kernel_dir = match modload::find_kernel_dir() {
        Some(d) => {
            println!("[bootstrap] modules dir: {}", d.display());
            d
        }
        None => {
            eprintln!("[bootstrap] no /lib/modules — fallback");
            fallback_ramdisk();
        }
    };
    modload::load_disk_stack(&kernel_dir);

    if !disk::wait_for_disk() {
        fallback_ramdisk();
    }

    if disk::is_formatted() {
        println!("[bootstrap] {} already formatted — skip mkfs", disk::DISK_DEV);
    } else {
        println!("[bootstrap] {} blank — formatting", disk::DISK_DEV);
        if let Err(e) = disk::format() {
            eprintln!("[bootstrap] format failed: {} — fallback", e);
            fallback_ramdisk();
        }
    }

    if let Err(e) = disk::mount_disk() {
        eprintln!("[bootstrap] mount failed: {} — fallback", e);
        fallback_ramdisk();
    }

    // M0: 동기화 생략. stub stage-2 = 부트스트랩 자기 자신을 디스크 /sbin/init로 복사.
    {
        let _ = std::fs::create_dir_all(format!("{}/sbin", disk::NEWROOT));
        let self_exe = std::env::current_exe().expect("current_exe");
        let dest = format!("{}/sbin/init", disk::NEWROOT);
        if let Err(e) = std::fs::copy(&self_exe, &dest) {
            eprintln!("[bootstrap] copy self -> {} failed: {} — fallback", dest, e);
            fallback_ramdisk();
        }
        // exec 비트 보장 (ext에선 보통 유지되나 명시).
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
        }
    }

    switchroot::move_virtual_filesystems();
    if let Err(e) = switchroot::switch_root_to_disk("stage2-stub") {
        eprintln!("[bootstrap] switch_root failed: {} — fallback", e);
        fallback_ramdisk();
    }
    // 도달 불가 (switch_root 성공 시 exec).
    unreachable!();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("geulos-bootstrap only runs on Linux (initramfs PID 1 role).");
    std::process::exit(1);
}
