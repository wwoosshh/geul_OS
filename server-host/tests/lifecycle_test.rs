use geulos_core::{std_types, ActorId, EventKind, EventKindFilter, LifecycleKind};
use geulos_server_host::{ObjectServerActor, ObjectServerHandle};

#[tokio::test]
async fn disconnect_emits_lifecycle_destroyed_for_actor_objects() {
    let handle: ObjectServerHandle = ObjectServerActor::spawn();

    let owner = ActorId::local_user();
    let txt = std_types::text(owner.clone(), "x");
    let id = handle.mount(txt).await.unwrap();

    // 관찰자 등록 — Lifecycle 이벤트 필터
    let observer = ActorId::system_compositor();
    let sub_id = handle.subscribe(observer, id, vec![EventKindFilter::Lifecycle]).await.unwrap();

    // disconnect 시뮬레이션
    handle.disconnect_actor(owner).await.unwrap();

    // Destroyed 이벤트가 와야 함
    let evs = handle.drain(sub_id).await.unwrap();
    assert!(
        evs.iter().any(|e| matches!(e.kind, EventKind::Lifecycle(LifecycleKind::Destroyed))),
        "expected Lifecycle::Destroyed event, got: {:?}",
        evs
    );
}
