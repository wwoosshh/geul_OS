//! ConsoleWindow@1 invoke handler — terminate / close / move / resize / focus / scroll.
//!
//! terminate는 AI sender이면 Dialog mount + PendingFs::ConsoleTerminate, compositor면 즉시
//! ProcessRegistry::terminate (TerminateJobObject로 cascade kill).
//! exit waiter task가 별도로 ConsoleEvent::Exit 발행 → main loop가 status SetState.

use geulos_core::{ActorId, Object, ObjectId};
use geulos_proto::{encode_frame, EventKindFilterWire, MountMsg, SubscribeMsg};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::dialog_ops::{self, PendingFs, PendingMap};
use crate::handlers::add_dialog_acl;
use crate::invoke_handler::InvokeOutcome;
use crate::process_registry::ProcessRegistry;

/// M13 — ConsoleWindow.terminate handler.
///
/// AI sender → PendingFs::ConsoleTerminate + Dialog. compositor → 즉시 registry.terminate.
#[allow(clippy::too_many_arguments)]
pub async fn handle_terminate(
    target_id: ObjectId,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    sender_actor: &ActorId,
    pending: &PendingMap,
    req_seq: &mut u64,
    process_registry: &ProcessRegistry,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    if sender_actor.as_str().starts_with("ai:") {
        let title_str = mounted_objects
            .iter()
            .find(|o| o.id == target_id)
            .and_then(|o| o.props.get("title"))
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string();
        let mut dialog = geulos_core::std_types::dialog(
            owner.clone(),
            "AI 프로세스 종료 확인",
            &format!("AI가 '{}' 프로세스 종료를 요청합니다. 허용?", title_str),
            "warn",
            vec!["허용".into(), "거부".into()],
        );
        dialog.parent = Some(desktop_id);
        add_dialog_acl(&mut dialog);
        let dialog_id = dialog.id;
        let mm = MountMsg {
            root_object_id: dialog_id.to_string(),
            tree: serde_json::to_value(&dialog)?,
        };
        stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
        *req_seq += 1;
        let sub = SubscribeMsg {
            subscription_id: format!("sub-runtime-{}", req_seq),
            target: dialog_id.to_string(),
            kinds: vec![EventKindFilterWire::Invoke],
            include_initial: false,
        };
        stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
        mounted_objects.push(dialog);
        let (tx, _rx) = tokio::sync::oneshot::channel::<String>();
        pending.insert(
            dialog_id,
            dialog_ops::PendingEntry {
                op: PendingFs::ConsoleTerminate {
                    target_id,
                    requesting_actor: sender_actor.clone(),
                },
                tx,
            },
        );
        eprintln!(
            "[desktop-shell] AI ConsoleWindow.terminate Dialog mount (target={}, dialog={})",
            target_id, dialog_id
        );
        return Ok(InvokeOutcome::empty());
    }

    // compositor 직접 (X 닫기 또는 사용자 단축키)
    match process_registry.terminate(target_id).await {
        Ok(_) => {
            eprintln!("[desktop-shell] ConsoleWindow {} terminate OK", target_id);
        }
        Err(e) => {
            eprintln!("[desktop-shell] ConsoleWindow {} terminate 실패: {}", target_id, e);
        }
    }
    // exit waiter task가 ConsoleEvent::Exit 발행 → main loop가 status SetState.
    Ok(InvokeOutcome::empty())
}

/// M13 — ConsoleWindow.close handler. terminate alias (UI 호환).
#[allow(clippy::too_many_arguments)]
pub async fn handle_close(
    target_id: ObjectId,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    sender_actor: &ActorId,
    pending: &PendingMap,
    req_seq: &mut u64,
    process_registry: &ProcessRegistry,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    handle_terminate(
        target_id,
        stream,
        mounted_objects,
        owner,
        desktop_id,
        sender_actor,
        pending,
        req_seq,
        process_registry,
    )
    .await
}

/// M13 — ConsoleWindow.move handler. Window@1과 동형 — state.x/y SetState.
pub fn handle_move(
    target_id: ObjectId,
    args: &Value,
    mounted_objects: &mut [Object],
) -> InvokeOutcome {
    let x = args.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let y = args.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
        o.state.insert("x".into(), json!(x));
        o.state.insert("y".into(), json!(y));
    }
    InvokeOutcome {
        state_sets: vec![(target_id, "x".into(), json!(x)), (target_id, "y".into(), json!(y))],
    }
}

/// M13 — ConsoleWindow.resize handler. Window@1과 동형 — state.w/h SetState.
pub fn handle_resize(
    target_id: ObjectId,
    args: &Value,
    mounted_objects: &mut [Object],
) -> InvokeOutcome {
    let w = args.get("w").and_then(|v| v.as_i64()).unwrap_or(800) as i32;
    let h = args.get("h").and_then(|v| v.as_i64()).unwrap_or(500) as i32;
    if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
        o.state.insert("w".into(), json!(w));
        o.state.insert("h".into(), json!(h));
    }
    InvokeOutcome {
        state_sets: vec![(target_id, "w".into(), json!(w)), (target_id, "h".into(), json!(h))],
    }
}

/// M13 — ConsoleWindow.focus handler. focused=true SetState.
#[allow(dead_code)]
pub fn handle_focus(target_id: ObjectId) -> InvokeOutcome {
    InvokeOutcome { state_sets: vec![(target_id, "focused".into(), json!(true))] }
}

/// M13 — ConsoleWindow.scroll handler. state.scroll_y SetState.
pub fn handle_scroll(
    target_id: ObjectId,
    args: &Value,
    mounted_objects: &mut [Object],
) -> InvokeOutcome {
    let y = args.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
        o.state.insert("scroll_y".into(), json!(y));
    }
    InvokeOutcome { state_sets: vec![(target_id, "scroll_y".into(), json!(y))] }
}
