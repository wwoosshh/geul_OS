//! GeulOS core crate.
//!
//! TCB(Trusted Computing Base)에 해당하는 컴포넌트들을 담는다:
//! 객체 서버, 이벤트 버스, 권한 매니저.

pub mod object;

pub use object::{ActorId, EventId, ObjectId, TypeUri};
