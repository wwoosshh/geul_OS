//! 객체 ID와 객체 타입의 기본 정의.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 시스템 전역에서 유일한 객체 식별자.
///
/// 객체는 한 번 생성되면 ID가 변하지 않는다. 객체가 *소멸*해도 ID는 재사용되지
/// 않는다 — 이벤트 로그의 인과성을 깨지 않기 위함.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectId(Uuid);

impl ObjectId {
    /// 새로운 임의 ObjectId를 발급한다.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ObjectId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
