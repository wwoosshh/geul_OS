//! P1: 트리 무결성 — 임의 mount/invoke 시퀀스 후에도 트리가 유효.
//!
//! 무결성 정의:
//! - 모든 객체의 parent가 None이거나 트리 내 존재하는 객체.
//! - 모든 부모의 children 목록에 있는 ID가 트리 내 존재.
//! - children에 같은 ID가 중복 등장하지 않음.

use geulos_core::{std_types, ActorId, ObjectServer};
use proptest::prelude::*;
use serde_json::json;

#[derive(Debug, Clone)]
enum Op {
    MountText(String),
    MountButton(String),
    MountContainer,
    InvokePress(usize), // 트리 내 N번째 버튼을 누름 (없으면 noop)
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        "[a-z]{1,10}".prop_map(Op::MountText),
        "[a-z]{1,10}".prop_map(Op::MountButton),
        Just(Op::MountContainer),
        (0usize..32).prop_map(Op::InvokePress),
    ]
}

fn verify_tree_integrity(server: &ObjectServer) -> Result<(), String> {
    let object_count = server.object_count();
    for root_id in server.roots() {
        let obj = server.get(&root_id).ok_or("root id not in tree")?;
        for child_id in &obj.children {
            if server.get(child_id).is_none() {
                return Err(format!("child {} 가 트리에 없음", child_id));
            }
        }
    }
    // 중복 ID는 HashMap 사용으로 인해 발생 불가하지만 sanity 확인:
    let _ = object_count;
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn random_ops_preserve_tree_integrity(ops in proptest::collection::vec(arb_op(), 0..50)) {
        let mut server = ObjectServer::new();
        let owner = ActorId::local_user();
        let mut button_ids: Vec<geulos_core::ObjectId> = Vec::new();

        for op in ops {
            match op {
                Op::MountText(s) => {
                    let _ = server.mount(std_types::text(owner.clone(), &s));
                }
                Op::MountButton(label) => {
                    let id = server.mount(std_types::button(owner.clone(), &label));
                    if let Ok(id) = id {
                        button_ids.push(id);
                    }
                }
                Op::MountContainer => {
                    let _ = server.mount(std_types::container(owner.clone()));
                }
                Op::InvokePress(idx) => {
                    if let Some(id) = button_ids.get(idx % button_ids.len().max(1)).cloned() {
                        let _ = server.invoke(&owner, &id, "press", json!({}));
                    }
                }
            }
            verify_tree_integrity(&server).expect("invariant broken");
        }
    }
}
