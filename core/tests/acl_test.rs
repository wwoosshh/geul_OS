use geulos_core::{AclEffect, AclEntry, ActorId, ActorPattern, ArgSpec, MethodPattern, MethodSig};

#[test]
fn method_sig_constructs() {
    let sig =
        MethodSig::new("press").with_arg(ArgSpec::new("force", "integer")).with_returns("void");
    assert_eq!(sig.name(), "press");
    assert_eq!(sig.args().len(), 1);
    assert_eq!(sig.returns(), Some("void"));
}

#[test]
fn acl_entry_exact_actor_exact_method_matches() {
    let actor = ActorId::local_user();
    let entry = AclEntry {
        actor: ActorPattern::Exact(actor.clone()),
        method: MethodPattern::Exact("press".to_string()),
        effect: AclEffect::Allow,
    };
    assert!(entry.matches(&actor, "press"));
    assert!(!entry.matches(&actor, "release"));
    assert!(!entry.matches(&ActorId::new_ai_session(), "press"));
}

#[test]
fn acl_entry_wildcard_method_matches_anything() {
    let actor = ActorId::local_user();
    let entry = AclEntry {
        actor: ActorPattern::Exact(actor.clone()),
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    };
    assert!(entry.matches(&actor, "press"));
    assert!(entry.matches(&actor, "anything"));
}

#[test]
fn acl_entry_serde_round_trip() {
    let entry = AclEntry {
        actor: ActorPattern::Exact(ActorId::local_user()),
        method: MethodPattern::Exact("press".to_string()),
        effect: AclEffect::Allow,
    };
    let s = serde_json::to_string(&entry).unwrap();
    let back: AclEntry = serde_json::from_str(&s).unwrap();
    assert_eq!(entry.effect, back.effect);
}
