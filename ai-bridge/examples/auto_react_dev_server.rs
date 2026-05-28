//! Auto React Dev Server demo — controller(Claude)가 GeulOS의 AI를 *외부에서* 호출해
//! ShellRunner.run_streamed로 `npm run dev`를 실행하고 ConsoleWindow stdout에서
//! Local URL을 발견하면 사용자에게 안내하는 시연.
//!
//! M13 ConsoleWindow@1 + run_streamed 검증용 end-to-end example.
//!
//! 실행: ANTHROPIC_API_KEY 설정 + launcher 띄움 + node/npm/npx 호스트 PATH에 있음.
//! `cargo run --example auto_react_dev_server`
//!
//! 비용: AI 실호출 + npm install (~60-90초) + dev server 시작 (~3초). 총 ~3-5분.
//!
//! polling 종료 기준:
//! - AI가 'http://localhost' 포함 메시지를 보내거나
//! - ConsoleWindow가 mount되어 state.status="running" + state.lines에 'localhost' 등장
//! - 또는 600s timeout.
//!
//! example 종료 *전에* ConsoleWindow terminate 안 함 — 사용자가 직접 X 닫아 시연.

use geulos_ai_bridge::wire::WireClient;
use geulos_proto::{decode_frame, encode_frame, Hello, HelloAck, InvokeAck, InvokeMsg, Role};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

const SERVER_ADDR: &str = "127.0.0.1:5550";
const SESSION_NAME: &str = "auto-react-dev-server";
const PROMPT: &str = "D:/GeulOS/tmp-react-app 프로젝트에서 vite dev server를 띄워줘. \
     **절차:** \
     1. tmp-react-app 폴더가 없으면 ShellRunner.run으로 npx create-vite + npm install로 생성. \
     2. ShellRunner를 subscribe(['StateSet']) 먼저, 그 다음 run_streamed cmd='npm' args=['run','dev'] cwd='D:/GeulOS/tmp-react-app'. \
     3. list_objects_by_type('aios.builtin/ConsoleWindow@1')로 ConsoleWindow id 발견 (1~3초 polling). \
     4. subscribe(<cw_id>, ['StateSet']) + drain → state.lines 실시간 read. drain empty면 get_object(<cw_id>) 폴백. \
     5. lines에 'Local:' 또는 'http://localhost' 등장하면 URL 추출 → 사용자에게 한국어로 명확히 안내. \
     6. 'dev server 띄워졌습니다. 브라우저에서 <URL> 열어보세요. 종료하려면 ConsoleWindow X 클릭 또는 저에게 종료 요청.' 메시지. \
     7. report_done.";

async fn connect_as_compositor(
    addr: &str,
) -> Result<(TcpStream, Vec<u8>, String), Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(addr).await?;
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Compositor,
        auth: serde_json::json!({}),
        client_id: "auto-react-dev-server-comp".to_string(),
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
        if n == 0 {
            return Err("응답 전에 끊김".into());
        }
        accum.extend_from_slice(&tmp[..n]);
    }
}

/// ConsoleWindow가 mount되어 status="running"이고 lines에 "localhost" 포함하는지 확인.
///
/// probe.query_by_type + probe.get_object로 state 폴백 확인 (KI-026 race 대응).
async fn check_console_window_url(probe: &mut WireClient) -> Option<String> {
    let cw_ids = probe.query_by_type("aios.builtin/ConsoleWindow@1").await.ok()?;
    for cw_id in &cw_ids {
        let obj = match probe.get_object(cw_id).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        let status =
            obj.get("state").and_then(|s| s.get("status")).and_then(|v| v.as_str()).unwrap_or("");
        if status != "running" {
            continue;
        }
        if let Some(lines) =
            obj.get("state").and_then(|s| s.get("lines")).and_then(|v| v.as_array())
        {
            for line in lines {
                let text = line.as_str().unwrap_or("");
                if text.contains("localhost") || text.contains("Local:") {
                    // URL 추출 — "http://localhost:NNNN" 형태 찾기
                    if let Some(url_start) = text.find("http://localhost") {
                        let url_end = text[url_start..]
                            .find(|c: char| c.is_whitespace())
                            .map(|i| url_start + i)
                            .unwrap_or(text.len());
                        return Some(text[url_start..url_end].to_string());
                    }
                    // "Local:   http://..." 패턴 — 전체 line 반환
                    return Some(text.trim().to_string());
                }
            }
        }
    }
    None
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Auto React Dev Server Demo ===");
    println!("controller가 외부에서 AI 통해 npm run dev 띄우고 ConsoleWindow URL 안내\n");

    // ─────────────────── 1. compositor (시나리오) connection ───────────────────
    let (mut comp1, mut accum1, comp1_actor) = connect_as_compositor(SERVER_ADDR).await?;
    println!("1) 시나리오용 compositor: {}", comp1_actor);

    // probe connection (Role::Ai로 가벼운 query 전용)
    let mut probe = WireClient::connect_as_ai(SERVER_ADDR).await?;
    println!("2) probe (Role::Ai): {}\n", probe.actor_id());

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
        let (mut bg_stream, mut bg_accum, _) = match connect_as_compositor(SERVER_ADDR).await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("bg compositor 실패: {}", e);
                return;
            }
        };
        let mut bg_probe = match WireClient::connect_as_ai(SERVER_ADDR).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("bg probe 실패: {}", e);
                return;
            }
        };
        loop {
            if *stop_clone.lock().await {
                break;
            }
            match bg_probe.query_by_type("aios.builtin/Dialog@1").await {
                Ok(dids) => {
                    for did in &dids {
                        if responded_ids.contains(did) {
                            continue;
                        }
                        let result = compositor_invoke(
                            &mut bg_stream,
                            &mut bg_accum,
                            did,
                            "respond",
                            serde_json::json!({"action": "허용"}),
                        )
                        .await;
                        match result {
                            Ok(Ok(_)) => {
                                println!(
                                    "    [bg] Dialog {} 자동 [허용]",
                                    &did[..8.min(did.len())]
                                );
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
    println!("\n4) /ai start {}", SESSION_NAME);
    let start_cmd = format!("/ai start {}", SESSION_NAME);
    if let Err(e) = compositor_invoke(
        &mut comp1,
        &mut accum1,
        &cli_id,
        "submit_input",
        serde_json::json!({"text": start_cmd}),
    )
    .await?
    {
        *stop_signal.lock().await = true;
        return Err(format!("/ai start 실패: {}", e).into());
    }
    println!("   /ai start ok");
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // ─────────────────── 5. AI prompt 송신 ───────────────────
    println!("\n5) Cli.submit_input(prompt)");
    println!("   prompt: {}", PROMPT);
    let started = Instant::now();
    if let Err(e) = compositor_invoke(
        &mut comp1,
        &mut accum1,
        &cli_id,
        "submit_input",
        serde_json::json!({"text": PROMPT}),
    )
    .await?
    {
        *stop_signal.lock().await = true;
        return Err(format!("prompt 송신 실패: {}", e).into());
    }
    println!("   prompt 송신 ok");

    // ─────────────────── 6. ConsoleWindow mount + URL polling ───────────────────
    println!("\n6) ConsoleWindow mount + state.lines에 localhost URL 등장 polling (max 600초)");
    println!("   (AI가 run_streamed → Dialog [허용] → ConsoleWindow mount → URL stdout 출력 대기)");
    let timeout = Duration::from_secs(600);
    let mut found_url: Option<String> = None;
    let mut console_window_seen = false;

    loop {
        if started.elapsed() > timeout {
            println!("   timeout 600초");
            break;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
        let elapsed = started.elapsed().as_secs();

        // ConsoleWindow 존재 확인
        let cw_ids = probe.query_by_type("aios.builtin/ConsoleWindow@1").await.unwrap_or_default();
        if !console_window_seen && !cw_ids.is_empty() {
            console_window_seen = true;
            println!(
                "   [{}s] ConsoleWindow {} mount 확인!",
                elapsed,
                &cw_ids[0][..8.min(cw_ids[0].len())]
            );
        }

        // URL 발견 확인
        if let Some(url) = check_console_window_url(&mut probe).await {
            found_url = Some(url);
            println!("   [{}s] URL 발견! {}", elapsed, found_url.as_deref().unwrap_or(""));
            break;
        }

        println!(
            "   [{}s] ConsoleWindow={} url_found={}",
            elapsed,
            if console_window_seen { "있음" } else { "없음" },
            found_url.is_some(),
        );
    }

    // bg responder 정리 (stop — ConsoleWindow는 terminate 안 함, 사용자가 직접 X)
    *stop_signal.lock().await = true;
    let _ = responder.await;

    // ─────────────────── 7. 최종 결과 ───────────────────
    println!("\n7) 최종 결과");
    let cw_ids = probe.query_by_type("aios.builtin/ConsoleWindow@1").await.unwrap_or_default();
    println!("   ConsoleWindow 객체 수: {}", cw_ids.len());
    for cw_id in &cw_ids {
        if let Ok(obj) = probe.get_object(cw_id).await {
            let status = obj
                .get("state")
                .and_then(|s| s.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let line_count = obj
                .get("state")
                .and_then(|s| s.get("line_count"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let title = obj
                .get("props")
                .and_then(|p| p.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            println!(
                "   ConsoleWindow {}: title='{}' status={} line_count={}",
                &cw_id[..8.min(cw_id.len())],
                title,
                status,
                line_count
            );
        }
    }

    if let Some(ref url) = found_url {
        println!("\n=== Dev Server 성공! ===");
        println!("URL: {}", url);
        println!("브라우저에서 위 URL을 열어 React 앱을 확인하세요.");
        println!("ConsoleWindow X 버튼으로 종료하거나 AI에게 '종료해줘'.");
    } else if console_window_seen {
        println!("\n=== ConsoleWindow mount 확인 (URL 미발견 또는 timeout) ===");
        println!("ConsoleWindow가 있으니 수동으로 state.lines 확인 가능.");
    } else {
        println!("\n=== ConsoleWindow 미mount (Dialog 거부 또는 spawn 실패) ===");
        println!("launcher 상태 + Dialog 응답 여부 확인 필요.");
    }

    Ok(())
}
