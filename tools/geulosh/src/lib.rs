//! geulosh 셸의 라이브러리 형태.
//!
//! 바이너리 `geulosh`와 통합 테스트 양쪽에서 같은 코드를 사용한다.

pub mod commands;
pub mod output;
pub mod parser;
pub mod shell;
pub mod transport;

pub use shell::{Shell, ShellError, ShellOutcome};
pub use transport::{RemoteOutcome, RemoteShell, RemoteTransport};
