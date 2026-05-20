//! set_state(): 객체 상태 직접 갱신.

use serde_json::Value;
use thiserror::Error;

use crate::event::EventKind;
use crate::object::{AclEffect, ActorId, ActorPattern, EventId, MethodPattern, ObjectId};
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
        // 1) 객체 존재 (tombstone은 NotFound와 동일하게 거부 — KI-011)
        let obj = self.objects.get_mut(target).ok_or(SetStateError::NotFound(*target))?;
        if obj.destroyed {
            return Err(SetStateError::NotFound(*target));
        }

        // 2) ACL — 소유자 우선, 그 외에는 wildcard Allow가 있으면 통과.
        //
        // T8.19: 컴포지터(Compositor actor)가 데스크톱-셸 actor 소유의 Window/FileTree/
        // Explorer에 대해 `scroll_y` 등의 set_state를 호출해야 함. 기존엔 *소유자만*
        // 허용했기에 silent PermissionDenied로 스크롤이 동작하지 않았음.
        //
        // invoke의 ACL과 동일한 임시 정책 (KI-001 — wildcard Allow ACL이 있으면 통과).
        // M9 권한 다이얼로그 마일스톤에서 wildcard ACL이 제거되고 매니페스트 기반
        // 권한이 강제될 예정.
        if &obj.owner != actor {
            let allowed_by_wildcard = obj.acl.iter().any(|entry| {
                matches!(entry.effect, AclEffect::Allow)
                    && matches!(entry.actor, ActorPattern::Wildcard)
                    && matches!(entry.method, MethodPattern::Wildcard)
            });
            if !allowed_by_wildcard {
                return Err(SetStateError::PermissionDenied {
                    actor: actor.as_str().to_string(),
                    target: *target,
                    key: key.to_string(),
                });
            }
        }

        // 3) 갱신
        obj.state.insert(key.to_string(), value.clone());

        // 4) 이벤트 발행 (이벤트 버스 + 구독자 알림)
        // type_uri는 ByType 구독 매칭에 필요 — borrow checker 회피를 위해 사전 캐싱.
        let type_uri = obj.type_uri.clone();
        let event_id = self.bus.emit(
            actor.clone(),
            *target,
            EventKind::StateSet { key: key.to_string(), value },
            None,
        );
        if let Some(ev) = self.bus.log().last() {
            self.subscriptions.deliver(ev, Some(&type_uri));
        }

        Ok(event_id)
    }
}
