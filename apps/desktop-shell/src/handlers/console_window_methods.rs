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

/// M13 — ConsoleWindow.close handler (X 버튼). Window@1.handle_close 패턴:
///
/// 1. running process면 terminate (TerminateJobObject cascade kill). 이미
///    exited/terminated면 registry 매핑 없어 Err — 무시 (창만 닫으면 됨).
/// 2. 객체 destroy — mounted_objects + Desktop.children 제거 + destroyed=true
///    broadcast. compositor layout_desktop이 destroyed=true를 skip → 창 사라짐.
///
/// close는 compositor(X 클릭) 전용 (ACL: AI는 terminate만) → Dialog 불필요, 즉시.
/// terminate(별 method)는 process kill만 + 창 유지 (AI가 종료 요청, 사용자가 확인 후
/// X로 닫음). close는 terminate + 창 제거를 *함께* — 사용자 "창 닫기" 기대와 일치.
pub async fn handle_close(
    target_id: ObjectId,
    desktop_id: ObjectId,
    mounted_objects: &mut Vec<Object>,
    process_registry: &ProcessRegistry,
) -> InvokeOutcome {
    // 1. running이면 process tree kill. 이미 죽었으면 Err — 무시.
    if let Err(e) = process_registry.terminate(target_id).await {
        eprintln!("[desktop-shell] ConsoleWindow {} close: 이미 종료됨 ({})", target_id, e);
    }
    // 2. 객체 destroy (Window@1.handle_close와 동일 — tombstone 우회).
    mounted_objects.retain(|o| o.id != target_id);
    if let Some(d) = mounted_objects.iter_mut().find(|o| o.id == desktop_id) {
        d.children.retain(|c| *c != target_id);
    }
    eprintln!("[desktop-shell] ConsoleWindow {} 닫기 (destroy)", target_id);
    InvokeOutcome { state_sets: vec![(target_id, "destroyed".to_string(), json!(true))] }
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
/// 최소 크기 (200x120) 강제 — window_methods::handle_resize와 일관 (M13 T8 M-1 fix).
pub fn handle_resize(
    target_id: ObjectId,
    args: &Value,
    mounted_objects: &mut [Object],
) -> InvokeOutcome {
    let w = (args.get("w").and_then(|v| v.as_i64()).unwrap_or(800) as i32).max(200);
    let h = (args.get("h").and_then(|v| v.as_i64()).unwrap_or(500) as i32).max(120);
    if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
        o.state.insert("w".into(), json!(w));
        o.state.insert("h".into(), json!(h));
    }
    InvokeOutcome {
        state_sets: vec![(target_id, "w".into(), json!(w)), (target_id, "h".into(), json!(h))],
    }
}

/// M13 — ConsoleWindow.focus handler.
///
/// Window@1.handle_focus와 동형: floating(Window@1 + ConsoleWindow@1) 전체의 max z+1을 target에
/// 부여, 다른 floating 객체들은 focused=false. Window@1과 같은 z-space를 공유해야 서로 앞으로
/// 올라올 수 있다.
pub fn handle_focus(target_id: ObjectId, mounted_objects: &mut [Object]) -> InvokeOutcome {
    let new_z = crate::window_ops::max_z(mounted_objects) + 1;
    let mut outs = vec![];
    for o in mounted_objects.iter_mut() {
        let is_floating =
            matches!(o.type_uri.as_str(), "aios.builtin/Window@1" | "aios.builtin/ConsoleWindow@1");
        if is_floating {
            let is_target = o.id == target_id;
            o.state.insert("focused".into(), json!(is_target));
            outs.push((o.id, "focused".to_string(), json!(is_target)));
            if is_target {
                // ConsoleWindow에 z state 부여 (factory에 초기값 없어도 state는 동적 — insert OK).
                o.state.insert("z".into(), json!(new_z));
                outs.push((o.id, "z".to_string(), json!(new_z)));
            }
        }
    }
    InvokeOutcome { state_sets: outs }
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
