//! 객체 관련 타입과 ID 정의.

pub mod acl;
pub mod identity;
pub mod method;
pub mod std_types;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use acl::{AclEffect, AclEntry, ActorPattern, MethodPattern};
pub use identity::{ActorId, EventId, ObjectId, TypeUri};
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

    /// 주어진 액터가 이 객체의 메서드를 호출할 수 있는지.
    ///
    /// 규칙:
    /// - ACL이 비어 있으면 소유자만 허용, 나머지 거부.
    /// - ACL이 있으면 순서대로 평가(마지막 매칭이 승리):
    ///   - 마지막으로 매칭된 Allow → 허용.
    ///   - 마지막으로 매칭된 Deny → 거부.
    ///   - 매칭 없으면 default deny.
    pub fn is_allowed(&self, actor: &ActorId, method: &str) -> bool {
        if self.acl.is_empty() {
            // ACL이 없으면 소유자만 허용.
            return &self.owner == actor;
        }
        // ACL을 순서대로 평가. 마지막 매칭이 승리.
        let mut effect: Option<AclEffect> = None;
        for entry in &self.acl {
            if entry.matches(actor, method) {
                effect = Some(entry.effect);
            }
        }
        matches!(effect, Some(AclEffect::Allow))
    }
}
