use geulos_core::{std_types, ActorId};
use geulos_server_host::actor::ObjectServerActor;

#[tokio::test]
async fn handle_can_mount_and_get_id() {
    let handle = ObjectServerActor::spawn();
    let owner = ActorId::local_user();
    let obj = std_types::text(owner, "hello");
    let expected_id = obj.id;

    let id = handle.mount(obj).await.expect("mount should succeed");
    assert_eq!(id, expected_id);
}

#[tokio::test]
async fn handle_can_invoke_owner_button() {
    let handle = ObjectServerActor::spawn();
    let owner = ActorId::local_user();
    let btn = std_types::button(owner.clone(), "OK");
    let id = handle.mount(btn).await.unwrap();

    let ev_id =
        handle.invoke(owner, id, "press".to_string(), serde_json::json!(null)).await.unwrap();
    assert!(ev_id.as_u64() > 0);
}

#[tokio::test]
async fn handle_invoke_denied_for_non_owner() {
    let handle = ObjectServerActor::spawn();
    let owner = ActorId::local_user();
    let intruder = ActorId::new_ai_session();
    let btn = std_types::button(owner, "OK");
    let id = handle.mount(btn).await.unwrap();

    let result = handle.invoke(intruder, id, "press".to_string(), serde_json::json!(null)).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn handle_clones_share_same_actor() {
    let handle = ObjectServerActor::spawn();
    let handle2 = handle.clone();

    let owner = ActorId::local_user();
    let obj = std_types::text(owner.clone(), "hi");
    let id1 = handle.mount(obj).await.unwrap();

    // 다른 핸들로 같은 액터의 객체에 접근 가능해야 함.
    let obj_back = handle2.get(id1).await.unwrap();
    assert!(obj_back.is_some());
}
