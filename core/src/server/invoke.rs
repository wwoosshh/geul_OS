//! invoke(): 객체 메서드 호출.

use serde_json::Value;
use thiserror::Error;

use crate::event::EventKind;
use crate::object::{ActorId, EventId, ObjectId};
use crate::server::ObjectServer;

/// invoke 실패 사유.
#[derive(Debug, Error)]
pub enum InvokeError {
    /// 대상 객체가 존재하지 않음.
    #[error("객체를 찾을 수 없음: {0}")]
    NotFound(ObjectId),
    /// 호출자가 권한 없음.
    #[error("권한 없음: 액터 {actor}, 객체 {target}, 메서드 {method}")]
    PermissionDenied { actor: String, target: ObjectId, method: String },
    /// 객체가 그 메서드를 지원하지 않음.
    #[error("객체 {target}는 메서드 '{method}'를 지원하지 않음")]
    UnknownMethod { target: ObjectId, method: String },
}

impl ObjectServer {
    /// 객체의 메서드를 호출한다.
    ///
    /// 흐름:
    /// 1. 대상 객체 존재 확인
    /// 2. 메서드 시그니처 존재 확인
    /// 3. ACL 검사 (소유자 우대 + ACL 평가)
    /// 4. Invoke 이벤트 발행
    pub fn invoke(
        &mut self,
        actor: &ActorId,
        target: &ObjectId,
        method: &str,
        args: Value,
    ) -> Result<EventId, InvokeError> {
        // 1) 객체 존재 (tombstone은 NotFound와 동일하게 거부 — KI-011)
        let obj = self.objects.get(target).ok_or(InvokeError::NotFound(*target))?;
        if obj.destroyed {
            return Err(InvokeError::NotFound(*target));
        }

        // 2) 메서드 존재
        if !obj.methods.iter().any(|m| m.name() == method) {
            return Err(InvokeError::UnknownMethod { target: *target, method: method.to_string() });
        }

        // 3) ACL
        if !obj.is_allowed(actor, method) {
            return Err(InvokeError::PermissionDenied {
                actor: actor.as_str().to_string(),
                target: *target,
                method: method.to_string(),
            });
        }

        // 4) Invoke 이벤트 발행
        // type_uri는 ByType 구독 매칭에 필요 — emit 직전 캐싱.
        let type_uri = obj.type_uri.clone();
        let event_id = self.bus.emit(
            actor.clone(),
            *target,
            EventKind::Invoke { method: method.to_string(), args },
            None,
        );
        if let Some(ev) = self.bus.log().last() {
            self.subscriptions.deliver(ev, Some(&type_uri));
        }

        Ok(event_id)
    }
}
