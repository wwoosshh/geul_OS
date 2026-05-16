use geulos_core::{std_types, ActorId, EventKind, ObjectServer};
use serde_json::json;

#[test]
fn set_state_by_owner_succeeds() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let txt = std_types::text(owner.clone(), "initial");
    let id = server.mount(txt).unwrap();

    let ev = server
        .set_state(&owner, &id, "content", json!("updated"))
        .expect("owner should be allowed");
    assert!(ev.as_u64() > 0);

    let obj = server.get(&id).unwrap();
    assert_eq!(obj.state.get("content"), Some(&json!("updated")));
}

#[test]
fn set_state_emits_state_set_event() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let txt = std_types::text(owner.clone(), "x");
    let id = server.mount(txt).unwrap();

    let log_len_before = server.bus().log().len();
    server.set_state(&owner, &id, "content", json!("y")).unwrap();

    let log = server.bus().log();
    assert_eq!(log.len(), log_len_before + 1);
    match &log.last().unwrap().kind {
        EventKind::StateSet { key, value } => {
            assert_eq!(key, "content");
            assert_eq!(value, &json!("y"));
        }
        _ => panic!("expected StateSet event"),
    }
}

#[test]
fn set_state_denied_for_non_owner() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let intruder = ActorId::new_ai_session();
    let txt = std_types::text(owner, "x");
    let id = server.mount(txt).unwrap();

    let result = server.set_state(&intruder, &id, "content", json!("hacked"));
    assert!(result.is_err());
}

#[test]
fn set_state_nonexistent_object_errors() {
    use geulos_core::ObjectId;
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let bogus = ObjectId::new();
    let result = server.set_state(&owner, &bogus, "content", json!("x"));
    assert!(result.is_err());
}
