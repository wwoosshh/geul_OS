use geulos_core::{ActorId, Event, EventKind, LifecycleKind, ObjectId};
use serde_json::json;

#[test]
fn event_carries_metadata() {
    let actor = ActorId::local_user();
    let target = ObjectId::new();
    let ev = Event::new(
        actor.clone(),
        target,
        EventKind::Invoke { method: "press".to_string(), args: json!(null) },
    );
    assert_eq!(ev.actor, actor);
    assert_eq!(ev.target, target);
    assert!(ev.causation.is_none());
}

#[test]
fn event_with_causation_links() {
    let actor = ActorId::local_user();
    let target = ObjectId::new();
    let first = Event::new(actor.clone(), target, EventKind::Lifecycle(LifecycleKind::Created));
    let second = Event::new(
        actor.clone(),
        target,
        EventKind::StateSet { key: "label".to_string(), value: json!("hello") },
    )
    .with_causation(first.id);
    assert_eq!(second.causation, Some(first.id));
}

#[test]
fn event_ids_are_monotonic() {
    let actor = ActorId::local_user();
    let t = ObjectId::new();
    let a = Event::new(actor.clone(), t, EventKind::Lifecycle(LifecycleKind::Created));
    let b = Event::new(actor.clone(), t, EventKind::Lifecycle(LifecycleKind::Destroyed));
    assert!(a.id.as_u64() < b.id.as_u64());
}

#[test]
fn event_serde_round_trip() {
    let ev = Event::new(
        ActorId::local_user(),
        ObjectId::new(),
        EventKind::Invoke { method: "press".to_string(), args: json!({"force": 5}) },
    );
    let s = serde_json::to_string(&ev).unwrap();
    let back: Event = serde_json::from_str(&s).unwrap();
    assert_eq!(ev.actor, back.actor);
    assert_eq!(ev.target, back.target);
    assert_eq!(ev.id, back.id);
}
