//! glue-ai 에러 타입.

use thiserror::Error;

pub type GlueResult<T> = Result<T, GlueError>;

/// glue-ai 작업 중 발생할 수 있는 모든 에러.
#[derive(Debug, Error)]
pub enum GlueError {
    /// 설정/환경 변수 누락 등.
    #[error("config: {0}")]
    Config(String),
    /// HTTP/네트워크 에러.
    #[error("network: {0}")]
    Network(String),
    /// LLM API의 비-200 응답.
    #[error("api error {status}: {detail}")]
    ApiError { status: u16, detail: String },
    /// 와이어 통신 에러.
    #[error("wire: {0}")]
    Wire(String),
    /// JSON 직렬화/역직렬화 에러.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// IO 에러.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// 세션 예산 소진.
    #[error("budget exhausted: {0}")]
    BudgetExhausted(String),
    /// 아직 구현되지 않은 부분 (M5.5 등으로 연기됨).
    #[error("not implemented: {0}")]
    NotImplemented(String),
}
