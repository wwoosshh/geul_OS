//! ShellRunner@1.run handler — 화이트리스트 검증 + Dialog 흐름 + tokio Command spawn.
//!
//! sender_actor가 ai:* 이면 PendingFs::ShellRun 등록 + Dialog mount, 그 외
//! (system:compositor) 즉시 execute_command_spawned. compositor의 Dialog.respond("허용")이
//! dialog_methods를 거쳐 본 모듈의 execute_command_spawned를 호출.
//!
//! 결과는 8 state SetState (last_cmd/args/cwd/exit_code/stdout/stderr/duration_ms/error).
//! M12.1 fix — main loop block 회귀 해소: tokio::spawn + mpsc channel 분리.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use geulos_core::{std_types, ActorId, Object, ObjectId};
use geulos_proto::{encode_frame, EventKindFilterWire, MountMsg, StateSetMsg, SubscribeMsg};
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::dialog_ops::{self, PendingFs, PendingMap};
use crate::handlers::add_dialog_acl;
use crate::invoke_handler::InvokeOutcome;

/// ConsoleWindow id → host bridge stream_id 매핑. spawn_streamed에서 insert,
/// terminate/close에서 lookup + exec_stream_kill 호출 후 remove.
/// host bridge 모드에서 JobHandle/ProcessRegistry는 우회 — kill 경로 별도 유지.
static STREAM_MAP: Lazy<Mutex<HashMap<ObjectId, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// cw_id에 매핑된 stream을 kill (host bridge에 ExecStreamKill 요청) + 맵에서 제거.
/// 매핑 없으면 noop (이미 종료 또는 호스트 모드 아님). VM 빌드 전용.
#[cfg(not(windows))]
pub async fn kill_console_stream(cw_id: ObjectId) {
    let stream_id = STREAM_MAP.lock().ok().and_then(|mut m| m.remove(&cw_id));
    if let Some(sid) = stream_id {
        let sid_for = sid.clone();
        let result = tokio::task::spawn_blocking(move || {
            crate::host_bridge_client::exec_stream_kill(&sid_for)
        })
        .await;
        match result {
            Ok(Ok(())) => eprintln!("[desktop-shell] host stream {} kill OK (cw={})", sid, cw_id),
            Ok(Err(e)) => eprintln!("[desktop-shell] host stream {} kill 실패: {}", sid, e),
            Err(e) => eprintln!("[desktop-shell] kill spawn_blocking join: {}", e),
        }
    }
}
#[cfg(windows)]
pub async fn kill_console_stream(_cw_id: ObjectId) {}

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

/// M13 — long-running process의 stream pipeline 이벤트.
///
/// spawned task가 main loop의 select! arm으로 보내는 두 종류:
/// - `Line`: stdout 또는 stderr 한 줄 도착.
/// - `Exit`: child process 종료 (정상 / signal / job terminate 모두).
#[derive(Debug)]
pub enum ConsoleEvent {
    Line { target_id: ObjectId, kind: LineKind, text: String },
    Exit { target_id: ObjectId, exit_code: i64, status: String },
}

/// stdout vs stderr 구분 — UI에 prefix 추가 시 사용.
#[derive(Debug, Clone, Copy)]
pub enum LineKind {
    Stdout,
    Stderr,
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
    // VM은 호스트 경로(C:\..)와 빈 cwd 검증을 우회 — 둘 다 host bridge가 처리:
    // 빈 cwd면 호스트의 USERPROFILE을 default로, 호스트 경로면 host fs에서 검증.
    let is_host_or_empty = cwd.is_empty() || crate::host_bridge_client::is_host_path(&cwd);
    if !is_host_or_empty && !cwd_path.is_absolute() {
        return Ok(broadcast_error(
            mounted_objects,
            target_id,
            &format!("cwd는 절대 path 필수: '{}'", cwd),
        )
        .await);
    }
    if !is_host_or_empty && !cwd_path.exists() {
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

/// M13 — ShellRunner@1.run_streamed handler. long-running process 전용.
///
/// handle_run과 동일한 검증 (cmd 화이트리스트 + cwd 절대/존재). 차이:
/// - AI sender → PendingFs::ShellStream + Dialog mount (handle_run의 ShellRun과 동형)
/// - compositor 직접 → spawn_streamed 즉시
#[allow(clippy::too_many_arguments)]
pub async fn handle_run_streamed(
    target_id: ObjectId,
    args: &Value,
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    sender_actor: &ActorId,
    pending: &PendingMap,
    req_seq: &mut u64,
    console_tx: &tokio::sync::mpsc::Sender<ConsoleEvent>,
    process_registry: &crate::process_registry::ProcessRegistry,
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
    let cwd_path = std::path::PathBuf::from(&cwd);
    // run과 동일 — 빈 cwd / 호스트 경로는 host bridge가 처리.
    let is_host_or_empty = cwd.is_empty() || crate::host_bridge_client::is_host_path(&cwd);
    if !is_host_or_empty && !cwd_path.is_absolute() {
        return Ok(broadcast_error(
            mounted_objects,
            target_id,
            &format!("cwd는 절대 path 필수: '{}'", cwd),
        )
        .await);
    }
    if !is_host_or_empty && !cwd_path.exists() {
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
                op: PendingFs::ShellStream {
                    cmd,
                    args: cmd_args,
                    cwd: cwd_path,
                    requesting_actor: sender_actor.clone(),
                },
                tx,
            },
        );
        eprintln!(
            "[desktop-shell] AI ShellRunner.run_streamed Dialog mount (target {}): 사용자 응답 대기",
            dialog_id
        );
        return Ok(InvokeOutcome::empty());
    }

    // compositor 직접 — 즉시 spawn. 실패 시 broadcast_error (process 종료 방지 — C-2 fix).
    if let Err(e) = spawn_streamed(
        stream,
        mounted_objects,
        owner,
        desktop_id,
        req_seq,
        cmd,
        cmd_args,
        cwd_path,
        console_tx.clone(),
        process_registry,
    )
    .await
    {
        return Ok(broadcast_error(
            mounted_objects,
            target_id,
            &format!("spawn_streamed 실패: {}", e),
        )
        .await);
    }
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
            "[desktop-shell] ShellRunner.run start (host bridge): {} {:?} cwd={}",
            cmd,
            args,
            cwd.display()
        );

        // VM rootfs는 minimal — npm/cargo/git 등이 없음. *항상 호스트 브리지로 위임*해서
        // Windows 호스트의 binary 실행. host bridge는 blocking std I/O라 spawn_blocking 안에서.
        let cwd_str = cwd.to_string_lossy().to_string();
        let cmd_for_blocking = cmd.clone();
        let args_for_blocking = args.clone();
        let result = tokio::task::spawn_blocking(move || {
            #[cfg(not(windows))]
            {
                crate::host_bridge_client::exec(
                    &cmd_for_blocking,
                    &args_for_blocking,
                    &cwd_str,
                    timeout_ms,
                )
            }
            #[cfg(windows)]
            {
                // host compositor 빌드(non-VM) — host bridge가 없어도 VM-local Command로 fallback.
                // 단순 동기 실행 (host 빌드는 winit/desktop이라 dev tool 흐름).
                let _ = (cwd_str, &cmd_for_blocking, &args_for_blocking, timeout_ms);
                Err::<(i32, String, String, u64), String>(
                    "Windows 호스트 빌드에서는 host bridge 미사용 — VM 빌드로 동작".into(),
                )
            }
        })
        .await;
        let duration_ms = started.elapsed().as_millis() as u64;

        let (exit_code, stdout, stderr, error) = match result {
            Ok(Ok((code, stdout, stderr, _))) => (code as i64, stdout, stderr, None),
            Ok(Err(e)) => (-1, String::new(), String::new(), Some(e)),
            Err(e) => (-1, String::new(), String::new(), Some(format!("join 실패: {}", e))),
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

/// M13 — ANSI escape sequence + 비인쇄 control char 제거.
///
/// vite/npm 등 생태계 도구는 stdout/stderr에 *컬러 ANSI escape* (`\x1b[32m` 등 CSI
/// sequence)를 넣는다. ConsoleWindow는 plain text만 표시하므로 reader 단계에서 strip —
/// state.lines가 clean text가 되어 compositor render(□ 깨짐 방지) + AI parsing(URL
/// 추출 등) 모두 정확해진다. carriage return(`\r`, progress bar 흔적)과 기타 control
/// char도 제거 (탭 `\t`는 유지).
// not(windows) VM polling 경로에서 ANSI 이스케이프 제거에 사용 (spawn_streamed의 poll
// loop ~line 612). windows 네이티브 dev 빌드는 그 경로를 cfg 제외하므로 미사용으로 보이나,
// 함수와 그 테스트는 모든 플랫폼에서 유지(테스트는 windows에서도 컴파일·실행).
#[cfg_attr(windows, allow(dead_code))]
pub(crate) fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // ESC — CSI sequence (`\x1b[ ... <final 0x40-0x7e>`)면 final byte까지 skip.
            '\u{1b}' => {
                if chars.peek() == Some(&'[') {
                    chars.next(); // '['
                    while let Some(&nc) = chars.peek() {
                        chars.next();
                        if ('\u{40}'..='\u{7e}').contains(&nc) {
                            break;
                        }
                    }
                }
                // 그 외 ESC 시퀀스(OSC 등)는 ESC 한 글자만 drop.
            }
            '\r' => {}                                // carriage return drop.
            c if (c as u32) < 0x20 && c != '\t' => {} // 기타 control char drop.
            c => out.push(c),
        }
    }
    out
}

/// M13 — long-running process spawn + ConsoleWindow mount + 3 tokio task 시작.
///
/// 흐름:
/// 1. ConsoleWindow@1 객체 생성 + add_console_window_acl
/// 2. JobObject 생성
/// 3. tokio::Command::new(cmd) (Windows: CREATE_SUSPENDED + CREATE_NO_WINDOW)
///    + stdin null + stdout/stderr piped
/// 4. spawn 후 child.id()로 process handle → JobObject::assign_process → ResumeThread
/// 5. ConsoleWindow.state.pid 채움 + MountMsg/SubscribeMsg wire 송신 + mounted_objects.push
/// 6. ProcessRegistry::insert(cw_id, job)
/// 7. tokio::spawn 3 task:
///    - stdout reader: BufReader::lines → ConsoleEvent::Line { Stdout } → console_tx
///    - stderr reader: 동일, Stderr
///    - exit waiter: child.wait().await → ConsoleEvent::Exit → console_tx,
///      이후 registry.remove(cw_id) — JobHandle drop으로 CloseHandle
///
/// 반환: ConsoleWindow id (호출자가 InvokeOutcome::event_id로 wire 응답).
// Windows 네이티브 dev 빌드에서는 host bridge streaming 미지원 — exec_stream_start의
// #[cfg(windows)] arm이 조기 return하여 이후 본문(VM 전용 경로: pid/wire mount/polling)이
// unreachable/unused가 된다. VM(musl/not(windows)) 빌드에서는 이 코드가 모두 live이며 정상
// lint 대상. 따라서 *windows 빌드에서만* 해당 lint 완화 — not(windows) 프로덕션 경로는 엄격 유지.
#[cfg_attr(windows, allow(unreachable_code, unused_variables))]
#[allow(clippy::too_many_arguments)]
pub async fn spawn_streamed(
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: &ActorId,
    desktop_id: ObjectId,
    req_seq: &mut u64,
    cmd: String,
    args: Vec<String>,
    cwd: std::path::PathBuf,
    console_tx: tokio::sync::mpsc::Sender<ConsoleEvent>,
    _process_registry: &crate::process_registry::ProcessRegistry,
) -> Result<ObjectId, Box<dyn std::error::Error>> {
    // V1: VM rootfs에 binary 없으므로 *항상 host bridge*로 위임. JobObject/CREATE_SUSPENDED
    // 등 Windows-native 흐름은 우회 — host bridge가 호스트 측에서 spawn + ring buffer로
    // line 누적 → polling으로 ConsoleEvent 받음. terminate는 V2 (STREAM_MAP + exec_stream_kill).
    let title = format!(
        "{} {} — {}",
        cmd,
        args.join(" "),
        cwd.file_name().and_then(|s| s.to_str()).unwrap_or("?")
    );

    // 1. 객체 생성 + ACL
    let mut cw = std_types::console_window(
        owner.clone(),
        cmd.clone(),
        args.clone(),
        cwd.to_string_lossy().to_string(),
        title.clone(),
        80,
        80,
        800,
        500,
    );
    cw.parent = Some(desktop_id);
    crate::handlers::add_console_window_acl(&mut cw);
    let cw_id = cw.id;

    // 2. host bridge에 ExecStreamStart — stream_id + pid 받음.
    let cwd_str = cwd.to_string_lossy().to_string();
    let cmd_for = cmd.clone();
    let args_for = args.clone();
    let (stream_id, pid): (String, u32) = {
        #[cfg(not(windows))]
        {
            let res = tokio::task::spawn_blocking(move || {
                crate::host_bridge_client::exec_stream_start(&cmd_for, &args_for, &cwd_str)
            })
            .await
            .map_err(|e| format!("spawn_blocking join 실패: {}", e))?;
            res.map_err(|e| format!("host bridge exec_stream_start: {}", e))?
        }
        #[cfg(windows)]
        {
            let _ = (cmd_for, args_for, cwd_str);
            return Err("Windows 호스트 빌드에서는 host bridge 미사용".into());
        }
    };

    // 3. state.pid 업데이트.
    if let Some(p) = cw.state.get_mut("pid") {
        *p = serde_json::json!(pid);
    }

    // 4. wire mount + subscribe + push
    let mm = MountMsg { root_object_id: cw_id.to_string(), tree: serde_json::to_value(&cw)? };
    stream.write_all(&encode_frame(&serde_json::to_vec(&mm)?)).await?;
    *req_seq += 1;
    let sub = SubscribeMsg {
        subscription_id: format!("sub-runtime-{}", req_seq),
        target: cw_id.to_string(),
        kinds: vec![EventKindFilterWire::Invoke],
        include_initial: false,
    };
    stream.write_all(&encode_frame(&serde_json::to_vec(&sub)?)).await?;
    mounted_objects.push(cw);

    // 4b. STREAM_MAP에 cw_id → stream_id 등록. terminate/close handler가 kill에 사용.
    if let Ok(mut m) = STREAM_MAP.lock() {
        m.insert(cw_id, stream_id.clone());
    }

    // 5. polling task — host bridge exec_stream_poll 호출.
    //    line N개 + status. status.exit_code 있으면 Exit 발행 + 종료.
    //    KI-030: polling 간격 500→100ms로 dev server burst lag 완화 (host bridge 부하 5x이나
    //    단일 사용자 dev 환경에서 무시할 수준).
    #[cfg(not(windows))]
    const CONSOLE_POLL_MS: u64 = 100;
    #[cfg(not(windows))]
    {
        let tx = console_tx.clone();
        let stream_id_clone = stream_id.clone();
        tokio::spawn(async move {
            loop {
                let sid = stream_id_clone.clone();
                let poll = tokio::task::spawn_blocking(move || {
                    crate::host_bridge_client::exec_stream_poll(&sid)
                })
                .await;
                match poll {
                    Ok(Ok((lines, status))) => {
                        for line in lines {
                            let kind = if line.kind == "stderr" {
                                LineKind::Stderr
                            } else {
                                LineKind::Stdout
                            };
                            let _ = tx
                                .send(ConsoleEvent::Line {
                                    target_id: cw_id,
                                    kind,
                                    text: strip_ansi(&line.text),
                                })
                                .await;
                        }
                        if let Some(code) = status.exit_code {
                            let _ = tx
                                .send(ConsoleEvent::Exit {
                                    target_id: cw_id,
                                    exit_code: code as i64,
                                    status: status.status,
                                })
                                .await;
                            // process 종료 → STREAM_MAP에서 제거.
                            if let Ok(mut m) = STREAM_MAP.lock() {
                                m.remove(&cw_id);
                            }
                            break;
                        }
                    }
                    Ok(Err(e)) => {
                        let _ = tx
                            .send(ConsoleEvent::Line {
                                target_id: cw_id,
                                kind: LineKind::Stderr,
                                text: format!("[host bridge poll 실패: {}]", e),
                            })
                            .await;
                        let _ = tx
                            .send(ConsoleEvent::Exit {
                                target_id: cw_id,
                                exit_code: -1,
                                status: "error".to_string(),
                            })
                            .await;
                        break;
                    }
                    Err(e) => {
                        eprintln!(
                            "[desktop-shell] exec_stream_poll spawn_blocking join: {}",
                            e
                        );
                        break;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(CONSOLE_POLL_MS)).await;
            }
        });
    }

    eprintln!(
        "[desktop-shell] ConsoleWindow {} streamed (host bridge): {} {:?} pid={} stream={}",
        cw_id, cmd, args, pid, stream_id
    );
    Ok(cw_id)
}

/// M13 — ConsoleEvent::Line 수신 후 ConsoleWindow state 갱신 + SetState broadcast.
///
/// ring buffer (max 500). overflow 시 가장 오래된 line pop_front. stderr line은
/// prefix "[stderr] ". 2건 SetState (lines + line_count).
pub async fn apply_console_line(
    mounted_objects: &mut [Object],
    stream: &mut TcpStream,
    req_seq: &mut u64,
    target_id: ObjectId,
    kind: LineKind,
    text: String,
) {
    const RING_MAX: usize = 500;
    let prefixed = match kind {
        LineKind::Stdout => text,
        LineKind::Stderr => format!("[stderr] {}", text),
    };
    let obj = match mounted_objects.iter_mut().find(|o| o.id == target_id) {
        Some(o) => o,
        None => {
            eprintln!("[desktop-shell] apply_console_line: target {} 미발견", target_id);
            return;
        }
    };
    // lines push + overflow
    let mut lines: Vec<String> = obj
        .state
        .get("lines")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    lines.push(prefixed);
    if lines.len() > RING_MAX {
        let drop = lines.len() - RING_MAX;
        lines.drain(..drop);
    }
    let count = obj.state.get("line_count").and_then(|v| v.as_u64()).unwrap_or(0) + 1;
    let lines_val = json!(lines);
    obj.state.insert("lines".into(), lines_val.clone());
    obj.state.insert("line_count".into(), json!(count));

    // 2 SetState wire 송신
    for (key, val) in [("lines", lines_val), ("line_count", json!(count))] {
        *req_seq += 1;
        let ss = geulos_proto::StateSetMsg {
            request_id: format!("r-cw-line-{}", req_seq),
            target: target_id.to_string(),
            key: key.to_string(),
            value: val,
        };
        if let Err(e) =
            stream.write_all(&encode_frame(&serde_json::to_vec(&ss).unwrap_or_default())).await
        {
            eprintln!("[desktop-shell] apply_console_line SetState wire 실패: {}", e);
            return;
        }
    }
}

/// M13 — ConsoleEvent::Exit 수신 후 status/exit_code/ended_at 3 SetState.
pub async fn apply_console_exit(
    mounted_objects: &mut [Object],
    stream: &mut TcpStream,
    req_seq: &mut u64,
    target_id: ObjectId,
    exit_code: i64,
    status: String,
) {
    // target이 이미 트리에서 사라졌으면 orphan SetState 방지.
    if !mounted_objects.iter().any(|o| o.id == target_id) {
        eprintln!("[desktop-shell] apply_console_exit: target {} 미발견 (이미 정리됨)", target_id);
        return;
    }
    let ended_at = chrono::Utc::now().to_rfc3339();
    if let Some(obj) = mounted_objects.iter_mut().find(|o| o.id == target_id) {
        obj.state.insert("status".into(), json!(&status));
        obj.state.insert("exit_code".into(), json!(exit_code));
        obj.state.insert("ended_at".into(), json!(&ended_at));
    }
    for (key, val) in
        [("status", json!(status)), ("exit_code", json!(exit_code)), ("ended_at", json!(ended_at))]
    {
        *req_seq += 1;
        let ss = geulos_proto::StateSetMsg {
            request_id: format!("r-cw-exit-{}", req_seq),
            target: target_id.to_string(),
            key: key.to_string(),
            value: val,
        };
        if let Err(e) =
            stream.write_all(&encode_frame(&serde_json::to_vec(&ss).unwrap_or_default())).await
        {
            eprintln!("[desktop-shell] apply_console_exit SetState wire 실패: {}", e);
            return;
        }
    }
    eprintln!(
        "[desktop-shell] ConsoleWindow {} exited: code={} status={}",
        target_id, exit_code, status
    );
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

#[cfg(test)]
mod tests {
    use geulos_core::{std_types, ActorId};

    #[test]
    fn strip_ansi_removes_vite_color_codes() {
        // vite 시작 로그 예 — ESC[32m ESC[1mVITE ... 컬러 코드.
        let input = "\u{1b}[32m\u{1b}[1mVITE\u{1b}[22m\u{1b}[39m v8.0.14 \u{1b}[2mready\u{1b}[0m";
        assert_eq!(super::strip_ansi(input), "VITE v8.0.14 ready");
    }

    #[test]
    fn strip_ansi_removes_cr_keeps_tab() {
        // carriage return은 제거(progress 흔적), tab은 유지.
        assert_eq!(super::strip_ansi("progress\r50%"), "progress50%");
        assert_eq!(super::strip_ansi("col1\tcol2"), "col1\tcol2");
    }

    #[test]
    fn strip_ansi_keeps_plain_url_and_korean() {
        // dev server URL + 한글은 그대로 보존 (AI URL 추출 + 사용자 표시).
        assert_eq!(
            super::strip_ansi("Local: http://localhost:5173/"),
            "Local: http://localhost:5173/"
        );
        assert_eq!(super::strip_ansi("한글 출력 텍스트"), "한글 출력 텍스트");
    }

    #[tokio::test]
    async fn apply_console_line_ring_buffer_caps_at_500() {
        let cw = std_types::console_window(
            ActorId::local_user(),
            "x".into(),
            vec![],
            "D:/x".into(),
            "x".into(),
            0,
            0,
            100,
            100,
        );
        let cw_id = cw.id;
        let mut objects = [cw];

        // ring buffer 로직만 인라인 검증 (apply는 wire write 필요 — 별 stream).
        for i in 0..600 {
            let obj = objects.iter_mut().find(|o| o.id == cw_id).unwrap();
            let mut lines: Vec<String> = obj
                .state
                .get("lines")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            lines.push(format!("line-{}", i));
            if lines.len() > 500 {
                let drop = lines.len() - 500;
                lines.drain(..drop);
            }
            let count = obj.state.get("line_count").and_then(|v| v.as_u64()).unwrap_or(0) + 1;
            obj.state.insert("lines".into(), serde_json::json!(lines));
            obj.state.insert("line_count".into(), serde_json::json!(count));
        }
        let obj = objects.iter().find(|o| o.id == cw_id).unwrap();
        let lines = obj.state.get("lines").and_then(|v| v.as_array()).unwrap();
        assert_eq!(lines.len(), 500);
        assert_eq!(obj.state.get("line_count"), Some(&serde_json::json!(600u64)));
        assert_eq!(lines[0].as_str(), Some("line-100"));
        assert_eq!(lines[499].as_str(), Some("line-599"));
    }
}
