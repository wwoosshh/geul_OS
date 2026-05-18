//! subscribe(): 객체 이벤트 구독.
//!
//! 두 종류의 타겟을 지원 (KI-004 해소):
//! - `SubscriptionTarget::ById(ObjectId)` — 특정 객체 한 개의 이벤트 (기존 동작).
//! - `SubscriptionTarget::ByType(TypeUri)` — 그 타입의 *모든* 객체 이벤트.
//!
//! 후자는 컴포지터가 startup 후 *동적으로 mount된 객체*를 추적하기 위한 메커니즘.
//! desktop-shell이 lazy_mount로 새 Folder/File을 만들면, 컴포지터는 그 type을 ByType
//! 구독해 두었으므로 Lifecycle::Created 이벤트를 즉시 받고 Get으로 본문을 fetch한다.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::event::{Event, EventKind};
use crate::object::{ActorId, ObjectId, TypeUri};
use crate::server::ObjectServer;

/// 구독 ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

impl SubscriptionId {
    /// 내부 u64로 변환.
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

/// 구독 타겟 — 객체 한 개 또는 타입 전체.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriptionTarget {
    /// 특정 객체 ID — 그 객체의 이벤트만 받음.
    ById(ObjectId),
    /// 특정 type_uri — 그 타입의 모든 객체 (현재 mount된 것 + 미래에 mount될 것) 이벤트.
    ByType(TypeUri),
}

/// 어떤 종류의 이벤트를 받을지 필터.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKindFilter {
    /// Invoke 이벤트.
    Invoke,
    /// StateSet 이벤트.
    StateSet,
    /// Lifecycle 이벤트.
    Lifecycle,
    /// ChildAdded/ChildRemoved 이벤트.
    ChildChange,
}

impl EventKindFilter {
    /// 주어진 이벤트가 이 필터에 매치되는지.
    pub fn matches(&self, kind: &EventKind) -> bool {
        matches!(
            (self, kind),
            (Self::Invoke, EventKind::Invoke { .. })
                | (Self::StateSet, EventKind::StateSet { .. })
                | (Self::Lifecycle, EventKind::Lifecycle(_))
                | (Self::ChildChange, EventKind::ChildAdded { .. })
                | (Self::ChildChange, EventKind::ChildRemoved { .. })
        )
    }
}

/// 한 구독의 상태.
#[derive(Debug)]
pub(super) struct Subscription {
    pub(super) target: SubscriptionTarget,
    pub(super) filters: Vec<EventKindFilter>,
    pub(super) queue: VecDeque<Event>,
}

/// 구독 관리자.
#[derive(Debug, Default)]
pub(super) struct SubscriptionManager {
    next_id: AtomicU64,
    subscriptions: HashMap<SubscriptionId, Subscription>,
}

impl SubscriptionManager {
    pub(super) fn new() -> Self {
        Self { next_id: AtomicU64::new(1), subscriptions: HashMap::new() }
    }

    pub(super) fn register(
        &mut self,
        _subscriber: ActorId,
        target: SubscriptionTarget,
        filters: Vec<EventKindFilter>,
    ) -> SubscriptionId {
        let id = SubscriptionId(self.next_id.fetch_add(1, Ordering::SeqCst));
        self.subscriptions.insert(id, Subscription { target, filters, queue: VecDeque::new() });
        id
    }

    pub(super) fn unregister(&mut self, id: SubscriptionId) {
        self.subscriptions.remove(&id);
    }

    /// 모든 매칭 구독에 이벤트를 enqueue한다.
    ///
    /// `ev_type_uri`는 이벤트 대상 객체의 type_uri (ByType 구독 매칭에 필요).
    /// 호출자가 emit *직전*에 캐싱해 전달해야 한다 — destroy 이벤트 이후엔
    /// 객체가 tombstone일 수 있지만 type_uri는 여전히 객체에 보존되어 있다
    /// (KI-011 tombstone 정책).
    pub(super) fn deliver(&mut self, ev: &Event, ev_type_uri: Option<&TypeUri>) {
        for sub in self.subscriptions.values_mut() {
            let target_match = match &sub.target {
                SubscriptionTarget::ById(id) => *id == ev.target,
                SubscriptionTarget::ByType(t) => match ev_type_uri {
                    Some(et) => et == t,
                    None => false,
                },
            };
            if !target_match {
                continue;
            }
            if !sub.filters.iter().any(|f| f.matches(&ev.kind)) {
                continue;
            }
            sub.queue.push_back(ev.clone());
        }
    }

    pub(super) fn drain(&mut self, id: SubscriptionId) -> Vec<Event> {
        match self.subscriptions.get_mut(&id) {
            Some(sub) => sub.queue.drain(..).collect(),
            None => Vec::new(),
        }
    }
}

impl ObjectServer {
    /// ID 기반 구독 등록 — 그 객체 한 개의 이벤트만 받는다.
    pub fn subscribe(
        &mut self,
        subscriber: ActorId,
        target: ObjectId,
        filters: Vec<EventKindFilter>,
    ) -> SubscriptionId {
        self.subscriptions.register(subscriber, SubscriptionTarget::ById(target), filters)
    }

    /// 타입 기반 구독 등록 — 그 type의 모든 객체 (현재+미래) 이벤트를 받는다.
    ///
    /// KI-004 해소용. 컴포지터가 startup 시 STD_TYPES 각각에 등록 → 런타임에 새 mount된
    /// 객체의 Created 이벤트가 자동으로 도달.
    pub fn subscribe_by_type(
        &mut self,
        subscriber: ActorId,
        type_uri: TypeUri,
        filters: Vec<EventKindFilter>,
    ) -> SubscriptionId {
        self.subscriptions.register(subscriber, SubscriptionTarget::ByType(type_uri), filters)
    }

    /// 구독 해제.
    pub fn unsubscribe(&mut self, id: SubscriptionId) {
        self.subscriptions.unregister(id);
    }

    /// 구독 큐에 쌓인 이벤트를 모두 가져온다 (큐 비움).
    pub fn drain_subscription(&mut self, id: SubscriptionId) -> Vec<Event> {
        self.subscriptions.drain(id)
    }
}
