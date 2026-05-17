//! AI 세션 매니저 — 한 작업의 처음부터 끝까지.

use std::path::PathBuf;
use std::time::Instant;

use chrono::Utc;
use serde_json::{json, Value};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::adapter::{LlmAdapter, LlmMessage, LlmResponse, LlmRole, LlmStop, ToolDef};
use crate::error::BridgeResult;
use crate::tools::{dispatch_tool, standard_tools, DispatchResult};
use crate::wire::WireClient;

/// 세션 예산.
#[derive(Debug, Clone)]
pub struct SessionBudget {
    pub max_turns: usize,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_wall_secs: u64,
}

impl Default for SessionBudget {
    fn default() -> Self {
        Self {
            max_turns: 12,
            max_input_tokens: 200_000,
            max_output_tokens: 8_000,
            max_wall_secs: 120,
        }
    }
}

/// 세션 결과.
#[derive(Debug, Clone)]
pub struct SessionOutcome {
    pub summary: Option<String>,
    pub turns_used: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub wall_secs: f64,
    /// `report_done`이 호출되었는지.
    pub completed: bool,
}

/// 한 세션 = (adapter, wire, budget, history, audit).
pub struct Session<A: LlmAdapter> {
    pub adapter: A,
    pub wire: WireClient,
    pub system: String,
    pub tools: Vec<ToolDef>,
    pub budget: SessionBudget,
    pub audit_path: Option<PathBuf>,
}

impl<A: LlmAdapter> Session<A> {
    /// 새 세션 (기본 도구 + 기본 예산).
    pub fn new(adapter: A, wire: WireClient, system: String) -> Self {
        Self {
            adapter,
            wire,
            system,
            tools: standard_tools(),
            budget: SessionBudget::default(),
            audit_path: None,
        }
    }

    pub fn with_budget(mut self, b: SessionBudget) -> Self {
        self.budget = b;
        self
    }

    pub fn with_audit(mut self, path: impl Into<PathBuf>) -> Self {
        self.audit_path = Some(path.into());
        self
    }

    /// 사용자 작업을 수행. `report_done` 호출 시 종료, 또는 budget 소진 시 종료.
    pub async fn run_task(&mut self, user_prompt: &str) -> BridgeResult<SessionOutcome> {
        let started = Instant::now();
        let mut history: Vec<LlmMessage> = vec![LlmMessage {
            role: LlmRole::User,
            content: Value::String(user_prompt.to_string()),
        }];

        let mut summary: Option<String> = None;
        let mut total_in: u64 = 0;
        let mut total_out: u64 = 0;
        let mut turn: usize = 0;

        self.audit(&format!(
            "=== session start ===\n actor: {}\n prompt: {}",
            self.wire.actor_id(),
            user_prompt
        ))
        .await;

        loop {
            turn += 1;
            if turn > self.budget.max_turns {
                self.audit(&format!("=== budget: max_turns ({}) ===", self.budget.max_turns)).await;
                break;
            }
            if started.elapsed().as_secs() >= self.budget.max_wall_secs {
                self.audit(&format!(
                    "=== budget: max_wall_secs ({}) ===",
                    self.budget.max_wall_secs
                ))
                .await;
                break;
            }
            if total_in >= self.budget.max_input_tokens {
                self.audit("=== budget: max_input_tokens ===").await;
                break;
            }
            if total_out >= self.budget.max_output_tokens {
                self.audit("=== budget: max_output_tokens ===").await;
                break;
            }

            self.audit(&format!("\n--- turn {} ---", turn)).await;

            let resp: LlmResponse =
                self.adapter.complete(&self.system, &history, &self.tools).await?;
            total_in += resp.tokens.0;
            total_out += resp.tokens.1;

            for t in &resp.text {
                self.audit(&format!("text: {}", t)).await;
            }
            for tu in &resp.tool_uses {
                self.audit(&format!(
                    "tool_use: {}({})",
                    tu.name,
                    serde_json::to_string(&tu.input).unwrap_or_default()
                ))
                .await;
            }

            history.push(LlmMessage {
                role: LlmRole::Assistant,
                content: response_to_assistant_content(&resp),
            });

            if resp.stop == LlmStop::EndTurn && resp.tool_uses.is_empty() {
                self.audit("=== stopped without tools ===").await;
                break;
            }

            let mut tool_results: Vec<Value> = Vec::new();
            let mut done = false;
            for tu in &resp.tool_uses {
                let r = dispatch_tool(&mut self.wire, &tu.name, &tu.input).await;
                match r {
                    Ok(DispatchResult::Output(v)) => {
                        self.audit(&format!("  -> {}", trim(&v))).await;
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tu.id,
                            "content": serde_json::to_string(&v).unwrap_or_default(),
                        }));
                    }
                    Ok(DispatchResult::Done { summary: s }) => {
                        self.audit(&format!("  -> report_done: {}", s)).await;
                        summary = Some(s);
                        done = true;
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tu.id,
                            "content": "ok",
                        }));
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        self.audit(&format!("  -> error: {}", msg)).await;
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tu.id,
                            "content": format!("error: {}", msg),
                            "is_error": true,
                        }));
                    }
                }
            }

            if done {
                break;
            }

            history.push(LlmMessage { role: LlmRole::User, content: Value::Array(tool_results) });
        }

        let wall = started.elapsed().as_secs_f64();
        self.audit(&format!(
            "\n=== session end ===\n turns: {}\n tokens (in/out): {}/{}\n wall: {:.1}s",
            turn, total_in, total_out, wall
        ))
        .await;

        let completed = summary.is_some();
        Ok(SessionOutcome {
            summary,
            turns_used: turn,
            input_tokens: total_in,
            output_tokens: total_out,
            wall_secs: wall,
            completed,
        })
    }

    async fn audit(&self, line: &str) {
        let stamped = format!("{} {}\n", Utc::now().to_rfc3339(), line);
        if let Some(path) = &self.audit_path {
            if let Ok(mut f) = File::options().create(true).append(true).open(path).await {
                let _ = f.write_all(stamped.as_bytes()).await;
            }
        }
    }
}

fn response_to_assistant_content(resp: &LlmResponse) -> Value {
    let mut blocks = Vec::new();
    for t in &resp.text {
        blocks.push(json!({ "type": "text", "text": t }));
    }
    for tu in &resp.tool_uses {
        blocks.push(json!({
            "type": "tool_use",
            "id": tu.id,
            "name": tu.name,
            "input": tu.input,
        }));
    }
    Value::Array(blocks)
}

fn trim(v: &Value) -> String {
    let s = serde_json::to_string(v).unwrap_or_default();
    if s.len() > 200 {
        format!("{}...", &s[..200])
    } else {
        s
    }
}
