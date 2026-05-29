//! switch_root — initramfs(rootfs)에서 디스크 루트로 전환.
//! rootfs는 pivot_root 불가 → util-linux switch_root 알고리즘 사용.

use std::ffi::CString;

use nix::mount::{mount, MsFlags};
use nix::unistd::{chdir, chroot, execv};

use crate::disk::NEWROOT;

/// /proc, /sys, /dev를 newroot 하위로 mount --move.
pub fn move_virtual_filesystems() {
    for vfs in ["proc", "sys", "dev"] {
        let from = format!("/{}", vfs);
        let to = format!("{}/{}", NEWROOT, vfs);
        if let Err(e) = std::fs::create_dir_all(&to) {
            eprintln!("[bootstrap]   mkdir {}: {}", to, e);
            continue;
        }
        match mount(Some(from.as_str()), to.as_str(), None::<&str>, MsFlags::MS_MOVE, None::<&str>) {
            Ok(()) => println!("[bootstrap]   moved {} -> {}", from, to),
            Err(e) => eprintln!("[bootstrap]   move {} failed: {}", from, e),
        }
    }
}

/// newroot를 /로 만들고 `/sbin/init`을 PID 1으로 exec. 성공 시 반환하지 않음.
pub fn switch_root_to_disk(init_arg: &str) -> Result<(), String> {
    chdir(NEWROOT).map_err(|e| format!("chdir {}: {}", NEWROOT, e))?;
    mount(Some(NEWROOT), "/", None::<&str>, MsFlags::MS_MOVE, None::<&str>)
        .map_err(|e| format!("mount --move {} /: {}", NEWROOT, e))?;
    chroot(".").map_err(|e| format!("chroot .: {}", e))?;
    chdir("/").map_err(|e| format!("chdir /: {}", e))?;

    let init = CString::new("/sbin/init").unwrap();
    let args: Vec<CString> = if init_arg.is_empty() {
        vec![init.clone()]
    } else {
        vec![init.clone(), CString::new(init_arg).unwrap()]
    };
    execv(&init, &args).map_err(|e| format!("execv /sbin/init: {}", e))?;
    Ok(())
}
