use geulos_core::{std_types, ActorId, EventKindFilter, ObjectServer, TypeUri};
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

/// KI-004 해소 — type-level subscribe로 등록한 구독은 *그 type의 모든 객체* 이벤트를
/// 받는다. 특히 *subscribe 후 mount된* 객체도 즉시 Created 이벤트가 도달해야 한다
/// (이게 KI-004의 핵심 — startup 시점 이후 mount된 객체를 컴포지터가 못 보는 문제).
#[test]
fn subscribe_by_type_receives_created_for_future_mounts() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let folder_type = TypeUri::parse("aios.std/Folder@1").unwrap();
    let sub =
        server.subscribe_by_type(owner.clone(), folder_type, vec![EventKindFilter::Lifecycle]);

    // 구독 *후*에 mount — KI-004의 시나리오.
    let _id = server.mount(std_types::folder(owner.clone(), "/a", "a", 0)).unwrap();

    let drained = server.drain_subscription(sub);
    assert_eq!(drained.len(), 1, "type-level 구독이 Created를 받지 못함");
}

/// type-level 구독은 *그 type만* 받는다 — 다른 type의 mount는 무관.
#[test]
fn subscribe_by_type_filters_by_type() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let folder_type = TypeUri::parse("aios.std/Folder@1").unwrap();
    let sub =
        server.subscribe_by_type(owner.clone(), folder_type, vec![EventKindFilter::Lifecycle]);

    // 다른 type (File) mount — 받으면 안 됨.
    server.mount(std_types::file(owner.clone(), "/", "x", "text/plain", 0)).unwrap();
    assert_eq!(server.drain_subscription(sub).len(), 0);

    // 매칭 type (Folder) mount — 받아야 함.
    server.mount(std_types::folder(owner.clone(), "/a", "a", 0)).unwrap();
    assert_eq!(server.drain_subscription(sub).len(), 1);
}

/// type-level + ID-based 두 구독자가 같은 객체의 *같은 이벤트*를 둘 다 받는다.
#[test]
fn subscribe_by_type_and_by_id_both_receive_state_set() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    // Button: state 변경에 press가 invoke가 아니라 set_state로 사용 가능. 단순화.
    let button_type = TypeUri::parse("aios.std/Button@1").unwrap();
    let sub_type = server.subscribe_by_type(
        owner.clone(),
        button_type,
        vec![EventKindFilter::Lifecycle, EventKindFilter::StateSet],
    );

    let id = server.mount(std_types::button(owner.clone(), "OK")).unwrap();
    let sub_id = server.subscribe(owner.clone(), id, vec![EventKindFilter::StateSet]);

    // StateSet 이벤트 — type 구독자 + id 구독자 둘 다 받아야 함.
    server.set_state(&owner, &id, "label", json!("Cancel")).unwrap();

    assert_eq!(server.drain_subscription(sub_type).len(), 2, "Created + StateSet");
    assert_eq!(server.drain_subscription(sub_id).len(), 1, "StateSet만");
}

/// Destroyed 이벤트도 type-level 구독자에게 전달된다 (tombstone 정책 + ByType 호환).
#[test]
fn subscribe_by_type_receives_destroyed() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let folder_type = TypeUri::parse("aios.std/Folder@1").unwrap();
    let sub =
        server.subscribe_by_type(owner.clone(), folder_type, vec![EventKindFilter::Lifecycle]);

    let id = server.mount(std_types::folder(owner.clone(), "/a", "a", 0)).unwrap();
    server.emit_destroyed(&owner, &id);

    let drained = server.drain_subscription(sub);
    assert_eq!(drained.len(), 2, "Created + Destroyed");
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
