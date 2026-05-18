//! 이벤트 모델.
//!
//! 모든 객체 mutate는 Event로 표현되어 EventBus에 직렬 enqueue된다 (ADR-003).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::object::identity::{ActorId, EventId, ObjectId};

/// 객체 라이프사이클 이벤트 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleKind {
    /// 객체가 생성되었다.
    Created,
    /// 객체가 소멸되었다.
    Destroyed,
}

/// 이벤트의 종류.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum EventKind {
    /// 메서드 호출.
    Invoke {
        /// 호출되는 메서드 이름.
        method: String,
        /// 인자 (JSON Value).
        args: Value,
    },
    /// 객체 상태(state) 변경.
    StateSet {
        /// 변경된 키.
        key: String,
        /// 새 값.
        value: Value,
    },
    /// 객체 라이프사이클.
    Lifecycle(LifecycleKind),
    /// 자식 객체 추가.
    ChildAdded {
        /// 추가된 자식의 ID.
        child: ObjectId,
    },
    /// 자식 객체 제거.
    ChildRemoved {
        /// 제거된 자식의 ID.
        child: ObjectId,
    },
}

/// 시스템에서 발생한 한 이벤트.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// 단조 증가 이벤트 ID.
    pub id: EventId,
    /// 이 이벤트를 일으킨 액터.
    pub actor: ActorId,
    /// 이벤트 대상 객체.
    pub target: ObjectId,
    /// 이벤트 종류.
    pub kind: EventKind,
    /// 이 이벤트를 유발한 다른 이벤트 (있다면).
    pub causation: Option<EventId>,
}

impl Event {
    /// 새 Event를 만든다 (id는 자동 발급).
    pub fn new(actor: ActorId, target: ObjectId, kind: EventKind) -> Self {
        Self { id: EventId::new(), actor, target, kind, causation: None }
    }

    /// 원인 이벤트 ID를 설정한다 (체이닝).
    pub fn with_causation(mut self, cause: EventId) -> Self {
        self.causation = Some(cause);
        self
    }
}

pub mod bus;
pub use bus::EventBus;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{ActorId, ObjectId};

    /// EventKind::Lifecycle(Created) 직렬화 형식이 *고정*되어야 함 — 컴포지터가
    /// 이 형식에 의존해 Created/Destroyed 분기. internally tagged enum + newtype variant
    /// 동작은 serde 버전마다 미세 변화 가능 — 회귀 시 즉시 발견하도록 보호.
    ///
    /// 기대 형식: `{"kind": "Lifecycle", "Created": null}` / `..., "Destroyed": null}`.
    /// 만약 이 형식이 바뀌면 compositor/src/server_client.rs::handle_event_frame의
    /// Lifecycle 파싱도 동기 수정 필요.
    #[test]
    fn lifecycle_created_wire_format_is_stable() {
        let ev = Event::new(
            ActorId::local_user(),
            ObjectId::new(),
            EventKind::Lifecycle(LifecycleKind::Created),
        );
        let v = serde_json::to_value(&ev).unwrap();
        let kind = v.get("kind").expect("Event.kind 직렬화");
        assert_eq!(kind.get("kind").and_then(|v| v.as_str()), Some("Lifecycle"));
        assert!(
            kind.get("Created").is_some(),
            "Lifecycle Created variant는 'Created' 키를 가져야 함 — \
             compositor handle_event_frame이 이 키 존재로 판정함. 실제 직렬화: {}",
            serde_json::to_string(kind).unwrap()
        );
    }

    #[test]
    fn lifecycle_destroyed_wire_format_is_stable() {
        let ev = Event::new(
            ActorId::local_user(),
            ObjectId::new(),
            EventKind::Lifecycle(LifecycleKind::Destroyed),
        );
        let v = serde_json::to_value(&ev).unwrap();
        let kind = v.get("kind").expect("Event.kind 직렬화");
        assert_eq!(kind.get("kind").and_then(|v| v.as_str()), Some("Lifecycle"));
        assert!(kind.get("Destroyed").is_some(), "Destroyed 키 부재 — 직렬화 회귀");
    }
}
