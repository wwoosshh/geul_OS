//! ShellRunner@1.run handler — 화이트리스트 검증 + Dialog 흐름 + tokio Command spawn.
//!
//! sender_actor가 ai:* 이면 PendingFs::ShellRun 등록 + Dialog mount, 그 외
//! (system:compositor) 즉시 execute_command. compositor의 Dialog.respond("허용")이
//! dialog_methods를 거쳐 본 모듈의 execute_command를 호출.
//!
//! 결과는 8 state SetState (last_cmd/args/cwd/exit_code/stdout/stderr/duration_ms/error).
//! M12 escape hatch — long-running 미지원, one-shot wait_with_output.

use std::path::PathBuf;
use std::time::Instant;

use geulos_core::{std_types, ActorId, Object, ObjectId};
use geulos_proto::{encode_frame, EventKindFilterWire, MountMsg, SubscribeMsg};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::dialog_ops::{self, PendingFs, PendingMap};
use crate::handlers::add_dialog_acl;
use crate::invoke_handler::InvokeOutcome;

/// ShellRunner.run(cmd, args, cwd) handler.
#[allow(clippy::too_many_arguments)]
pub async fn handle_run(
    target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    sender_actor: &ActorId,
    pending: &PendingMap,
    req_seq: &mut u64,
) -> Result<InvokeOutcome, Box<dyn std::error::Error>> {
    let cmd = args.get("cmd").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let cmd_args: Vec<String> = args
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let cwd = args.get("cwd").and_then(|v| v.as_str()).unwrap_or("").to_string();

    if cmd.is_empty() {
        return Ok(broadcast_error(mounted_objects, target_id, "cmd 비어있음").await);
    }
    let allowed = lookup_allowed_binaries(mounted_objects, target_id);
    if !allowed.contains(&cmd) {
        let msg = format!(
            "화이트리스트 외 binary: '{}'. 허용: {:?}. props.allowed_binaries 확장은 사용자만.",
            cmd, allowed
        );
        return Ok(broadcast_error(mounted_objects, target_id, &msg).await);
    }
    let cwd_path = PathBuf::from(&cwd);
    if cwd.is_empty() || !cwd_path.is_absolute() {
        return Ok(broadcast_error(
            mounted_objects,
            target_id,
            &format!("cwd는 절대 path 필수: '{}'", cwd),
        )
        .await);
    }
    if !cwd_path.exists() {
        return Ok(broadcast_error(
            mounted_objects,
            target_id,
            &format!("cwd 존재하지 않음: '{}'", cwd),
        )
        .await);
    }

    if sender_actor.as_str().starts_with("ai:") {
        let dialog_id = mount_run_dialog(
            stream,
            mounted_objects,
            owner,
            desktop_id,
            req_seq,
            &cmd,
            &cmd_args,
            &cwd,
        )
        .await?;
        let (tx, _rx) = tokio::sync::oneshot::channel::<String>();
        pending.insert(
            dialog_id,
            dialog_ops::PendingEntry {
                op: PendingFs::ShellRun {
                    cmd,
                    args: cmd_args,
                    cwd: cwd_path,
                    requesting_actor: sender_actor.clone(),
                },
                tx,
            },
        );
        eprintln!(
            "[desktop-shell] AI ShellRunner.run Dialog mount (target {}): 사용자 응답 대기",
            dialog_id
        );
        return Ok(InvokeOutcome::empty());
    }

    Ok(execute_command(mounted_objects, target_id, &cmd, &cmd_args, &cwd_path).await)
}

#[allow(clippy::too_many_arguments)]
async fn mount_run_dialog(
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    req_seq: &mut u64,
    cmd: &str,
    args: &[String],
    cwd: &str,
) -> Result<ObjectId, Box<dyn std::error::Error>> {
    let mut dialog = std_types::dialog(
        owner.clone(),
        "AI 명령 실행 확인",
        &format!(
            "AI가 다음 명령을 실행하려 합니다.\n\n  {} {}\n\ncwd: {}\n\n허용?",
            cmd,
            args.join(" "),
            cwd
        ),
        "warn",
        vec!["허용".to_string(), "거부".to_string()],
    );
    dialog.parent = Some(desktop_id);
    add_dialog_acl(&mut dialog);
    let dialog_id = dialog.id;
    let mm =
        MountMsg { root_object_id: dialog_id.to_string(), tree: serde_json::to_value(&dialog)? };
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
    Ok(dialog_id)
}

async fn broadcast_error(
    mounted_objects: &mut [Object],
    target_id: ObjectId,
    msg: &str,
) -> InvokeOutcome {
    if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
        o.state.insert("last_error".into(), json!(msg));
        o.state.insert("last_exit_code".into(), json!(-1));
    }
    eprintln!("[desktop-shell] ShellRunner 거부: {}", msg);
    InvokeOutcome {
        state_sets: vec![
            (target_id, "last_error".to_string(), json!(msg)),
            (target_id, "last_exit_code".to_string(), json!(-1)),
        ],
    }
}

fn lookup_allowed_binaries(objects: &[Object], target_id: ObjectId) -> Vec<String> {
    objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|o| o.props.get("allowed_binaries"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

fn lookup_default_timeout_ms(objects: &[Object], target_id: ObjectId) -> u64 {
    objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|o| o.props.get("default_timeout_ms"))
        .and_then(|v| v.as_u64())
        .unwrap_or(120_000)
}

/// 실제 binary 실행 + 결과 SetState. compositor 호출 또는 dialog 응답에서 진입.
pub async fn execute_command(
    mounted_objects: &mut [Object],
    target_id: ObjectId,
    cmd: &str,
    args: &[String],
    cwd: &std::path::Path,
) -> InvokeOutcome {
    let started = Instant::now();
    let timeout_ms = lookup_default_timeout_ms(mounted_objects, target_id);

    eprintln!("[desktop-shell] ShellRunner.run start: {} {:?} cwd={}", cmd, args, cwd.display());

    let spawn_result = tokio::process::Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    let child = match spawn_result {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("spawn 실패: {}", e);
            return broadcast_error(mounted_objects, target_id, &msg).await;
        }
    };

    let wait = tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms),
        child.wait_with_output(),
    )
    .await;

    let duration_ms = started.elapsed().as_millis() as u64;

    let (exit_code, stdout, stderr, error_msg) = match wait {
        Ok(Ok(out)) => (
            out.status.code().unwrap_or(-1) as i64,
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            Value::Null,
        ),
        Ok(Err(e)) => (-1, String::new(), String::new(), json!(format!("wait 실패: {}", e))),
        Err(_) => (-1, String::new(), String::new(), json!(format!("timeout {}ms", timeout_ms))),
    };

    eprintln!(
        "[desktop-shell] ShellRunner.run done: exit={} duration={}ms stdout={}b stderr={}b",
        exit_code,
        duration_ms,
        stdout.len(),
        stderr.len()
    );

    if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
        o.state.insert("last_cmd".into(), json!(cmd));
        o.state.insert("last_args".into(), json!(args));
        o.state.insert("last_cwd".into(), json!(cwd.to_string_lossy()));
        o.state.insert("last_exit_code".into(), json!(exit_code));
        o.state.insert("last_stdout".into(), json!(stdout.clone()));
        o.state.insert("last_stderr".into(), json!(stderr.clone()));
        o.state.insert("last_duration_ms".into(), json!(duration_ms));
        o.state.insert("last_error".into(), error_msg.clone());
    }

    InvokeOutcome {
        state_sets: vec![
            (target_id, "last_cmd".to_string(), json!(cmd)),
            (target_id, "last_args".to_string(), json!(args)),
            (target_id, "last_cwd".to_string(), json!(cwd.to_string_lossy())),
            (target_id, "last_exit_code".to_string(), json!(exit_code)),
            (target_id, "last_stdout".to_string(), json!(stdout)),
            (target_id, "last_stderr".to_string(), json!(stderr)),
            (target_id, "last_duration_ms".to_string(), json!(duration_ms)),
            (target_id, "last_error".to_string(), error_msg),
        ],
    }
}
