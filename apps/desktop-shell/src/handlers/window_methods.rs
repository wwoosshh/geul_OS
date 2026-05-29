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

/// Window.focus — z 최상위로 + 다른 모든 floating(Window@1 + ConsoleWindow@1 +
/// FileManager@1)은 focused=false batch update. 세 타입이 같은 z-space를 공유하므로 모두
/// focused 리셋 (SP1 M2: FileManager도 떠있는 창). z는 target Window@1에만 부여
/// (ConsoleWindow/FileManager는 자체 handle_focus에서 처리).
pub fn handle_focus(target_id: ObjectId, mounted_objects: &mut [Object]) -> InvokeOutcome {
    let new_z = window_ops::max_z(mounted_objects) + 1;
    let mut outs = vec![];
    for o in mounted_objects.iter_mut() {
        let is_floating = matches!(
            o.type_uri.as_str(),
            "aios.builtin/Window@1"
                | "aios.builtin/ConsoleWindow@1"
                | "aios.builtin/FileManager@1"
        );
        if is_floating {
            let is_target = o.id == target_id;
            o.state.insert("focused".into(), json!(is_target));
            outs.push((o.id, "focused".to_string(), json!(is_target)));
            if is_target && o.type_uri.as_str() == "aios.builtin/Window@1" {
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

/// FileManager.close — Window.close와 동형이되 *자식 서브트리(FileTree/Explorer + 그
/// 자손 드라이브/lazy Folder/File)까지 정리*. launch 시 open_file_manager_window가 여러
/// 객체를 한 번에 mount하므로 close도 대칭으로 전부 제거 — 안 하면 매 재실행마다 트리에
/// 고아 객체가 누적된다 (메모리/렌더 누수).
///
/// 정리 방식: mounted_objects의 children 링크를 BFS로 따라 FileManager 서브트리 전체 id를
/// 모은 뒤 mounted_objects에서 제거. compositor는 state.destroyed=true Window/FileManager만
/// skip하므로 FileManager 자체에 destroyed=true SetState를 broadcast (자식은 부모가 사라지면
/// 렌더 경로에서 자연 소멸 — desktop-shell 측 mounted_objects 제거로 재실행 시 충돌 방지).
pub fn handle_close_file_manager(
    target_id: ObjectId,
    desktop_id: ObjectId,
    mounted_objects: &mut Vec<Object>,
) -> InvokeOutcome {
    // BFS로 FileManager 서브트리 id 수집.
    let mut subtree: Vec<ObjectId> = vec![target_id];
    let mut frontier: Vec<ObjectId> = vec![target_id];
    while let Some(id) = frontier.pop() {
        if let Some(o) = mounted_objects.iter().find(|o| o.id == id) {
            for c in &o.children {
                if !subtree.contains(c) {
                    subtree.push(*c);
                    frontier.push(*c);
                }
            }
        }
    }
    mounted_objects.retain(|o| !subtree.contains(&o.id));
    if let Some(d) = mounted_objects.iter_mut().find(|o| o.id == desktop_id) {
        d.children.retain(|c| *c != target_id);
    }
    // FileManager 자체에만 destroyed broadcast — compositor layout이 즉시 skip.
    InvokeOutcome { state_sets: vec![(target_id, "destroyed".to_string(), json!(true))] }
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
