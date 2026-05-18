//! GeulOS AI bridge.
//!
//! LLM 어댑터(Claude 등) + GeulOS 와이어 클라이언트 + 세션 매니저.
//! M5 plan `docs/plans/2026-05-17-geulos-m5-ai-adapter.md` 참고.
//!
//! 모듈 책임:
//! - `wire` — GeulOS server-host에 TCP로 접속한 와이어 클라이언트
//! - `adapter` — LLM(Claude 등) 어댑터 trait + 구현
//! - `tools` — Claude 도구 정의 + dispatch
//! - `session` — 한 작업 세션의 lifecycle (task 모델 — 예산, 감사 로그)
//! - `chat_session` — 대화식 multi-prompt 세션 (M7 T7.7, ADR-030)
//! - `chat_persist` — chat 세션의 영구 저장/로드 (M7 T7.8, ADR-031)
//! - `api_key` — API key resolution chain + 검증 + 영속 저장 (M7 T7.9, ADR-032)
//! - `scenario` — TOML 시나리오 파일 형식 + runner

pub mod adapter;
pub mod api_key;
pub mod chat_persist;
pub mod chat_session;
pub mod error;
pub mod scenario;
pub mod session;
pub mod tools;
pub mod wire;

pub use error::{BridgeError, BridgeResult};
pub use wire::WireClient;

/// 테스트 전용 — `HOME`/`USERPROFILE` 환경 변수를 건드리는 모든 테스트가 공유하는 단일
/// 글로벌 mutex. 여러 모듈(chat_persist, api_key)이 동시에 env를 set하면 race가 나서
/// 어느 모듈이 마지막에 set한 디렉터리를 가리키게 된다. 공유 mutex로 직렬화.
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
