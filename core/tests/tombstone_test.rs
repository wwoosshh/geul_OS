//! KI-011 — tombstone 동작 회귀 테스트.
//!
//! `emit_destroyed`는 객체를 *제거*하지 않고 `destroyed: true` 플래그만 설정.
//! 결과: query/roots에서 사라지고, invoke/set_state는 NotFound, get은 그대로 반환.

use geulos_core::{std_types, ActorId, ObjectServer, Query, TypeUri};
use serde_json::json;

#[test]
fn destroyed_object_disappears_from_query_by_type() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let txt = std_types::text(owner.clone(), "alive");
    let id = server.mount(txt).unwrap();

    let type_uri = TypeUri::parse("aios.std/Text@1").unwrap();
    assert_eq!(server.query(&Query::by_type(type_uri.clone())).len(), 1);

    server.emit_destroyed(&owner, &id);
    assert_eq!(
        server.query(&Query::by_type(type_uri)).len(),
        0,
        "destroyed object must not show up in query"
    );
}

#[test]
fn destroyed_object_disappears_from_roots() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let c = std_types::container(owner.clone());
    let id = server.mount(c).unwrap();

    assert!(server.roots().contains(&id));
    server.emit_destroyed(&owner, &id);
    assert!(!server.roots().contains(&id), "destroyed root must not appear");
}

#[test]
fn destroyed_object_can_still_be_fetched_by_id() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let txt = std_types::text(owner.clone(), "x");
    let id = server.mount(txt).unwrap();
    server.emit_destroyed(&owner, &id);

    let obj = server.get(&id).expect("get should still return the tombstone");
    assert!(obj.destroyed, "tombstone flag must be set");
}

#[test]
fn invoke_on_destroyed_returns_not_found() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let btn = std_types::button(owner.clone(), "OK");
    let id = server.mount(btn).unwrap();
    server.emit_destroyed(&owner, &id);

    let result = server.invoke(&owner, &id, "press", json!(null));
    assert!(result.is_err(), "invoke on destroyed must fail");
}

#[test]
fn set_state_on_destroyed_returns_not_found() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let txt = std_types::text(owner.clone(), "before");
    let id = server.mount(txt).unwrap();
    server.emit_destroyed(&owner, &id);

    let result = server.set_state(&owner, &id, "content", json!("after"));
    assert!(result.is_err(), "set_state on destroyed must fail");
}

#[test]
fn children_of_query_excludes_destroyed_children() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let mut c = std_types::container(owner.clone());
    let txt = std_types::text(owner.clone(), "child");
    let btn = std_types::button(owner.clone(), "B");
    let c_id = c.id;
    let txt_id = txt.id;
    let btn_id = btn.id;
    c.children.push(txt_id);
    c.children.push(btn_id);

    server.mount_with_descendants(c, vec![txt, btn]).unwrap();
    assert_eq!(server.query(&Query::children_of(c_id)).len(), 2);

    server.emit_destroyed(&owner, &txt_id);
    let live_children = server.query(&Query::children_of(c_id));
    assert_eq!(live_children.len(), 1);
    assert!(live_children.contains(&btn_id));
    assert!(!live_children.contains(&txt_id));
}
