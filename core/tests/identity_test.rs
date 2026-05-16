use std::str::FromStr;

use geulos_core::object::identity::ActorIdParseError;
use geulos_core::{ActorId, EventId, TypeUri};

#[test]
fn event_id_is_monotonic_via_new_in_sequence() {
    let a = EventId::new();
    let b = EventId::new();
    let c = EventId::new();
    assert!(a.as_u64() < b.as_u64());
    assert!(b.as_u64() < c.as_u64());
}

#[test]
fn actor_id_local_user_is_constant() {
    let u1 = ActorId::local_user();
    let u2 = ActorId::local_user();
    assert_eq!(u1, u2);
    assert_eq!(u1.as_str(), "user:local");
}

#[test]
fn actor_id_ai_session_is_unique() {
    let s1 = ActorId::new_ai_session();
    let s2 = ActorId::new_ai_session();
    assert_ne!(s1, s2);
    assert!(s1.as_str().starts_with("ai:"));
}

#[test]
fn type_uri_parses_namespace_and_version() {
    let t = TypeUri::parse("aios.std/Button@1").expect("should parse");
    assert_eq!(t.as_str(), "aios.std/Button@1");
}

#[test]
fn type_uri_rejects_malformed() {
    assert!(TypeUri::parse("nostuff").is_err());
    assert!(TypeUri::parse("missing@version").is_err());
}

#[test]
fn type_uri_serializes_round_trip() {
    let t = TypeUri::parse("aios.std/Container@1").unwrap();
    let s = serde_json::to_string(&t).unwrap();
    let back: TypeUri = serde_json::from_str(&s).unwrap();
    assert_eq!(t, back);
}

#[test]
fn actor_id_from_str_accepts_known_prefixes() {
    let u = ActorId::from_str("user:local").unwrap();
    assert_eq!(u.as_str(), "user:local");

    let s = ActorId::from_str("system:compositor").unwrap();
    assert_eq!(s.as_str(), "system:compositor");

    let a = ActorId::from_str("ai:abc-123").unwrap();
    assert_eq!(a.as_str(), "ai:abc-123");

    let p = ActorId::from_str("app:memo:xyz-789").unwrap();
    assert_eq!(p.as_str(), "app:memo:xyz-789");
}

#[test]
fn actor_id_from_str_rejects_unknown_prefix() {
    let err = ActorId::from_str("wat:something").unwrap_err();
    assert!(matches!(err, ActorIdParseError::UnknownPrefix(_)));
}

#[test]
fn actor_id_from_str_rejects_empty() {
    assert!(ActorId::from_str("").is_err());
}

#[test]
fn actor_id_round_trip_via_serde() {
    let original = ActorId::from_str("ai:test-session-1").unwrap();
    let json = serde_json::to_string(&original).unwrap();
    let back: ActorId = serde_json::from_str(&json).unwrap();
    assert_eq!(original, back);
}
