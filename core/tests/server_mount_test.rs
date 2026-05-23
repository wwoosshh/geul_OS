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
    assert_eq!(server.roots(), vec![root_id]);
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

/// M10 결함 2 회귀 방지: 자식이 mount될 때 부모.children에 자동 push되어야 한다.
/// 이전엔 mount가 항상 roots에 push하고 parent.children 갱신은 호출자 책임이었음 —
/// 그래서 AI가 server에 `get(parent)` 호출 시 children=[]로 "빈 폴더" 응답 회귀.
#[test]
fn mount_with_parent_pushes_to_parent_children() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let parent = std_types::folder(owner.clone(), "/p", "p", 0);
    let parent_id = parent.id;
    server.mount(parent).unwrap();
    assert!(server.get(&parent_id).unwrap().children.is_empty());

    let mut child = std_types::file(owner, "/p/c.txt", "c.txt", "text/plain", 0);
    child.parent = Some(parent_id);
    let child_id = child.id;
    server.mount(child).unwrap();

    assert_eq!(server.get(&parent_id).unwrap().children, vec![child_id]);
    // 자식은 roots에 들어가면 안 됨 (부모 아래).
    assert!(!server.roots().contains(&child_id), "자식은 roots 등록 X");
}

/// 같은 자식을 두 번 mount하려 하면 DuplicateId 에러 — children 중복 push도 X.
#[test]
fn mount_with_parent_dedup() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    let parent = std_types::folder(owner.clone(), "/p", "p", 0);
    let parent_id = parent.id;
    server.mount(parent).unwrap();

    let mut child = std_types::file(owner, "/p/c.txt", "c.txt", "text/plain", 0);
    child.parent = Some(parent_id);
    let child_clone = child.clone();
    server.mount(child).unwrap();
    // 두 번째 mount는 DuplicateId — 호출자 책임 회피, store는 그대로.
    assert!(server.mount(child_clone).is_err());
    assert_eq!(server.get(&parent_id).unwrap().children.len(), 1);
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
