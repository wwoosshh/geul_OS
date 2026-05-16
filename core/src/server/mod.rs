//! 객체 서버.
//!
//! TCB의 핵심. 모든 객체의 단일 진실원이며, 모든 mutate는 이 모듈을 통해서만 일어난다.

pub mod invoke;
pub use invoke::InvokeError;
pub mod mount;
pub use mount::MountError;
pub mod query;
pub use query::Query;
pub mod set_state;
pub use set_state::SetStateError;
pub mod subscribe;
pub use subscribe::{EventKindFilter, SubscriptionId};

use std::collections::HashMap;

use crate::event::EventBus;
use crate::object::{Object, ObjectId};

use subscribe::SubscriptionManager;

/// 객체 트리를 보관하고 모든 mutate를 직렬화하는 서버.
#[derive(Debug, Default)]
pub struct ObjectServer {
    objects: HashMap<ObjectId, Object>,
    /// 트리의 루트 객체 ID 목록 (앱 단위 서브트리 루트들).
    roots: Vec<ObjectId>,
    /// 이벤트 버스.
    bus: EventBus,
    /// 구독 관리자.
    subscriptions: SubscriptionManager,
}

impl ObjectServer {
    /// 빈 ObjectServer 생성.
    pub fn new() -> Self {
        Self {
            objects: HashMap::new(),
            roots: Vec::new(),
            bus: EventBus::new(),
            subscriptions: SubscriptionManager::new(),
        }
    }

    /// 객체 개수.
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// 루트 ID 목록.
    pub fn roots(&self) -> &[ObjectId] {
        &self.roots
    }

    /// ID로 객체 조회.
    pub fn get(&self, id: &ObjectId) -> Option<&Object> {
        self.objects.get(id)
    }

    /// 이벤트 버스에 대한 읽기 전용 접근.
    pub fn bus(&self) -> &EventBus {
        &self.bus
    }
}
