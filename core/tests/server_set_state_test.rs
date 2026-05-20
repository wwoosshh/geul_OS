use geulos_core::{
    std_types, AclEffect, AclEntry, ActorId, ActorPattern, EventKind, MethodPattern, ObjectServer,
};
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

// T8.19: 컴포지터가 desktop-shell 소유 객체의 scroll_y를 갱신해야 함. wildcard ACL이
// 있으면 non-owner도 set_state 허용 (invoke ACL 정책과 일관 — KI-001 임시).

#[test]
fn set_state_allowed_via_wildcard_acl_for_non_owner() {
    let mut server = ObjectServer::new();
    let owner_actor = ActorId::new_app("test-owner");
    let other_actor = ActorId::system_compositor();
    let mut obj = std_types::cli(owner_actor.clone());
    // wildcard ACL 추가 — 다른 actor도 set_state 가능해야 한다.
    obj.acl.push(AclEntry {
        actor: ActorPattern::Wildcard,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
    let id = server.mount(obj).unwrap();
    let result = server.set_state(&other_actor, &id, "scroll_y", json!(42));
    assert!(result.is_ok(), "wildcard ACL이 있으면 non-owner set_state 허용");

    // 실제 state도 갱신되었는지 확인.
    let stored = server.get(&id).unwrap().state.get("scroll_y").cloned();
    assert_eq!(stored, Some(json!(42)));
}

#[test]
fn set_state_rejected_for_non_owner_without_wildcard_acl() {
    use geulos_core::SetStateError;
    let mut server = ObjectServer::new();
    let owner_actor = ActorId::new_app("test-owner");
    let other_actor = ActorId::system_compositor();
    // ACL 비어있음 — non-owner는 거부되어야 한다.
    let obj = std_types::cli(owner_actor.clone());
    let id = server.mount(obj).unwrap();
    let result = server.set_state(&other_actor, &id, "scroll_y", json!(42));
    assert!(matches!(result, Err(SetStateError::PermissionDenied { .. })));
}
