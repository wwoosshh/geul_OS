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
pub use subscribe::{EventKindFilter, SubscriptionId, SubscriptionTarget};

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::event::EventBus;
use crate::object::{GrantContext, Object, ObjectId};

use subscribe::SubscriptionManager;

/// 임시 grant 저장소 — M11 T4 placeholder. T6에서 wire 메시지 (GrantUpdate) 처리와
/// 함께 정식 사용. AI invoke의 AllowIfGrantedDir 효과가 평가될 때 grants가 조회된다.
#[derive(Default, Debug, Clone)]
pub struct GrantStore {
    by_actor: HashMap<crate::object::ActorId, HashSet<PathBuf>>,
}

impl GrantStore {
    /// actor에게 path 디렉터리 grant 추가.
    pub fn add(&mut self, actor: crate::object::ActorId, path: PathBuf) {
        self.by_actor.entry(actor).or_default().insert(path);
    }
    /// 철회.
    pub fn remove(&mut self, actor: &crate::object::ActorId, path: &PathBuf) {
        if let Some(set) = self.by_actor.get_mut(actor) {
            set.remove(path);
        }
    }
}

impl GrantContext for GrantStore {
    /// `path` 또는 그 상위 디렉터리 중 하나라도 grant되어 있으면 true.
    /// 예: grant("D:/proj") → "D:/proj/sub/file.txt" 도 통과.
    fn is_granted(&self, actor: &crate::object::ActorId, path: &std::path::Path) -> bool {
        self.by_actor.get(actor).is_some_and(|set| {
            set.iter().any(|granted| path == granted.as_path() || path.starts_with(granted))
        })
    }
}

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
    /// M11 신규 — AI의 path-aware grant 저장소. T6에서 wire 통합.
    pub grants: GrantStore,
}

impl ObjectServer {
    /// 빈 ObjectServer 생성.
    pub fn new() -> Self {
        Self {
            objects: HashMap::new(),
            roots: Vec::new(),
            bus: EventBus::new(),
            subscriptions: SubscriptionManager::new(),
            grants: GrantStore::default(),
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
        // type_uri를 먼저 캐싱 — 그 후 destroyed 플래그를 세팅 (tombstone). type_uri는
        // 객체에 남아 있어 사실 destroy 후에도 lookup 가능하지만, ByType 구독 매칭의
        // 의미를 명확히 하기 위해 *emit 시점*의 type을 사용한다.
        let type_uri = self.objects.get(id).map(|o| o.type_uri.clone());
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
            self.subscriptions.deliver(ev, type_uri.as_ref());
        }
        event_id
    }
}
