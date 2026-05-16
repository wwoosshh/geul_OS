//! 출력 포맷팅 helper.

use geulos_core::{Event, EventKind, Object};

/// 한 줄짜리 객체 요약 (`#N  타입  owner`).
pub fn one_line(label: u32, obj: &Object) -> String {
    format!("#{:<3}  {:<28}  owner={}", label, obj.type_uri.as_str(), obj.owner.as_str())
}

/// 객체 상세 (JSON).
pub fn object_detail(obj: &Object) -> String {
    serde_json::to_string_pretty(obj).unwrap_or_else(|e| format!("<직렬화 실패: {}>", e))
}

/// 한 이벤트의 짧은 표현.
pub fn event_short(ev: &Event) -> String {
    let kind_str = match &ev.kind {
        EventKind::Invoke { method, .. } => format!("Invoke(method={})", method),
        EventKind::StateSet { key, .. } => format!("StateSet(key={})", key),
        EventKind::Lifecycle(l) => format!("Lifecycle({:?})", l),
        EventKind::ChildAdded { child } => format!("ChildAdded({})", child),
        EventKind::ChildRemoved { child } => format!("ChildRemoved({})", child),
    };
    format!("{}  actor={}  target={}  kind={}", ev.id, ev.actor.as_str(), ev.target, kind_str)
}
