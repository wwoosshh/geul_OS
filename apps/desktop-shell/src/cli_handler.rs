//! 하단 CLI 패널 명령 dispatch — 순수 함수.
//!
//! desktop-shell main이 `submit_input` invoke를 받으면 입력 텍스트를 *현재 Cli.state.mode*
//! 에 따라 `dispatch_command`(shell) 또는 `dispatch_chat`(ai)으로 넘긴다. 결과로 받은
//! 출력 라인은 Cli.state.lines에 append하고 special action(예: Clear, AiStart 등)이 있으면
//! main이 그에 맞춰 처리한다.
//!
//! - **T7.5 v1:** `help`, `clear`, `echo <text>`, unknown → "unknown command".
//! - **T7.7 (ADR-030):** prefix-free routing — unknown → `AiPrompt(text)` 자동 라우팅.
//! - **T7.8 (ADR-031):** *사용자 요청*으로 *명시적 mode + 영속 세션*으로 재설계. T7.7의
//!   prefix-free routing은 *제거*. 사용자가 `/ai start [name]` / `/ai load <name>` /
//!   `/ai list` / `/exit`을 명시. shell 모드에서 등록 외 명령은 다시 "unknown command".

/// 한 입력에 대한 dispatch 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    /// CLI에 출력으로 누적할 라인 목록 (입력 echo는 포함하지 않음 — 호출자가 별도로 처리).
    pub output_lines: Vec<String>,
    /// lines 자체를 다루는 특별 동작 (예: Clear → 출력 히스토리 비움, AiStart/Load → 모드 전환).
    pub special: Option<SpecialAction>,
}

/// 일반 출력 라인 외 특별 동작.
///
/// **T7.8 (ADR-031):** T7.7의 `AiPrompt`는 제거되고 mode 명시 액션으로 대체:
/// - `AiStart(Option<String>)` — `/ai start [name]`. None이면 caller가 auto-name 생성.
/// - `AiLoad(String)` — `/ai load <name>`.
/// - `AiList` — `/ai list`.
/// - `AiExit` — `/exit` (AI 모드 안에서만 의미).
/// - `AiSend(String)` — AI 모드에서 일반 입력 (caller가 chat_session.send 호출).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecialAction {
    /// `clear` 명령 — lines를 빈 배열로 설정.
    Clear,
    /// `/ai start [name]` — 새 세션 + AI 모드 진입. payload는 사용자 명시 이름(있으면).
    AiStart(Option<String>),
    /// `/ai load <name>` — 디스크 세션 로드 + AI 모드 진입.
    AiLoad(String),
    /// `/ai list` — `~/.geulos/ai-sessions/` 안 모든 세션 목록.
    AiList,
    /// `/exit` — AI 모드 → shell 모드 복귀.
    AiExit,
    /// AI 모드에서 일반 입력 (slash 명령 제외). payload는 trim된 원본.
    AiSend(String),
    /// `/workspace add|list|remove <path>` — AI 신뢰 워크스페이스 관리 (2026-06-02).
    /// **사용자 전용**: Cli.submit_input(컴포지터/사용자) 경로에서만 도달 — AI tool 표면엔
    /// 슬래시 명령이 없어 self-grant 불가 (권한 모델 핵심).
    Workspace(WorkspaceCmd),
}

/// `/workspace` 하위 명령.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceCmd {
    /// `/workspace add <path>` — 절대경로를 영속 신뢰 워크스페이스로 등록.
    Add(String),
    /// `/workspace list` — 영속 + 세션 grant 목록 출력.
    List,
    /// `/workspace remove <path>` — 워크스페이스 철회.
    Remove(String),
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

/// **Shell 모드** dispatch. 빈 입력은 빈 outcome. 등록된 명령(help/clear/echo + /ai 하위)이면
/// 그것을 실행. 그 외는 `unknown command` 안내.
pub fn dispatch_command(input: &str) -> CommandOutcome {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return CommandOutcome::lines(vec![]);
    }
    // slash 명령 — `/ai`/`/workspace`/`/exit` 우선 분기.
    if let Some(rest) = trimmed.strip_prefix("/ai") {
        return dispatch_ai_slash(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("/workspace") {
        return dispatch_workspace_slash(rest);
    }
    if trimmed == "/exit" {
        // shell 모드의 `/exit`은 의미 없음 — 안내만.
        return CommandOutcome::line("이미 셸 모드입니다. (/exit은 AI 모드 안에서만 의미)");
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");
    match cmd {
        "help" => handle_help(),
        "clear" => handle_clear(),
        "echo" => handle_echo(rest),
        _ => {
            CommandOutcome::line(format!("unknown command: {}. /help로 사용 가능한 명령 확인", cmd))
        }
    }
}

/// **AI(chat) 모드** dispatch. slash 명령만 처리하고 그 외 모든 입력은 `AiSend`로 위임.
///
/// AI 모드 안에서도 `/ai start`/`load`/`list`로 세션 *전환*이 가능 — main이 그 분기에서
/// 현재 세션을 끝내고 새 세션으로 갈아탄다. (이미 매 send 후 디스크에 dump되므로 별도
/// flush 불필요.)
pub fn dispatch_chat(input: &str) -> CommandOutcome {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return CommandOutcome::lines(vec![]);
    }
    if trimmed == "/exit" {
        return CommandOutcome::special_only(SpecialAction::AiExit);
    }
    if let Some(rest) = trimmed.strip_prefix("/ai") {
        return dispatch_ai_slash(rest);
    }
    // 그 외 모든 입력은 AI에게 전달.
    CommandOutcome::special_only(SpecialAction::AiSend(trimmed.to_string()))
}

/// `/ai ...` 슬래시 명령 dispatch. `rest`는 `/ai` 뒤의 *나머지 문자열* (앞 공백 포함 가능).
///
/// 분기:
/// - 비어있거나 공백만 → 사용법 안내.
/// - `start` (+ optional name) → `AiStart`.
/// - `load <name>` → `AiLoad`.
/// - `list` → `AiList`.
/// - 그 외 → 안내.
fn dispatch_ai_slash(rest: &str) -> CommandOutcome {
    let rest_trim = rest.trim();
    if rest_trim.is_empty() {
        return CommandOutcome::lines(ai_usage_lines());
    }
    let mut sp = rest_trim.splitn(2, char::is_whitespace);
    let sub = sp.next().unwrap_or("");
    let arg = sp.next().map(|s| s.trim()).filter(|s| !s.is_empty());
    match sub {
        "start" => CommandOutcome::special_only(SpecialAction::AiStart(arg.map(String::from))),
        "load" => match arg {
            Some(name) => CommandOutcome::special_only(SpecialAction::AiLoad(name.to_string())),
            None => CommandOutcome::line("사용법: /ai load <name>"),
        },
        "list" => CommandOutcome::special_only(SpecialAction::AiList),
        other => CommandOutcome::line(format!(
            "/ai 하위 명령 모름: {}. /ai start [name] | /ai load <name> | /ai list",
            other
        )),
    }
}

/// `/workspace ...` 슬래시 명령 dispatch. `rest`는 `/workspace` 뒤 나머지 문자열.
///
/// 분기:
/// - 비어있거나 공백만 → 사용법 안내.
/// - `add <path>` → `Workspace(Add)` (path는 main에서 절대경로 검증).
/// - `list` → `Workspace(List)`.
/// - `remove <path>` → `Workspace(Remove)`.
/// - 그 외 → 안내.
///
/// **주의:** `strip_prefix("/workspace")`는 `/workspaceX` 같은 입력도 매치하므로, 첫 글자가
/// 공백이 아니면(= 별개 명령) 분기에서 unknown 처리로 흘려보낸다.
fn dispatch_workspace_slash(rest: &str) -> CommandOutcome {
    // `/workspace` 바로 뒤가 비어있지도, 공백도 아니면(`/workspaceX`) 이 명령이 아님.
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return CommandOutcome::line(format!(
            "unknown command: /workspace{}. /help로 사용 가능한 명령 확인",
            rest
        ));
    }
    let rest_trim = rest.trim();
    if rest_trim.is_empty() {
        return CommandOutcome::lines(workspace_usage_lines());
    }
    let mut sp = rest_trim.splitn(2, char::is_whitespace);
    let sub = sp.next().unwrap_or("");
    let arg = sp.next().map(|s| s.trim()).filter(|s| !s.is_empty());
    match sub {
        "add" => match arg {
            Some(p) => CommandOutcome::special_only(SpecialAction::Workspace(WorkspaceCmd::Add(
                p.to_string(),
            ))),
            None => CommandOutcome::line("사용법: /workspace add <절대경로>"),
        },
        "list" => CommandOutcome::special_only(SpecialAction::Workspace(WorkspaceCmd::List)),
        "remove" => match arg {
            Some(p) => CommandOutcome::special_only(SpecialAction::Workspace(
                WorkspaceCmd::Remove(p.to_string()),
            )),
            None => CommandOutcome::line("사용법: /workspace remove <절대경로>"),
        },
        other => CommandOutcome::line(format!(
            "/workspace 하위 명령 모름: {}. /workspace add <path> | list | remove <path>",
            other
        )),
    }
}

fn workspace_usage_lines() -> Vec<String> {
    vec![
        "AI 워크스페이스(신뢰 폴더) 관리:".to_string(),
        "  /workspace add <절대경로>     해당 폴더 + 하위 전체를 AI 신뢰 영역으로 (무프롬프트)"
            .to_string(),
        "  /workspace list               등록된 워크스페이스 + 세션 grant 목록".to_string(),
        "  /workspace remove <절대경로>  워크스페이스 철회".to_string(),
    ]
}

fn ai_usage_lines() -> Vec<String> {
    vec![
        "AI 대화 명령:".to_string(),
        "  /ai start [name]   새 대화 시작 (name 생략 시 conv-YYYYMMDD-HHMMSS 자동)".to_string(),
        "  /ai load <name>    저장된 대화 로드".to_string(),
        "  /ai list           저장된 대화 목록".to_string(),
        "  /exit              AI 모드 종료 (대화 모드 안에서만)".to_string(),
    ]
}

/// `help` — 사용 가능한 명령 목록을 안내.
fn handle_help() -> CommandOutcome {
    CommandOutcome::lines(vec![
        "사용 가능 명령 (shell 모드):".to_string(),
        "  help               이 도움말을 표시".to_string(),
        "  clear              출력 히스토리를 비움".to_string(),
        "  echo <text>        text를 그대로 출력".to_string(),
        "  /ai start [name]   AI 대화 시작 (새 세션, 이름 옵션)".to_string(),
        "  /ai load <name>    저장된 AI 대화 로드".to_string(),
        "  /ai list           저장된 AI 대화 목록".to_string(),
        "  /workspace add <path>   AI 신뢰 폴더 등록 (하위 전체 무프롬프트)".to_string(),
        "  /workspace list         워크스페이스 목록".to_string(),
        "  /workspace remove <path> 워크스페이스 철회".to_string(),
        "  (AI 모드 안)       /exit으로 셸 모드 복귀".to_string(),
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
