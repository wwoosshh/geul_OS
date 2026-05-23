//! Cli@1 method handler — clear/append_line.
//!
//! `submit_input`은 매우 복잡 (T7.8/T7.9 awaiting_api_key, AI start/load/list/send 흐름,
//! ai_session/chat_session 상태와 강하게 결합)이라 main.rs에 잔존. 본 모듈은
//! 간단한 두 분기만 처리 — 외부 (AI bridge) 또는 cli 자신이 직접 호출하는 lines 조작.

use geulos_core::{Object, ObjectId};
use serde_json::Value;

use crate::cli_handler::SpecialAction;
use crate::handlers::handle_cli_outcome;
use crate::invoke_handler::InvokeOutcome;

/// Cli.clear — 외부에서 직접 clear 호출. lines 비움.
pub fn handle_clear(target_id: ObjectId, mounted_objects: &mut [Object]) -> InvokeOutcome {
    handle_cli_outcome(mounted_objects, target_id, "", "", vec![], Some(SpecialAction::Clear))
}

/// Cli.append_line(text) — 외부(AI bridge 등)에서 한 라인 추가.
pub fn handle_append_line(
    target_id: ObjectId,
    args: &Value,
    mounted_objects: &mut [Object],
) -> InvokeOutcome {
    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
    handle_cli_outcome(mounted_objects, target_id, "", "", vec![text.to_string()], None)
}
