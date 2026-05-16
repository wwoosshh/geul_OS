//! 시스템 식별자 타입들.
//!
//! - `ObjectId`: 객체 인스턴스 고유 ID (UUID v4)
//! - `EventId`: 이벤트의 전순서를 부여하는 단조 증가 ID
//! - `ActorId`: 동작을 일으킨 주체 식별자 (사용자/AI/앱/시스템)
//! - `TypeUri`: 객체 타입 식별자 (`aios.std/Button@1` 형식)

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;
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

/// 이벤트의 전순서를 부여하는 단조 증가 ID.
///
/// 단일 라이터 모델(ADR-003)에서 이벤트 버스가 발급. 시스템 부팅 시 0부터 시작.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EventId(u64);

static NEXT_EVENT_ID: AtomicU64 = AtomicU64::new(1);

impl EventId {
    /// 새 EventId를 발급한다.
    ///
    /// 본 함수는 프로세스 전역 카운터를 사용. M1에서는 이 정도로 충분.
    /// 향후 ObjectServer가 자체 카운터를 들고 갈 수도 있음.
    pub fn new() -> Self {
        Self(NEXT_EVENT_ID.fetch_add(1, Ordering::SeqCst))
    }

    /// 내부 u64 값을 얻는다.
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Default for EventId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for EventId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ev:{}", self.0)
    }
}

/// 동작을 일으킨 주체 식별자.
///
/// 형식: `user:local`, `ai:<UUID>`, `app:<manifest-id>:<instance-UUID>`,
/// `system:compositor` 등.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActorId(String);

impl ActorId {
    /// 콘솔 로컬 사용자.
    pub fn local_user() -> Self {
        Self("user:local".to_string())
    }

    /// 새 AI 세션.
    pub fn new_ai_session() -> Self {
        Self(format!("ai:{}", Uuid::new_v4()))
    }

    /// 앱 인스턴스.
    pub fn new_app(manifest_id: &str) -> Self {
        Self(format!("app:{}:{}", manifest_id, Uuid::new_v4()))
    }

    /// 시스템 컴포지터.
    pub fn system_compositor() -> Self {
        Self("system:compositor".to_string())
    }

    /// 원시 문자열로 변환.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ActorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 객체 타입 식별자.
///
/// 형식: `<namespace>/<name>@<version>` 예: `aios.std/Button@1`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeUri(String);

/// TypeUri 파싱 오류.
#[derive(Debug, Error)]
pub enum TypeUriParseError {
    /// 슬래시(`/`) 또는 골뱅이(`@`) 누락.
    #[error("TypeUri 형식이 잘못됨: '{0}' (예상: <namespace>/<name>@<version>)")]
    Malformed(String),
}

impl TypeUri {
    /// 문자열을 파싱해 TypeUri를 만든다.
    pub fn parse(s: &str) -> Result<Self, TypeUriParseError> {
        // 최소 검증: '/'와 '@'가 정확히 한 번씩 등장하고 순서가 맞아야 함.
        let slash = s.find('/').ok_or_else(|| TypeUriParseError::Malformed(s.to_string()))?;
        let at = s.find('@').ok_or_else(|| TypeUriParseError::Malformed(s.to_string()))?;
        if slash >= at {
            return Err(TypeUriParseError::Malformed(s.to_string()));
        }
        // 각 부분이 비어있지 않아야 함.
        if s[..slash].is_empty() || s[slash + 1..at].is_empty() || s[at + 1..].is_empty() {
            return Err(TypeUriParseError::Malformed(s.to_string()));
        }
        Ok(Self(s.to_string()))
    }

    /// 원시 문자열로 변환.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TypeUri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// ActorId 파싱 오류.
#[derive(Debug, Error)]
pub enum ActorIdParseError {
    /// 빈 문자열.
    #[error("empty actor id")]
    Empty,
    /// 알 수 없는 접두사.
    #[error("unknown actor prefix: '{0}' (expected user:/system:/ai:/app:)")]
    UnknownPrefix(String),
}

impl std::str::FromStr for ActorId {
    type Err = ActorIdParseError;

    /// 문자열로부터 ActorId를 구성.
    ///
    /// 허용 접두사: `user:`, `system:`, `ai:`, `app:`.
    fn from_str(s: &str) -> Result<Self, ActorIdParseError> {
        if s.is_empty() {
            return Err(ActorIdParseError::Empty);
        }
        let prefix = s.split(':').next().unwrap_or("");
        if !matches!(prefix, "user" | "system" | "ai" | "app") {
            return Err(ActorIdParseError::UnknownPrefix(prefix.to_string()));
        }
        Ok(Self(s.to_string()))
    }
}
