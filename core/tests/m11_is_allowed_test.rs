//! M11 — Object::is_allowed 신규 시그니처 회귀 테스트.

use geulos_core::{
    object::{AclEffect, AclEntry, AclOp, ActorId, ActorPattern, GrantContext, MethodPattern},
    std_types,
};
use std::path::Path;

struct EmptyGrants;
impl GrantContext for EmptyGrants {
    fn is_granted(&self, _actor: &ActorId, _path: &Path) -> bool {
        false
    }
}

struct FixedGrants {
    actor: ActorId,
    path: std::path::PathBuf,
}
impl GrantContext for FixedGrants {
    fn is_granted(&self, actor: &ActorId, path: &Path) -> bool {
        actor == &self.actor && path == self.path
    }
}

#[test]
fn invoke_op_matches_method_pattern() {
    let owner = ActorId::local_user();
    let mut obj = std_types::folder(owner.clone(), "/x", "x", 0);
    obj.acl.push(AclEntry {
        actor: ActorPattern::SystemCompositor,
        method: MethodPattern::Exact("list".to_string()),
        effect: AclEffect::Allow,
    });
    let g = EmptyGrants;
    assert!(obj.is_allowed(&ActorId::system_compositor(), AclOp::Invoke("list".to_string()), &g));
    assert!(!obj.is_allowed(
        &ActorId::system_compositor(),
        AclOp::Invoke("delete".to_string()),
        &g
    ));
}

#[test]
fn set_state_op_only_matches_set_state_pattern() {
    let owner = ActorId::local_user();
    let mut obj = std_types::folder(owner.clone(), "/x", "x", 0);
    obj.acl.push(AclEntry {
        actor: ActorPattern::App("desktop-shell".to_string()),
        method: MethodPattern::SetState,
        effect: AclEffect::Allow,
    });
    let g = EmptyGrants;
    let shell = ActorId::new_app("desktop-shell");
    // SetState op은 통과.
    assert!(obj.is_allowed(&shell, AclOp::SetState("child_count".to_string()), &g));
    // 같은 actor라도 Invoke op은 SetState 패턴에 매칭 X → 거부.
    assert!(!obj.is_allowed(&shell, AclOp::Invoke("list".to_string()), &g));
}

#[test]
fn allow_if_granted_dir_uses_path_prop() {
    let owner = ActorId::local_user();
    let mut obj = std_types::folder(owner.clone(), "D:/proj/foo", "foo", 0);
    obj.acl.push(AclEntry {
        actor: ActorPattern::AiSession,
        method: MethodPattern::Wildcard,
        effect: AclEffect::AllowIfGrantedDir,
    });
    let ai = ActorId::new_ai_session();
    // grant 없으면 거부.
    let empty = EmptyGrants;
    assert!(!obj.is_allowed(&ai, AclOp::Invoke("create_file".to_string()), &empty));
    // grant 있으면 통과.
    let granted = FixedGrants { actor: ai.clone(), path: "D:/proj/foo".into() };
    assert!(obj.is_allowed(&ai, AclOp::Invoke("create_file".to_string()), &granted));
}

#[test]
fn empty_acl_falls_back_to_owner_only() {
    let owner = ActorId::local_user();
    let obj = std_types::folder(owner.clone(), "/x", "x", 0);
    let g = EmptyGrants;
    // ACL이 비어있으면 owner만 허용 — 기존 동작 유지.
    assert!(obj.acl.is_empty(), "std_types::folder는 ACL이 비어있어야");
    assert!(obj.is_allowed(&owner, AclOp::Invoke("list".to_string()), &g));
    assert!(!obj.is_allowed(&ActorId::system_compositor(), AclOp::Invoke("list".to_string()), &g));
}
