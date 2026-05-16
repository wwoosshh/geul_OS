use geulos_core::{std_types, ActorId, EventKind, ObjectServer};
use serde_json::json;

#[test]
fn invoke_existing_method_succeeds_for_owner() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let btn = std_types::button(owner.clone(), "OK");
    let btn_id = server.mount(btn).unwrap();

    let result = server.invoke(&owner, &btn_id, "press", json!({}));
    assert!(result.is_ok());
}

#[test]
fn invoke_emits_invoke_event() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let btn = std_types::button(owner.clone(), "OK");
    let btn_id = server.mount(btn).unwrap();

    let log_len_before = server.bus().log().len();
    server.invoke(&owner, &btn_id, "press", json!(null)).unwrap();

    assert_eq!(server.bus().log().len(), log_len_before + 1);
    let last = server.bus().log().last().unwrap();
    assert_eq!(last.actor, owner);
    assert_eq!(last.target, btn_id);
    match &last.kind {
        EventKind::Invoke { method, .. } => assert_eq!(method, "press"),
        _ => panic!("expected Invoke event"),
    }
}

#[test]
fn invoke_denied_for_non_owner_without_acl() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let intruder = ActorId::new_ai_session();
    let btn = std_types::button(owner.clone(), "OK");
    let btn_id = server.mount(btn).unwrap();

    let result = server.invoke(&intruder, &btn_id, "press", json!({}));
    assert!(result.is_err());
}

#[test]
fn invoke_nonexistent_object_errors() {
    use geulos_core::ObjectId;
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let bogus = ObjectId::new();
    let result = server.invoke(&owner, &bogus, "press", json!({}));
    assert!(result.is_err());
}

#[test]
fn invoke_unknown_method_errors() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let btn = std_types::button(owner.clone(), "OK");
    let btn_id = server.mount(btn).unwrap();

    let result = server.invoke(&owner, &btn_id, "self_destruct", json!({}));
    assert!(result.is_err());
}
