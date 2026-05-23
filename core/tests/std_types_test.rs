use geulos_core::std_types;
use geulos_core::{ActorId, Object, ObjectId};
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

// ───── M10 (ADR-036): Folder/File write 메서드 복귀 ─────
//
// M8 read-only(ADR-027) negative invariant는 M10에서 정반대로 변경됨 — write 메서드가
// 등록됨. positive 검증은 std_types.rs의 `folder_has_fs_methods` / `file_has_fs_methods`
// 단위 테스트에서 수행. integration 단의 중복 negative 검사는 제거.

// ───────────────── M7 T7.5: 하단 CLI 패널 ─────────────────

#[test]
fn cli_initial_state_and_methods() {
    let owner = ActorId::local_user();
    let c = std_types::cli(owner.clone());
    assert_eq!(c.type_uri.as_str(), "aios.builtin/Cli@1");
    assert_eq!(c.owner, owner);
    assert_eq!(c.state.get("lines"), Some(&json!([] as [&str; 0])));
    assert_eq!(c.state.get("history"), Some(&json!([] as [&str; 0])));
    // T7.8 / ADR-031: chat mode + session_name. (placeholder session_id 제거됨.)
    assert_eq!(c.state.get("mode"), Some(&json!("shell")));
    assert_eq!(c.state.get("session_name"), Some(&json!(null)));
    assert!(!c.state.contains_key("session_id"), "session_id 필드는 T7.8에서 제거됐어야 함");
    // T7.9 / ADR-032: pending_action 초기값 null.
    assert_eq!(c.state.get("pending_action"), Some(&json!(null)));

    let method_names: Vec<&str> = c.methods.iter().map(|m| m.name()).collect();
    for expected in ["submit_input", "clear", "append_line"] {
        assert!(method_names.contains(&expected), "method {} not found", expected);
    }
}

#[test]
fn cli_factory_has_pending_action_state() {
    // T7.9 / ADR-032 회귀 — pending_action이 null로 시작해야 함 (검증 후 ADR-032 흐름에서 set).
    let owner = ActorId::local_user();
    let c = std_types::cli(owner);
    assert!(c.state.contains_key("pending_action"));
    assert!(c.state.get("pending_action").unwrap().is_null(), "pending_action 초기값은 null");
}

#[test]
fn cli_factory_has_mode_and_session_name_state() {
    // T7.8 / ADR-031 회귀 — mode가 "shell"로 시작하고 session_name이 null로 시작해야 함.
    let owner = ActorId::local_user();
    let c = std_types::cli(owner);
    assert_eq!(c.state.get("mode").and_then(|v| v.as_str()), Some("shell"));
    assert!(c.state.contains_key("session_name"));
    assert!(c.state.get("session_name").unwrap().is_null(), "session_name 초기값은 null");
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

// ───────────────── M8: 멀티-윈도우 탐색기 ─────────────────

#[test]
fn window_factory_initializes_geometry_and_methods() {
    let owner = ActorId::local_user();
    let file_id = ObjectId::new();
    let w = std_types::window(owner.clone(), "todo.md", file_id, 100, 80, 600, 400);
    assert_eq!(w.type_uri.as_str(), "aios.builtin/Window@1");
    assert_eq!(w.props.get("title").and_then(|v| v.as_str()), Some("todo.md"));
    assert_eq!(w.state.get("x").and_then(|v| v.as_i64()), Some(100));
    assert_eq!(w.state.get("w").and_then(|v| v.as_i64()), Some(600));
    assert_eq!(w.state.get("z").and_then(|v| v.as_i64()), Some(0));
    assert_eq!(w.state.get("focused").and_then(|v| v.as_bool()), Some(false));
    let methods: Vec<&str> = w.methods.iter().map(|m| m.name()).collect();
    assert!(methods.contains(&"move"));
    assert!(methods.contains(&"resize"));
    assert!(methods.contains(&"focus"));
    assert!(methods.contains(&"close"));
}

#[test]
fn window_round_trip_preserves_all_fields() {
    let owner = ActorId::local_user();
    let file_id = ObjectId::new();
    let w = std_types::window(owner, "x", file_id, 10, 20, 300, 200);
    let json = serde_json::to_string(&w).unwrap();
    let parsed: Object = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, w);
}

#[test]
fn explorer_factory_has_navigate_and_open_file() {
    let owner = ActorId::local_user();
    let e = std_types::explorer(owner);
    assert_eq!(e.type_uri.as_str(), "aios.builtin/Explorer@1");
    assert_eq!(e.state.get("active_folder"), Some(&serde_json::Value::Null));
    assert_eq!(e.state.get("view_mode").and_then(|v| v.as_str()), Some("list"));
    let methods: Vec<&str> = e.methods.iter().map(|m| m.name()).collect();
    assert!(methods.contains(&"navigate_to"));
    assert!(methods.contains(&"open_file"));
}

// ───────────────── M8 T8.13 / ADR-033: viewer scroll_y + Window content ─────────────────
//
// Window는 type-aware text viewer로서 *직접* file 본문(`content`)을 보유한다 (별 객체 X).
// FileTree/Explorer도 같은 의미의 `scroll_y` (라인 단위 i32) — 컴포지터가 24px 곱해 픽셀
// 오프셋 계산. 1MB cap은 *호출자(desktop-shell file_read)* 책임이며 std_types는 default
// `false`만 둔다.

#[test]
fn window_factory_has_scroll_y_content_state() {
    let owner = ActorId::local_user();
    let file_id = ObjectId::new();
    let w = std_types::window(owner, "x", file_id, 0, 0, 600, 400);
    assert_eq!(w.state.get("scroll_y").and_then(|v| v.as_i64()), Some(0));
    assert_eq!(w.state.get("content").and_then(|v| v.as_str()), Some(""));
    assert_eq!(w.state.get("content_too_large").and_then(|v| v.as_bool()), Some(false));
}

#[test]
fn file_tree_factory_has_scroll_y() {
    let owner = ActorId::local_user();
    let ft = std_types::file_tree(owner, "/");
    assert_eq!(ft.state.get("scroll_y").and_then(|v| v.as_i64()), Some(0));
}

#[test]
fn explorer_factory_has_scroll_y() {
    let owner = ActorId::local_user();
    let ex = std_types::explorer(owner);
    assert_eq!(ex.state.get("scroll_y").and_then(|v| v.as_i64()), Some(0));
}

#[test]
fn window_round_trip_preserves_new_state_fields() {
    let owner = ActorId::local_user();
    let file_id = ObjectId::new();
    let mut w = std_types::window(owner, "x", file_id, 0, 0, 600, 400);
    w.set_state("scroll_y", serde_json::json!(42));
    w.set_state("content", serde_json::json!("hello\nworld"));
    w.set_state("content_too_large", serde_json::json!(true));
    let json = serde_json::to_string(&w).unwrap();
    let parsed: Object = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, w);
}
