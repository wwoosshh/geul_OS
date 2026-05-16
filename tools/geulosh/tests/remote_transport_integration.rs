//! 통합 테스트: geulosh RemoteTransport + RemoteShell — in-process 서버로 동작 검증.
//!
//! 서브프로세스를 직접 스폰하면 Windows에서 경쟁 조건이 생길 수 있으므로,
//! 동일 프로세스 안에서 server-host의 run_listener를 tokio::spawn으로 띄우고
//! RemoteTransport로 연결한다.

use geulos_proto::Role;
use geulos_server_host::run_listener;
use geulos_shell::transport::{RemoteShell, RemoteTransport};
use tokio::net::TcpListener;

/// 임의 포트에 서버를 spawn하고 RemoteTransport를 연결해 반환.
async fn spawn_server_and_connect() -> RemoteShell {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("TcpListener::bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(run_listener(listener));

    let transport = RemoteTransport::connect(&addr.to_string(), Role::Ai)
        .await
        .expect("RemoteTransport::connect");
    RemoteShell::new(transport)
}

#[tokio::test]
async fn remote_connect_handshake_succeeds() {
    // 연결 자체가 성공하면 actor_id가 ai: 접두사를 가진다.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));

    let t = RemoteTransport::connect(&addr.to_string(), Role::Ai).await.unwrap();
    assert!(t.actor_id.starts_with("ai:"), "actor_id should start with 'ai:', got: {}", t.actor_id);
}

#[tokio::test]
async fn remote_mount_text_returns_label() {
    let mut rsh = spawn_server_and_connect().await;
    let result = rsh.mount_text("hello remote").await;
    assert!(result.is_ok(), "mount_text failed: {:?}", result);
    let msg = result.unwrap();
    assert!(msg.contains("#1"), "expected label #1 in: {}", msg);
    assert!(msg.contains("Text") || msg.contains("aios.std"), "expected type in: {}", msg);
}

#[tokio::test]
async fn remote_mount_button_returns_label() {
    let mut rsh = spawn_server_and_connect().await;
    let result = rsh.mount_button("OK").await;
    assert!(result.is_ok(), "mount_button failed: {:?}", result);
    let msg = result.unwrap();
    assert!(msg.contains("#1"), "expected label #1 in: {}", msg);
}

#[tokio::test]
async fn remote_ls_after_mount_lists_objects() {
    let mut rsh = spawn_server_and_connect().await;

    // 두 객체 마운트
    rsh.mount_text("first").await.expect("mount_text first");
    rsh.mount_button("Second").await.expect("mount_button Second");

    // ls는 QueryResult를 반환; 서버의 ObjectServer에 등록된 객체를 돌려줌.
    // 단, Query::ByOwner(user:local)이므로 ai: actor가 마운트한 객체는 안 나올 수도 있음.
    // M2 한계 — 그래도 응답 자체는 성공해야 함.
    let ls_result = rsh.ls().await;
    assert!(ls_result.is_ok(), "ls failed: {:?}", ls_result);
}

#[tokio::test]
async fn remote_execute_dispatch_mount_and_ls() {
    use geulos_shell::transport::RemoteOutcome;

    let mut rsh = spawn_server_and_connect().await;

    // execute 인터페이스 — mount text
    let out = rsh.execute(r#"mount text "dispatch test""#).await;
    assert!(matches!(out, RemoteOutcome::Output(_)), "expected Output, got {:?}", out);

    // execute 인터페이스 — ls
    let out2 = rsh.execute("ls").await;
    assert!(matches!(out2, RemoteOutcome::Output(_)), "expected Output from ls, got {:?}", out2);
}

#[tokio::test]
async fn remote_execute_invoke_after_mount() {
    use geulos_shell::transport::RemoteOutcome;

    let mut rsh = spawn_server_and_connect().await;

    // mount button — 서버의 ObjectServer에 등록됨
    let out = rsh.execute(r#"mount button "ClickMe""#).await;
    assert!(matches!(out, RemoteOutcome::Output(_)), "mount failed: {:?}", out);

    // invoke #1 press — ai actor가 user:local 소유 button 을 누름
    // 서버 측에서는 permission error (InvokeError[permission]) 가 올 수 있음.
    // 중요한 것은 wire 왕복이 성공하는 것 (OutCome::Error 도 wire 통신이 성공한 것).
    let out2 = rsh.execute("invoke #1 press").await;
    assert!(
        matches!(out2, RemoteOutcome::Output(_) | RemoteOutcome::Error(_)),
        "expected wire response, got {:?}",
        out2
    );
}

#[tokio::test]
async fn remote_execute_help_returns_output() {
    use geulos_shell::transport::RemoteOutcome;

    let mut rsh = spawn_server_and_connect().await;
    let out = rsh.execute("help").await;
    assert!(matches!(out, RemoteOutcome::Output(_)), "expected Output, got {:?}", out);
    if let RemoteOutcome::Output(s) = out {
        assert!(s.contains("mount"), "help should mention mount: {}", s);
    }
}

#[tokio::test]
async fn remote_execute_quit_returns_quit() {
    use geulos_shell::transport::RemoteOutcome;

    let mut rsh = spawn_server_and_connect().await;
    let out = rsh.execute("quit").await;
    assert!(matches!(out, RemoteOutcome::Quit), "expected Quit, got {:?}", out);
}
