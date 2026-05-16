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
}

/// 액터 매칭 패턴.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActorPattern {
    /// 정확히 일치하는 액터.
    Exact(ActorId),
    /// 임의의 액터 (`*`).
    Wildcard,
}

impl ActorPattern {
    /// 주어진 액터가 이 패턴과 일치하는지.
    pub fn matches(&self, actor: &ActorId) -> bool {
        match self {
            ActorPattern::Exact(a) => a == actor,
            ActorPattern::Wildcard => true,
        }
    }
}

/// 메서드 이름 매칭 패턴.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MethodPattern {
    /// 정확히 일치.
    Exact(String),
    /// 임의의 메서드.
    Wildcard,
}

impl MethodPattern {
    /// 주어진 메서드 이름이 이 패턴과 일치하는지.
    pub fn matches(&self, method: &str) -> bool {
        match self {
            MethodPattern::Exact(m) => m == method,
            MethodPattern::Wildcard => true,
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
