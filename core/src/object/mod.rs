//! 객체 관련 타입과 ID 정의.

pub mod acl;
pub mod identity;
pub mod manifest;
pub mod method;
pub mod std_types;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use acl::{AclEffect, AclEntry, AclOp, ActorPattern, GrantContext, MethodPattern};
pub use identity::{ActorId, ActorIdParseError, EventId, ObjectId, TypeUri};
pub use manifest::{AppManifest, ManifestError};
pub use method::{ArgSpec, MethodSig};

/// 시스템 상의 기본 객체.
///
/// `Object`는 모든 UI/서비스용 요소를 표현한다. 사용자가 보는 GUI 위젯부터
/// AI가 호출하는 메서드도 모두 단일 객체 트리 안에서 정의된다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Object {
    /// 고유 ID.
    pub id: ObjectId,
    /// 객체 타입 URI (예: `aios.std/Button@1`).
    pub type_uri: TypeUri,
    /// 부모 객체 (없으면 루트).
    pub parent: Option<ObjectId>,
    /// 자식 객체 ID 목록.
    pub children: Vec<ObjectId>,
    /// 정적 속성 (예: 레이블, 아이콘).
    pub props: HashMap<String, Value>,
    /// 동적 상태 (예: 토글 on/off, 텍스트박스 값).
    pub state: HashMap<String, Value>,
    /// 호출 가능한 메서드 서명 목록.
    pub methods: Vec<MethodSig>,
    /// 이 객체를 *소유하는* 액터.
    pub owner: ActorId,
    /// 접근 제어 목록.
    pub acl: Vec<AclEntry>,
    /// Tombstone 플래그. `true`면 객체는 *제거된 것으로 간주*되어 query/roots에
    /// 나타나지 않고 invoke/set_state가 거부된다 (단, get으로는 여전히 조회 가능).
    /// 액터 disconnect 시 ObjectServer가 자동으로 true 설정.
    /// (KI-011 fix — 옛 구현은 Destroyed 이벤트만 발행하고 객체 데이터를 그대로
    /// 두어 *유령 객체* 상태가 발생.)
    #[serde(default)]
    pub destroyed: bool,
}

impl Object {
    /// 새 Object를 만든다(id 자동 발급).
    pub fn new(type_uri: TypeUri, owner: ActorId) -> Self {
        Self {
            id: ObjectId::new(),
            type_uri,
            parent: None,
            children: Vec::new(),
            props: HashMap::new(),
            state: HashMap::new(),
            methods: Vec::new(),
            owner,
            acl: Vec::new(),
            destroyed: false,
        }
    }

    /// state에 값을 설정.
    pub fn set_state(&mut self, key: impl Into<String>, value: Value) {
        self.state.insert(key.into(), value);
    }

    /// props에 값을 설정.
    pub fn set_prop(&mut self, key: impl Into<String>, value: Value) {
        self.props.insert(key.into(), value);
    }

    /// 객체의 `props.path` 값을 `Path`로 반환. M11 AllowIfGrantedDir 평가용.
    pub fn path(&self) -> Option<std::path::PathBuf> {
        self.props.get("path").and_then(|v| v.as_str()).map(std::path::PathBuf::from)
    }

    /// ACL 평가. M11에서 `op: AclOp`와 `grants: &dyn GrantContext` 인자 추가.
    ///
    /// 규칙:
    /// - ACL이 비어 있으면 *소유자만* 허용 (기존 동작).
    /// - ACL이 있으면 *순서대로 평가, 마지막 매칭이 승리*:
    ///   - 마지막 Allow → 허용.
    ///   - 마지막 Deny → 거부.
    ///   - 마지막 AllowIfGrantedDir → path prop 조회 후 grants.is_granted → 통과/거부.
    /// - 어떤 entry도 매칭 안 되면 default deny.
    ///
    /// `op` 매칭 규칙:
    /// - `AclOp::Invoke(method)` → MethodPattern::{Exact, OneOf, Wildcard}와 매칭.
    /// - `AclOp::SetState(_)` → MethodPattern::SetState와 매칭 (key 이름 무관).
    pub fn is_allowed(&self, actor: &ActorId, op: AclOp, grants: &dyn GrantContext) -> bool {
        if self.acl.is_empty() {
            return &self.owner == actor;
        }
        let mut decision: Option<AclEffect> = None;
        for entry in &self.acl {
            if !entry.actor.matches(actor) {
                continue;
            }
            let method_match = match (&entry.method, &op) {
                (MethodPattern::SetState, AclOp::SetState(_)) => true,
                (MethodPattern::SetState, _) => false,
                // Wildcard pattern은 Invoke·SetState 모두 매칭 (범용 허용/거부).
                (MethodPattern::Wildcard, _) => true,
                (_, AclOp::SetState(_)) => false,
                (pat, AclOp::Invoke(m)) => pat.matches(m),
            };
            if method_match {
                decision = Some(entry.effect);
            }
        }
        match decision {
            Some(AclEffect::Allow) => true,
            Some(AclEffect::AllowIfGrantedDir) => {
                self.path().map(|p| grants.is_granted(actor, &p)).unwrap_or(false)
            }
            _ => false,
        }
    }
}
