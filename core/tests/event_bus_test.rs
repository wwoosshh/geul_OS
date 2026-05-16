use geulos_core::{ActorId, Event, EventBus, EventKind, LifecycleKind, ObjectId};

#[test]
fn event_bus_empty_log() {
    let bus = EventBus::new();
    assert_eq!(bus.log().len(), 0);
}

#[test]
fn event_bus_emit_appends_to_log() {
    let mut bus = EventBus::new();
    let target = ObjectId::new();
    let id = bus.emit(
        ActorId::local_user(),
        target,
        EventKind::Lifecycle(LifecycleKind::Created),
        None,
    );
    assert_eq!(bus.log().len(), 1);
    assert_eq!(bus.log()[0].id, id);
}

#[test]
fn event_bus_total_order_preserved() {
    let mut bus = EventBus::new();
    let target = ObjectId::new();
    let actor = ActorId::local_user();
    let a = bus.emit(actor.clone(), target, EventKind::Lifecycle(LifecycleKind::Created), None);
    let b = bus.emit(actor.clone(), target, EventKind::Lifecycle(LifecycleKind::Destroyed), None);
    assert!(a.as_u64() < b.as_u64());
}

#[test]
fn event_bus_causation_links() {
    let mut bus = EventBus::new();
    let actor = ActorId::local_user();
    let t = ObjectId::new();
    let parent_id = bus.emit(actor.clone(), t, EventKind::Lifecycle(LifecycleKind::Created), None);
    let child_id = bus.emit(
        actor.clone(),
        t,
        EventKind::Lifecycle(LifecycleKind::Destroyed),
        Some(parent_id),
    );
    let child_event = &bus.log()[1];
    assert_eq!(child_event.id, child_id);
    assert_eq!(child_event.causation, Some(parent_id));
}

#[test]
fn event_bus_iter_log_by_actor() {
    let mut bus = EventBus::new();
    let user = ActorId::local_user();
    let ai = ActorId::new_ai_session();
    let t = ObjectId::new();
    bus.emit(user.clone(), t, EventKind::Lifecycle(LifecycleKind::Created), None);
    bus.emit(ai.clone(), t, EventKind::Lifecycle(LifecycleKind::Destroyed), None);
    bus.emit(user.clone(), t, EventKind::Lifecycle(LifecycleKind::Created), None);

    let user_events: Vec<&Event> = bus.log().iter().filter(|e| e.actor == user).collect();
    assert_eq!(user_events.len(), 2);
}
