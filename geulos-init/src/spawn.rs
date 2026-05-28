//! server-host + echo-app + vm-skeleton 자식 프로세스 spawn.

use std::process::{Child, Command};

pub struct SpawnedProcesses {
    pub server: Child,
    pub desktop_shell: Option<Child>,
    pub echo_app: Option<Child>,
    pub skeleton: Option<Child>,
}

pub fn spawn_all() -> Result<SpawnedProcesses, String> {
    println!("[init] spawning /bin/geulosd ...");
    let server = Command::new("/bin/geulosd")
        .arg("0.0.0.0:5550")
        .spawn()
        .map_err(|e| format!("spawn geulosd: {}", e))?;
    println!("[init] geulosd PID = {}", server.id());

    std::thread::sleep(std::time::Duration::from_secs(1));

    // desktop-shell — 진짜 데스크톱(FileTree/Explorer/Cli 등)을 서버에 mount.
    // 컴포지터가 이 객체들을 render_frame으로 그린다.
    println!("[init] spawning /bin/geulos-desktop-shell ...");
    let desktop_shell = match Command::new("/bin/geulos-desktop-shell").arg("127.0.0.1:5550").spawn()
    {
        Ok(child) => {
            println!("[init] desktop-shell PID = {}", child.id());
            Some(child)
        }
        Err(e) => {
            eprintln!("[init] desktop-shell spawn failed: {} (continuing)", e);
            None
        }
    };

    println!("[init] spawning /bin/geulos-echo-app ...");
    let echo_app = match Command::new("/bin/geulos-echo-app").arg("127.0.0.1:5550").spawn() {
        Ok(child) => {
            println!("[init] echo-app PID = {}", child.id());
            Some(child)
        }
        Err(e) => {
            eprintln!("[init] echo-app spawn failed: {} (continuing)", e);
            None
        }
    };

    println!("[init] spawning /bin/geulos-vm-compositor ...");
    let skeleton = match Command::new("/bin/geulos-vm-compositor").arg("127.0.0.1:5550").spawn() {
        Ok(child) => {
            println!("[init] vm-compositor PID = {}", child.id());
            Some(child)
        }
        Err(e) => {
            eprintln!("[init] vm-compositor spawn failed: {} (continuing)", e);
            None
        }
    };

    Ok(SpawnedProcesses { server, desktop_shell, echo_app, skeleton })
}
