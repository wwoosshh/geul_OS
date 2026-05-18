//! cli_handler 순수 함수 단위 테스트.
//!
//! - **M7 T7.5** — help/clear/echo/empty/whitespace dispatch.
//! - **M7 T7.7 (ADR-030)** — *prefix-free routing*: unknown 명령은 AiPrompt로 자동 라우팅. **제거됨.**
//! - **M7 T7.8 (ADR-031)** — 명시적 `/ai start [name]` / `/ai load <name>` / `/ai list` / `/exit`
//!   + AI 모드 dispatch_chat. shell 모드의 unknown은 다시 "unknown command".

use geulos_desktop_shell::cli_handler::{dispatch_chat, dispatch_command, SpecialAction};

// ──────────────────────────── Shell 모드 (dispatch_command) ────────────────────────────

#[test]
fn help_lists_available_commands() {
    let outcome = dispatch_command("help");
    assert!(outcome.special.is_none(), "help는 special action 없음");
    let joined = outcome.output_lines.join("\n");
    assert!(joined.contains("help"), "help 출력에 'help' 누락");
    assert!(joined.contains("clear"), "help 출력에 'clear' 누락");
    assert!(joined.contains("echo"), "help 출력에 'echo' 누락");
    // T7.8: AI 안내가 명시적 슬래시 명령 형태로 나와야 함.
    assert!(joined.contains("/ai"), "help 출력에 '/ai' 명시 누락 (T7.8)");
    assert!(joined.contains("/exit"), "help 출력에 '/exit' 안내 누락 (T7.8)");
}

#[test]
fn clear_returns_clear_special_action() {
    let outcome = dispatch_command("clear");
    assert_eq!(outcome.special, Some(SpecialAction::Clear));
    assert!(outcome.output_lines.is_empty());
}

#[test]
fn echo_prints_arguments_verbatim() {
    let outcome = dispatch_command("echo hello world");
    assert!(outcome.special.is_none());
    assert_eq!(outcome.output_lines, vec!["hello world".to_string()]);
}

#[test]
fn unknown_shell_command_is_not_routed_to_ai_anymore() {
    // T7.8 회귀 — prefix-free routing 제거. unknown은 안내 메시지 한 줄만.
    let outcome = dispatch_command("unknown_cmd");
    assert!(outcome.special.is_none(), "shell 모드의 unknown은 special 없음 (T7.8)");
    assert_eq!(outcome.output_lines.len(), 1);
    assert!(
        outcome.output_lines[0].contains("unknown command"),
        "unknown 메시지 누락: {:?}",
        outcome.output_lines
    );
}

#[test]
fn korean_natural_language_in_shell_mode_is_unknown() {
    // 한글 자연어 입력도 shell 모드에서는 unknown — AI 모드 진입이 명시되어야 한다.
    let outcome = dispatch_command("오늘 워크스페이스에 어떤 파일이 있나요?");
    assert!(outcome.special.is_none());
    assert_eq!(outcome.output_lines.len(), 1);
    assert!(outcome.output_lines[0].contains("unknown"));
}

#[test]
fn empty_input_yields_no_output() {
    let outcome = dispatch_command("");
    assert!(outcome.output_lines.is_empty());
    assert!(outcome.special.is_none());
}

#[test]
fn whitespace_only_input_yields_no_output() {
    let outcome = dispatch_command("   \t  ");
    assert!(outcome.output_lines.is_empty());
    assert!(outcome.special.is_none());
}

#[test]
fn echo_without_args_prints_empty_line() {
    let outcome = dispatch_command("echo");
    assert!(outcome.special.is_none());
    assert_eq!(outcome.output_lines, vec!["".to_string()]);
}

#[test]
fn shell_exit_explains_it_is_only_for_ai_mode() {
    // `/exit`을 shell 모드에서 치면 의미 없음 — 안내만.
    let outcome = dispatch_command("/exit");
    assert!(outcome.special.is_none());
    assert_eq!(outcome.output_lines.len(), 1);
    assert!(outcome.output_lines[0].contains("셸 모드"));
}

// ──────────────────────────── /ai 슬래시 명령 (T7.8 ADR-031) ────────────────────────────

#[test]
fn ai_start_without_name_returns_special_with_none() {
    let outcome = dispatch_command("/ai start");
    assert!(outcome.output_lines.is_empty());
    assert_eq!(outcome.special, Some(SpecialAction::AiStart(None)));
}

#[test]
fn ai_start_with_name_carries_name() {
    let outcome = dispatch_command("/ai start mysess");
    assert_eq!(outcome.special, Some(SpecialAction::AiStart(Some("mysess".to_string()))));
}

#[test]
fn ai_load_requires_name() {
    // name 없음 → 사용법 안내, special 없음.
    let outcome = dispatch_command("/ai load");
    assert!(outcome.special.is_none());
    assert_eq!(outcome.output_lines.len(), 1);
    assert!(outcome.output_lines[0].contains("/ai load"));
}

#[test]
fn ai_load_with_name_returns_special() {
    let outcome = dispatch_command("/ai load conv-20260518-180000");
    assert_eq!(outcome.special, Some(SpecialAction::AiLoad("conv-20260518-180000".to_string())));
    assert!(outcome.output_lines.is_empty());
}

#[test]
fn ai_list_returns_special() {
    let outcome = dispatch_command("/ai list");
    assert_eq!(outcome.special, Some(SpecialAction::AiList));
    assert!(outcome.output_lines.is_empty());
}

#[test]
fn ai_bare_shows_usage() {
    // `/ai` 단독 → 사용법 라인들.
    let outcome = dispatch_command("/ai");
    assert!(outcome.special.is_none());
    let joined = outcome.output_lines.join("\n");
    assert!(joined.contains("start"));
    assert!(joined.contains("load"));
    assert!(joined.contains("list"));
}

#[test]
fn ai_unknown_subcommand_explains() {
    let outcome = dispatch_command("/ai whoops");
    assert!(outcome.special.is_none());
    assert_eq!(outcome.output_lines.len(), 1);
    assert!(outcome.output_lines[0].contains("whoops"));
}

// ──────────────────────────── AI 모드 (dispatch_chat) ────────────────────────────

#[test]
fn chat_mode_exit_returns_ai_exit() {
    let outcome = dispatch_chat("/exit");
    assert_eq!(outcome.special, Some(SpecialAction::AiExit));
}

#[test]
fn chat_mode_natural_language_is_ai_send() {
    let outcome = dispatch_chat("안녕 AI");
    match outcome.special {
        Some(SpecialAction::AiSend(p)) => assert_eq!(p, "안녕 AI"),
        other => panic!("AI 모드의 자연어는 AiSend여야 함, got {:?}", other),
    }
    assert!(outcome.output_lines.is_empty());
}

#[test]
fn chat_mode_empty_input_yields_no_action() {
    let outcome = dispatch_chat("   ");
    assert!(outcome.special.is_none());
    assert!(outcome.output_lines.is_empty());
}

#[test]
fn chat_mode_ai_list_still_routed() {
    // AI 모드 안에서도 `/ai list`는 메타 명령으로 정상 작동.
    let outcome = dispatch_chat("/ai list");
    assert_eq!(outcome.special, Some(SpecialAction::AiList));
}

#[test]
fn chat_mode_ai_start_routed_for_session_switch() {
    // AI 모드 안의 `/ai start newsess`는 *세션 전환* — main이 현재 세션 dump 후 새 세션으로 교체.
    let outcome = dispatch_chat("/ai start newsess");
    assert_eq!(outcome.special, Some(SpecialAction::AiStart(Some("newsess".to_string()))));
}

#[test]
fn chat_mode_help_text_is_just_a_prompt_to_ai() {
    // AI 모드에서 `help`는 slash가 아니므로 AI에게 전달 — 사용자가 *AI에게 help를 묻는 것*.
    let outcome = dispatch_chat("help");
    match outcome.special {
        Some(SpecialAction::AiSend(p)) => assert_eq!(p, "help"),
        other => panic!("AI 모드의 'help'는 AiSend여야 함, got {:?}", other),
    }
}
