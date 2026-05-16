//! geulosd: GeulOS 객체 서버 데몬.
//!
//! Task 4: 액터만 spawn하고 즉시 종료. Task 6 이후 TCP 리스너 추가.

use geulos_server_host::ObjectServerActor;

#[tokio::main]
async fn main() {
    let _handle = ObjectServerActor::spawn();
    println!("geulosd actor spawned. (TCP listener: Task 6+)");
}
