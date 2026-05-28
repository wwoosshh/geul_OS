//! Auto React Project demo — controller(Claude)가 GeulOS의 AI를 *외부에서* 호출해
//! ShellRunner.run으로 실제 npx create-vite + npm install을 자동 실행하는 시연.
//!
//! M11.1 auto_website_project의 확장 — *생태계 도구 (npx/npm) 실행*까지 자동화.
//!
//! 실행: ANTHROPIC_API_KEY 설정 + launcher 띄움 + node/npm/npx 호스트 PATH에 있음.
//! `cargo run --example auto_react_project`
//!
//! 비용: AI 실호출 (~5-10 turn) + npm install (~60-90초). 총 5-10분.

use geulos_ai_bridge::wire::WireClient;
use geulos_proto::{decode_frame, encode_frame, Hello, HelloAck, InvokeAck, InvokeMsg, Role};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

const SERVER_ADDR: &str = "127.0.0.1:5550";
const PROJECT_DIR: &str = r"D:\GeulOS\tmp-react-app";
const SESSION_NAME: &str = "auto-react-demo";
const PROMPT: &str = "D:/GeulOS/tmp-react-app 폴더에 React 프로젝트를 만들어줘. \
     \
     **ShellRunner 사용 규칙 (반드시 준수 — race 회피):** \
     (1) ShellRunner singleton id 확보. \
     (2) **subscribe(<sr_id>, ['StateSet']) 먼저 호출** — invoke 이전 필수. \
     (3) invoke_method run. \
     (4) drain 1~2회 시도. events 비어있으면 *get_object(<sr_id>)*로 현재 state 폴백 — \
         last_cmd가 방금 보낸 cmd와 같고 last_exit_code가 채워졌으면 완료. \
     (5) drain + get_object를 1~2초 간격 *최대 5회* polling. last_exit_code=0이면 다음 단계. \
     ShellRunner는 stdin 미지원이라 모든 명령에 non-interactive flag 사용. \
     \
     **단계:** \
     1. ShellRunner.run으로 npx create-vite \
        (cmd='npx', args=['--yes', 'create-vite@latest', 'tmp-react-app', '--template', 'react'], \
        cwd='D:/GeulOS'). 1초 안에 끝나는 경우 많음. \
     2. ShellRunner.run으로 npm install (cmd='npm', args=['install'], \
        cwd='D:/GeulOS/tmp-react-app'). ~60-90초 소요. \
     3. File.save로 src/App.jsx의 기존 내용을 단순 'Hello GeulOS React' h1 하나로 교체. \
        (cwd 안 path → Folder.list로 lazy-mount 후 File@1 발견 → save) \
     빌드/실행은 안 해도 됨. 진행 상황 짧게 보고하고 완료 시 report_done.";

async fn connect_as_compositor(
    addr: &str,
) -> Result<(TcpStream, Vec<u8>, String), Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(addr).await?;
    let hello = Hello {
        version: "0.1".to_string(),
        role: Role::Compositor,
        auth: serde_json::json!({}),
        client_id: "auto-react-comp".to_string(),
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Auto React Project Demo ===");
    println!("controller가 외부에서 AI 통해 npx create-vite + npm install + App.jsx 교체\n");

    // ─────────────────── Setup: 기존 sandbox 정리 ───────────────────
    if Path::new(PROJECT_DIR).exists() {
        println!("기존 {} 정리 중...", PROJECT_DIR);
        std::fs::remove_dir_all(PROJECT_DIR).ok();
    }

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

    // ─────────────────── 6. AI 응답 + 파일 생성 polling ───────────────────
    println!("\n6) AI 응답 + 파일 생성 polling (max 600초)");
    let timeout = Duration::from_secs(600);
    let mut all_ok = false;
    loop {
        if started.elapsed() > timeout {
            println!("   timeout 600초");
            break;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
        let elapsed = started.elapsed().as_secs();

        let pkg_json = Path::new(PROJECT_DIR).join("package.json");
        let node_react = Path::new(PROJECT_DIR).join("node_modules").join("react");
        let app_jsx = Path::new(PROJECT_DIR).join("src").join("App.jsx");
        let pkg_ok = pkg_json.exists();
        let nm_ok = node_react.exists();
        let jsx_has_hello = if app_jsx.exists() {
            std::fs::read_to_string(&app_jsx)
                .map(|s| s.contains("Hello GeulOS React"))
                .unwrap_or(false)
        } else {
            false
        };

        println!(
            "   [{}s] package.json={} node_modules/react={} App.jsx 'Hello GeulOS React'={}",
            elapsed, pkg_ok, nm_ok, jsx_has_hello
        );
        if pkg_ok && nm_ok && jsx_has_hello {
            all_ok = true;
            println!("   모든 검증 통과 (총 {}초)", elapsed);
            break;
        }
    }
    *stop_signal.lock().await = true;
    let _ = responder.await;

    // ─────────────────── 7. 최종 디스크 검증 ───────────────────
    println!("\n7) 최종 디스크 검증");
    if Path::new(PROJECT_DIR).exists() {
        let entries: Vec<_> = std::fs::read_dir(PROJECT_DIR)
            .map(|rd| {
                rd.filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        println!("   {} 안 entries: {:?}", PROJECT_DIR, entries);
    } else {
        println!("   {} 자체 미생성", PROJECT_DIR);
    }

    if all_ok {
        println!(
            "\n=== React 프로젝트 자동 생성 성공 — controller가 AI + ShellRunner.run으로 npx/npm 실행 ==="
        );
    } else {
        println!("\n=== 부분 성공 또는 timeout ===");
    }
    Ok(())
}
