//! Auto Website Project demo — controller(Claude)가 GeulOS의 AI를 *외부에서* 호출해
//! 실제 웹사이트 프로젝트를 생성하는 end-to-end 시연.
//!
//! 사용자 비전: 외부에서도 GeulOS를 *사용자와 동등하게* 조작 가능. controller가
//! Role::Compositor로 wire connect → Cli.submit_input invoke → desktop-shell이 AI 호출 →
//! AI가 Folder/File mutation 시도 → Dialog 등장 → background responder가 자동 [허용] →
//! 실제 디스크에 파일 생성.
//!
//! 실행: ANTHROPIC_API_KEY 설정 + launcher 띄운 상태에서
//! `cargo run --example auto_website_project`
//!
//! 비용: AI 실호출 (Claude Sonnet 4.6) — typical 1-3 turn, 수천 token.

use geulos_ai_bridge::wire::WireClient;
use geulos_proto::{decode_frame, encode_frame, Hello, HelloAck, InvokeAck, InvokeMsg, Role};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use std::sync::Arc;

const SERVER_ADDR: &str = "127.0.0.1:5550";
const PROJECT_DIR: &str = r"D:\GeulOS\tmp-website-demo";
const SESSION_NAME: &str = "auto-website-demo";
const PROMPT: &str = "D:/GeulOS/tmp-website-demo 폴더를 만들고 그 안에 간단한 hello world 웹사이트를 만들어줘. \
                     index.html (h1 'Hello GeulOS' + paragraph), \
                     style.css (body 배경색 + h1 색상), \
                     script.js (window.onload에 console.log 한 줄) \
                     세 파일이면 충분해. 사용자에게 보일 만큼 단순하게.";

async fn connect_as_compositor(
    addr: &str,
) -> Result<(TcpStream, Vec<u8>, String), Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(addr).await?;
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Compositor,
        auth: serde_json::json!({}),
        client_id: "auto-website-comp".to_string(),
    };
    let body = serde_json::to_vec(&hello)?;
    stream.write_all(&encode_frame(&body)).await?;
    let mut accum: Vec<u8> = Vec::new();
    let mut tmp = vec![0u8; 4096];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 { return Err("hello 전에 끊김".into()); }
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

async fn compositor_invoke(
    stream: &mut TcpStream,
    accum: &mut Vec<u8>,
    target: &str,
    method: &str,
    args: serde_json::Value,
) -> Result<Result<String, String>, Box<dyn std::error::Error>> {
    let req_id = format!("c-{}", uuid::Uuid::new_v4());
    let inv = InvokeMsg {
        request_id: req_id,
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
            let v: serde_json::Value = serde_json::from_slice(&frame)?;
            match v.get("kind").and_then(|k| k.as_str()) {
                Some("InvokeAck") => {
                    let a: InvokeAck = serde_json::from_value(v)?;
                    return Ok(Ok(a.event_id));
                }
                Some("InvokeError") => {
                    let detail = v.get("detail").and_then(|d| d.as_str()).unwrap_or("").to_string();
                    return Ok(Err(detail));
                }
                _ => continue,
            }
        }
        let n = stream.read(&mut tmp).await?;
        if n == 0 { return Err("응답 전에 끊김".into()); }
        accum.extend_from_slice(&tmp[..n]);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Auto Website Project Demo ===");
    println!("controller가 외부에서 GeulOS의 AI를 호출해 실제 웹사이트 생성\n");

    // ─────────────────── Setup: 기존 sandbox 정리 ───────────────────
    if Path::new(PROJECT_DIR).exists() {
        println!("기존 {} 정리 중...", PROJECT_DIR);
        std::fs::remove_dir_all(PROJECT_DIR).ok();
    }

    // ─────────────────── 1. compositor (시나리오) connection ───────────────────
    let (mut comp1, mut accum1, comp1_actor) = connect_as_compositor(SERVER_ADDR).await?;
    println!("1) 시나리오용 compositor connection: {}", comp1_actor);

    // probe connection (Role::Ai로 가벼운 query 전용)
    let mut probe = WireClient::connect_as_ai(SERVER_ADDR).await?;
    println!("2) probe connection (Role::Ai): {}\n", probe.actor_id());

    // ─────────────────── 2. Cli 객체 찾기 ───────────────────
    let cli_ids = probe.query_by_type("aios.builtin/Cli@1").await?;
    if cli_ids.is_empty() {
        return Err("Cli 객체 없음 — launcher 정상 띄워졌는지 확인".into());
    }
    let cli_id = cli_ids[0].clone();
    println!("Cli id = {}", cli_id);

    // ─────────────────── 3. Dialog 자동 응답 background task ───────────────────
    println!("\n3) Dialog 자동 응답 background task 시작 ([허용] 자동)");
    let stop_signal = Arc::new(Mutex::new(false));
    let stop_clone = stop_signal.clone();
    let responder = tokio::spawn(async move {
        let mut responded_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        let (mut bg_stream, mut bg_accum, _) =
            match connect_as_compositor(SERVER_ADDR).await {
                Ok(t) => t,
                Err(e) => { eprintln!("background compositor 실패: {}", e); return; }
            };
        let mut bg_probe = match WireClient::connect_as_ai(SERVER_ADDR).await {
            Ok(p) => p,
            Err(e) => { eprintln!("bg probe 실패: {}", e); return; }
        };
        loop {
            if *stop_clone.lock().await { break; }
            match bg_probe.query_by_type("aios.builtin/Dialog@1").await {
                Ok(dids) => {
                    for did in &dids {
                        if responded_ids.contains(did) { continue; }
                        let result = compositor_invoke(
                            &mut bg_stream, &mut bg_accum,
                            did, "respond",
                            serde_json::json!({"action": "허용"}),
                        ).await;
                        match result {
                            Ok(Ok(_)) => {
                                println!("    [bg] Dialog {} 자동 [허용]", &did[..8]);
                                responded_ids.insert(did.clone());
                            }
                            Ok(Err(e)) => eprintln!("    [bg] respond err: {}", e),
                            Err(e) => eprintln!("    [bg] wire err: {}", e),
                        }
                    }
                }
                Err(e) => eprintln!("    [bg] query err: {}", e),
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    });

    // ─────────────────── 4. /ai start 명령 ───────────────────
    println!("\n4) Cli.submit_input(\"/ai start {}\")", SESSION_NAME);
    let start_cmd = format!("/ai start {}", SESSION_NAME);
    match compositor_invoke(
        &mut comp1, &mut accum1, &cli_id, "submit_input",
        serde_json::json!({"text": start_cmd}),
    ).await? {
        Ok(eid) => println!("   /ai start ok — eid={}", eid),
        Err(e) => {
            eprintln!("   /ai start 실패: {}", e);
            *stop_signal.lock().await = true;
            return Err(e.into());
        }
    }
    // session 시작 대기
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // ─────────────────── 5. AI prompt 송신 ───────────────────
    println!("\n5) Cli.submit_input(prompt)");
    println!("   prompt: {}", PROMPT);
    let started = Instant::now();
    match compositor_invoke(
        &mut comp1, &mut accum1, &cli_id, "submit_input",
        serde_json::json!({"text": PROMPT}),
    ).await? {
        Ok(eid) => println!("   prompt 송신 ok — eid={}", eid),
        Err(e) => {
            *stop_signal.lock().await = true;
            return Err(format!("prompt 송신 실패: {}", e).into());
        }
    }

    // ─────────────────── 6. AI 응답 + 파일 생성 polling ───────────────────
    println!("\n6) AI 응답 + 파일 생성 polling (max 120초)");
    let timeout = Duration::from_secs(120);
    let mut all_files_ok = false;
    loop {
        if started.elapsed() > timeout {
            println!("   ⏱ timeout 120초 — 부분 결과만 검증");
            break;
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
        let elapsed = started.elapsed().as_secs();

        // 디스크 확인
        let dir_exists = Path::new(PROJECT_DIR).exists();
        let html = Path::new(PROJECT_DIR).join("index.html");
        let css = Path::new(PROJECT_DIR).join("style.css");
        let js = Path::new(PROJECT_DIR).join("script.js");
        let html_ok = html.exists();
        let css_ok = css.exists();
        let js_ok = js.exists();

        println!(
            "   [{}s] dir={} html={} css={} js={}",
            elapsed, dir_exists, html_ok, css_ok, js_ok
        );
        if dir_exists && html_ok && css_ok && js_ok {
            all_files_ok = true;
            println!("   ✅ 모든 파일 생성 확인 (총 {}초)", elapsed);
            break;
        }
    }
    *stop_signal.lock().await = true;
    let _ = responder.await;

    // ─────────────────── 7. 결과 검증 ───────────────────
    println!("\n7) 최종 검증 — 디스크 파일 본문");
    if Path::new(PROJECT_DIR).exists() {
        for file_name in &["index.html", "style.css", "script.js"] {
            let p = Path::new(PROJECT_DIR).join(file_name);
            if p.exists() {
                let size = std::fs::metadata(&p)?.len();
                let content = std::fs::read_to_string(&p).unwrap_or_default();
                let preview: String = content.chars().take(80).collect();
                println!("   ✅ {} ({} bytes)", p.display(), size);
                println!("      preview: {}{}", preview, if content.len() > 80 { "..." } else { "" });
            } else {
                println!("   ❌ {} 누락", p.display());
            }
        }
    } else {
        println!("   ❌ {} 자체 미생성", PROJECT_DIR);
    }

    if all_files_ok {
        println!("\n=== ✅ 시연 성공 — controller가 외부에서 AI 통해 웹사이트 프로젝트 생성 ===");
    } else {
        println!("\n=== ⚠️ 부분 성공 또는 timeout — 위 출력으로 진단 ===");
    }
    Ok(())
}
