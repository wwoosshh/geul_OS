use geulos_core::{std_types, ActorId, EventKind, LifecycleKind, ObjectServer};
#[allow(unused_imports)]
use serde_json::json;

#[test]
fn mount_single_object() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let obj = std_types::text(owner.clone(), "hello");
    let id = obj.id;

    let root_id = server.mount(obj).expect("mount should succeed");

    assert_eq!(root_id, id);
    assert_eq!(server.object_count(), 1);
    assert!(server.get(&root_id).is_some());
    assert_eq!(server.roots(), &[root_id]);
}

#[test]
fn mount_emits_lifecycle_created_event() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let obj = std_types::container(owner.clone());

    server.mount(obj).unwrap();

    assert_eq!(server.bus().log().len(), 1);
    let ev = &server.bus().log()[0];
    assert_eq!(ev.actor, owner);
    assert!(matches!(ev.kind, EventKind::Lifecycle(LifecycleKind::Created)));
}

#[test]
fn mount_subtree_with_children() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let mut root = std_types::container(owner.clone());
    let child_a = std_types::text(owner.clone(), "a");
    let child_b = std_types::text(owner.clone(), "b");

    let child_a_id = child_a.id;
    let child_b_id = child_b.id;
    root.children.push(child_a_id);
    root.children.push(child_b_id);

    server.mount_with_descendants(root, vec![child_a, child_b]).unwrap();

    assert_eq!(server.object_count(), 3);
    assert!(server.get(&child_a_id).is_some());
    assert!(server.get(&child_b_id).is_some());
    // 각 객체에 대해 Created 이벤트 1개씩
    assert_eq!(server.bus().log().len(), 3);
}

#[test]
fn mount_duplicate_id_rejected() {
    use geulos_core::ObjectId;
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let obj1 = std_types::text(owner.clone(), "first");
    let shared_id = obj1.id;
    let mut obj2 = std_types::text(owner, "second");
    obj2.id = shared_id; // 의도적 충돌

    server.mount(obj1).unwrap();
    assert!(server.mount(obj2).is_err());

    // suppress unused import warning
    let _ = ObjectId::new();
}
