//! Shell 상태와 명령 디스패치.

/// 셸 명령 한 줄의 실행 결과.
#[derive(Debug, Clone)]
pub enum ShellOutcome {
    /// 정상 출력.
    Output(String),
    /// 에러 메시지.
    Error(String),
    /// 종료 요청 (exit/quit).
    Quit,
    /// 출력 없음 (빈 줄 등).
    NoOp,
}

/// 셸 에러.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    /// 다음 태스크에서 채울 변종들.
    #[error("not implemented")]
    NotImplemented,
}

/// 셸 상태 (placeholder).
#[derive(Default)]
pub struct Shell {
    // 다음 태스크에서 필드 추가
}

impl Shell {
    /// 빈 셸.
    pub fn new() -> Self {
        Self {}
    }

    /// 한 줄 명령을 실행.
    ///
    /// 다음 태스크에서 본격 구현.
    pub fn execute(&mut self, _line: &str) -> ShellOutcome {
        ShellOutcome::Output("(not implemented)".to_string())
    }
}
