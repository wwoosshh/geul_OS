//! 하단 CLI 패널 명령 dispatch — 순수 함수.
//!
//! desktop-shell main이 `submit_input` invoke를 받으면 입력 텍스트를 이 모듈의
//! `dispatch_command`로 넘긴다. 결과로 받은 출력 라인은 Cli.state.lines에 append하고
//! special action(예: Clear)이 있으면 그에 맞춰 lines 자체를 비우거나 한다.
//!
//! T7.5 v1 명령: `help`, `clear`, `echo <text>`, unknown.
//! AI 호출은 T7.7부터 — `dispatch_command`가 "unknown"으로 떨어지는 경우의 분기에
//! AI fallback을 추가한다.

/// 한 입력에 대한 dispatch 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    /// CLI에 출력으로 누적할 라인 목록 (입력 echo는 포함하지 않음 — 호출자가 별도로 처리).
    pub output_lines: Vec<String>,
    /// lines 자체를 다루는 특별 동작 (예: Clear → 출력 히스토리 비움).
    pub special: Option<SpecialAction>,
}

/// 일반 출력 라인 외 특별 동작.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecialAction {
    /// `clear` 명령 — lines를 빈 배열로 설정.
    Clear,
}

impl CommandOutcome {
    /// 출력 라인만 있는 결과 (special 없음).
    pub fn lines(lines: Vec<String>) -> Self {
        Self { output_lines: lines, special: None }
    }

    /// 단일 라인 출력 헬퍼.
    pub fn line(line: impl Into<String>) -> Self {
        Self::lines(vec![line.into()])
    }

    /// 출력 없이 special만.
    pub fn special_only(action: SpecialAction) -> Self {
        Self { output_lines: vec![], special: Some(action) }
    }
}

/// 입력 라인을 파싱·dispatch한다.
///
/// 빈 입력(공백만 또는 빈 문자열)은 빈 outcome 반환. 그 외엔 첫 단어를 명령으로
/// 인식하고 나머지를 인자로 전달한다.
pub fn dispatch_command(input: &str) -> CommandOutcome {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return CommandOutcome::lines(vec![]);
    }
    // splitn(2, ' ')로 첫 단어와 나머지 분리. 빈 인자도 허용 (예: `echo` 단독).
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");
    match cmd {
        "help" => handle_help(),
        "clear" => handle_clear(),
        "echo" => handle_echo(rest),
        unknown => CommandOutcome::line(format!("unknown command: {}", unknown)),
    }
}

/// `help` — 사용 가능한 명령 목록을 안내.
fn handle_help() -> CommandOutcome {
    CommandOutcome::lines(vec![
        "사용 가능 명령:".to_string(),
        "  help          이 도움말을 표시".to_string(),
        "  clear         출력 히스토리를 비움".to_string(),
        "  echo <text>   text를 그대로 출력".to_string(),
    ])
}

/// `clear` — lines를 비우는 special action.
fn handle_clear() -> CommandOutcome {
    CommandOutcome::special_only(SpecialAction::Clear)
}

/// `echo <text>` — text를 그대로 출력.
///
/// `echo` 단독(`rest`가 빈 문자열) 호출은 빈 라인 한 줄을 출력 — POSIX echo와 일관.
fn handle_echo(rest: &str) -> CommandOutcome {
    CommandOutcome::line(rest.to_string())
}
