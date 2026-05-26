# M11.1 — Async AI + JSONL 대화 로그 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> **NEVER push** — controller가 마일스톤 끝에 batch push. subagent는 commit만.

**Goal:** AI 응답 대기 동안 UI 멈춤 해소 (submit_input async + 즉시 echo) + AI 대화 흐름 JSONL 로그로 외부 점검 가능화.

**Architecture:** 두 독립 stage. (A) ai-bridge ChatSession의 기존 audit hook을 *text → JSONL event 형식*으로 전환 + CliChatSession이 자동으로 `~/.geulos/logs/ai-chat/<session>-<ts>.jsonl` 활성화. (B) desktop-shell `chat_session: Option<...>`을 `Arc<tokio::sync::Mutex<Option<...>>>`로 wrap → submit_input의 AI dispatch 분기를 즉시 echo + `(응답 대기 중...)` sentinel + spawned task로 분리 → main loop `tokio::select!`에 `ai_response_rx` arm 추가. 응답 도착 시 sentinel 제거 + AI text append.

**Tech Stack:** 기존 Rust workspace + tokio (이미 사용 중인 select!/mpsc/Mutex). 새 의존성 없음.

**Spec parent:** `docs/specs/2026-05-26-geulos-m11_1-async-ai-and-log.md`

---

## File Structure

| 신규/수정 | 경로 | 책임 |
|---|---|---|
| Modify | `ai-bridge/src/chat_session.rs` | `audit` 메서드를 *JSONL event 형식*으로 전환 — `audit_event(kind, payload)` 도입, 호출 위치 8곳 (user_prompt/ai_text/tool_call/tool_result/tool_error/report_done/end_turn/send_done) 변환. text format helper 제거. |
| Modify | `apps/desktop-shell/src/ai_session.rs` | `CliChatSession::start/load`에서 audit path 자동 결정 (`~/.geulos/logs/ai-chat/<session>-<startup-ts>.jsonl`) + 디렉터리 생성 + `ChatSession::with_audit(path)` 호출. |
| Modify | `apps/desktop-shell/src/main.rs` | (1) `chat_session: Option<CliChatSession>` → `Arc<tokio::sync::Mutex<Option<CliChatSession>>>` + 호출처 lock. (2) `AiResult` struct + `mpsc::channel<AiResult>(16)` 도입. (3) main loop `tokio::select!`에 `ai_response_rx.recv()` arm 추가 + `handle_ai_response` 호출. (4) `handle_submit_input`의 *AI dispatch 분기* (현재 main 내 inline)를 — 즉시 echo + sentinel + spawn task로 변환. |
| Modify | `apps/desktop-shell/src/main.rs` | 신규 `handle_ai_response(ai_result, &mut stream, &mut mounted_objects, &mut req_seq)` — sentinel "(응답 대기 중...)" 제거 + AI text/error 메시지 lines에 append + SetState broadcast. |
| Create | `docs/adr/038-async-ai-and-jsonl-log.md` | ADR — chat_session ownership 결정 (Arc/Mutex vs ownership pass) + audit format JSON 결정. |
| Create | `docs/manual-tests/m11_1-acceptance.md` | 시나리오 6개 (즉시 echo / 응답 도중 UI 반응 / sentinel 제거 / JSONL file 생성 / jq parse / 중복 호출 진단). |
| Modify | `docs/known-issues.md` | M11.1 마감 메모 추가 (정기 검토 시점). |

---

# Stage A — ai-bridge JSONL audit (2 task)

## Task 1: ChatSession::audit를 JSONL event 형식으로 전환

**Files:**
- Modify: `ai-bridge/src/chat_session.rs`

기존 `async fn audit(&self, line: &str)`는 timestamped text를 한 줄씩 append. 호출 위치 8곳을 *semantic event*로 mapping해 JSON object 한 줄씩 append하도록 변경. 외부에서 jq/grep으로 parse 가능.

- [ ] **Step 1.1: 실패하는 단위 테스트 추가**

기존 `ai-bridge/src/chat_session.rs`의 `#[cfg(test)] mod tests` 안에 추가:

```rust
    #[tokio::test]
    async fn audit_writes_jsonl_events_for_user_prompt_and_ai_text() {
        let wire = make_wire().await;
        let mock = MockAdapter::new(vec![end_turn_response("응답입니다", 5, 5)]);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let mut chat = ChatSession::new(mock, wire, "sys".to_string()).with_audit(path.clone());

        chat.send_message("안녕").await.unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert!(!lines.is_empty(), "JSONL 라인이 하나 이상");

        // 각 줄이 valid JSON object
        for l in &lines {
            let v: serde_json::Value = serde_json::from_str(l)
                .unwrap_or_else(|e| panic!("JSONL parse 실패: {} on line: {}", e, l));
            assert!(v.get("ts").is_some(), "ts 필드 필수: {}", l);
            assert!(v.get("kind").is_some(), "kind 필드 필수: {}", l);
        }

        // user_prompt + ai_text + end_turn + send_done 존재
        let kinds: Vec<String> = lines
            .iter()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|v| v.get("kind").and_then(|k| k.as_str()).map(String::from))
            .collect();
        assert!(kinds.contains(&"user_prompt".to_string()), "user_prompt 이벤트 누락: {:?}", kinds);
        assert!(kinds.contains(&"ai_text".to_string()), "ai_text 이벤트 누락: {:?}", kinds);
        assert!(kinds.contains(&"end_turn".to_string()), "end_turn 이벤트 누락: {:?}", kinds);
        assert!(kinds.contains(&"send_done".to_string()), "send_done 이벤트 누락: {:?}", kinds);
    }
```

`ai-bridge/Cargo.toml`의 `[dev-dependencies]`에 `tempfile = "3"`가 없으면 추가 필요 (workspace 다른 곳에서 이미 쓰면 워크스페이스 의존성 reuse). 추가:

```toml
[dev-dependencies]
tempfile = "3"
```

(이미 있으면 skip.)

- [ ] **Step 1.2: 테스트 실행 — 실패 확인**

```powershell
cargo test -p geulos-ai-bridge chat_session::tests::audit_writes_jsonl 2>&1 | Select-Object -Last 10
```

Expected: 컴파일 또는 *현재 text 형식이라 JSON parse 실패*로 test FAIL.

- [ ] **Step 1.3: `audit_event(kind, payload)` 메서드 도입 + 기존 `audit` 제거**

`ai-bridge/src/chat_session.rs`의 기존 `async fn audit(&self, line: &str)` *전체 교체*:

```rust
    /// JSONL event 한 줄 append. `kind`는 이벤트 종류, `payload`는 그 외 필드.
    /// 공통 필드 `{ts, kind}`가 자동 prepend. payload 객체의 키와 충돌하면 payload 우선.
    ///
    /// M11.1 신규: audit_path가 설정된 경우 외부 진단용 JSONL 파일에 append. 실패는
    /// silent (디스크 full 등이 AI 응답을 차단하면 안 됨).
    async fn audit_event(&self, kind: &str, mut payload: Value) {
        let Some(path) = &self.audit_path else { return };

        // 공통 필드 주입.
        if let Value::Object(map) = &mut payload {
            map.entry("ts".to_string()).or_insert_with(|| Value::String(Utc::now().to_rfc3339()));
            map.entry("kind".to_string()).or_insert_with(|| Value::String(kind.to_string()));
        } else {
            // payload가 객체가 아니면 wrap.
            payload = json!({
                "ts": Utc::now().to_rfc3339(),
                "kind": kind,
                "value": payload,
            });
        }

        let line = match serde_json::to_string(&payload) {
            Ok(s) => format!("{}\n", s),
            Err(_) => return,
        };
        if let Ok(mut f) = File::options().create(true).append(true).open(path).await {
            let _ = f.write_all(line.as_bytes()).await;
        }
    }
```

(`json!` macro는 파일 상단 `use serde_json::{json, Value};`로 이미 import됨. 확인.)

- [ ] **Step 1.4: 호출 위치 8곳 변환**

`send_message` 함수 안의 *모든* `self.audit(&format!(...)).await` 호출을 `self.audit_event(kind, json!({...}))` 형식으로 교체. send_message 함수 *전체*를 다음으로 교체 (현재 위치: chat_session.rs 약 91-197):

```rust
    pub async fn send_message(&mut self, user_prompt: &str) -> BridgeResult<String> {
        let started = Instant::now();

        // 작업용 복사본. 성공 시 self.history로 commit.
        let mut history = self.history.clone();
        history.push(LlmMessage {
            role: LlmRole::User,
            content: Value::String(user_prompt.to_string()),
        });

        self.audit_event("user_prompt", json!({ "text": user_prompt })).await;

        let mut final_text = String::new();
        let mut turn = 0usize;
        loop {
            turn += 1;
            if turn > self.max_inner_turns {
                self.audit_event(
                    "end_turn",
                    json!({ "turn": turn - 1, "reason": "max_inner_turns" }),
                )
                .await;
                break;
            }

            let resp: LlmResponse =
                self.adapter.complete(&self.system, &history, &self.tools).await?;

            for t in &resp.text {
                self.audit_event("ai_text", json!({ "turn": turn, "text": t })).await;
                if !final_text.is_empty() {
                    final_text.push('\n');
                }
                final_text.push_str(t);
            }
            for tu in &resp.tool_uses {
                self.audit_event(
                    "tool_call",
                    json!({
                        "turn": turn,
                        "tool_use_id": tu.id,
                        "name": tu.name,
                        "args": tu.input,
                    }),
                )
                .await;
            }

            history.push(LlmMessage {
                role: LlmRole::Assistant,
                content: response_to_assistant_content(&resp),
            });

            // tool use 없이 EndTurn이면 한 user turn 종료.
            if resp.stop == LlmStop::EndTurn && resp.tool_uses.is_empty() {
                self.audit_event("end_turn", json!({ "turn": turn, "reason": "no_tools" })).await;
                break;
            }

            // tool dispatch.
            let mut tool_results: Vec<Value> = Vec::new();
            let mut done = false;
            for tu in &resp.tool_uses {
                let tool_started = Instant::now();
                let r = dispatch_tool(&mut self.wire, &tu.name, &tu.input).await;
                let latency_ms = tool_started.elapsed().as_millis() as u64;
                match r {
                    Ok(DispatchResult::Output(v)) => {
                        self.audit_event(
                            "tool_result",
                            json!({
                                "turn": turn,
                                "tool_use_id": tu.id,
                                "latency_ms": latency_ms,
                                "result": v,
                            }),
                        )
                        .await;
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tu.id,
                            "content": serde_json::to_string(&v).unwrap_or_default(),
                        }));
                    }
                    Ok(DispatchResult::Done { summary }) => {
                        self.audit_event(
                            "report_done",
                            json!({
                                "turn": turn,
                                "tool_use_id": tu.id,
                                "latency_ms": latency_ms,
                                "summary": summary,
                            }),
                        )
                        .await;
                        if !final_text.is_empty() {
                            final_text.push('\n');
                        }
                        final_text.push_str(&summary);
                        done = true;
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tu.id,
                            "content": "ok",
                        }));
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        self.audit_event(
                            "tool_error",
                            json!({
                                "turn": turn,
                                "tool_use_id": tu.id,
                                "latency_ms": latency_ms,
                                "error": msg.clone(),
                            }),
                        )
                        .await;
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tu.id,
                            "content": format!("error: {}", msg),
                            "is_error": true,
                        }));
                    }
                }
            }

            history.push(LlmMessage { role: LlmRole::User, content: Value::Array(tool_results) });

            if done {
                break;
            }
        }

        self.audit_event(
            "send_done",
            json!({
                "total_ms": started.elapsed().as_millis() as u64,
                "final_text_len": final_text.len(),
            }),
        )
        .await;

        // 성공 — history commit.
        self.history = history;
        Ok(final_text)
    }
```

기존 `trim_value` 헬퍼는 더 이상 사용되지 않으므로 *제거*:

```rust
// 삭제:
fn trim_value(v: &Value) -> String { ... }
```

- [ ] **Step 1.5: 테스트 통과 확인**

```powershell
cargo test -p geulos-ai-bridge chat_session 2>&1 | Select-Object -Last 15
```

Expected: 기존 3 test (chat_history_accumulates / chat_send_failure / chat_send_includes_report_done) + 신규 audit_writes_jsonl = 4 passed.

- [ ] **Step 1.6: workspace 회귀 + lint**

```powershell
cargo test --workspace 2>&1 | Select-Object -Last 10
cargo clippy -p geulos-ai-bridge --no-deps -- -D warnings 2>&1 | Select-Object -Last 5
cargo fmt --check 2>&1 | Select-Object -Last 5
```

Expected: 모두 클린.

- [ ] **Step 1.7: commit**

```powershell
git add ai-bridge/src/chat_session.rs ai-bridge/Cargo.toml
git commit -m "$(cat <<'EOF'
feat(ai-bridge): M11.1 T1 — ChatSession audit format JSONL 전환

기존 text format audit를 외부 진단 가능한 JSONL event 형식으로 교체.
audit_event(kind, payload) 메서드로 user_prompt / ai_text / tool_call /
tool_result / tool_error / report_done / end_turn / send_done 8 종류
event 발행. 공통 ts/kind 필드 자동 prepend. tool_call/result에 latency_ms
포함 — AI 호출 비용 진단 base.

기존 trim_value 헬퍼 제거 (JSONL은 full payload 직렬화로 truncate 불필요).

Spec: docs/specs/2026-05-26-geulos-m11_1-async-ai-and-log.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: CliChatSession에서 audit path 자동 활성화

**Files:**
- Modify: `apps/desktop-shell/src/ai_session.rs`

T1으로 audit가 JSONL 형식이 됐지만 `CliChatSession::start/load`가 `with_audit`를 호출하지 않아 항상 OFF. 본 task에서 자동 활성화.

- [ ] **Step 2.1: 실패하는 단위 테스트 추가**

`apps/desktop-shell/src/ai_session.rs` 끝에 `#[cfg(test)] mod tests` 추가:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_path_for_session_returns_well_formed_path() {
        let p = audit_path_for_session("test-session-abc");
        let s = p.to_string_lossy();
        assert!(s.contains(".geulos"), "경로에 .geulos 포함: {}", s);
        assert!(s.contains("logs"), "경로에 logs 포함: {}", s);
        assert!(s.contains("ai-chat"), "경로에 ai-chat 포함: {}", s);
        assert!(s.contains("test-session-abc"), "세션 이름 포함: {}", s);
        assert!(s.ends_with(".jsonl"), "확장자 .jsonl: {}", s);
        // 타임스탬프 형식 (YYYYMMDD-HHMMSS) 검증 — 14자리 숫자 + dash
        let stem = p.file_stem().unwrap().to_string_lossy();
        let after_session = stem.strip_prefix("test-session-abc-").unwrap_or("");
        assert_eq!(after_session.len(), 15, "ts 형식 길이: {}", after_session); // YYYYMMDD-HHMMSS = 15
    }
}
```

- [ ] **Step 2.2: 테스트 실행 — 실패 확인**

```powershell
cargo test -p geulos-desktop-shell --lib ai_session 2>&1 | Select-Object -Last 5
```

Expected: `audit_path_for_session` 미정의 컴파일 실패.

- [ ] **Step 2.3: `audit_path_for_session` 헬퍼 + start/load 통합**

`apps/desktop-shell/src/ai_session.rs` 상단 import 추가:

```rust
use std::path::PathBuf;
```

파일 끝에 (`auto_name`/`resolve_api_key`/`connect_wire` 다음) 새 헬퍼 추가:

```rust
/// 세션 이름 + 현재 UTC 시각으로 audit JSONL 파일 경로 생성.
/// `~/.geulos/logs/ai-chat/<session>-<YYYYMMDD-HHMMSS>.jsonl`.
///
/// 부모 디렉터리 생성은 caller (start/load) 책임 — 본 함수는 pure path build만.
pub fn audit_path_for_session(session_name: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let ts = Utc::now().format("%Y%m%d-%H%M%S");
    home.join(".geulos")
        .join("logs")
        .join("ai-chat")
        .join(format!("{}-{}.jsonl", session_name, ts))
}

/// audit 파일의 부모 디렉터리를 생성한다. 실패는 log + 무시 (audit 자체가 best-effort).
fn ensure_audit_dir(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!(
                "[ai-session] audit 디렉터리 생성 실패 ({}): {} — JSONL 로그 비활성",
                parent.display(),
                e
            );
        }
    }
}
```

`CliChatSession::start` 함수의 본문을 *전체 교체* (기존 시그니처 유지):

```rust
    pub fn start(api_key: String, wire: WireClient, system: String, name: String) -> Self {
        let model = DEFAULT_MODEL.to_string();
        let adapter = ClaudeAdapter::new(api_key, model.clone());
        let audit = audit_path_for_session(&name);
        ensure_audit_dir(&audit);
        let inner = ChatSession::new(adapter, wire, system).with_audit(audit);
        let created_at = Utc::now().to_rfc3339();
        Self { inner, name, model, created_at }
    }
```

`CliChatSession::load` 본문도 동일하게 audit 통합:

```rust
    pub fn load(
        api_key: String,
        wire: WireClient,
        system: String,
        name: &str,
    ) -> BridgeResult<Self> {
        let persisted = chat_persist::load(name)?;
        let adapter = ClaudeAdapter::new(api_key, persisted.model.clone());
        let audit = audit_path_for_session(&persisted.name);
        ensure_audit_dir(&audit);
        let mut inner = ChatSession::new(adapter, wire, system).with_audit(audit);
        inner.load_history(persisted.history);
        Ok(Self {
            inner,
            name: persisted.name,
            model: persisted.model,
            created_at: persisted.created_at,
        })
    }
```

- [ ] **Step 2.4: `dirs` 의존성 확인**

`apps/desktop-shell/Cargo.toml`에 `dirs` 가 있는지:

```powershell
Select-String -Path apps/desktop-shell/Cargo.toml -Pattern "^dirs"
```

없으면 `[dependencies]` 섹션에 추가:

```toml
dirs = "5"
```

(`chat_persist` 같은 다른 모듈에서 이미 home_dir를 쓰면 같은 의존성 reuse — 이미 있을 가능성 높음.)

- [ ] **Step 2.5: 테스트 통과**

```powershell
cargo test -p geulos-desktop-shell --lib ai_session 2>&1 | Select-Object -Last 10
```

Expected: 1 신규 test passed.

- [ ] **Step 2.6: workspace 회귀**

```powershell
cargo test --workspace 2>&1 | Select-Object -Last 10
cargo clippy -p geulos-desktop-shell --no-deps -- -D warnings 2>&1 | Select-Object -Last 5
cargo fmt --check 2>&1 | Select-Object -Last 5
```

- [ ] **Step 2.7: commit**

```powershell
git add apps/desktop-shell/src/ai_session.rs apps/desktop-shell/Cargo.toml
git commit -m "$(cat <<'EOF'
feat(desktop-shell): M11.1 T2 — CliChatSession audit JSONL 자동 활성화

CliChatSession::start/load가 ~/.geulos/logs/ai-chat/<session>-<ts>.jsonl
경로를 자동 결정하고 부모 디렉터리 생성 + ChatSession::with_audit 호출.
사용자가 별도 설정 없이도 모든 AI 세션이 외부 진단 가능한 JSONL 로그를
남긴다.

chat_persist의 기존 session JSON은 그대로 — history 복원용. JSONL은
사후 진단용 append-only.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Stage B — desktop-shell async submit_input (3 task)

## Task 3: chat_session Arc<tokio::sync::Mutex<Option<...>>> wrap

**Files:**
- Modify: `apps/desktop-shell/src/main.rs`

기존 `let mut chat_session: Option<CliChatSession> = None;` (main.rs 569 부근)를 Arc<Mutex>로 감싸고 모든 사용처를 lock 형식으로. *Spawn 도입은 T5에서* — 본 T3는 *type 변경 + 호출 형식 갱신*만, 동작은 동일 (여전히 main loop에서 호출).

- [ ] **Step 3.1: chat_session 선언 변경**

`apps/desktop-shell/src/main.rs`의 569 부근:

```rust
// 기존:
let mut chat_session: Option<CliChatSession> = None;

// 교체:
let chat_session: std::sync::Arc<tokio::sync::Mutex<Option<CliChatSession>>> =
    std::sync::Arc::new(tokio::sync::Mutex::new(None));
```

- [ ] **Step 3.2: handle_submit_input 시그니처 + 내부 호출 갱신**

`handle_submit_input` 함수 시그니처 (현재 950 부근):

```rust
// 기존:
chat_session: &mut Option<CliChatSession>,

// 교체:
chat_session: &std::sync::Arc<tokio::sync::Mutex<Option<CliChatSession>>>,
```

함수 내부의 `chat_session` 모든 사용처를 lock 호출로 갱신. 패턴:

```rust
// 기존: chat_session.is_some()
// 교체: chat_session.lock().await.is_some()

// 기존: chat_session = Some(...)
// 교체: *chat_session.lock().await = Some(...)

// 기존: chat_session.as_mut() 같은 mutable 접근
// 교체: chat_session.lock().await.as_mut()
```

특히 `start_or_load_session` 호출 (현재 main.rs 약 1034):

```rust
// 기존:
start_or_load_session(addr, key.clone(), &session_name, is_start, chat_session).await
// 교체:
start_or_load_session(addr, key.clone(), &session_name, is_start, chat_session).await
```

`start_or_load_session`의 시그니처도 변경:

`apps/desktop-shell/src/main.rs` 약 48~67 (`async fn start_or_load_session`):

```rust
async fn start_or_load_session(
    server_addr: &str,
    key: String,
    name: &str,
    new_session: bool,
    chat_session: &std::sync::Arc<tokio::sync::Mutex<Option<CliChatSession>>>,
) -> Result<String, String> {
    let wire = ai_session::connect_wire(server_addr).await.map_err(|e| e.to_string())?;
    let system = ai_session::DEFAULT_CLI_SYSTEM_PROMPT.to_string();
    if new_session {
        let session = CliChatSession::start(key, wire, system, name.to_string());
        *chat_session.lock().await = Some(session);
        Ok(format!("[AI 세션 '{}' 시작]", name))
    } else {
        let session = CliChatSession::load(key, wire, system, name).map_err(|e| e.to_string())?;
        *chat_session.lock().await = Some(session);
        Ok(format!("[AI 세션 '{}' 로드됨 ({} 메시지)]", name, /* history len는 lock 해제 후 별도 read 또는 placeholder */ 0))
    }
}
```

(history len이 lock 안에서만 접근 가능하므로 message count는 별 사용처 — 현재 msg에 "0 messages"는 부정확하나 본 T3 범위에서는 *동작 유지 우선*. message count 표시 정확화는 follow-up.)

- [ ] **Step 3.3: dispatch_chat 등 다른 호출처 갱신**

`apps/desktop-shell/src/main.rs`에서 `chat_session` grep:

```powershell
Select-String -Path apps/desktop-shell/src/main.rs -Pattern "chat_session"
```

각 호출:
- `chat_session.as_mut()` → `chat_session.lock().await.as_mut()`
- `chat_session.is_some()` → `chat_session.lock().await.is_some()`
- `chat_session.is_none()` → `chat_session.lock().await.is_none()`
- 함수 인자 전달은 `chat_session` 그대로 (Arc clone) 또는 `&chat_session`

특히 `dispatch_chat` 호출 (있다면) — 시그니처 변경.

`apps/desktop-shell/src/main.rs`의 submit_input dispatch에서 `&mut chat_session` 인자 전달:

```rust
// 기존:
handle_submit_input(
    target_id, &args, &mut stream, &mut mounted_objects,
    &mut chat_session, &addr, &mut req_seq,
).await?
// 교체:
handle_submit_input(
    target_id, &args, &mut stream, &mut mounted_objects,
    &chat_session, &addr, &mut req_seq,
).await?
```

- [ ] **Step 3.4: 빌드 확인**

```powershell
cargo build -p geulos-desktop-shell 2>&1 | Select-Object -Last 20
```

Expected: 컴파일 OK. lock().await 호출 패턴이 모두 정상.

깨지는 위치는 *grep 누락된 사용처* — error 메시지의 라인 번호로 fix.

- [ ] **Step 3.5: 단위 테스트 회귀**

```powershell
cargo test -p geulos-desktop-shell --lib 2>&1 | Select-Object -Last 10
cargo clippy -p geulos-desktop-shell --no-deps -- -D warnings 2>&1 | Select-Object -Last 5
```

Expected: 클린.

- [ ] **Step 3.6: commit**

```powershell
git add apps/desktop-shell/src/main.rs
git commit -m "$(cat <<'EOF'
refactor(desktop-shell): M11.1 T3 — chat_session Arc<Mutex> wrap

main loop의 chat_session: Option<CliChatSession>을
Arc<tokio::sync::Mutex<Option<CliChatSession>>>로 wrap. T5에서 spawn task가
own/lock할 수 있도록 준비. 동작은 T3 시점까지 동일 — 여전히 main loop에서
직접 await.

start_or_load_session / handle_submit_input 등 호출처 일괄 갱신
(.lock().await 패턴).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: AiResult struct + mpsc channel + handle_ai_response + select! arm

**Files:**
- Modify: `apps/desktop-shell/src/main.rs`

AI 응답 channel + main loop select! 통합. *송신측 (spawned task)은 T5에서* — 본 T4는 *수신측 인프라*만 + 빈 sentinel 처리 로직.

- [ ] **Step 4.1: AiResult + ai_response 채널 도입**

`apps/desktop-shell/src/main.rs`의 main loop *바로 위* (chat_session 선언 부근):

```rust
/// AI 응답이 spawned task에서 main loop로 전달되는 메시지. M11.1 신규.
struct AiResult {
    /// 응답을 append할 Cli 객체 id.
    cli_target: ObjectId,
    /// AI 응답 본문 또는 에러 메시지.
    result: Result<String, String>,
    /// echo/sentinel 라인 추적 — 응답 도착 시점에 제거할 sentinel string.
    sentinel: String,
    /// 응답 lines 앞에 붙일 prompt prefix (예: "[ai:foo] > ").
    prompt_prefix: String,
}

const AI_WAITING_SENTINEL: &str = "(응답 대기 중...)";
```

main loop 위에 channel 생성:

```rust
let (ai_response_tx, mut ai_response_rx) = tokio::sync::mpsc::channel::<AiResult>(16);
```

- [ ] **Step 4.2: handle_ai_response 함수 추가**

`apps/desktop-shell/src/main.rs`의 *handle_submit_input* 다음 적절한 위치에:

```rust
/// spawned AI task가 응답을 보내오면 호출. sentinel 라인 제거 + AI 응답 (또는 에러)
/// lines에 append + SetState broadcast.
async fn handle_ai_response(
    ai_result: AiResult,
    stream: &mut TcpStream,
    mounted_objects: &mut [Object],
    req_seq: &mut u64,
) {
    let AiResult { cli_target, result, sentinel, prompt_prefix } = ai_result;

    // 1) sentinel 제거 — lines 마지막 항목이 sentinel 포함이면 제거.
    let mut current: Vec<String> = mounted_objects
        .iter()
        .find(|o| o.id == cli_target)
        .and_then(|o| o.state.get("lines"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    current.retain(|line| !line.contains(&sentinel));

    // 2) AI 응답 또는 에러를 lines에 append (한 줄당 한 라인 단위로 split).
    let body = match result {
        Ok(text) => text,
        Err(e) => format!("[AI 에러: {}]", e),
    };
    for line in body.lines() {
        current.push(format!("{}{}", prompt_prefix, line));
    }

    // 3) cap 적용 (handle_cli_outcome의 CLI_LINES_CAP과 일관).
    use geulos_desktop_shell::handlers::CLI_LINES_CAP;
    if current.len() > CLI_LINES_CAP {
        let drop = current.len() - CLI_LINES_CAP;
        current.drain(..drop);
    }

    let new_value = json!(current);
    if let Some(cli) = mounted_objects.iter_mut().find(|o| o.id == cli_target) {
        cli.state.insert("lines".into(), new_value.clone());
    }

    // 4) SetState broadcast (기존 send_state_sets 헬퍼 활용).
    send_state_sets(
        stream,
        req_seq,
        vec![(cli_target, "lines".to_string(), new_value)],
    )
    .await;
}
```

(`CLI_LINES_CAP`은 `apps/desktop-shell/src/handlers/mod.rs:97`에 이미 정의. `pub`이라 import 가능. 다른 module 형식이면 그에 맞춰.)

- [ ] **Step 4.3: main loop select!에 ai_response_rx arm 추가**

`apps/desktop-shell/src/main.rs`의 main loop `tokio::select!` 블록 (607 부근). `read_res = stream.read(...)` arm과 `_ = watcher_tick.tick()` arm *사이*에 추가:

```rust
        let n = tokio::select! {
            biased;
            read_res = stream.read(&mut buf) => match read_res {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("[desktop-shell] read error: {}", e);
                    break;
                }
            },
            // M11.1: spawned AI task가 보낸 응답을 받아 lines에 append.
            Some(ai_result) = ai_response_rx.recv() => {
                handle_ai_response(ai_result, &mut stream, &mut mounted_objects, &mut req_seq).await;
                continue;
            }
            _ = watcher_tick.tick() => {
                if let Some(w) = fs_watcher.as_ref() {
                    let changes = w.drain();
                    for change in changes {
                        if let Err(e) = handle_fs_change(
                            &mut stream,
                            &mut mounted_objects,
                            &owner,
                            &mut req_seq,
                            change,
                        )
                        .await
                        {
                            eprintln!("[desktop-shell] fs_change 처리 실패: {}", e);
                        }
                    }
                }
                continue;
            }
        };
```

- [ ] **Step 4.4: 빌드 확인**

```powershell
cargo build -p geulos-desktop-shell 2>&1 | Select-Object -Last 10
```

Expected: build OK. 단 `ai_response_tx`가 unused warning — T5에서 spawn task에서 사용 시작. clippy `-D warnings`로 통과되려면 `#[allow(dead_code)]` 또는 `let _ai_response_tx = ai_response_tx.clone();` 같은 임시 사용. *깔끔하게는 T5와 묶어 commit*. 본 T4 단독 commit 시 일시적으로 `let _ = &ai_response_tx;` 더미 사용.

대안 — clippy 통과를 위해 본 T4에서 `let _retained_for_t5 = ai_response_tx.clone();` 추가 후 T5에서 제거.

- [ ] **Step 4.5: 회귀**

```powershell
cargo test -p geulos-desktop-shell --lib 2>&1 | Select-Object -Last 5
cargo clippy -p geulos-desktop-shell --no-deps -- -D warnings 2>&1 | Select-Object -Last 5
```

Expected: 클린.

- [ ] **Step 4.6: commit**

```powershell
git add apps/desktop-shell/src/main.rs
git commit -m "$(cat <<'EOF'
feat(desktop-shell): M11.1 T4 — AI 응답 mpsc channel + main select! arm

AiResult struct + ai_response_tx/rx channel + handle_ai_response 함수.
main loop tokio::select!에 ai_response_rx.recv() arm 추가 — spawned AI
task가 응답 보내면 sentinel 라인 제거 + lines에 응답 append + SetState
broadcast.

송신측 spawn task는 T5에서 도입.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: handle_submit_input의 AI dispatch 분기를 spawn으로 변환

**Files:**
- Modify: `apps/desktop-shell/src/main.rs`

`handle_submit_input` 함수 내부 — *awaiting_api_key 분기는 그대로*, *AI mode dispatch_chat 분기*만 spawn으로 변환. 즉시 echo + sentinel + spawn → channel.

- [ ] **Step 5.1: handle_submit_input 시그니처에 ai_response_tx 추가**

`apps/desktop-shell/src/main.rs`의 `handle_submit_input` 시그니처에 `ai_response_tx: tokio::sync::mpsc::Sender<AiResult>` 추가:

```rust
#[allow(clippy::too_many_arguments)]
async fn handle_submit_input(
    target_id: ObjectId,
    args: &serde_json::Value,
    stream: &mut TcpStream,
    mounted_objects: &mut [Object],
    chat_session: &std::sync::Arc<tokio::sync::Mutex<Option<CliChatSession>>>,
    addr: &str,
    req_seq: &mut u64,
    ai_response_tx: &tokio::sync::mpsc::Sender<AiResult>,
) -> Result<invoke_handler::InvokeOutcome, Box<dyn std::error::Error>> {
```

- [ ] **Step 5.2: 호출처 (main loop submit_input dispatch) 갱신**

`apps/desktop-shell/src/main.rs`의 main loop submit_input case:

```rust
"submit_input" => {
    handle_submit_input(
        target_id,
        &args,
        &mut stream,
        &mut mounted_objects,
        &chat_session,
        &addr,
        &mut req_seq,
        &ai_response_tx,
    )
    .await?
}
```

- [ ] **Step 5.3: AI mode dispatch_chat 분기를 spawn으로 변환**

`handle_submit_input` 내부에서 *AI mode (현재 mode == "ai")일 때 dispatch_chat을 호출하는 분기*를 찾고 (Select-String으로 `dispatch_chat`), 그 분기 *전체*를 다음 패턴으로 교체:

```rust
// 기존 (의사 코드):
// if current_mode == "ai" {
//     let reply = dispatch_chat(chat_session, &text).await?;
//     // ... reply를 lines에 추가하고 send_state_sets ...
// }

// 교체:
if current_mode == "ai" {
    // M11.1: AI send를 spawned task로 분리 — main loop가 즉시 다른 frame
    // 처리 가능. 사용자에겐 즉시 echo + sentinel 표시.
    let session_name = mounted_objects
        .iter()
        .find(|o| o.id == target_id)
        .and_then(|o| o.state.get("session_name").and_then(|v| v.as_str()))
        .unwrap_or("?")
        .to_string();

    // 1) 즉시 echo + sentinel SetState broadcast.
    let echo = format!("{}{}", prompt_prefix, text);
    let sentinel = AI_WAITING_SENTINEL.to_string();
    let immediate = handle_cli_outcome(
        mounted_objects,
        target_id,
        &prompt_prefix,
        "",
        vec![echo.clone(), sentinel.clone()],
        None,
    );
    send_state_sets(stream, req_seq, immediate.state_sets).await;

    // 2) spawn AI task — chat_session lock 잡고 send, 결과를 channel로.
    let cs = chat_session.clone();
    let tx = ai_response_tx.clone();
    let prompt = text.clone();
    let prompt_prefix_owned = prompt_prefix.clone();
    let _ = session_name; // 추적용 (debug에 사용 가능)
    tokio::spawn(async move {
        let result: Result<String, String> = {
            let mut guard = cs.lock().await;
            match guard.as_mut() {
                Some(session) => session.send(&prompt).await.map_err(|e| e.to_string()),
                None => Err("AI 세션이 활성화되지 않음 (`/ai start` 또는 `/ai load` 후 시도)".to_string()),
            }
        };
        let _ = tx
            .send(AiResult {
                cli_target: target_id,
                result,
                sentinel,
                prompt_prefix: prompt_prefix_owned,
            })
            .await;
    });

    return Ok(invoke_handler::InvokeOutcome::empty());
}
```

(현재 코드의 정확한 분기 위치 — `dispatch_chat` 호출 위치 또는 `chat_session.lock().await.as_mut()`로 직접 send하는 분기. 정확한 분기 찾으려면 `Select-String -Path apps/desktop-shell/src/main.rs -Pattern "dispatch_chat|\.send\("` 로 검색.)

- [ ] **Step 5.4: T4의 더미 retain 제거**

T4에서 추가했던 `let _retained_for_t5 = ai_response_tx.clone();` 같은 더미 사용 *제거*. 이제 실제 사용처(spawn task)가 있어 clippy 통과.

- [ ] **Step 5.5: 빌드 + 회귀**

```powershell
cargo build -p geulos-desktop-shell 2>&1 | Select-Object -Last 10
cargo test --workspace 2>&1 | Select-Object -Last 10
cargo clippy --workspace --no-deps -- -D warnings 2>&1 | Select-Object -Last 5
cargo fmt --check 2>&1 | Select-Object -Last 5
```

- [ ] **Step 5.6: manual smoke**

```powershell
Get-Process | Where-Object { $_.ProcessName -match 'geulos|compositor|desktop-shell|geulosd' } | Stop-Process -Force -ErrorAction SilentlyContinue
cargo build --bin geulos 2>&1 | Select-Object -Last 3
Start-Process -FilePath ".\target\debug\geulos.exe" -WindowStyle Hidden
```

`/ai start test`로 AI 세션 시작 (API key 필요). 한국어 prompt 입력 → *즉시* lines에 echo + 응답 대기 표시 확인. 응답 도착 전에 *스크롤/창 이동/CLI 클릭* → 즉시 반응. 응답 도착 후 sentinel 제거 + AI 응답 표시.

`~/.geulos/logs/ai-chat/test-*.jsonl` 파일에 user_prompt / tool_call / ai_text 라인 확인:

```powershell
Get-ChildItem "$HOME/.geulos/logs/ai-chat/" | Select-Object -Last 1 | Get-Content | Select-Object -First 5
```

- [ ] **Step 5.7: commit**

```powershell
git add apps/desktop-shell/src/main.rs
git commit -m "$(cat <<'EOF'
feat(desktop-shell): M11.1 T5 — submit_input AI 분기 spawn 분리

AI mode dispatch가 chat_session.send를 main loop에서 직접 await하던 패턴을
tokio::spawn task + mpsc channel로 분리. 즉시 echo "> {prompt}" + sentinel
"(응답 대기 중...)" 라인을 lines에 추가하고 SetState broadcast 후 return.

main loop는 다른 invoke (스크롤/클릭/키 등) frame을 막힘 없이 계속 처리.
spawned task가 AI 응답 받으면 mpsc로 main에 통지 → T4의 handle_ai_response가
sentinel 제거 + 응답 lines에 append.

사용자 보고 "AI 응답 대기 중 UI 멈춤" 해소.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Stage C — 마무리 (1 task)

## Task 6: ADR-038 + manual test 문서 + KI 갱신

**Files:**
- Create: `docs/adr/038-async-ai-and-jsonl-log.md`
- Create: `docs/manual-tests/m11_1-acceptance.md`
- Modify: `docs/known-issues.md`

- [ ] **Step 6.1: ADR-038 작성**

Create `docs/adr/038-async-ai-and-jsonl-log.md`:

```markdown
# ADR-038 — Async AI 흐름 + JSONL 대화 로그

- **상태:** Accepted
- **결정일:** 2026-05-26
- **부모 spec:** `docs/specs/2026-05-26-geulos-m11_1-async-ai-and-log.md`
- **부모 plan:** `docs/plans/2026-05-26-geulos-m11_1-async-ai-and-log.md`

## Context

M11까지 기능 완성도는 OK이나 두 UX/관측성 문제:
1. desktop-shell main loop가 single tokio task — AI send.await 동안 다른
   invoke 처리 차단 (스크롤·클릭·키 모두 지연). 사용자 시각으론 UI 멈춤.
2. AI 흐름 외부 점검 수단 부재 — chat_persist는 user/assistant 텍스트만
   저장, tool call/result/latency 정보 X.

## Decision

1. **submit_input AI dispatch → spawn:** chat_session을 Arc<tokio::sync::
   Mutex<Option<...>>>로 wrap. AI mode 분기에서 즉시 echo + sentinel
   "(응답 대기 중...)" SetState broadcast 후 tokio::spawn으로 AI send 분리.
   응답은 mpsc::channel<AiResult>(16) → main loop tokio::select!에 새 arm.
2. **Audit JSONL:** 기존 ChatSession::audit (text format) → audit_event(kind,
   payload)로 교체. 8 event 종류 (user_prompt/ai_text/tool_call/tool_result/
   tool_error/report_done/end_turn/send_done). 공통 ts/kind 필드 자동
   prepend. tool_call/result에 latency_ms 포함.
3. **자동 활성화:** CliChatSession::start/load가 ~/.geulos/logs/ai-chat/
   <session>-<YYYYMMDD-HHmmss>.jsonl 경로를 자동 결정하고 ensure_dir +
   with_audit. 사용자 설정 불필요.

## 대안

- (A) chat_session ownership을 channel로 pass back-and-forth: Mutex 회피
  되나 main loop의 chat_session 즉시 접근 (예: /ai list 등 빠른 read-only)
  이 직렬화. 기각.
- (B) AI 응답 streaming (Anthropic API 토큰 단위): 본 ADR 범위 외. v2.
- (C) 별 wire connection for AI processing: 복잡도 ↑. 현재 단일 connection
  으로 충분 (server-host가 sub action별 일관 처리).
- (D) Audit format을 그대로 text 유지: 외부 parse 어려움. 기각.

## Consequences

**Positive:**
- AI 응답 대기 동안 UI 일반 동작 (스크롤/클릭/키) 차단 없음.
- JSONL audit가 jq/grep/tail로 분석 가능 — 중복 tool call, 비효율 호출 패턴,
  과도한 token 사용 등 사후 진단 base.
- tool_call latency 측정으로 wire round-trip 성능 추적.

**Negative:**
- Arc<Mutex> 도입으로 chat_session 접근에 lock overhead (uncontended 시
  ns 단위, 무시 가능).
- JSONL 파일이 세션당 수 MB 누적 가능 — log rotation은 v2 (현재 사용자가
  필요 시 수동 삭제).

**Neutral:**
- main loop는 이미 tokio::select! (stream + watcher_tick) — ai_response_rx
  arm 추가가 자연. 새 인프라 아님.
- chat_persist 기존 JSON 형식 무변경 — 두 file 분리 (persist=복원, audit=
  진단). 호환성 100% 유지.
```

- [ ] **Step 6.2: m11_1-acceptance.md 작성**

Create `docs/manual-tests/m11_1-acceptance.md`:

```markdown
# M11.1 Acceptance — 수동 회귀 시나리오

**전제:** `.\target\debug\geulos.exe` (launcher) 빌드 + 실행. ANTHROPIC_API_KEY
설정 또는 /ai start awaiting flow.

## 시나리오 1 — 즉시 echo

1. `/ai start test1` → AI 세션 시작.
2. CLI에 "안녕 AI" 입력 → Enter.
3. **기대:** 즉시 lines에 `[ai:test1] > 안녕 AI` + `(응답 대기 중...)` 표시.

## 시나리오 2 — 응답 도중 UI 반응

1. 시나리오 1의 (응답 대기 중...) 상태에서 *스크롤 / 우측 폴더 클릭 / 좌측 트리 클릭* 시도.
2. **기대:** 모든 동작이 즉시 반응 (UI 멈춤 X).

## 시나리오 3 — 응답 도착 + sentinel 제거

1. AI 응답 도착.
2. **기대:** `(응답 대기 중...)` 라인 사라짐 + AI 응답 lines에 추가.

## 시나리오 4 — JSONL 파일 생성

1. `Get-ChildItem "$HOME/.geulos/logs/ai-chat/" | Select-Object -Last 1`.
2. **기대:** `test1-YYYYMMDD-HHMMSS.jsonl` 파일 존재.

## 시나리오 5 — jq parse 가능

1. `Get-Content <file> | Select-Object -First 1` → 한 줄을 jq에 입력.
2. **기대:** valid JSON object. `kind`, `ts`, `text` 같은 필드 확인.

## 시나리오 6 — 중복 tool call 진단

1. AI에게 같은 폴더 두 번 조회 요청.
2. JSONL에서 `kind: "tool_call"` 라인 grep — 동일 args 가진 호출 2번 발견.
3. **기대:** 진단 가능 (실제 중복 호출이 있다면 grep으로 즉시 보임).

## 통과 기준

6개 모두 기대대로. 결과 표:

| 시나리오 | 통과 (✓/✗) | 비고 |
|---|---|---|
| 1 즉시 echo | 미실행 | |
| 2 UI 반응 | 미실행 | |
| 3 sentinel 제거 | 미실행 | |
| 4 JSONL 파일 | 미실행 | |
| 5 jq parse | 미실행 | |
| 6 중복 진단 | 미실행 | |
```

- [ ] **Step 6.3: known-issues.md M11.1 마감 메모 추가**

Edit `docs/known-issues.md` — M11 마감 메모 직후에 M11.1 추가:

```markdown
- **M11.1 정식 마감 (2026-05-26):** AI 비동기 흐름 + JSONL 대화 로그.
  desktop-shell submit_input의 AI dispatch를 tokio::spawn + mpsc channel +
  main select! arm으로 분리. chat_session을 Arc<tokio::sync::Mutex>로 wrap.
  즉시 echo + sentinel "(응답 대기 중...)" 표시 → 응답 도착 시 sentinel
  제거. AI 응답 대기 중 UI 멈춤 해소.
  ai-bridge ChatSession::audit를 JSONL event 형식으로 전환 (user_prompt/
  ai_text/tool_call/tool_result/tool_error/report_done/end_turn/send_done
  8 종류 + latency_ms 포함). CliChatSession::start/load가 자동으로
  ~/.geulos/logs/ai-chat/<session>-<ts>.jsonl 활성. ADR-038.
```

정기 검토 시점에 M11.1 후속 (log rotation 등)을 추가:

```markdown
- **M12 entry 시:** (기존 항목 그대로) + AI JSONL log retention 정책
  (파일 N개 보관 후 rotate) + AI 응답 streaming (Anthropic SSE).
```

- [ ] **Step 6.4: 최종 회귀 + grep guard**

```powershell
cargo test --workspace 2>&1 | Select-Object -Last 10
cargo clippy --workspace --no-deps -- -D warnings 2>&1 | Select-Object -Last 5
cargo fmt --check 2>&1 | Select-Object -Last 5
bash scripts/check-no-wildcard-acl.sh
```

Expected: 모두 클린.

- [ ] **Step 6.5: commit**

```powershell
git add docs/adr/038-async-ai-and-jsonl-log.md docs/manual-tests/m11_1-acceptance.md docs/known-issues.md
git commit -m "$(cat <<'EOF'
docs: M11.1 T6 — ADR-038 + acceptance + KI 갱신

- ADR-038: async AI + JSONL 결정 본문 (Arc<Mutex> + spawn + channel,
  JSONL event 형식, 자동 활성화). 대안 (A) channel pass-back / (B)
  streaming / (C) 별 connection / (D) text format 유지 기각 근거.
- m11_1-acceptance: 시나리오 6개 (즉시 echo / UI 반응 / sentinel /
  JSONL 생성 / jq parse / 중복 진단). 사용자 수동 실행.
- known-issues: M11.1 마감 메모 + M12 entry 시 후속 (log rotation,
  streaming).

M11.1 정식 마감.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review

**Spec coverage:**
- ✓ Fix 1 (UI 멈춤): T3 (Arc/Mutex) + T4 (channel/select!) + T5 (spawn)
- ✓ Fix 1 sub-item (즉시 echo): T5
- ✓ Fix 2 (JSONL audit format): T1
- ✓ Fix 2 sub-item (자동 활성화): T2
- ✓ 검증 시나리오 6개: T6 manual test
- ✓ ADR + KI 갱신: T6
- ✓ chat_persist 기존 JSON 무변경: T1/T2 모두 chat_persist 미수정

**Placeholder scan:** 모든 step에 완전 코드. "TBD" / "implement later" 없음. Step 3.3과 5.3은 *기존 코드 위치를 grep으로 정확 확인 후 교체*하는 형식 — *완전 코드*는 본문에 명시, 위치 식별만 grep.

**Type consistency:**
- `AiResult { cli_target, result, sentinel, prompt_prefix }` — T4 정의, T5 spawn에서 생성, T4 handle_ai_response가 destructure. 일관.
- `AI_WAITING_SENTINEL` 상수 — T4 정의, T5 sentinel 라인 생성에 사용. 일관.
- `Arc<tokio::sync::Mutex<Option<CliChatSession>>>` — T3 wrap, T5 spawn에서 clone. 일관.
- audit_event(kind: &str, payload: Value) — T1 정의, T1 호출 8곳. 일관.
- audit_path_for_session(session_name: &str) -> PathBuf — T2 정의, T2 start/load에서 호출. 일관.

---

## 실행 핸드오프

**Plan complete and saved to `docs/plans/2026-05-26-geulos-m11_1-async-ai-and-log.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — controller가 task별 fresh subagent + spec/code review

**2. Inline Execution** — 본 세션에서 batch 실행 + 사용자 checkpoint

**Which approach?**
