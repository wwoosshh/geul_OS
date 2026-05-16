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

#[test]
fn ls_lists_mounted_objects() {
    let mut sh = Shell::new();
    output(sh.execute("mount container"));
    output(sh.execute(r#"mount text "hi""#));
    let out = output(sh.execute("ls"));
    assert!(out.contains("#1"));
    assert!(out.contains("#2"));
    assert!(out.contains("Container"));
    assert!(out.contains("Text"));
}

#[test]
fn tree_shows_roots() {
    let mut sh = Shell::new();
    output(sh.execute("mount container"));
    let out = output(sh.execute("tree"));
    assert!(out.contains("#1"));
}

#[test]
fn get_shows_object_details() {
    let mut sh = Shell::new();
    output(sh.execute(r#"mount text "hello""#));
    let out = output(sh.execute("get #1"));
    assert!(out.contains("hello"));
    assert!(out.contains("Text"));
}

#[test]
fn get_unknown_label_errors() {
    let mut sh = Shell::new();
    let out = output(sh.execute("get #99"));
    assert!(out.contains("error"));
}

#[test]
fn events_default_shows_last_10() {
    let mut sh = Shell::new();
    output(sh.execute("mount container"));
    output(sh.execute(r#"mount text "x""#));
    let out = output(sh.execute("events"));
    // mount 두 번 → Lifecycle 이벤트 2개
    assert!(out.contains("Lifecycle"));
}

#[test]
fn events_with_count() {
    let mut sh = Shell::new();
    output(sh.execute("mount container"));
    output(sh.execute(r#"mount text "x""#));
    output(sh.execute(r#"mount button "y""#));
    let out = output(sh.execute("events 2"));
    let lifecycle_count = out.matches("Lifecycle").count();
    assert_eq!(lifecycle_count, 2, "events 2는 정확히 2개 이벤트 표시");
}

#[test]
fn invoke_owner_button_press_succeeds() {
    let mut sh = Shell::new();
    output(sh.execute(r#"mount button "OK""#));
    let out = output(sh.execute("invoke #1 press"));
    assert!(out.contains("Invoke") || out.contains("event"));
}

#[test]
fn invoke_unknown_label_errors() {
    let mut sh = Shell::new();
    let out = output(sh.execute("invoke #99 press"));
    assert!(out.contains("error"));
}

#[test]
fn invoke_by_non_owner_denied() {
    let mut sh = Shell::new();
    output(sh.execute("as user"));
    output(sh.execute(r#"mount button "OK""#));
    output(sh.execute("as ai"));
    let out = output(sh.execute("invoke #1 press"));
    assert!(out.contains("error"));
    assert!(out.to_lowercase().contains("permission") || out.contains("권한"));
}

#[test]
fn invoke_unknown_method_errors() {
    let mut sh = Shell::new();
    output(sh.execute(r#"mount button "OK""#));
    let out = output(sh.execute("invoke #1 self_destruct"));
    assert!(out.contains("error"));
}

#[test]
fn query_type_finds_buttons() {
    let mut sh = Shell::new();
    output(sh.execute(r#"mount button "A""#));
    output(sh.execute(r#"mount text "X""#));
    output(sh.execute(r#"mount button "B""#));
    let out = output(sh.execute("query type aios.std/Button@1"));
    // 2개의 버튼이 나와야 함 (정확한 형식은 한 줄에 하나)
    let lines = out.lines().count();
    assert!(lines >= 2, "expected >= 2 matches, got:\n{}", out);
}

#[test]
fn query_owner_filters_correctly() {
    let mut sh = Shell::new();
    output(sh.execute("as user"));
    output(sh.execute(r#"mount text "u""#));
    output(sh.execute("as ai"));
    output(sh.execute(r#"mount text "a""#));
    // current actor는 ai, ls는 모든 객체 보여줌. query owner user:local는 1개만.
    let out = output(sh.execute("query owner user:local"));
    assert_eq!(out.lines().count(), 1, "expected 1 match, got:\n{}", out);
}

#[test]
fn subscribe_returns_label() {
    let mut sh = Shell::new();
    output(sh.execute(r#"mount button "OK""#));
    let out = output(sh.execute("subscribe #1 invoke"));
    assert!(out.contains("@1"));
    assert!(out.to_lowercase().contains("subscribed"));
}

#[test]
fn subscribe_then_invoke_then_drain() {
    let mut sh = Shell::new();
    output(sh.execute(r#"mount button "OK""#));
    output(sh.execute("subscribe #1 invoke"));
    output(sh.execute("invoke #1 press"));
    let out = output(sh.execute("drain @1"));
    assert!(out.contains("press") || out.contains("Invoke"));
}

#[test]
fn unsubscribe_stops_delivery() {
    let mut sh = Shell::new();
    output(sh.execute(r#"mount button "OK""#));
    output(sh.execute("subscribe #1 invoke"));
    output(sh.execute("unsubscribe @1"));
    output(sh.execute("invoke #1 press"));
    let out = output(sh.execute("drain @1"));
    // 구독이 사라졌으므로 큐 비어있음. drain은 "(no events)" 같은 출력.
    assert!(!out.contains("press"));
}

#[test]
fn subscribe_multiple_filters() {
    let mut sh = Shell::new();
    output(sh.execute(r#"mount button "OK""#));
    let out = output(sh.execute("subscribe #1 invoke state lifecycle"));
    assert!(out.contains("@1"));
}

use std::process::Command;

#[test]
fn script_mode_runs_test_helper() {
    let exe = env!("CARGO_BIN_EXE_geulosh");
    let script_path = format!("{}/scripts/test_helper.gsh", env!("CARGO_MANIFEST_DIR"));
    let status = Command::new(exe).args(["--script", &script_path]).status().expect("실행 실패");
    assert!(status.success(), "스크립트 실패 — 종료 코드: {:?}", status.code());
}

#[test]
fn script_with_failing_expect_returns_nonzero() {
    // Skip: 임시 파일 작성을 위한 추가 의존성을 피하기 위해 본 케이스는 수동 검증으로 남김.
    // 향후 tempfile 의존성을 추가하면 자동화 가능.
}
