//! 이벤트 버스.
//!
//! 단일 라이터 모델 (ADR-003). 모든 이벤트가 직렬로 emit되어 로그에 영구 기록된다.

use super::{Event, EventKind};
use crate::object::identity::{ActorId, EventId, ObjectId};

/// 이벤트 버스: 시스템 내 모든 이벤트의 전순서를 관리.
#[derive(Debug, Default)]
pub struct EventBus {
    log: Vec<Event>,
}

impl EventBus {
    pub fn new() -> Self {
        Self { log: Vec::new() }
    }

    pub fn emit(
        &mut self,
        actor: ActorId,
        target: ObjectId,
        kind: EventKind,
        causation: Option<EventId>,
    ) -> EventId {
        let mut ev = Event::new(actor, target, kind);
        if let Some(cause) = causation {
            ev.causation = Some(cause);
        }
        let id = ev.id;
        self.log.push(ev);
        id
    }

    pub fn log(&self) -> &[Event] {
        &self.log
    }

    pub fn len(&self) -> usize {
        self.log.len()
    }

    pub fn is_empty(&self) -> bool {
        self.log.is_empty()
    }
}
