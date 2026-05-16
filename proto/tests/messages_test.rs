use geulos_proto::messages::{
    EventKindFilterWire, EventMsg, InvokeAck, InvokeError as InvokeErrorWire, InvokeMsg, MountMsg,
    QueryMsg, QueryPredicate, SubscribeMsg, UnsubscribeMsg,
};
use serde_json::json;

#[test]
fn mount_message_round_trip() {
    let msg = MountMsg {
        root_object_id: "00000000-0000-0000-0000-000000000000".to_string(),
        tree: json!({"id": "..."}),
    };
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""kind":"Mount""#));
    let back: MountMsg = serde_json::from_str(&s).unwrap();
    assert_eq!(msg.root_object_id, back.root_object_id);
}

#[test]
fn invoke_message_carries_request_id() {
    let msg = InvokeMsg {
        request_id: "req-1".to_string(),
        target: "obj-uuid".to_string(),
        method: "press".to_string(),
        args: json!({"force": 5}),
    };
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""kind":"Invoke""#));
    assert!(s.contains(r#""request_id":"req-1""#));
}

#[test]
fn invoke_ack_round_trip() {
    let ack = InvokeAck {
        request_id: "req-1".to_string(),
        event_id: "ev:42".to_string(),
        result: json!(null),
    };
    let s = serde_json::to_string(&ack).unwrap();
    assert!(s.contains(r#""kind":"InvokeAck""#));
}

#[test]
fn invoke_error_carries_kind() {
    let err = InvokeErrorWire {
        request_id: "req-1".to_string(),
        kind: "permission".to_string(),
        detail: "denied for ai:abc".to_string(),
    };
    let s = serde_json::to_string(&err).unwrap();
    assert!(s.contains(r#""kind":"InvokeError""#));
    assert!(s.contains("permission"));
}

#[test]
fn subscribe_round_trip() {
    let msg = SubscribeMsg {
        subscription_id: "sub-1".to_string(),
        target: "obj-uuid".to_string(),
        kinds: vec![EventKindFilterWire::Invoke, EventKindFilterWire::Lifecycle],
        include_initial: true,
    };
    let s = serde_json::to_string(&msg).unwrap();
    assert!(s.contains(r#""kind":"Subscribe""#));
    let back: SubscribeMsg = serde_json::from_str(&s).unwrap();
    assert_eq!(msg.kinds.len(), back.kinds.len());
}

#[test]
fn event_message_round_trip() {
    let ev = EventMsg {
        subscription_id: "sub-1".to_string(),
        event: json!({
            "id": "ev:1",
            "actor": "user:local",
            "target": "obj-x",
            "kind": "Lifecycle",
            "payload": {},
            "causation": null
        }),
    };
    let s = serde_json::to_string(&ev).unwrap();
    assert!(s.contains(r#""kind":"Event""#));
}

#[test]
fn query_predicate_serializes() {
    let q = QueryMsg {
        request_id: "q-1".to_string(),
        query: QueryPredicate::ByType { type_uri: "aios.std/Button@1".to_string() },
    };
    let s = serde_json::to_string(&q).unwrap();
    assert!(s.contains(r#""kind":"Query""#));
    assert!(s.contains(r#""ByType""#));
}

#[test]
fn unsubscribe_round_trip() {
    let m = UnsubscribeMsg { subscription_id: "sub-1".to_string() };
    let s = serde_json::to_string(&m).unwrap();
    assert!(s.contains(r#""kind":"Unsubscribe""#));
}

use geulos_proto::{StateSetAck, StateSetError, StateSetMsg};

#[test]
fn state_set_message_round_trip() {
    let m = StateSetMsg {
        request_id: "r-1".to_string(),
        target: "obj-uuid".to_string(),
        key: "content".to_string(),
        value: serde_json::json!("hello"),
    };
    let s = serde_json::to_string(&m).unwrap();
    assert!(s.contains(r#""kind":"StateSet""#));
    let back: StateSetMsg = serde_json::from_str(&s).unwrap();
    assert_eq!(m, back);
}

#[test]
fn state_set_ack_round_trip() {
    let a = StateSetAck { request_id: "r-1".to_string(), event_id: "ev:42".to_string() };
    let s = serde_json::to_string(&a).unwrap();
    assert!(s.contains(r#""kind":"StateSetAck""#));
}

#[test]
fn state_set_error_uses_error_kind_wire_name() {
    let e = StateSetError {
        request_id: "r-1".to_string(),
        kind: "permission".to_string(),
        detail: "denied".to_string(),
    };
    let s = serde_json::to_string(&e).unwrap();
    assert!(s.contains(r#""kind":"StateSetError""#));
    assert!(s.contains(r#""error_kind":"permission""#));
}
