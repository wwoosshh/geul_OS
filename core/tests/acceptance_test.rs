//! M1 인수 테스트
//!
//! 설계 문서 §9.2 완료 기준: "임포터블로 import 후 Container > Text("hello") 트리 만들고
//! 직렬화/역직렬화해 검증하는 인수 테스트 달성."

use geulos_core::{std_types, ActorId, ObjectServer};

#[test]
fn container_text_round_trip() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();

    // 1. Container > Text("hello") 트리 생성
    let mut container = std_types::container(owner.clone());
    let text = std_types::text(owner.clone(), "hello");
    let text_id = text.id;
    container.children.push(text_id);
    let container_id = container.id;

    // 2. mount
    server.mount_with_descendants(container, vec![text]).unwrap();
    assert_eq!(server.object_count(), 2);

    // 3. Container 객체를 꺼내 JSON 직렬화
    let c_obj = server.get(&container_id).unwrap();
    let json = serde_json::to_string(c_obj).expect("직렬화 실패해선 안 됨");
    assert!(json.contains("Container"));

    // 4. 역직렬화 후 검증
    let back: geulos_core::Object = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, container_id);
    assert_eq!(back.type_uri.as_str(), "aios.std/Container@1");
    assert_eq!(back.children, vec![text_id]);

    // 5. Text 객체도 확인
    let t_obj = server.get(&text_id).unwrap();
    let json2 = serde_json::to_string(t_obj).unwrap();
    let back2: geulos_core::Object = serde_json::from_str(&json2).unwrap();
    assert_eq!(back2.id, text_id);
    assert_eq!(
        back2.state.get("content"),
        Some(&serde_json::json!("hello"))
    );
}

#[test]
fn invoke_lifecycle_and_subscribe_observed() {
    use geulos_core::{EventKind, EventKindFilter};
    use serde_json::json;

    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();

    let btn = std_types::button(owner.clone(), "OK");
    let btn_id = server.mount(btn).unwrap();

    let sub = server.subscribe(owner.clone(), btn_id, vec![EventKindFilter::Invoke]);

    server.invoke(&owner, &btn_id, "press", json!({"force": 5})).unwrap();
    server.invoke(&owner, &btn_id, "press", json!({"force": 10})).unwrap();

    let drained = server.drain_subscription(sub);
    assert_eq!(drained.len(), 2);
    for ev in drained {
        match ev.kind {
            EventKind::Invoke { method, .. } => assert_eq!(method, "press"),
            _ => panic!("expected Invoke"),
        }
    }
}
