//! M11 — GrantUpdate wire 직렬화 회귀.

use geulos_proto::{GrantOp, GrantUpdate};

#[test]
fn grant_update_add_serializes_round_trip() {
    let g = GrantUpdate {
        actor: "ai:abc-123".to_string(),
        path: "D:/proj/foo".to_string(),
        op: GrantOp::Add,
    };
    let json = serde_json::to_string(&g).unwrap();
    let back: GrantUpdate = serde_json::from_str(&json).unwrap();
    assert_eq!(back.actor, g.actor);
    assert_eq!(back.path, g.path);
    assert!(matches!(back.op, GrantOp::Add));
}

#[test]
fn grant_update_remove_op_serializes() {
    let g =
        GrantUpdate { actor: "ai:abc".to_string(), path: "/x".to_string(), op: GrantOp::Remove };
    let json = serde_json::to_string(&g).unwrap();
    assert!(json.contains("Remove"));
    let back: GrantUpdate = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.op, GrantOp::Remove));
}
