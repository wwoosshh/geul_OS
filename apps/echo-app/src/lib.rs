//! echo-app의 핵심 로직 — UI 트리 구성 + 이벤트 반응.

use geulos_core::{std_types, AclEffect, AclEntry, ActorId, ActorPattern, MethodPattern, Object};

/// echo-app의 초기 UI 트리를 만든다.
///
/// 반환값: (container, text, button) — 모두 같은 owner.
///
/// button에는 wildcard Allow ACL 항목이 포함되어 있으므로, 임의의
/// 외부 클라이언트가 press를 호출할 수 있다 (Task 8 acceptance 요구사항).
pub fn build_ui(owner: ActorId) -> (Object, Object, Object) {
    let mut container = std_types::container(owner.clone());
    let mut text = std_types::text(owner.clone(), "count: 0");
    let mut button = std_types::button(owner, "+1");

    container.children.push(text.id);
    container.children.push(button.id);
    text.parent = Some(container.id);
    button.parent = Some(container.id);

    // 외부 클라이언트가 press를 호출할 수 있도록 wildcard ACL 추가.
    // M3에서는 echo-app이 명시적으로 "내 버튼은 누구나 누를 수 있다"고 선언한다.
    button.acl.push(AclEntry {
        actor: ActorPattern::Wildcard,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });

    (container, text, button)
}

/// 현재 count 값으로부터 다음 count 값과 새 텍스트 컨텐츠를 만든다.
pub fn next_count(current: i64) -> (i64, String) {
    let next = current + 1;
    (next, format!("count: {}", next))
}
