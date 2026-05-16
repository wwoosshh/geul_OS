use geulos_proto::handshake::{Hello, HelloAck, HelloReject, Role};

#[test]
fn hello_serializes_with_kind_tag() {
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Ai,
        auth: serde_json::json!({"token": "abc123"}),
        client_id: "client-1".to_string(),
    };
    let s = serde_json::to_string(&hello).unwrap();
    assert!(s.contains(r#""kind":"Hello""#));
    assert!(s.contains(r#""version":"0.1""#));
    assert!(s.contains(r#""role":"ai""#));
}

#[test]
fn hello_round_trip() {
    let original = Hello {
        version: "0.1".to_string(),
        role: Role::App,
        auth: serde_json::json!({"manifest": {"id": "memo"}}),
        client_id: "client-1".to_string(),
    };
    let s = serde_json::to_string(&original).unwrap();
    let back: Hello = serde_json::from_str(&s).unwrap();
    assert_eq!(original.role, back.role);
    assert_eq!(original.client_id, back.client_id);
}

#[test]
fn hello_ack_carries_session() {
    let ack = HelloAck {
        session_id: "abc".to_string(),
        actor_id: "user:local".to_string(),
        server_version: "0.1".to_string(),
        capabilities: vec!["mount".to_string(), "invoke".to_string()],
    };
    let s = serde_json::to_string(&ack).unwrap();
    assert!(s.contains(r#""kind":"HelloAck""#));
}

#[test]
fn hello_reject_carries_reason() {
    let rej = HelloReject {
        reason: "version_mismatch".to_string(),
        detail: "expected 0.1, got 0.2".to_string(),
    };
    let s = serde_json::to_string(&rej).unwrap();
    assert!(s.contains(r#""kind":"HelloReject""#));
    assert!(s.contains("version_mismatch"));
}

#[test]
fn role_serializes_lowercase() {
    assert_eq!(serde_json::to_string(&Role::Ai).unwrap(), r#""ai""#);
    assert_eq!(serde_json::to_string(&Role::App).unwrap(), r#""app""#);
    assert_eq!(serde_json::to_string(&Role::Compositor).unwrap(), r#""compositor""#);
}
