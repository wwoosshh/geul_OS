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
fn build_ui_button_has_explicit_allow_acl() {
    let owner = ActorId::new_app("echo");
    let (_container, _text, button) = build_ui(owner);

    // M11: wildcard 제거 — button은 SystemCompositor / AiSession / App("echo-app") 에
    // 대해 press Allow 항목을 가져야 한다.
    // 외부 AI client(Role::Ai)가 press를 호출하면 AiSession 패턴이 매칭된다.
    let has_explicit_allow = button.acl.iter().any(|entry| {
        use geulos_core::ActorPattern;
        matches!(
            entry.actor,
            ActorPattern::SystemCompositor | ActorPattern::AiSession | ActorPattern::App(_)
        ) && entry.effect == AclEffect::Allow
    });
    assert!(
        has_explicit_allow,
        "button must have explicit Allow ACL entries for SystemCompositor/AiSession/App"
    );
}
