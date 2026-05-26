//! ShellRunner@1.run handler — 화이트리스트 검증 + Dialog 흐름 + tokio Command spawn.
//!
//! sender_actor가 ai:* 이면 PendingFs::ShellRun 등록 + Dialog mount, 그 외
//! (system:compositor) 즉시 execute_command_spawned. compositor의 Dialog.respond("허용")이
//! dialog_methods를 거쳐 본 모듈의 execute_command_spawned를 호출.
//!
//! 결과는 8 state SetState (last_cmd/args/cwd/exit_code/stdout/stderr/duration_ms/error).
//! M12.1 fix — main loop block 회귀 해소: tokio::spawn + mpsc channel 분리.

use std::path::PathBuf;
use std::time::Instant;

use geulos_core::{std_types, ActorId, Object, ObjectId};
use geulos_proto::{encode_frame, EventKindFilterWire, MountMsg, StateSetMsg, SubscribeMsg};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::dialog_ops::{self, PendingFs, PendingMap};
use crate::handlers::add_dialog_acl;
use crate::invoke_handler::InvokeOutcome;

/// spawned task가 main loop로 보내는 명령 실행 결과. M12.1 신규 — handler가 main loop를
/// block하지 않도록 spawn 분리. 결과는 mpsc channel로 main loop의 select! arm에 도착.
#[derive(Debug)]
pub struct ShellRunResult {
    pub target_id: ObjectId,
    pub cmd: String,
    pub args: Vec<String>,
    pub cwd: std::path::PathBuf,
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}

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
    shellrun_tx: &tokio::sync::mpsc::Sender<ShellRunResult>,
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

    // compositor 직접 호출 — spawn 분리 (M12.1 fix: block X).
    let timeout_ms = lookup_default_timeout_ms(mounted_objects, target_id);
    execute_command_spawned(target_id, cmd, cmd_args, cwd_path, timeout_ms, shellrun_tx.clone());
    Ok(InvokeOutcome::empty())
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

pub fn lookup_default_timeout_ms(objects: &[Object], target_id: ObjectId) -> u64 {
    objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|o| o.props.get("default_timeout_ms"))
        .and_then(|v| v.as_u64())
        .unwrap_or(120_000)
}

/// spawn 분리 버전 — handler block 회피. M12.1 fix.
///
/// `tokio::spawn` 안에서 Command::new + wait_with_output 실행 후 결과를 mpsc tx로 main에
/// 통지. handler 자신은 즉시 return — main loop가 다른 invoke 계속 처리.
pub fn execute_command_spawned(
    target_id: ObjectId,
    cmd: String,
    args: Vec<String>,
    cwd: std::path::PathBuf,
    timeout_ms: u64,
    tx: tokio::sync::mpsc::Sender<ShellRunResult>,
) {
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        eprintln!(
            "[desktop-shell] ShellRunner.run start (spawned): {} {:?} cwd={}",
            cmd,
            args,
            cwd.display()
        );

        // Windows .cmd/.bat fallback (M12 후속 fix).
        let spawn_one = |c: &str| -> std::io::Result<tokio::process::Child> {
            tokio::process::Command::new(c)
                .args(&args)
                .current_dir(&cwd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
        };
        let spawn_result = spawn_one(&cmd).or_else(|e| {
            if cfg!(windows) && e.kind() == std::io::ErrorKind::NotFound {
                for ext in &[".cmd", ".bat"] {
                    let with_ext = format!("{}{}", cmd, ext);
                    if let Ok(child) = spawn_one(&with_ext) {
                        eprintln!(
                            "[desktop-shell] ShellRunner: '{}' not found, fallback to '{}'",
                            cmd, with_ext
                        );
                        return Ok(child);
                    }
                }
            }
            Err(e)
        });

        let child = match spawn_result {
            Ok(c) => c,
            Err(e) => {
                let _ = tx
                    .send(ShellRunResult {
                        target_id,
                        cmd: cmd.clone(),
                        args: args.clone(),
                        cwd: cwd.clone(),
                        exit_code: -1,
                        stdout: String::new(),
                        stderr: String::new(),
                        duration_ms: started.elapsed().as_millis() as u64,
                        error: Some(format!("spawn 실패: {}", e)),
                    })
                    .await;
                return;
            }
        };

        let wait = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            child.wait_with_output(),
        )
        .await;
        let duration_ms = started.elapsed().as_millis() as u64;

        let (exit_code, stdout, stderr, error) = match wait {
            Ok(Ok(out)) => (
                out.status.code().unwrap_or(-1) as i64,
                String::from_utf8_lossy(&out.stdout).into_owned(),
                String::from_utf8_lossy(&out.stderr).into_owned(),
                None,
            ),
            Ok(Err(e)) => (-1, String::new(), String::new(), Some(format!("wait 실패: {}", e))),
            Err(_) => (-1, String::new(), String::new(), Some(format!("timeout {}ms", timeout_ms))),
        };

        eprintln!(
            "[desktop-shell] ShellRunner.run done (spawned): exit={} duration={}ms stdout={}b stderr={}b",
            exit_code,
            duration_ms,
            stdout.len(),
            stderr.len()
        );

        let _ = tx
            .send(ShellRunResult {
                target_id,
                cmd,
                args,
                cwd,
                exit_code,
                stdout,
                stderr,
                duration_ms,
                error,
            })
            .await;
    });
}

/// ShellRunResult → 8 state SetState wire 송신. main loop의 select! arm에서 호출.
pub async fn broadcast_shellrun_result(
    result: ShellRunResult,
    stream: &mut TcpStream,
    mounted_objects: &mut [Object],
    req_seq: &mut u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let target_id = result.target_id;
    let error_val = match &result.error {
        Some(s) => serde_json::json!(s),
        None => serde_json::json!(null),
    };
    if let Some(o) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
        o.state.insert("last_cmd".into(), json!(&result.cmd));
        o.state.insert("last_args".into(), json!(&result.args));
        o.state.insert("last_cwd".into(), json!(result.cwd.to_string_lossy()));
        o.state.insert("last_exit_code".into(), json!(result.exit_code));
        o.state.insert("last_stdout".into(), json!(&result.stdout));
        o.state.insert("last_stderr".into(), json!(&result.stderr));
        o.state.insert("last_duration_ms".into(), json!(result.duration_ms));
        o.state.insert("last_error".into(), error_val.clone());
    }
    let state_sets: Vec<(ObjectId, String, serde_json::Value)> = vec![
        (target_id, "last_cmd".to_string(), json!(&result.cmd)),
        (target_id, "last_args".to_string(), json!(&result.args)),
        (target_id, "last_cwd".to_string(), json!(result.cwd.to_string_lossy())),
        (target_id, "last_exit_code".to_string(), json!(result.exit_code)),
        (target_id, "last_stdout".to_string(), json!(&result.stdout)),
        (target_id, "last_stderr".to_string(), json!(&result.stderr)),
        (target_id, "last_duration_ms".to_string(), json!(result.duration_ms)),
        (target_id, "last_error".to_string(), error_val),
    ];
    for (oid, key, val) in state_sets {
        *req_seq += 1;
        let ss = StateSetMsg {
            request_id: format!("r-shellrun-{}", req_seq),
            target: oid.to_string(),
            key,
            value: val,
        };
        stream.write_all(&encode_frame(&serde_json::to_vec(&ss)?)).await?;
    }
    Ok(())
}

/// 실제 binary 실행 + 결과 SetState. compositor 호출 또는 dialog 응답에서 진입.
/// M12.1: 기존 execute_command는 유지 — 단위 test 의존성 보존.
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

    // Windows: npm/npx/yarn 같은 Node.js 도구는 *.cmd wrapper* (e.g. npx.cmd).
    // Rust Command가 PATH 검색 시 .exe만 자동 추가 → .cmd missing → NotFound.
    // *첫 spawn 실패 시 .cmd/.bat extension 자동 시도*로 우회.
    let spawn_one = |c: &str| -> std::io::Result<tokio::process::Child> {
        tokio::process::Command::new(c)
            .args(args)
            .current_dir(cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
    };
    let spawn_result = spawn_one(cmd).or_else(|e| {
        if cfg!(windows) && e.kind() == std::io::ErrorKind::NotFound {
            // .cmd → .bat 순서로 fallback (npx/npm/yarn 모두 .cmd).
            for ext in &[".cmd", ".bat"] {
                let with_ext = format!("{}{}", cmd, ext);
                if let Ok(child) = spawn_one(&with_ext) {
                    eprintln!(
                        "[desktop-shell] ShellRunner: '{}' not found, fallback to '{}'",
                        cmd, with_ext
                    );
                    return Ok(child);
                }
            }
        }
        Err(e)
    });

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
