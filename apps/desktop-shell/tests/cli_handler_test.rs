//! cli_handler 순수 함수 단위 테스트 — M7 T7.5 selection criteria 자동 검증.

use geulos_desktop_shell::cli_handler::{dispatch_command, SpecialAction};

#[test]
fn help_lists_available_commands() {
    let outcome = dispatch_command("help");
    assert!(outcome.special.is_none(), "help는 special action 없음");
    // 적어도 3개 명령(help/clear/echo)이 안내문에 등장해야 함.
    let joined = outcome.output_lines.join("\n");
    assert!(joined.contains("help"), "help 출력에 'help' 누락");
    assert!(joined.contains("clear"), "help 출력에 'clear' 누락");
    assert!(joined.contains("echo"), "help 출력에 'echo' 누락");
}

#[test]
fn clear_returns_clear_special_action() {
    let outcome = dispatch_command("clear");
    assert_eq!(
        outcome.special,
        Some(SpecialAction::Clear),
        "clear 명령은 SpecialAction::Clear 반환"
    );
    assert!(outcome.output_lines.is_empty(), "clear는 출력 라인 없음");
}

#[test]
fn echo_prints_arguments_verbatim() {
    let outcome = dispatch_command("echo hello world");
    assert!(outcome.special.is_none());
    assert_eq!(outcome.output_lines, vec!["hello world".to_string()]);
}

#[test]
fn unknown_command_prints_friendly_message() {
    let outcome = dispatch_command("unknown_cmd");
    assert!(outcome.special.is_none());
    assert_eq!(outcome.output_lines.len(), 1);
    assert!(
        outcome.output_lines[0].contains("unknown command"),
        "unknown 메시지에 'unknown command' 포함되어야 함"
    );
    assert!(
        outcome.output_lines[0].contains("unknown_cmd"),
        "unknown 메시지에 원래 명령어 포함되어야 함"
    );
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
