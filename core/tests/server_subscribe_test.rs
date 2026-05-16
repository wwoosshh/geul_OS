use geulos_core::{std_types, ActorId, EventKindFilter, ObjectServer};
use serde_json::json;

#[test]
fn subscribe_returns_subscription_id() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let id = server.mount(std_types::button(owner.clone(), "OK")).unwrap();

    let sub_id = server.subscribe(owner.clone(), id, vec![EventKindFilter::Invoke]);
    assert_ne!(sub_id.as_u64(), 0);
}

#[test]
fn subscribe_receives_invoke_events() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let id = server.mount(std_types::button(owner.clone(), "OK")).unwrap();

    let sub_id = server.subscribe(owner.clone(), id, vec![EventKindFilter::Invoke]);

    // mount는 Lifecycle 이벤트를 발행했지만 구독은 Invoke만 필터링하므로 받지 않음.
    let drained = server.drain_subscription(sub_id);
    assert_eq!(drained.len(), 0);

    server.invoke(&owner, &id, "press", json!(null)).unwrap();
    let drained = server.drain_subscription(sub_id);
    assert_eq!(drained.len(), 1);
}

#[test]
fn subscribe_filters_by_target() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let a = server.mount(std_types::button(owner.clone(), "A")).unwrap();
    let b = server.mount(std_types::button(owner.clone(), "B")).unwrap();

    let sub_a = server.subscribe(owner.clone(), a, vec![EventKindFilter::Invoke]);

    server.invoke(&owner, &b, "press", json!(null)).unwrap();
    assert_eq!(server.drain_subscription(sub_a).len(), 0); // b의 invoke는 무관

    server.invoke(&owner, &a, "press", json!(null)).unwrap();
    assert_eq!(server.drain_subscription(sub_a).len(), 1);
}

#[test]
fn subscribe_lifecycle_filter() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let id = server.mount(std_types::container(owner.clone())).unwrap();

    // mount 후 구독 — Lifecycle Created 이벤트는 이미 발행되었으므로 못 받음.
    let sub = server.subscribe(owner, id, vec![EventKindFilter::Lifecycle]);
    let drained = server.drain_subscription(sub);
    assert_eq!(drained.len(), 0);
}

#[test]
fn unsubscribe_stops_delivery() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let id = server.mount(std_types::button(owner.clone(), "OK")).unwrap();
    let sub = server.subscribe(owner.clone(), id, vec![EventKindFilter::Invoke]);

    server.unsubscribe(sub);
    server.invoke(&owner, &id, "press", json!(null)).unwrap();
    // 이미 unsubscribe 후 drain 시도 — empty여야 함 (구독이 없으므로)
    assert_eq!(server.drain_subscription(sub).len(), 0);
}
