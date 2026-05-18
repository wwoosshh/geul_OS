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
//! - `scenario` — TOML 시나리오 파일 형식 + runner

pub mod adapter;
pub mod chat_session;
pub mod error;
pub mod scenario;
pub mod session;
pub mod tools;
pub mod wire;

pub use error::{BridgeError, BridgeResult};
pub use wire::WireClient;
