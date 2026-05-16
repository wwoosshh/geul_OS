use geulos_core::ObjectId;

#[test]
fn object_id_new_returns_unique_ids() {
    let a = ObjectId::new();
    let b = ObjectId::new();
    assert_ne!(a, b, "두 번 호출한 ObjectId::new()가 같으면 안 됨");
}

#[test]
fn object_id_is_displayable() {
    let id = ObjectId::new();
    let s = format!("{}", id);
    assert!(!s.is_empty(), "ObjectId Display는 비어있지 않은 문자열을 내야 함");
}

#[test]
fn object_id_serializes_to_string() {
    let id = ObjectId::new();
    let json = serde_json::to_string(&id).expect("ObjectId는 serde 직렬화 가능해야 함");
    assert!(json.starts_with('"') && json.ends_with('"'));
}
