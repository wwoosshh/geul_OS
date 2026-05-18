//! cli_handler 순수 함수 단위 테스트.
//!
//! M7 T7.5 — help/clear/echo/empty/whitespace dispatch.
//! M7 T7.7 (ADR-030) — *prefix-free routing*: unknown 명령은 SpecialAction::AiPrompt로 라우팅.

use geulos_desktop_shell::cli_handler::{dispatch_command, SpecialAction};

#[test]
fn help_lists_available_commands() {
    let outcome = dispatch_command("help");
    assert!(outcome.special.is_none(), "help는 special action 없음");
    // 적어도 3개 명령(help/clear/echo)이 안내문에 등장해야 함 + AI 안내.
    let joined = outcome.output_lines.join("\n");
    assert!(joined.contains("help"), "help 출력에 'help' 누락");
    assert!(joined.contains("clear"), "help 출력에 'clear' 누락");
    assert!(joined.contains("echo"), "help 출력에 'echo' 누락");
    assert!(joined.contains("AI"), "help 출력에 'AI' 안내 누락 (T7.7 prefix-free routing)");
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
fn unknown_command_routed_to_ai() {
    // T7.7 (ADR-030): unknown 입력은 AI에게 prompt로 전달.
    let outcome = dispatch_command("unknown_cmd");
    assert!(outcome.output_lines.is_empty(), "AI 라우팅 시 즉시 출력 없음");
    match outcome.special {
        Some(SpecialAction::AiPrompt(p)) => {
            assert_eq!(p, "unknown_cmd", "AiPrompt 페이로드는 trim된 원본 입력");
        }
        other => panic!("unknown 명령은 SpecialAction::AiPrompt여야 함, got {:?}", other),
    }
}

#[test]
fn korean_natural_language_routed_to_ai() {
    // 한글 자연어 prompt — 등록 명령이 아니므로 AI로 위임.
    let outcome = dispatch_command("오늘 워크스페이스에 어떤 파일이 있나요?");
    assert!(outcome.output_lines.is_empty());
    match outcome.special {
        Some(SpecialAction::AiPrompt(p)) => {
            assert_eq!(p, "오늘 워크스페이스에 어떤 파일이 있나요?");
        }
        other => panic!("한글 입력은 AiPrompt여야 함, got {:?}", other),
    }
}

#[test]
fn empty_input_yields_no_output() {
    let outcome = dispatch_command("");
    assert!(outcome.output_lines.is_empty());
    assert!(outcome.special.is_none(), "빈 입력은 AI에게도 전달 안 함");
}

#[test]
fn whitespace_only_input_yields_no_output() {
    let outcome = dispatch_command("   \t  ");
    assert!(outcome.output_lines.is_empty());
    assert!(outcome.special.is_none(), "공백만은 AI에게도 전달 안 함");
}

#[test]
fn echo_without_args_prints_empty_line() {
    let outcome = dispatch_command("echo");
    assert!(outcome.special.is_none());
    assert_eq!(outcome.output_lines, vec!["".to_string()]);
}
