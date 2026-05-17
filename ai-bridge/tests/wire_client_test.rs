//! WireClient 통합 테스트 — 실제 server-host와 TCP로 통신.

use geulos_ai_bridge::WireClient;
use geulos_core::{std_types, AclEffect, AclEntry, ActorId, ActorPattern, MethodPattern};
use geulos_proto::EventKindFilterWire;
use geulos_server_host::run_listener;

async fn spawn_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(run_listener(listener));
    addr.to_string()
}

#[tokio::test]
async fn connect_as_ai_returns_session() {
    let addr = spawn_server().await;
    let client = WireClient::connect_as_ai(&addr).await.unwrap();
    assert!(client.actor_id().starts_with("ai:"));
}

#[tokio::test]
async fn query_by_type_returns_object_ids() {
    let addr = spawn_server().await;

    let mut mounter = WireClient::connect_as_ai(&addr).await.unwrap();
    let txt = std_types::text(ActorId::local_user(), "hi");
    let txt_id = txt.id;
    mounter.mount(txt).await.unwrap();

    let mut client = WireClient::connect_as_ai(&addr).await.unwrap();
    let ids = client.query_by_type("aios.std/Text@1").await.unwrap();
    assert!(ids.iter().any(|s| s == &txt_id.to_string()));
}

#[tokio::test]
async fn get_object_returns_full_data() {
    let addr = spawn_server().await;

    let mut mounter = WireClient::connect_as_ai(&addr).await.unwrap();
    let btn = std_types::button(ActorId::local_user(), "OK");
    let btn_id = btn.id.to_string();
    mounter.mount(btn).await.unwrap();

    let mut client = WireClient::connect_as_ai(&addr).await.unwrap();
    let val = client.get_object(&btn_id).await.unwrap();
    assert_eq!(val["type_uri"], "aios.std/Button@1");
}

#[tokio::test]
async fn invoke_method_against_wildcard_acl_succeeds() {
    let addr = spawn_server().await;

    let mut mounter = WireClient::connect_as_ai(&addr).await.unwrap();
    let mut btn = std_types::button(ActorId::local_user(), "OK");
    btn.acl.push(AclEntry {
        actor: ActorPattern::Wildcard,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
    let btn_id = btn.id.to_string();
    mounter.mount(btn).await.unwrap();

    let mut client = WireClient::connect_as_ai(&addr).await.unwrap();
    let event_id = client.invoke(&btn_id, "press", serde_json::Value::Null).await.unwrap();
    assert!(event_id.starts_with("ev:"));
}

#[tokio::test]
async fn subscribe_and_drain_receive_event() {
    let addr = spawn_server().await;

    let mut mounter = WireClient::connect_as_ai(&addr).await.unwrap();
    let mut btn = std_types::button(ActorId::local_user(), "OK");
    btn.acl.push(AclEntry {
        actor: ActorPattern::Wildcard,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
    let btn_id = btn.id.to_string();
    mounter.mount(btn).await.unwrap();

    let mut sub_client = WireClient::connect_as_ai(&addr).await.unwrap();
    let sub_id = sub_client.subscribe(&btn_id, &[EventKindFilterWire::Invoke]).await.unwrap();

    let mut invoker = WireClient::connect_as_ai(&addr).await.unwrap();
    invoker.invoke(&btn_id, "press", serde_json::Value::Null).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let events = sub_client.drain(&sub_id).await.unwrap();
    assert!(!events.is_empty(), "expected at least one event after press");
}
