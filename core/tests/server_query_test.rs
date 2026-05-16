use geulos_core::{std_types, ActorId, ObjectServer, Query, TypeUri};

#[test]
fn query_by_type_finds_all() {
    let mut server = ObjectServer::new();
    let owner = ActorId::local_user();
    server.mount(std_types::text(owner.clone(), "a")).unwrap();
    server.mount(std_types::text(owner.clone(), "b")).unwrap();
    server.mount(std_types::button(owner.clone(), "btn")).unwrap();

    let type_uri = TypeUri::parse("aios.std/Text@1").unwrap();
    let results = server.query(&Query::by_type(type_uri));
    assert_eq!(results.len(), 2);
}

#[test]
fn query_by_owner_filters_correctly() {
    let mut server = ObjectServer::new();
    let user = ActorId::local_user();
    let ai = ActorId::new_ai_session();
    server.mount(std_types::text(user.clone(), "user_owned")).unwrap();
    server.mount(std_types::text(ai.clone(), "ai_owned")).unwrap();

    let user_results = server.query(&Query::by_owner(user.clone()));
    assert_eq!(user_results.len(), 1);

    let ai_results = server.query(&Query::by_owner(ai));
    assert_eq!(ai_results.len(), 1);
}

#[test]
fn query_returns_empty_when_no_match() {
    let server = ObjectServer::new();
    let owner = ActorId::local_user();
    let results = server.query(&Query::by_owner(owner));
    assert!(results.is_empty());
}
