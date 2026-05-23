//! 접근 제어 목록 (ACL).

use serde::{Deserialize, Serialize};

use super::identity::ActorId;

/// 호출 허용/거부 결정.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AclEffect {
    /// 호출 허용.
    Allow,
    /// 호출 거부.
    Deny,
    /// 객체의 props.path가 호출자(actor)의 granted_dirs에 포함될 때만 Allow.
    /// path prop이 없거나 grant 미등록이면 Deny와 동일. M11 신규.
    ///
    /// **주의 (T2 시점):** `Object::is_allowed` 시그니처가 아직 grants 인자를
    /// 받지 않아 (T3에서 변경 예정) 본 variant가 ACL에 등록되면 *Deny로 처리*된다.
    /// AllowIfGrantedDir의 정상 동작은 T3 완료 후부터.
    AllowIfGrantedDir,
}

/// ACL 검사 시 *어떤 operation*인지 구분 — invoke의 method 이름 vs set_state의 key.
/// M11 신규: set_state ACL 검사를 invoke와 동일한 평가 경로로 통일.
///
/// `Serialize`/`Deserialize` derive 없음 — *런타임 전용*. ACL 검사에만 사용되고
/// wire/config 파일에 들어가지 않는다.
#[derive(Debug, Clone)]
pub enum AclOp {
    /// invoke 호출 — method 이름 포함.
    Invoke(String),
    /// set_state 호출 — 변경 key (참고용, 매칭에는 사용 X — MethodPattern::SetState).
    SetState(String),
}

impl AclOp {
    /// invoke op일 때만 method 이름 반환. MethodPattern::Exact/OneOf와 매칭에 사용.
    pub fn method_name(&self) -> Option<&str> {
        match self {
            AclOp::Invoke(m) => Some(m.as_str()),
            AclOp::SetState(_) => None,
        }
    }
}

/// 동적 권한 컨텍스트 — `AllowIfGrantedDir` 효과 평가 시 호출자의 granted path를 조회.
///
/// 구현체는 server-host의 GrantStore가 일반적. 단위 테스트는 Empty/Fixed 구현 사용.
pub trait GrantContext {
    /// `actor`가 `path` (또는 그 상위)에 대해 grant를 보유하고 있는지.
    fn is_granted(&self, actor: &ActorId, path: &std::path::Path) -> bool;
}

/// 액터 매칭 패턴.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActorPattern {
    /// 정확히 일치하는 액터.
    Exact(ActorId),
    /// 임의의 액터 (`*`). M11에서 *helper 사용 금지* — 회귀 grep 가드용.
    Wildcard,
    /// `system:compositor` 단독 매칭. M11 신규.
    SystemCompositor,
    /// `ai:<uuid>` 접두사 매칭 — 모든 AI 세션. M11 신규.
    AiSession,
    /// `app:<id>:<uuid>` — 특정 app id 매칭. instance UUID는 무관. M11 신규.
    App(String),
}

impl ActorPattern {
    /// 주어진 액터가 이 패턴과 일치하는지.
    pub fn matches(&self, actor: &ActorId) -> bool {
        match self {
            ActorPattern::Exact(a) => a == actor,
            ActorPattern::Wildcard => true,
            ActorPattern::SystemCompositor => actor.as_str() == "system:compositor",
            ActorPattern::AiSession => actor.as_str().starts_with("ai:"),
            ActorPattern::App(id) => {
                let s = actor.as_str();
                s.starts_with("app:") && s[4..].starts_with(&format!("{}:", id))
            }
        }
    }
}

/// 메서드 이름 매칭 패턴.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MethodPattern {
    /// 정확히 일치.
    Exact(String),
    /// 임의의 메서드. M11에서 *helper 사용 금지*.
    Wildcard,
    /// 여러 method 중 하나. M11 신규.
    OneOf(Vec<String>),
    /// set_state 호출 한정. invoke method 이름과는 매칭 X (별 dispatch).
    /// M11 신규.
    SetState,
}

impl MethodPattern {
    /// invoke 호출의 method 문자열과 매칭. set_state op은 별도 dispatch.
    pub fn matches(&self, method: &str) -> bool {
        match self {
            MethodPattern::Exact(m) => m == method,
            MethodPattern::Wildcard => true,
            MethodPattern::OneOf(v) => v.iter().any(|m| m == method),
            MethodPattern::SetState => false,
        }
    }
}

/// ACL의 한 항목.
///
/// 액터·메서드 패턴이 모두 일치할 때 `effect`가 적용된다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AclEntry {
    /// 누구에게 적용되나.
    pub actor: ActorPattern,
    /// 어떤 메서드에 적용되나.
    pub method: MethodPattern,
    /// 허용 또는 거부.
    pub effect: AclEffect,
}

impl AclEntry {
    /// 액터와 메서드가 이 항목에 매치되는지.
    pub fn matches(&self, actor: &ActorId, method: &str) -> bool {
        self.actor.matches(actor) && self.method.matches(method)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::ActorId;

    #[test]
    fn actor_system_compositor_matches_only_compositor() {
        let pat = ActorPattern::SystemCompositor;
        assert!(pat.matches(&ActorId::system_compositor()));
        assert!(!pat.matches(&ActorId::local_user()));
        assert!(!pat.matches(&ActorId::new_ai_session()));
        assert!(!pat.matches(&ActorId::new_app("foo")));
    }

    #[test]
    fn actor_ai_session_matches_any_ai_uuid() {
        let pat = ActorPattern::AiSession;
        assert!(pat.matches(&ActorId::new_ai_session()));
        assert!(pat.matches(&ActorId::new_ai_session())); // 다른 UUID도
        assert!(!pat.matches(&ActorId::local_user()));
        assert!(!pat.matches(&ActorId::system_compositor()));
        assert!(!pat.matches(&ActorId::new_app("foo")));
    }

    #[test]
    fn actor_app_matches_specific_id_any_uuid() {
        let pat = ActorPattern::App("desktop-shell".to_string());
        assert!(pat.matches(&ActorId::new_app("desktop-shell")));
        assert!(pat.matches(&ActorId::new_app("desktop-shell"))); // 다른 instance UUID도
        assert!(!pat.matches(&ActorId::new_app("echo")));
        assert!(!pat.matches(&ActorId::local_user()));
    }

    #[test]
    fn method_one_of_matches_listed() {
        let pat = MethodPattern::OneOf(vec!["read_external".into(), "write_external".into()]);
        assert!(pat.matches("read_external"));
        assert!(pat.matches("write_external"));
        assert!(!pat.matches("delete"));
    }

    #[test]
    fn method_set_state_matches_set_state_op_only() {
        // SetState pattern은 invoke method 이름 매칭에는 false, op이 SetState일 때만 true.
        // 본 변경은 Object::is_allowed에서 AclOp 인자 도입 후 검증.
        // 단위 수준에서는 method 이름과 비교하지 않음 (별 dispatch).
        let pat = MethodPattern::SetState;
        // 의도: invoke 호출 시 method 문자열 매칭으로는 항상 false.
        assert!(!pat.matches("anything"));
    }

    #[test]
    fn acl_op_invoke_carries_method_name() {
        let op = AclOp::Invoke("save".to_string());
        assert_eq!(op.method_name(), Some("save"));
        let setop = AclOp::SetState("scroll_y".to_string());
        assert_eq!(setop.method_name(), None);
    }

    #[test]
    fn grant_context_empty_denies_all() {
        struct Empty;
        impl GrantContext for Empty {
            fn is_granted(&self, _actor: &ActorId, _path: &std::path::Path) -> bool {
                false
            }
        }
        let ctx = Empty;
        assert!(!ctx.is_granted(&ActorId::new_ai_session(), std::path::Path::new("/x")));
    }
}
