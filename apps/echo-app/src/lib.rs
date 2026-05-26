//! echo-app의 핵심 로직 — UI 트리 구성 + 이벤트 반응.

use geulos_core::{std_types, AclEffect, AclEntry, ActorId, ActorPattern, MethodPattern, Object};

/// echo-app의 초기 UI 트리를 만든다.
///
/// 반환값: (container, text, button) — 모두 같은 owner.
///
/// button에는 명시적 actor ACL 항목이 포함되어 있으므로,
/// SystemCompositor / AiSession / echo-app 자신이 press를 호출할 수 있다.
/// M11: wildcard 영구 제거 — 명시적 enumeration으로 교체.
pub fn build_ui(owner: ActorId) -> (Object, Object, Object) {
    let mut container = std_types::container(owner.clone());
    let mut text = std_types::text(owner.clone(), "count: 0");
    let mut button = std_types::button(owner, "+1");

    container.children.push(text.id);
    container.children.push(button.id);
    text.parent = Some(container.id);
    button.parent = Some(container.id);

    add_acl(&mut button);

    (container, text, button)
}

/// button ACL 설정.
///
/// M11: wildcard 제거. 외부 client는 SystemCompositor + AI + echo-app 자신 모두 press 가능.
/// 명시적 enumeration — 'Wildcard 폐지' 정책 일관성 유지.
fn add_acl(obj: &mut Object) {
    for actor_pat in [
        ActorPattern::SystemCompositor,
        ActorPattern::AiSession,
        ActorPattern::App("echo-app".to_string()),
    ] {
        obj.acl.push(AclEntry {
            actor: actor_pat,
            method: MethodPattern::Exact("press".to_string()),
            effect: AclEffect::Allow,
        });
    }
    // 자기 set_state.
    obj.acl.push(AclEntry {
        actor: ActorPattern::App("echo-app".to_string()),
        method: MethodPattern::SetState,
        effect: AclEffect::Allow,
    });
}

/// 현재 count 값으로부터 다음 count 값과 새 텍스트 컨텐츠를 만든다.
pub fn next_count(current: i64) -> (i64, String) {
    let next = current + 1;
    (next, format!("count: {}", next))
}
