//! GeulOS core crate.
//!
//! 이 크레이트는 TCB(Trusted Computing Base)에 해당하는 컴포넌트들을 담는다:
//! 객체 서버, 이벤트 버스, 권한 매니저. 모든 외부 컴포넌트(컴포지터, 앱 런타임,
//! 글 AI I/O 드라이버)는 이 크레이트의 공개 API를 통해서만 코어와 대화한다.

pub mod object;

pub use object::ObjectId;
