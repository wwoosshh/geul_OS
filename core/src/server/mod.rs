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

    /// 루트 ID 목록. *살아있는(live)* 루트만 반환 — destroyed 객체는 제외.
    pub fn roots(&self) -> Vec<ObjectId> {
        self.roots
            .iter()
            .filter(|id| self.objects.get(id).map(|o| !o.destroyed).unwrap_or(false))
            .copied()
            .collect()
    }

    /// ID로 객체 조회. destroyed 여부에 관계없이 *그대로* 반환 (event log
    /// 재생·tombstone 시각화 용도). 호출자는 `Object.destroyed`를 직접 확인.
    pub fn get(&self, id: &ObjectId) -> Option<&Object> {
        self.objects.get(id)
    }

    /// 이벤트 버스에 대한 읽기 전용 접근.
    pub fn bus(&self) -> &EventBus {
        &self.bus
    }

    /// 모든 객체 순회 (살아있는 + tombstone 모두). 필터링은 호출자가.
    pub fn objects_iter(&self) -> impl Iterator<Item = (&ObjectId, &crate::object::Object)> {
        self.objects.iter()
    }

    /// 객체에 *tombstone 마킹* + Lifecycle::Destroyed 이벤트 발행.
    ///
    /// 객체 데이터는 보관 (이벤트 로그 재생 일관성을 위해) — `destroyed: true` 플래그만
    /// 세팅. 결과: query/roots에서 사라지고, invoke/set_state가 거부됨. get으로는
    /// 여전히 조회 가능 (호출자가 tombstone 시각화 가능).
    pub fn emit_destroyed(
        &mut self,
        actor: &crate::object::ActorId,
        id: &ObjectId,
    ) -> crate::object::EventId {
        if let Some(obj) = self.objects.get_mut(id) {
            obj.destroyed = true;
        }
        let event_id = self.bus.emit(
            actor.clone(),
            *id,
            crate::event::EventKind::Lifecycle(crate::event::LifecycleKind::Destroyed),
            None,
        );
        if let Some(ev) = self.bus.log().last() {
            self.subscriptions.deliver(ev);
        }
        event_id
    }
}
