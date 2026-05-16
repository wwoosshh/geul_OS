//! set_state(): 객체 상태 직접 갱신.

use serde_json::Value;
use thiserror::Error;

use crate::event::EventKind;
use crate::object::{ActorId, EventId, ObjectId};
use crate::server::ObjectServer;

/// set_state 실패 사유.
#[derive(Debug, Error)]
pub enum SetStateError {
    /// 객체 없음.
    #[error("객체를 찾을 수 없음: {0}")]
    NotFound(ObjectId),
    /// 권한 없음.
    #[error("권한 없음: 액터 {actor}, 객체 {target}, 키 {key}")]
    PermissionDenied { actor: String, target: ObjectId, key: String },
}

impl ObjectServer {
    /// 객체의 state 필드 하나를 갱신하고 StateSet 이벤트를 발행.
    ///
    /// ACL: 소유자만 허용 (M3 기본 정책). 추후 매니페스트 권한과 연동.
    pub fn set_state(
        &mut self,
        actor: &ActorId,
        target: &ObjectId,
        key: &str,
        value: Value,
    ) -> Result<EventId, SetStateError> {
        // 1) 객체 존재
        let obj = self.objects.get_mut(target).ok_or(SetStateError::NotFound(*target))?;

        // 2) ACL — 소유자만 허용 (M3 기본).
        if &obj.owner != actor {
            return Err(SetStateError::PermissionDenied {
                actor: actor.as_str().to_string(),
                target: *target,
                key: key.to_string(),
            });
        }

        // 3) 갱신
        obj.state.insert(key.to_string(), value.clone());

        // 4) 이벤트 발행 (이벤트 버스 + 구독자 알림)
        let event_id = self.bus.emit(
            actor.clone(),
            *target,
            EventKind::StateSet { key: key.to_string(), value },
            None,
        );
        if let Some(ev) = self.bus.log().last() {
            self.subscriptions.deliver(ev);
        }

        Ok(event_id)
    }
}
