use geulos_core::std_types;
use geulos_core::ActorId;
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
