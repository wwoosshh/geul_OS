//! geulosd: GeulOS 객체 서버 데몬.

use geulos_server_host::run_listener;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:5550".to_string());
    let listener = TcpListener::bind(&addr).await.expect("bind failed");
    println!("geulosd listening on {}", addr);
    run_listener(listener).await;
}
