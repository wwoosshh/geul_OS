//! 외부(컴포지터·AI)로부터 받은 invoke를 처리하는 순수 함수.
//!
//! 각 핸들러는 *입력* (target, 현재 state, args)을 받고 *결과*를 반환:
//! - `state_sets`: 서버에 보낼 StateSet 메시지 목록
//!
//! main.rs는 이 결과를 받아 wire 메시지로 변환하여 server에 전송.

use geulos_core::ObjectId;
use serde_json::{json, Value};

/// invoke 처리 결과.
pub struct InvokeOutcome {
    /// 상태 갱신 (StateSet으로 broadcast).
    pub state_sets: Vec<(ObjectId, String, Value)>,
}

impl InvokeOutcome {
    /// 빈 결과 (알 수 없는 메서드 등에서 반환).
    pub fn empty() -> Self {
        Self { state_sets: vec![] }
    }
}

/// FileTree.expand(id) — `expanded` 배열에 folder_id 추가.
///
/// 멱등성: 이미 expanded에 포함된 folder_id면 변경 없이 같은 배열을 보낸다.
pub fn handle_file_tree_expand(
    target: ObjectId,
    expanded: &[ObjectId],
    folder_id: ObjectId,
) -> InvokeOutcome {
    let mut new_list: Vec<String> = expanded.iter().map(|i| i.to_string()).collect();
    let s = folder_id.to_string();
    if !new_list.contains(&s) {
        new_list.push(s);
    }
    InvokeOutcome { state_sets: vec![(target, "expanded".into(), json!(new_list))] }
}

/// FileTree.collapse(id) — `expanded` 배열에서 folder_id 제거.
pub fn handle_file_tree_collapse(
    target: ObjectId,
    expanded: &[ObjectId],
    folder_id: ObjectId,
) -> InvokeOutcome {
    let s = folder_id.to_string();
    let new_list: Vec<String> =
        expanded.iter().map(|i| i.to_string()).filter(|x| x != &s).collect();
    InvokeOutcome { state_sets: vec![(target, "expanded".into(), json!(new_list))] }
}

/// FileTree.select(id) — `selected`를 node_id로 설정.
///
/// **M8 dead** — Explorer.navigate_to / open_file로 selection 흐름이 분리됨.
/// 메서드는 `STD_TYPES`에 남아있고 컴포지터가 호출할 수 있으나 desktop-shell이
/// 직접 처리하지는 않는다. M9에서 선택 상태를 UI에 표시할 때 재활용 가능.
#[allow(dead_code)]
pub fn handle_file_tree_select(target: ObjectId, node_id: ObjectId) -> InvokeOutcome {
    InvokeOutcome { state_sets: vec![(target, "selected".into(), json!(node_id.to_string()))] }
}

/// Canvas.set_file(id) — `active_file`을 file_id로 설정.
///
/// **M8 dead** — Canvas는 Explorer로 대체됨 (ADR-026/027). 함수와 테스트는
/// `STD_TYPES`에 남아있는 `Canvas@1` 호환성을 위해 유지하지만 main.rs는 호출하지 않음.
#[allow(dead_code)]
pub fn handle_canvas_set_file(target: ObjectId, file_id: ObjectId) -> InvokeOutcome {
    InvokeOutcome { state_sets: vec![(target, "active_file".into(), json!(file_id.to_string()))] }
}
