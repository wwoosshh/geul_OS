use geulos_core::{AclEffect, ActorId};
use geulos_echo_app::{build_ui, next_count};

#[test]
fn build_ui_returns_3_objects_with_parent_relations() {
    let owner = ActorId::new_app("echo");
    let (container, text, button) = build_ui(owner.clone());

    assert_eq!(container.children.len(), 2);
    assert_eq!(text.parent, Some(container.id));
    assert_eq!(button.parent, Some(container.id));
}

#[test]
fn next_count_increments() {
    let (n, s) = next_count(0);
    assert_eq!(n, 1);
    assert_eq!(s, "count: 1");
}

#[test]
fn build_ui_button_has_wildcard_allow_acl() {
    let owner = ActorId::new_app("echo");
    let (_container, _text, button) = build_ui(owner);

    // button must have at least one wildcard Allow ACL entry so arbitrary
    // external clients can invoke `press` (Task 8 requirement).
    let has_wildcard_allow = button.acl.iter().any(|entry| {
        use geulos_core::ActorPattern;
        matches!(entry.actor, ActorPattern::Wildcard) && entry.effect == AclEffect::Allow
    });
    assert!(
        has_wildcard_allow,
        "button must have a wildcard Allow ACL entry for Task 8 external press"
    );
}
