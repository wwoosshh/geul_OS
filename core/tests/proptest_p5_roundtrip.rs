//! P5: 직렬화 → 역직렬화 → 동등 (라운드트립 속성).

use geulos_core::{ActorId, Event, EventKind, LifecycleKind, ObjectId};
use proptest::prelude::*;
use serde_json::json;

prop_compose! {
    fn arb_actor_id()(kind in 0u8..4u8, suffix in any::<u64>()) -> ActorId {
        match kind {
            0 => ActorId::local_user(),
            1 => ActorId::new_ai_session(),
            2 => ActorId::new_app(&format!("app{}", suffix)),
            _ => ActorId::system_compositor(),
        }
    }
}

prop_compose! {
    fn arb_event_kind()(
        which in 0u8..4u8,
        key in "[a-z]{1,8}",
        val in any::<i64>(),
    ) -> EventKind {
        match which {
            0 => EventKind::Invoke { method: key.clone(), args: json!(val) },
            1 => EventKind::StateSet { key, value: json!(val) },
            2 => EventKind::Lifecycle(LifecycleKind::Created),
            _ => EventKind::Lifecycle(LifecycleKind::Destroyed),
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn event_round_trip(actor in arb_actor_id(), kind in arb_event_kind()) {
        let ev = Event::new(actor, ObjectId::new(), kind);
        let s = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(ev, back);
    }
}
