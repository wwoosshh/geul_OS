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

#[test]
fn mount_container_assigns_label_one() {
    let mut sh = Shell::new();
    let out = output(sh.execute("mount container"));
    assert!(out.contains("#1"));
    assert!(out.contains("Container"));
}

#[test]
fn mount_text_with_quoted_content() {
    let mut sh = Shell::new();
    let out = output(sh.execute(r#"mount text "hello world""#));
    assert!(out.contains("#1"));
    assert!(out.contains("Text"));
}

#[test]
fn mount_button_with_label() {
    let mut sh = Shell::new();
    let out = output(sh.execute(r#"mount button "OK""#));
    assert!(out.contains("Button"));
}

#[test]
fn mount_toggle_on() {
    let mut sh = Shell::new();
    let out = output(sh.execute("mount toggle on"));
    assert!(out.contains("Toggle"));
}

#[test]
fn mount_toggle_off() {
    let mut sh = Shell::new();
    let out = output(sh.execute("mount toggle off"));
    assert!(out.contains("Toggle"));
}

#[test]
fn mount_assigns_labels_sequentially() {
    let mut sh = Shell::new();
    let out1 = output(sh.execute("mount container"));
    let out2 = output(sh.execute(r#"mount text "x""#));
    let out3 = output(sh.execute(r#"mount button "B""#));
    assert!(out1.contains("#1"));
    assert!(out2.contains("#2"));
    assert!(out3.contains("#3"));
}

#[test]
fn mount_uses_current_actor_as_owner() {
    let mut sh = Shell::new();
    output(sh.execute("as ai"));
    let out = output(sh.execute(r#"mount text "ai owned""#));
    assert!(out.contains("#1"));
    // 본 셸의 후속 명령(ls)에서 owner를 확인하는 것은 Task 4에서.
}

#[test]
fn mount_invalid_kind_errors() {
    let mut sh = Shell::new();
    let out = output(sh.execute("mount widget"));
    assert!(out.contains("error"));
}

#[test]
fn mount_text_without_content_errors() {
    let mut sh = Shell::new();
    let out = output(sh.execute("mount text"));
    assert!(out.contains("error"));
}
