use geulos_core::std_types;
use geulos_core::{ActorId, ObjectId};
use serde_json::json;

#[test]
fn container_constructs_with_correct_type_uri() {
    let owner = ActorId::local_user();
    let c = std_types::container(owner.clone());
    assert_eq!(c.type_uri.as_str(), "aios.std/Container@1");
    assert_eq!(c.owner, owner);
    assert!(c.methods.is_empty());
}

#[test]
fn text_carries_content_in_state() {
    let owner = ActorId::local_user();
    let t = std_types::text(owner, "hello world");
    assert_eq!(t.type_uri.as_str(), "aios.std/Text@1");
    assert_eq!(t.state.get("content"), Some(&json!("hello world")));
}

#[test]
fn button_carries_label_and_exposes_press() {
    let owner = ActorId::local_user();
    let b = std_types::button(owner, "OK");
    assert_eq!(b.type_uri.as_str(), "aios.std/Button@1");
    assert_eq!(b.state.get("label"), Some(&json!("OK")));
    assert_eq!(b.methods.len(), 1);
    assert_eq!(b.methods[0].name(), "press");
}

#[test]
fn toggle_carries_state_and_exposes_methods() {
    let owner = ActorId::local_user();
    let t = std_types::toggle(owner, true);
    assert_eq!(t.type_uri.as_str(), "aios.std/Toggle@1");
    assert_eq!(t.state.get("on"), Some(&json!(true)));
    let method_names: Vec<&str> = t.methods.iter().map(|m| m.name()).collect();
    assert!(method_names.contains(&"toggle"));
    assert!(method_names.contains(&"set"));
}

// ───────────────── M7 메모장 타입 ─────────────────

#[test]
fn memo_initial_state_and_methods() {
    let owner = ActorId::local_user();
    let m = std_types::memo(owner.clone(), "first note", 1_700_000_000_000);
    assert_eq!(m.type_uri.as_str(), "aios.std/Memo@1");
    assert_eq!(m.owner, owner);
    assert_eq!(m.state.get("title"), Some(&json!("first note")));
    assert_eq!(m.state.get("body"), Some(&json!("")));
    assert_eq!(m.state.get("created_at"), Some(&json!(1_700_000_000_000_i64)));
    assert_eq!(m.state.get("updated_at"), Some(&json!(1_700_000_000_000_i64)));
    assert_eq!(m.state.get("tags"), Some(&json!([] as [&str; 0])));

    let method_names: Vec<&str> = m.methods.iter().map(|x| x.name()).collect();
    for expected in ["insert_text", "delete_range", "set_title", "set_tags", "save"] {
        assert!(method_names.contains(&expected), "method {} not found", expected);
    }
}

#[test]
fn memo_insert_text_has_at_and_text_args() {
    let owner = ActorId::local_user();
    let m = std_types::memo(owner, "x", 0);
    let insert = m.methods.iter().find(|x| x.name() == "insert_text").unwrap();
    let args = insert.args();
    assert_eq!(args.len(), 2);
    assert_eq!(args[0].name(), "at");
    assert_eq!(args[0].type_hint(), "usize");
    assert_eq!(args[1].name(), "text");
    assert_eq!(args[1].type_hint(), "string");
}

#[test]
fn text_area_binds_memo_and_starts_with_zero_cursor() {
    let owner = ActorId::local_user();
    let memo_id = ObjectId::new();
    let ta = std_types::text_area(owner.clone(), memo_id);
    assert_eq!(ta.type_uri.as_str(), "aios.std/TextArea@1");
    assert_eq!(ta.owner, owner);
    assert!(ta.methods.is_empty(), "TextArea는 와이어 메서드 노출하지 않음");
    assert_eq!(ta.props.get("bound_memo"), Some(&serde_json::to_value(memo_id).unwrap()));
    assert_eq!(ta.state.get("cursor_pos"), Some(&json!(0)));
    assert_eq!(ta.state.get("selection"), Some(&json!(null)));
    assert_eq!(ta.state.get("focused"), Some(&json!(false)));
}

#[test]
fn memo_list_methods_and_initial_state() {
    let owner = ActorId::local_user();
    let ml = std_types::memo_list(owner);
    assert_eq!(ml.type_uri.as_str(), "aios.std/MemoList@1");
    assert_eq!(ml.state.get("active_memo"), Some(&json!(null)));

    let method_names: Vec<&str> = ml.methods.iter().map(|x| x.name()).collect();
    for expected in ["create_memo", "delete_memo", "set_active"] {
        assert!(method_names.contains(&expected), "method {} not found", expected);
    }
}

#[test]
fn memo_serializes_through_serde_round_trip() {
    let owner = ActorId::local_user();
    let original = std_types::memo(owner, "round-trip 메모", 1_700_000_000_000);
    let json_str = serde_json::to_string(&original).unwrap();
    let reparsed: geulos_core::Object = serde_json::from_str(&json_str).unwrap();
    assert_eq!(original, reparsed, "P5 round-trip 보존");
}

#[test]
fn text_area_serializes_through_serde_round_trip() {
    let owner = ActorId::local_user();
    let memo_id = ObjectId::new();
    let original = std_types::text_area(owner, memo_id);
    let json_str = serde_json::to_string(&original).unwrap();
    let reparsed: geulos_core::Object = serde_json::from_str(&json_str).unwrap();
    assert_eq!(original, reparsed);
}

// ───────────────── M7 데스크톱 셸 타입 ─────────────────

#[test]
fn desktop_shell_types_roundtrip_through_serde() {
    let owner = ActorId::local_user();
    let candidates = vec![
        std_types::desktop(owner.clone()),
        std_types::file_tree(owner.clone(), "/tmp/workspace"),
        std_types::canvas(owner.clone()),
        std_types::folder(owner.clone(), "/tmp/workspace/a", "a", 1_700_000_000_000),
        std_types::file(
            owner.clone(),
            "/tmp/workspace/a.txt",
            "a.txt",
            "text/plain",
            1_700_000_000_000,
        ),
    ];
    for obj in candidates {
        let json_str = serde_json::to_string(&obj).expect("serialize");
        let back: geulos_core::Object = serde_json::from_str(&json_str).expect("deserialize");
        assert_eq!(back, obj);
    }
}

#[test]
fn file_state_includes_visualization_fields() {
    let owner = ActorId::local_user();
    let f = std_types::file(owner, "/x/y.md", "y.md", "text/markdown", 1_700_000_000_000);
    assert!(f.state.contains_key("last_change_ms"));
    assert!(f.state.contains_key("last_change_actor"));
    assert_eq!(f.state.get("last_change_actor").unwrap(), &json!("system"));
}

// ───────────────── M7 T7.5: 하단 CLI 패널 ─────────────────

#[test]
fn cli_initial_state_and_methods() {
    let owner = ActorId::local_user();
    let c = std_types::cli(owner.clone());
    assert_eq!(c.type_uri.as_str(), "aios.builtin/Cli@1");
    assert_eq!(c.owner, owner);
    assert_eq!(c.state.get("lines"), Some(&json!([] as [&str; 0])));
    assert_eq!(c.state.get("history"), Some(&json!([] as [&str; 0])));
    assert_eq!(c.state.get("session_id"), Some(&json!(null)));

    let method_names: Vec<&str> = c.methods.iter().map(|m| m.name()).collect();
    for expected in ["submit_input", "clear", "append_line"] {
        assert!(method_names.contains(&expected), "method {} not found", expected);
    }
}

#[test]
fn cli_submit_input_has_text_arg() {
    let owner = ActorId::local_user();
    let c = std_types::cli(owner);
    let submit = c.methods.iter().find(|m| m.name() == "submit_input").unwrap();
    let args = submit.args();
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].name(), "text");
    assert_eq!(args[0].type_hint(), "string");
}

#[test]
fn cli_append_line_has_text_arg() {
    let owner = ActorId::local_user();
    let c = std_types::cli(owner);
    let append = c.methods.iter().find(|m| m.name() == "append_line").unwrap();
    let args = append.args();
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].name(), "text");
    assert_eq!(args[0].type_hint(), "string");
}

#[test]
fn cli_clear_has_no_args() {
    let owner = ActorId::local_user();
    let c = std_types::cli(owner);
    let clear = c.methods.iter().find(|m| m.name() == "clear").unwrap();
    assert_eq!(clear.args().len(), 0);
}

#[test]
fn cli_serializes_through_serde_round_trip() {
    let owner = ActorId::local_user();
    let original = std_types::cli(owner);
    let json_str = serde_json::to_string(&original).unwrap();
    let reparsed: geulos_core::Object = serde_json::from_str(&json_str).unwrap();
    assert_eq!(original, reparsed, "P5 round-trip 보존");
}
