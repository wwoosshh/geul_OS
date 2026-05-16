//! 핸드셰이크 메시지.

use serde::{Deserialize, Serialize};

/// 클라이언트 역할.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// AI 클라이언트 (Claude / GPT / 로컬 LLM).
    Ai,
    /// 앱 프로세스.
    App,
    /// 컴포지터 (시스템 권한).
    Compositor,
}

/// 클라이언트가 처음 보내는 핸드셰이크.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "Hello")]
pub struct Hello {
    /// 프로토콜 버전 ("0.1").
    pub version: String,
    /// 역할.
    pub role: Role,
    /// 인증 정보 (역할에 따라 형식 다름).
    pub auth: serde_json::Value,
    /// 클라이언트 자기 식별자 (디버깅용).
    pub client_id: String,
}

/// 서버의 Hello 수락 응답.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "HelloAck")]
pub struct HelloAck {
    /// 발급된 세션 ID.
    pub session_id: String,
    /// 발급된 ActorId의 문자열 표현 (`user:local`, `ai:<uuid>`, ...).
    pub actor_id: String,
    /// 서버 측 프로토콜 버전.
    pub server_version: String,
    /// 이 세션에서 사용 가능한 기능 목록.
    pub capabilities: Vec<String>,
}

/// 서버의 Hello 거부 응답.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "HelloReject")]
pub struct HelloReject {
    /// 거부 사유 코드 (예: "version_mismatch", "auth_failed").
    pub reason: String,
    /// 사람이 읽을 수 있는 설명.
    pub detail: String,
}
