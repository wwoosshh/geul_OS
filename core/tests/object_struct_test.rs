use geulos_core::{AclEntry, ActorId, ActorPattern, MethodPattern, MethodSig, Object,
                  AclEffect, TypeUri};
use serde_json::json;

#[test]
fn object_constructs_with_required_fields() {
    let owner = ActorId::local_user();
    let type_uri = TypeUri::parse("aios.std/Container@1").unwrap();
    let obj = Object::new(type_uri.clone(), owner.clone());

    assert_eq!(obj.type_uri, type_uri);
    assert_eq!(obj.owner, owner);
    assert!(obj.parent.is_none());
    assert!(obj.children.is_empty());
    assert!(obj.props.is_empty());
    assert!(obj.state.is_empty());
    assert!(obj.methods.is_empty());
    assert!(obj.acl.is_empty());
}

#[test]
fn object_can_set_state_and_get() {
    let mut obj = Object::new(
        TypeUri::parse("aios.std/Text@1").unwrap(),
        ActorId::local_user(),
    );
    obj.set_state("content", json!("hello"));
    assert_eq!(obj.state.get("content"), Some(&json!("hello")));
}

#[test]
fn object_can_attach_acl_and_check() {
    let actor = ActorId::local_user();
    let mut obj = Object::new(
        TypeUri::parse("aios.std/Button@1").unwrap(),
        actor.clone(),
    );
    obj.acl.push(AclEntry {
        actor: ActorPattern::Exact(actor.clone()),
        method: MethodPattern::Exact("press".to_string()),
        effect: AclEffect::Allow,
    });
    assert!(obj.is_allowed(&actor, "press"));
    assert!(!obj.is_allowed(&actor, "explode"));
}

#[test]
fn object_owner_implicit_allow_all() {
    // 소유자는 별도 ACL 없이도 모든 메서드가 허용.
    let owner = ActorId::local_user();
    let obj = Object::new(
        TypeUri::parse("aios.std/Button@1").unwrap(),
        owner.clone(),
    );
    assert!(obj.is_allowed(&owner, "any_method"));
}

#[test]
fn object_default_deny_for_others() {
    let owner = ActorId::local_user();
    let other = ActorId::new_ai_session();
    let obj = Object::new(
        TypeUri::parse("aios.std/Button@1").unwrap(),
        owner,
    );
    // 소유자가 아니고 ACL도 없으면 거부.
    assert!(!obj.is_allowed(&other, "press"));
}

#[test]
fn object_explicit_deny_overrides_allow() {
    let actor = ActorId::local_user();
    let mut obj = Object::new(
        TypeUri::parse("aios.std/Button@1").unwrap(),
        ActorId::new_ai_session(), // 다른 owner
    );
    // 와일드카드로 허용
    obj.acl.push(AclEntry {
        actor: ActorPattern::Wildcard,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
    // 특정 액터+메서드는 deny
    obj.acl.push(AclEntry {
        actor: ActorPattern::Exact(actor.clone()),
        method: MethodPattern::Exact("press".to_string()),
        effect: AclEffect::Deny,
    });
    assert!(!obj.is_allowed(&actor, "press"));
    assert!(obj.is_allowed(&actor, "anything_else"));
}

#[test]
fn object_serde_round_trip() {
    let mut obj = Object::new(
        TypeUri::parse("aios.std/Container@1").unwrap(),
        ActorId::local_user(),
    );
    obj.set_state("title", json!("test"));
    obj.methods.push(MethodSig::new("show"));

    let s = serde_json::to_string(&obj).unwrap();
    let back: Object = serde_json::from_str(&s).unwrap();
    assert_eq!(obj.id, back.id);
    assert_eq!(obj.type_uri, back.type_uri);
    assert_eq!(obj.state, back.state);
}
