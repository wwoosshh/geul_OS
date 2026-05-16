//! GeulOS server-host: ObjectServer 액터 + 비동기 TCP 리스너.

pub mod actor;
pub mod connection;
pub mod dispatch;

pub use actor::{ObjectServerActor, ObjectServerHandle};

use tokio::net::TcpListener;

/// 주어진 TcpListener에서 클라이언트 연결을 accept하고 각각 task로 처리.
///
/// 액터는 함수 안에서 한 번 spawn되어 모든 연결이 공유.
pub async fn run_listener(listener: TcpListener) {
    let handle = ObjectServerActor::spawn();
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let handle = handle.clone();
                tokio::spawn(async move {
                    connection::handle_connection(stream, handle).await;
                });
            }
            Err(e) => {
                eprintln!("accept error: {}", e);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}
