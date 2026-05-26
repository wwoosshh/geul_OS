//! 요청/응답 메시지 타입

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `Mount` 요청: 클라이언트가 객체 트리를 제시.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "Mount")]
pub struct MountMsg {
    /// 루트 ObjectId (UUID 문자열).
    pub root_object_id: String,
    /// 객체 트리 (JSON 직렬화된 Object 또는 트리구조 스펙).
    pub tree: Value,
}

/// `MountAck`: 서버가 mount 수락.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "MountAck")]
pub struct MountAck {
    /// 서버가 확인/검증한 root ObjectId.
    pub root_object_id: String,
}

/// `MountReject`: 서버가 mount 거절.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "MountReject")]
pub struct MountReject {
    pub reason: String,
    pub detail: String,
}

/// `Invoke` 요청: 객체 메서드를 호출.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "Invoke")]
pub struct InvokeMsg {
    /// 클라이언트 측 요청 메시지 ID (응답 매핑용).
    pub request_id: String,
    /// 대상 ObjectId.
    pub target: String,
    /// 메서드 이름.
    pub method: String,
    /// 인자.
    pub args: Value,
}

/// `InvokeAck`: 호출 성공 응답.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "InvokeAck")]
pub struct InvokeAck {
    pub request_id: String,
    /// 생성된 EventId.
    pub event_id: String,
    /// 메서드 결과 (현재는 null).
    pub result: Value,
}

/// `InvokeError`: 호출 실패 응답.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "InvokeError")]
pub struct InvokeError {
    pub request_id: String,
    /// 오류 코드: "permission" / "not_found" / "unknown_method" / ...
    /// JSON 키는 "error_kind"로 직렬화 (tag="kind"와 이름 충돌 방지).
    #[serde(rename = "error_kind")]
    pub kind: String,
    pub detail: String,
}

/// `Subscribe` 요청.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "Subscribe")]
pub struct SubscribeMsg {
    /// 클라이언트 측 구독 ID.
    pub subscription_id: String,
    pub target: String,
    pub kinds: Vec<EventKindFilterWire>,
    /// (M2에서는 무시. 이후 mount 시점의 Lifecycle을 보낼지 결정.)
    pub include_initial: bool,
}

/// `SubscribeAck`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "SubscribeAck")]
pub struct SubscribeAck {
    pub subscription_id: String,
}

/// 와이어 레벨 EventKindFilter (core의 EventKindFilter를 미러로 복제).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKindFilterWire {
    Invoke,
    StateSet,
    Lifecycle,
    ChildChange,
}

/// `Unsubscribe` 요청.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "Unsubscribe")]
pub struct UnsubscribeMsg {
    pub subscription_id: String,
}

/// `Event`: 서버가 클라이언트에게 이벤트를 푸시.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "Event")]
pub struct EventMsg {
    pub subscription_id: String,
    /// core::Event를 JSON으로 직렬화한 값.
    pub event: Value,
}

/// `Query` 요청.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "Query")]
pub struct QueryMsg {
    pub request_id: String,
    pub query: QueryPredicate,
}

/// 조회 술어.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QueryPredicate {
    /// 타입 URI로 검색.
    ByType { type_uri: String },
    /// 소유자 ActorId 문자열로 검색.
    ByOwner { actor: String },
    /// 부모 ObjectId 문자열의 직계 자식만 검색.
    ChildrenOf { parent: String },
}

/// `QueryResult` 응답.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "QueryResult")]
pub struct QueryResult {
    pub request_id: String,
    /// 매칭되는 ObjectId 문자열의 목록.
    pub objects: Vec<String>,
}

/// `Glscript` 요청 (M5에서 완전 구현; M2에서는 placeholder 응답).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "Glscript")]
pub struct GlscriptMsg {
    pub request_id: String,
    pub source: String,
    pub budget: serde_json::Value,
}

/// `GlscriptError`: M2 기준에서는 항상 NotImplemented.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "GlscriptError")]
pub struct GlscriptError {
    pub request_id: String,
    pub kind: String,
    pub detail: String,
}

/// `StateSet` 요청: 객체의 state 키 하나를 갱신.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "StateSet")]
pub struct StateSetMsg {
    pub request_id: String,
    pub target: String,
    pub key: String,
    pub value: Value,
}

/// `StateSetAck`: 성공 응답.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "StateSetAck")]
pub struct StateSetAck {
    pub request_id: String,
    pub event_id: String,
}

/// `StateSetError`: 실패 응답.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "StateSetError")]
pub struct StateSetError {
    pub request_id: String,
    /// (M2 InvokeError와 동일한 직렬화 트릭: 와이어 키는 `error_kind`)
    #[serde(rename = "error_kind")]
    pub kind: String,
    pub detail: String,
}

/// `Get` 요청: 객체의 전체 내용 조회.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "Get")]
pub struct GetMsg {
    pub request_id: String,
    pub target: String,
}

/// `GetResult` 응답.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "GetResult")]
pub struct GetResult {
    pub request_id: String,
    /// core::Object를 JSON으로 직렬화한 값.
    pub object: Value,
}

/// `GetError` 응답.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "GetError")]
pub struct GetError {
    pub request_id: String,
    #[serde(rename = "error_kind")]
    pub kind: String,
    pub detail: String,
}

/// 호출자(desktop-shell)가 server에게 *AI grant* 갱신을 알리는 메시지. M11 신규.
///
/// server-host는 sender의 actor가 `app:desktop-shell:*`일 때만 수락. 그 외 거부.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "GrantUpdate")]
pub struct GrantUpdate {
    /// grant 받을 actor (예: "ai:<uuid>")
    pub actor: String,
    /// grant 대상 디렉터리 경로 (호스트 OS path)
    pub path: String,
    /// Add: grant 등록 / Remove: 철회
    pub op: GrantOp,
}

/// AI grant 조작 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GrantOp {
    Add,
    Remove,
}
