#[allow(unused_imports)]
use geulos_core::{ActorId, Object, ObjectServer, TypeUri};

#[test]
fn server_starts_empty() {
    let server = ObjectServer::new();
    assert_eq!(server.object_count(), 0);
}

#[test]
fn server_get_nonexistent_returns_none() {
    let server = ObjectServer::new();
    let random_id = geulos_core::ObjectId::new();
    assert!(server.get(&random_id).is_none());
}
