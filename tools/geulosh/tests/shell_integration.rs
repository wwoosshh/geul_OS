use geulos_shell::{Shell, ShellOutcome};

fn output(s: ShellOutcome) -> String {
    match s {
        ShellOutcome::Output(s) => s,
        ShellOutcome::Error(e) => format!("error: {}", e),
        ShellOutcome::Quit => "<quit>".to_string(),
        ShellOutcome::NoOp => "<noop>".to_string(),
    }
}

#[test]
fn help_lists_commands() {
    let mut sh = Shell::new();
    let out = output(sh.execute("help"));
    assert!(out.contains("help"));
    assert!(out.contains("mount"));
    assert!(out.contains("invoke"));
}

#[test]
fn exit_returns_quit() {
    let mut sh = Shell::new();
    assert!(matches!(sh.execute("exit"), ShellOutcome::Quit));
    assert!(matches!(sh.execute("quit"), ShellOutcome::Quit));
}

#[test]
fn empty_or_comment_line_noop() {
    let mut sh = Shell::new();
    assert!(matches!(sh.execute(""), ShellOutcome::NoOp));
    assert!(matches!(sh.execute("   "), ShellOutcome::NoOp));
    assert!(matches!(sh.execute("# 주석"), ShellOutcome::NoOp));
}

#[test]
fn actor_default_is_user_local() {
    let mut sh = Shell::new();
    let out = output(sh.execute("actor"));
    assert!(out.contains("user:local"));
}

#[test]
fn as_ai_changes_then_actor_shows_ai_prefix() {
    let mut sh = Shell::new();
    output(sh.execute("as ai"));
    let out = output(sh.execute("actor"));
    assert!(out.starts_with("ai:"));
}

#[test]
fn as_ai_is_sticky_in_session() {
    let mut sh = Shell::new();
    output(sh.execute("as ai"));
    let first = output(sh.execute("actor"));
    output(sh.execute("as user"));
    output(sh.execute("as ai"));
    let second = output(sh.execute("actor"));
    assert_eq!(first, second, "두 번째 `as ai`가 동일 ID로 복원되어야 함");
}

#[test]
fn unknown_command_returns_error() {
    let mut sh = Shell::new();
    let out = output(sh.execute("flibberty jib"));
    assert!(out.contains("error"));
    assert!(out.contains("unknown"));
}
