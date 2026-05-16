//! subscribe(): 객체 이벤트 구독.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::event::{Event, EventKind};
use crate::object::{ActorId, ObjectId};
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
    pub(super) target: ObjectId,
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
        Self {
            next_id: AtomicU64::new(1),
            subscriptions: HashMap::new(),
        }
    }

    pub(super) fn register(
        &mut self,
        _subscriber: ActorId,
        target: ObjectId,
        filters: Vec<EventKindFilter>,
    ) -> SubscriptionId {
        let id = SubscriptionId(self.next_id.fetch_add(1, Ordering::SeqCst));
        self.subscriptions.insert(
            id,
            Subscription { target, filters, queue: VecDeque::new() },
        );
        id
    }

    pub(super) fn unregister(&mut self, id: SubscriptionId) {
        self.subscriptions.remove(&id);
    }

    /// 모든 매칭 구독에 이벤트를 enqueue한다.
    pub(super) fn deliver(&mut self, ev: &Event) {
        for sub in self.subscriptions.values_mut() {
            if sub.target != ev.target {
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
    /// 구독 등록.
    pub fn subscribe(
        &mut self,
        subscriber: ActorId,
        target: ObjectId,
        filters: Vec<EventKindFilter>,
    ) -> SubscriptionId {
        self.subscriptions.register(subscriber, target, filters)
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
