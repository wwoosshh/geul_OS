//! server-host + echo-app 자식 프로세스 spawn.

use std::process::{Child, Command};

/// 우리가 띄운 자식 프로세스들.
pub struct SpawnedProcesses {
    pub server: Child,
    pub echo_app: Option<Child>,
}

/// 1) geulosd (server-host)를 0.0.0.0:5550에 listen으로 띄움.
/// 2) 1초 대기 후 echo-app을 spawn (server-host listening 준비 시간 부여).
/// 3) echo-app spawn 실패해도 진행 — server-host만 살아있어도 외부 ai-bridge가 들어와 작동.
pub fn spawn_all() -> Result<SpawnedProcesses, String> {
    println!("[init] spawning /bin/geulosd ...");
    let server = Command::new("/bin/geulosd")
        .arg("0.0.0.0:5550")
        .spawn()
        .map_err(|e| format!("spawn geulosd: {}", e))?;
    println!("[init] geulosd PID = {}", server.id());

    // server-host가 listen 시작할 시간
    std::thread::sleep(std::time::Duration::from_secs(1));

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

    Ok(SpawnedProcesses { server, echo_app })
}
