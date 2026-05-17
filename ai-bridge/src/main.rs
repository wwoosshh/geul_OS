//! geulos-ai-bridge: AI 어댑터 드라이버 바이너리.
//!
//! 사용:
//!   geulos-ai-bridge run --scenario scenarios/01_explore.toml \
//!                       [--server 127.0.0.1:5550] [--model claude-sonnet-4-6]
//!
//! `.env` 또는 환경 변수에 `ANTHROPIC_API_KEY` 필요.

use std::path::PathBuf;
use std::process::ExitCode;

use geulos_ai_bridge::adapter::ClaudeAdapter;
use geulos_ai_bridge::scenario::Scenario;
use geulos_ai_bridge::session::Session;
use geulos_ai_bridge::WireClient;

const DEFAULT_SYSTEM_PROMPT: &str = include_str!("system_prompt.md");
const DEFAULT_SERVER: &str = "127.0.0.1:5550";
const DEFAULT_MODEL: &str = "claude-sonnet-4-6";

#[tokio::main]
async fn main() -> ExitCode {
    // workspace root의 .env 자동 로드 (있으면). probe.py와 동등한 UX.
    // 파일 없으면 silent — 환경 변수가 이미 설정된 경우 정상.
    let _ = dotenvy::dotenv();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 || args[1] != "run" {
        eprintln!("Usage: geulos-ai-bridge run --scenario <path> [--server <addr>] [--model <id>]");
        return ExitCode::from(2);
    }

    let mut scenario_path: Option<PathBuf> = None;
    let mut server_addr = DEFAULT_SERVER.to_string();
    let mut model = DEFAULT_MODEL.to_string();
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--scenario" if i + 1 < args.len() => {
                scenario_path = Some(args[i + 1].clone().into());
                i += 2;
            }
            "--server" if i + 1 < args.len() => {
                server_addr = args[i + 1].clone();
                i += 2;
            }
            "--model" if i + 1 < args.len() => {
                model = args[i + 1].clone();
                i += 2;
            }
            other => {
                eprintln!("unknown arg: {}", other);
                return ExitCode::from(2);
            }
        }
    }
    let scenario_path = match scenario_path {
        Some(p) => p,
        None => {
            eprintln!("--scenario required");
            return ExitCode::from(2);
        }
    };

    // 시나리오 로드
    let scenario = match Scenario::load(&scenario_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("scenario load error: {}", e);
            return ExitCode::from(1);
        }
    };

    // ANTHROPIC_API_KEY 확인
    if std::env::var("ANTHROPIC_API_KEY").is_err() {
        eprintln!(
            "ANTHROPIC_API_KEY not set. Either export it or place in a .env file \
             (the workspace root has one)."
        );
        return ExitCode::from(1);
    }
    let adapter = match ClaudeAdapter::from_env(&model) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("adapter: {}", e);
            return ExitCode::from(1);
        }
    };

    // 와이어 클라이언트
    let wire = match WireClient::connect_as_ai(&server_addr).await {
        Ok(w) => w,
        Err(e) => {
            eprintln!("connect to {}: {}", server_addr, e);
            return ExitCode::from(1);
        }
    };

    // audit 로그 경로 — 현재 디렉터리/ai-bridge-audit-<timestamp>.log
    let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let audit =
        std::env::current_dir().unwrap_or_default().join(format!("ai-bridge-audit-{}.log", ts));

    let mut session = Session::new(adapter, wire, DEFAULT_SYSTEM_PROMPT.to_string())
        .with_budget(scenario.to_session_budget())
        .with_audit(audit.clone());

    println!("[ai-bridge] scenario={} model={} server={}", scenario.name, model, server_addr);
    let outcome = match session.run_task(&scenario.goal).await {
        Ok(o) => o,
        Err(e) => {
            eprintln!("session error: {}", e);
            return ExitCode::from(1);
        }
    };

    println!("\n=== outcome ===");
    println!("turns: {}", outcome.turns_used);
    println!("tokens: in={}, out={}", outcome.input_tokens, outcome.output_tokens);
    println!("wall: {:.1}s", outcome.wall_secs);
    if let Some(s) = &outcome.summary {
        println!("summary: {}", s);
    } else {
        println!("(no summary — see audit log)");
    }
    println!("audit: {}", audit.display());

    if outcome.completed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    }
}
