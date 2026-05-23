//! Window@1 method handler — move/resize/focus/close/close_confirm (T8.10).
//!
//! 컴포지터가 마우스 드래그/클릭/[x]를 invoke로 변환해 보냄. desktop-shell이
//! mounted_objects의 Window 상태를 갱신하고 StateSet으로 broadcast → 컴포지터가
//! 다음 프레임에 반영. close는 정식 DestroyMsg/emit_destroyed 와이어 경로가
//! proto에 *없으므로* SetState destroyed=true 우회 (KI-011 tombstone과 형식 일치).
//! 컴포지터 layout/render는 state.destroyed=true Window를 skip — 자연스럽게 사라짐.

use geulos_core::{Object, ObjectId};
use serde_json::{json, Value};

use crate::invoke_handler::InvokeOutcome;
use crate::window_ops;

/// Window.move(x, y) — 위치 갱신.
pub fn handle_move(
    target_id: ObjectId,
    args: &Value,
    mounted_objects: &mut [Object],
) -> InvokeOutcome {
    let x = args.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let y = args.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    if let Some(w) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
        w.state.insert("x".into(), json!(x));
        w.state.insert("y".into(), json!(y));
    }
    InvokeOutcome {
        state_sets: vec![
            (target_id, "x".to_string(), json!(x)),
            (target_id, "y".to_string(), json!(y)),
        ],
    }
}

/// Window.resize(w, h) — 크기 갱신. 최소 크기 (200x120) 강제 — title bar/[x]/resize handle
/// 보존 (너무 작으면 UI 잃음).
pub fn handle_resize(
    target_id: ObjectId,
    args: &Value,
    mounted_objects: &mut [Object],
) -> InvokeOutcome {
    let w_val = (args.get("w").and_then(|v| v.as_i64()).unwrap_or(600) as i32).max(200);
    let h_val = (args.get("h").and_then(|v| v.as_i64()).unwrap_or(400) as i32).max(120);
    if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
        o.state.insert("w".into(), json!(w_val));
        o.state.insert("h".into(), json!(h_val));
    }
    InvokeOutcome {
        state_sets: vec![
            (target_id, "w".to_string(), json!(w_val)),
            (target_id, "h".to_string(), json!(h_val)),
        ],
    }
}

/// Window.focus — z 최상위로 + 다른 모든 Window는 focused=false batch update.
pub fn handle_focus(target_id: ObjectId, mounted_objects: &mut [Object]) -> InvokeOutcome {
    let new_z = window_ops::max_z(mounted_objects) + 1;
    let mut outs = vec![];
    for o in mounted_objects.iter_mut() {
        if o.type_uri.as_str() == "aios.builtin/Window@1" {
            let is_target = o.id == target_id;
            o.state.insert("focused".into(), json!(is_target));
            outs.push((o.id, "focused".to_string(), json!(is_target)));
            if is_target {
                o.state.insert("z".into(), json!(new_z));
                outs.push((o.id, "z".to_string(), json!(new_z)));
            }
        }
    }
    InvokeOutcome { state_sets: outs }
}

/// Window.close — proto에 DestroyMsg / emit_destroyed 와이어 trigger가 없어 (확인 완료
/// — server-host/src/dispatch.rs는 Mount/Invoke/Query/StateSet/Get만 처리.
/// emit_destroyed는 DisconnectActor 시 server 내부에서만 호출), SetState
/// destroyed=true로 tombstone 플래그. desktop-shell 측 mounted_objects와
/// Desktop.children에서도 즉시 제거 — 같은 파일 재open 시 새 Window가 정상 생성.
/// 컴포지터의 layout_desktop이 state.destroyed=true Window를 skip하므로
/// 다음 프레임에서 시각적으로 사라진다.
pub fn handle_close(
    target_id: ObjectId,
    desktop_id: ObjectId,
    mounted_objects: &mut Vec<Object>,
) -> InvokeOutcome {
    let close_id = target_id;
    mounted_objects.retain(|o| o.id != close_id);
    if let Some(d) = mounted_objects.iter_mut().find(|o| o.id == desktop_id) {
        d.children.retain(|c| *c != close_id);
    }
    InvokeOutcome { state_sets: vec![(close_id, "destroyed".to_string(), json!(true))] }
}

/// Window.close_confirm — close button 클릭. dirty=false면 즉시 destroy (기존
/// close와 동일). dirty=true면 v1 단순화: close 거부 + eprintln 안내. 사용자는
/// Ctrl+S로 저장 후 다시 [x] 클릭 필요. 3-버튼 Dialog 흐름은 v2 (spec 시나리오 B).
pub fn handle_close_confirm(
    target_id: ObjectId,
    desktop_id: ObjectId,
    mounted_objects: &mut Vec<Object>,
) -> InvokeOutcome {
    let dirty = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|w| w.state.get("dirty").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    if !dirty {
        let close_id = target_id;
        mounted_objects.retain(|o| o.id != close_id);
        if let Some(d) = mounted_objects.iter_mut().find(|o| o.id == desktop_id) {
            d.children.retain(|c| *c != close_id);
        }
        InvokeOutcome { state_sets: vec![(close_id, "destroyed".to_string(), json!(true))] }
    } else {
        // v1: 3-버튼 Dialog 흐름은 v2 — PendingFs::Save variant가 (file_id,
        // content) 전용이라 close 정보 보관이 어색. 일단 close 거부 + 안내.
        eprintln!("[desktop-shell] dirty Window {} 닫기 거부 — Ctrl+S 후 다시 [x] 클릭", target_id);
        InvokeOutcome::empty()
    }
}
