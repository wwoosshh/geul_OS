//! Auto CRUD demo — controller(Claude)가 GeulOS를 직접 wire connection으로 검증.
//!
//! 사용자 비전: 사용자/AI/외부 client 모두 *동일 wire 프로토콜*이라 *외부에서 직접 조작
//! 가능*. 이 example로 launcher가 띄운 GeulOS에 wire connect → 실제 동작 + 보안 + 토큰
//! 효율을 자동 검증.
//!
//! 실행: `cargo run --example auto_crud_demo` (launcher 띄워둔 상태)
//!
//! 본 T1 단계: read-only baseline + 보안 일부.
//! - 모든 STD_TYPES 객체 dump (mounted 상태)
//! - 각 객체의 ACL 패턴이 M11 spec과 일치하는지
//! - File@1 한 건 read 시도 (state.content 확인)
//! - AI가 Dialog.respond 시도 → PermissionDenied (KI-001 차단)
//!
//! T2/T3/T4는 후속.

use geulos_ai_bridge::wire::WireClient;
use geulos_proto::{decode_frame, encode_frame, Hello, HelloAck, InvokeAck, InvokeMsg, Role};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// 시뮬 compositor connect — Role::Compositor로 hello. server-host는 auth 무검증
/// 발급 (KI 후보) 이라 외부 client가 system:compositor 권한 받음. *사용자 비전*상
/// 외부 검증 가능 = AI 검증 가능. 본 example의 핵심 메커니즘.
async fn connect_as_compositor(
    addr: &str,
) -> Result<(TcpStream, Vec<u8>, String), Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(addr).await?;
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Compositor,
        auth: serde_json::json!({}),
        client_id: "auto-crud-compositor".to_string(),
    };
    let body = serde_json::to_vec(&hello)?;
    stream.write_all(&encode_frame(&body)).await?;
    let mut accum: Vec<u8> = Vec::new();
    let mut tmp = vec![0u8; 4096];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Err("hello 전에 끊김".into());
        }
        accum.extend_from_slice(&tmp[..n]);
        let mut slice = accum.as_slice();
        if let Ok(frame) = decode_frame(&mut slice) {
            let consumed = accum.len() - slice.len();
            accum.drain(..consumed);
            let ack: HelloAck = serde_json::from_slice(&frame)?;
            return Ok((stream, accum, ack.actor_id));
        }
    }
}

/// 시뮬 compositor가 invoke 송신 + 응답 받음. WireClient::invoke와 동등 로직 (생 wire).
async fn compositor_invoke(
    stream: &mut TcpStream,
    accum: &mut Vec<u8>,
    target: &str,
    method: &str,
    args: serde_json::Value,
) -> Result<Result<String, String>, Box<dyn std::error::Error>> {
    let req_id = format!("c-{}", uuid::Uuid::new_v4());
    let inv = InvokeMsg {
        request_id: req_id.clone(),
        target: target.to_string(),
        method: method.to_string(),
        args,
    };
    let body = serde_json::to_vec(&inv)?;
    stream.write_all(&encode_frame(&body)).await?;
    let mut tmp = vec![0u8; 16384];
    loop {
        let mut slice = accum.as_slice();
        if let Ok(frame) = decode_frame(&mut slice) {
            let consumed = accum.len() - slice.len();
            accum.drain(..consumed);
            // InvokeAck 또는 InvokeError 두 가지 응답.
            let v: serde_json::Value = serde_json::from_slice(&frame)?;
            match v.get("kind").and_then(|k| k.as_str()) {
                Some("InvokeAck") => {
                    let a: InvokeAck = serde_json::from_value(v)?;
                    return Ok(Ok(a.event_id));
                }
                Some("InvokeError") => {
                    let detail =
                        v.get("detail").and_then(|d| d.as_str()).unwrap_or("unknown").to_string();
                    return Ok(Err(detail));
                }
                _ => continue, // 다른 frame (event 등) — 무시하고 다음 read
            }
        }
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Err("응답 전에 끊김".into());
        }
        accum.extend_from_slice(&tmp[..n]);
    }
}

const SERVER_ADDR: &str = "127.0.0.1:5550";

const STD_TYPES: &[&str] = &[
    "aios.builtin/Desktop@1",
    "aios.builtin/FileTree@1",
    "aios.builtin/Explorer@1",
    "aios.builtin/Cli@1",
    "aios.builtin/Window@1",
    "aios.builtin/Dialog@1",
    "aios.builtin/Filesystem@1",
    "aios.builtin/ShellRunner@1", // M12 추가
    "aios.std/Folder@1",
    "aios.std/File@1",
];

/// 통계 누적 — 토큰 효율 측정.
#[derive(Default)]
struct Stats {
    wire_frames_sent: usize,
    wire_frames_received: usize,
    total_wire_bytes_received: usize,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Auto CRUD Demo (controller-as-tester) ===");
    println!("Connecting to {}...", SERVER_ADDR);

    let mut ai = WireClient::connect_as_ai(SERVER_ADDR).await?;
    println!("AI connection OK. actor_id = {}\n", ai.actor_id());

    let mut stats = Stats::default();

    // ─────────────────── STAGE 1: 객체 tree dump ───────────────────
    println!("─── Stage 1: 객체 tree dump (mounted state baseline) ───");
    let mut total_objects = 0;
    let mut file_id_for_read: Option<String> = None;
    let mut file_path_for_read: Option<String> = None;
    let mut dialog_ids: Vec<String> = Vec::new();

    for type_uri in STD_TYPES {
        let started = Instant::now();
        let ids = ai.query_by_type(type_uri).await?;
        let latency_ms = started.elapsed().as_millis();
        stats.wire_frames_sent += 1;
        stats.wire_frames_received += 1;
        println!("  {} → {} objects ({}ms)", type_uri, ids.len(), latency_ms);
        total_objects += ids.len();

        // Filesystem/Window 같은 singleton은 dump 안 함 (너무 크거나 redundant).
        // File@1만 첫 한 건 props 추출.
        if *type_uri == "aios.std/File@1" && !ids.is_empty() {
            for id in &ids {
                let obj = ai.get_object(id).await?;
                stats.wire_frames_sent += 1;
                stats.wire_frames_received += 1;
                if let Some(bytes) = serde_json::to_vec(&obj).ok() {
                    stats.total_wire_bytes_received += bytes.len();
                }
                let path = obj
                    .get("props")
                    .and_then(|p| p.get("path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let name = obj
                    .get("props")
                    .and_then(|p| p.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let mime = obj
                    .get("props")
                    .and_then(|p| p.get("mime"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                // 첫 text/markdown 또는 text/plain 파일을 read 후보로 선택.
                if file_id_for_read.is_none()
                    && (mime.starts_with("text/")
                        || name.ends_with(".md")
                        || name.ends_with(".txt")
                        || name.ends_with(".toml"))
                {
                    file_id_for_read = Some(id.clone());
                    file_path_for_read = Some(path.to_string());
                }
            }
        }
        if *type_uri == "aios.builtin/Dialog@1" {
            dialog_ids = ids.clone();
        }
    }
    println!("\n총 {} 객체 mounted.\n", total_objects);

    // ─────────────────── STAGE 2: ACL 패턴 검증 ───────────────────
    println!("─── Stage 2: ACL 패턴 spec 일치 검증 ───");
    let mut acl_issues: Vec<String> = Vec::new();

    let dialog_ids_for_check = ai.query_by_type("aios.builtin/Dialog@1").await?;
    for d_id in &dialog_ids_for_check {
        let obj = ai.get_object(d_id).await?;
        let acl = obj.get("acl").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        // Dialog@1 spec: SystemCompositor + Exact("respond") + Allow / App("desktop-shell") + SetState + Allow
        let has_compositor_respond = acl.iter().any(|e| {
            e.get("actor").and_then(|a| a.as_str()) == Some("SystemCompositor")
                && e.get("method").and_then(|m| m.get("Exact")).and_then(|s| s.as_str())
                    == Some("respond")
        });
        if !has_compositor_respond {
            acl_issues.push(format!("Dialog {} — SystemCompositor + respond Allow 누락", d_id));
        }
    }
    let filesystem_ids = ai.query_by_type("aios.builtin/Filesystem@1").await?;
    for fs_id in &filesystem_ids {
        let obj = ai.get_object(fs_id).await?;
        let acl = obj.get("acl").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let has_shell_setstate = acl.iter().any(|e| {
            e.get("actor").and_then(|a| a.get("App")).and_then(|s| s.as_str())
                == Some("desktop-shell")
                && e.get("method").and_then(|m| m.as_str()) == Some("SetState")
        });
        if !has_shell_setstate {
            acl_issues.push(format!("Filesystem {} — desktop-shell + SetState Allow 누락", fs_id));
        }
    }
    if acl_issues.is_empty() {
        println!("  ✅ ACL 패턴 일치 (Dialog respond/Filesystem SetState 등)");
    } else {
        println!("  ❌ ACL issues:");
        for issue in &acl_issues {
            println!("    - {}", issue);
        }
    }
    println!();

    // ─────────────────── STAGE 3: File.read 한 건 시뮬 ───────────────────
    println!("─── Stage 3: AI File.read 시뮬 (가장 기본 흐름) ───");
    if let Some(fid) = &file_id_for_read {
        let path = file_path_for_read.as_deref().unwrap_or("?");
        println!("  대상: {} (path={})", fid, path);
        let started = Instant::now();
        let event_id = ai.invoke(fid, "read", serde_json::json!({})).await?;
        let latency_ms = started.elapsed().as_millis();
        stats.wire_frames_sent += 1;
        stats.wire_frames_received += 1;
        println!("  invoke File.read → event_id={} ({}ms)", event_id, latency_ms);

        // SetState 도착 대기 — 100ms 후 get_object로 state.content 확인.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let obj_after = ai.get_object(fid).await?;
        stats.wire_frames_sent += 1;
        stats.wire_frames_received += 1;
        let content_len = obj_after
            .get("state")
            .and_then(|s| s.get("content"))
            .and_then(|v| v.as_str())
            .map(|s| s.len())
            .unwrap_or(0);
        if content_len > 0 {
            println!("  ✅ state.content {} bytes 도착", content_len);
        } else {
            println!("  ⚠️ state.content 비어있음 (read 처리 실패 또는 SetState 미도착)");
        }
    } else {
        println!("  ⚠️ text File 객체 없음 — skip");
    }
    println!();

    // ─────────────────── STAGE 4: 보안 — AI가 Dialog.respond 시도 ───────────────────
    println!("─── Stage 4: 보안 — AI가 Dialog.respond 시도 → PermissionDenied 기대 ───");
    if dialog_ids.is_empty() {
        println!("  ℹ️ Dialog 객체 없음 — 보안 시나리오 시뮬 위해 임의 UUID로 시도");
        // 임의 UUID로도 *NotFound or PermissionDenied* 차단되어야.
        let fake_id = uuid::Uuid::new_v4().to_string();
        let result = ai.invoke(&fake_id, "respond", serde_json::json!({"action": "허용"})).await;
        stats.wire_frames_sent += 1;
        stats.wire_frames_received += 1;
        match result {
            Ok(eid) => println!("  ❌ invoke 통과! event_id={} — 보안 문제!", eid),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("NotFound")
                    || msg.contains("PermissionDenied")
                    || msg.contains("not found")
                {
                    println!("  ✅ 차단됨: {}", msg);
                } else {
                    println!("  ⚠️ 예상 외 응답: {}", msg);
                }
            }
        }
    } else {
        for d_id in &dialog_ids {
            let result = ai.invoke(d_id, "respond", serde_json::json!({"action": "허용"})).await;
            stats.wire_frames_sent += 1;
            stats.wire_frames_received += 1;
            match result {
                Ok(eid) => {
                    println!("  ❌ Dialog {} respond 통과! event_id={} — AI 우회 가능!", d_id, eid)
                }
                Err(e) => println!("  ✅ Dialog {} 차단: {}", d_id, e),
            }
        }
    }
    println!();

    // ─────────────────── STAGE 5: 보안 — AI가 Window.close 시도 ───────────────────
    println!("─── Stage 5: 보안 — AI가 Window.close 시도 → PermissionDenied 기대 ───");
    let window_ids = ai.query_by_type("aios.builtin/Window@1").await?;
    stats.wire_frames_sent += 1;
    stats.wire_frames_received += 1;
    if window_ids.is_empty() {
        println!("  ℹ️ 열린 Window 없음 — 임의 UUID로 시도");
        let fake_id = uuid::Uuid::new_v4().to_string();
        let result = ai.invoke(&fake_id, "close", serde_json::json!({})).await;
        stats.wire_frames_sent += 1;
        stats.wire_frames_received += 1;
        match result {
            Ok(eid) => println!("  ❌ invoke 통과! event_id={} — 보안 문제!", eid),
            Err(e) => println!("  ✅ 차단됨: {}", e),
        }
    } else {
        for w_id in &window_ids {
            let result = ai.invoke(w_id, "close", serde_json::json!({})).await;
            stats.wire_frames_sent += 1;
            stats.wire_frames_received += 1;
            match result {
                Ok(eid) => {
                    println!("  ❌ Window {} close 통과! eid={} — AI UI 조작 가능!", w_id, eid)
                }
                Err(e) => println!("  ✅ Window {} 차단: {}", w_id, e),
            }
        }
    }
    println!();

    // ─────────────────── STAGE 6: write 시나리오 — Dialog 자동 응답 ───────────────────
    println!("─── Stage 6: write 시나리오 (create_folder + Dialog 자동 응답) ───");

    // 6.1: 시뮬 compositor connect
    let (mut comp_stream, mut comp_accum, comp_actor) = connect_as_compositor(SERVER_ADDR).await?;
    println!("  시뮬 compositor connection OK. actor = {}", comp_actor);

    // 6.2: D:\ Folder 찾기 (props.path 기반)
    let folder_ids = ai.query_by_type("aios.std/Folder@1").await?;
    stats.wire_frames_sent += 1;
    stats.wire_frames_received += 1;
    let mut d_drive_id: Option<String> = None;
    for fid in &folder_ids {
        let obj = ai.get_object(fid).await?;
        stats.wire_frames_sent += 1;
        stats.wire_frames_received += 1;
        let path =
            obj.get("props").and_then(|p| p.get("path")).and_then(|v| v.as_str()).unwrap_or("");
        if path == "D:\\" {
            d_drive_id = Some(fid.clone());
            break;
        }
    }
    let d_drive_id = match d_drive_id {
        Some(id) => id,
        None => {
            println!("  ⚠️ D:\\ Folder 없음 — write 시나리오 skip");
            println!(
                "\n=== Done. wire stats: sent={} recv={} bytes={} ===",
                stats.wire_frames_sent, stats.wire_frames_received, stats.total_wire_bytes_received
            );
            return Ok(());
        }
    };
    println!("  D:\\ Folder id = {}", d_drive_id);

    // 6.3: AI가 create_folder invoke — desktop-shell이 Dialog mount
    let sandbox_name = format!("auto-crud-sandbox-{}", uuid::Uuid::new_v4().simple());
    println!("  AI invoke Folder.create_folder(name='{}') — Dialog 예상", sandbox_name);
    let event_id = ai
        .invoke(&d_drive_id, "create_folder", serde_json::json!({ "name": sandbox_name }))
        .await?;
    stats.wire_frames_sent += 1;
    stats.wire_frames_received += 1;
    println!("  invoke event_id={} (fire-and-forget — Dialog 등장 후 사용자 응답 대기)", event_id);

    // 6.4: 200ms 대기 후 Dialog 등장 polling (최대 1초)
    let mut dialog_id: Option<String> = None;
    for attempt in 0..10 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let dids = ai.query_by_type("aios.builtin/Dialog@1").await?;
        stats.wire_frames_sent += 1;
        stats.wire_frames_received += 1;
        if !dids.is_empty() {
            dialog_id = Some(dids[0].clone());
            println!("  Dialog mount 확인 (polling {}회 후) — id={}", attempt + 1, dids[0]);
            break;
        }
    }
    let dialog_id = match dialog_id {
        Some(id) => id,
        None => {
            println!("  ⚠️ Dialog 1초 안에 등장 안 함 — desktop-shell handler 미작동?");
            println!("\n=== Partial done ===");
            return Ok(());
        }
    };

    // 6.5: 보안 — AI가 Dialog.respond 시도 → PermissionDenied 기대
    println!("  보안 확인: AI가 Dialog.respond 시도");
    let ai_respond = ai.invoke(&dialog_id, "respond", serde_json::json!({"action": "허용"})).await;
    stats.wire_frames_sent += 1;
    stats.wire_frames_received += 1;
    match ai_respond {
        Ok(eid) => println!("    ❌ AI Dialog.respond 통과! eid={} — KI-001 회귀!", eid),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("PermissionDenied") || msg.contains("permission") {
                println!("    ✅ AI 차단됨 (KI-001 정상): {}", msg);
            } else {
                println!("    ⚠️ 예상 외 응답: {}", msg);
            }
        }
    }

    // 6.6: 시뮬 compositor가 Dialog.respond("allow") → 통과 기대
    println!("  시뮬 compositor가 Dialog.respond(allow)");
    let comp_respond = compositor_invoke(
        &mut comp_stream,
        &mut comp_accum,
        &dialog_id,
        "respond",
        serde_json::json!({"action": "허용"}),
    )
    .await?;
    match comp_respond {
        Ok(eid) => println!("    ✅ compositor respond 통과 — eid={}", eid),
        Err(e) => println!("    ❌ compositor respond 차단: {} — system:compositor도 막힘?", e),
    }

    // 6.7: 300ms 대기 후 새 Folder 등장 확인
    tokio::time::sleep(Duration::from_millis(300)).await;
    let folder_ids_after = ai.query_by_type("aios.std/Folder@1").await?;
    stats.wire_frames_sent += 1;
    stats.wire_frames_received += 1;
    let mut new_folder_id: Option<String> = None;
    let expected_path = format!("D:\\{}", sandbox_name);
    for fid in &folder_ids_after {
        if !folder_ids.contains(fid) {
            // 새 객체. props.path 확인.
            let obj = ai.get_object(fid).await?;
            stats.wire_frames_sent += 1;
            stats.wire_frames_received += 1;
            let path =
                obj.get("props").and_then(|p| p.get("path")).and_then(|v| v.as_str()).unwrap_or("");
            if path == expected_path {
                new_folder_id = Some(fid.clone());
                println!("    ✅ 새 Folder mount 확인: id={} path={}", fid, path);
                break;
            }
        }
    }
    if new_folder_id.is_none() {
        println!("    ⚠️ 새 Folder 객체 detect 못 함 (mount/SetState 지연 또는 실패)");
    }

    // 6.8: cleanup — 시뮬 compositor가 새 Folder.delete invoke (Dialog 또 등장 → respond)
    if let Some(nf_id) = &new_folder_id {
        println!("\n  cleanup: AI가 새 Folder.delete invoke");
        let _ = ai.invoke(nf_id, "delete", serde_json::json!({"recursive": false})).await?;
        stats.wire_frames_sent += 1;
        stats.wire_frames_received += 1;
        // Dialog 등장 polling
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let dids = ai.query_by_type("aios.builtin/Dialog@1").await?;
            stats.wire_frames_sent += 1;
            stats.wire_frames_received += 1;
            // 첫 시도의 Dialog는 이미 응답됐으니 새 Dialog는 다른 id (또는 동일 id가 두 번째 사용)
            if let Some(d) = dids.iter().find(|d| d != &&dialog_id) {
                println!("  delete Dialog 등장 id={}", d);
                let _ = compositor_invoke(
                    &mut comp_stream,
                    &mut comp_accum,
                    d,
                    "respond",
                    serde_json::json!({"action": "허용"}),
                )
                .await?;
                println!("  compositor respond(allow) → delete 통과 기대");
                tokio::time::sleep(Duration::from_millis(200)).await;
                break;
            }
        }
        // 객체 destroyed 확인
        let after_del = ai.get_object(nf_id).await;
        stats.wire_frames_sent += 1;
        stats.wire_frames_received += 1;
        match after_del {
            Ok(v) => {
                let destroyed = v.get("destroyed").and_then(|d| d.as_bool()).unwrap_or(false);
                if destroyed {
                    println!("    ✅ destroyed=true (tombstone) 확인");
                } else {
                    println!("    ⚠️ destroyed=false — delete 미반영?");
                }
            }
            Err(e) => println!("    ℹ️ get_object 실패 (이미 hard-delete?): {}", e),
        }
    }

    println!("\n─── 최종 wire 통계 ───");
    println!("  wire frames sent: {}", stats.wire_frames_sent);
    println!("  wire frames received: {}", stats.wire_frames_received);
    println!(
        "  total bytes received: {} ({:.1} KB)",
        stats.total_wire_bytes_received,
        stats.total_wire_bytes_received as f64 / 1024.0
    );
    println!("\n=== Auto CRUD Demo 완료 ===");
    Ok(())
}
