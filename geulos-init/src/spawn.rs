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

    // desktop-shell이 Desktop/FileTree/Explorer/Cli 등을 모두 mount할 시간을 준다.
    // 없으면 컴포지터 startup query↔구독 등록 사이 틈에 Desktop/Explorer가 mount되어
    // 둘 다 놓침(race) → Desktop 루트 부재로 layout이 echo-app fallback. 호스트 launcher가
    // desktop-shell "subscribed" 로그를 기다리는 것과 동일 목적의 VM판(고정 지연).
    std::thread::sleep(std::time::Duration::from_secs(3));

    // echo-app은 데스크톱 시나리오에 불필요 — spawn 안 함 (Desktop 우선 layout과 경쟁 방지 +
    // 트리 단순화). M6 ai-bridge 데모가 필요하면 별도로 재추가.
    let echo_app: Option<Child> = None;

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
