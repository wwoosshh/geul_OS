use geulos_compositor::tree_model::TreeModel;
use geulos_core::{std_types, ActorId};

#[test]
fn upsert_adds_to_objects_and_roots_if_no_parent() {
    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    let t = std_types::text(owner, "hi");
    let id = t.id;
    tm.upsert(t);
    assert_eq!(tm.len(), 1);
    assert!(tm.roots().contains(&id));
}

#[test]
fn upsert_with_parent_not_added_to_roots() {
    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    let mut t = std_types::text(owner, "child");
    t.parent = Some(geulos_core::ObjectId::new());
    let id = t.id;
    tm.upsert(t);
    assert_eq!(tm.len(), 1);
    assert!(!tm.roots().contains(&id));
}

#[test]
fn remove_takes_object_and_root_out() {
    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    let t = std_types::text(owner, "x");
    let id = t.id;
    tm.upsert(t);
    tm.remove(id);
    assert_eq!(tm.len(), 0);
    assert!(!tm.roots().contains(&id));
}

#[test]
fn set_state_updates_object_state() {
    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    let t = std_types::text(owner, "before");
    let id = t.id;
    tm.upsert(t);
    tm.set_state(id, "content".to_string(), serde_json::json!("after"));
    let obj = tm.get(id).unwrap();
    assert_eq!(obj.state.get("content"), Some(&serde_json::json!("after")));
}

#[test]
fn objects_of_type_filters() {
    use geulos_core::TypeUri;
    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    tm.upsert(std_types::text(owner.clone(), "a"));
    tm.upsert(std_types::button(owner.clone(), "b"));
    tm.upsert(std_types::text(owner, "c"));

    let txt_type = TypeUri::parse("aios.std/Text@1").unwrap();
    let texts = tm.objects_of_type(&txt_type);
    assert_eq!(texts.len(), 2);
}
