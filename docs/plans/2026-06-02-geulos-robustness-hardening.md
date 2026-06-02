# 견고성 하드닝 (Robustness Hardening) Implementation Plan

> **Status:** planned (2026-06-02)
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 도그푸딩 루프를 직접 망가뜨리는 누적 부채 5건(AI hang, node 좀비, 로그 무제한 누적, 콘솔 lag, 컴파일 경고)을 새 마일스톤 진입 전에 일괄 해소한다.

**Architecture:** 기존 인프라를 *건드리지 않고* 좁은 지점만 보강한다. (1) `ai-bridge` wire 클라이언트에 per-request deadline, (2) `geulos-host-bridge` 종료 시 자식 프로세스 cascade kill, (3) `desktop-shell`의 ai-chat JSONL audit에 retention rotate, (4) 콘솔 polling 간격 단축, (5) dead/unused 코드 정리.

**Tech Stack:** Rust (tokio async for wire, std threads for host-bridge), `ctrlc` crate (신규, host-bridge 종료 훅), `taskkill /F /T` (Windows process tree kill, 기존 패턴 재사용).

**근거 / Spec 출처:** `docs/known-issues.md` — KI-032, KI-029, KI-031, KI-030 + "정기 검토 시점" 절. 사용자 결정(2026-06-02): 견고성 하드닝을 다음 작업으로 선택.

---

## File Structure

| 파일 | 책임 | 변경 |
|---|---|---|
| `ai-bridge/src/wire.rs` | wire RPC | `WireError::Timeout` 추가, `WireClient`에 `request_timeout` 필드 + setter, `request()` 루프를 `tokio::time::timeout`으로 감쌈 |
| `ai-bridge/tests/wire_timeout_test.rs` | KI-032 회귀 | 신규 — 응답 안 오는 mock 서버로 timeout 검증 |
| `crates/geulos-host-bridge/Cargo.toml` | deps | `ctrlc = "3"` 추가 |
| `crates/geulos-host-bridge/src/exec.rs` | process registry | `taskkill_pid()` private 헬퍼 추출 + `exec_stream_kill_all()` 신규 |
| `crates/geulos-host-bridge/src/main.rs` | 진입점 | `ctrlc::set_handler`로 종료 시 `exec_stream_kill_all()` |
| `apps/desktop-shell/src/ai_session.rs` | ai-chat 세션 | `rotate_audit_logs()` 신규 + `start`/`load`에서 호출 |
| `apps/desktop-shell/src/handlers/shellrunner_methods.rs` | 콘솔 스트리밍 | polling 500ms→100ms (2곳) + 경고 정리 |

각 Task는 독립적으로 빌드·테스트·커밋 가능하다. Task 순서는 위험도 오름차순(독립 fix → 종료 훅 → 정리)이나 상호 의존 없음 — 병렬 배정 가능.

---

### Task 1: KI-032 — wire `request()`에 per-request deadline

**증상:** `ai-bridge/src/wire.rs:92-103`의 `request()` 루프가 일치하는 `request_id` 프레임이 올 때까지 무한 대기. 서버가 응답을 영영 안 보내면(dead lock / 네트워크) AI 도구 호출이 hang — 사용자가 Ctrl+C 외 회복 수단 없음.

**해법:** `WireClient`에 `request_timeout: Duration` (기본 30s) 필드 추가. `request()` 전체를 `tokio::time::timeout`으로 감싸고 만료 시 `WireError::Timeout` 반환. 테스트용 `with_request_timeout` setter로 짧은 값 주입.

**Files:**
- Modify: `ai-bridge/src/wire.rs` (WireError enum ~20, WireClient struct ~41, connect_as_ai ~73, request ~87)
- Test: `ai-bridge/tests/wire_timeout_test.rs` (Create)

- [ ] **Step 1: `WireError::Timeout` variant 추가**

`ai-bridge/src/wire.rs` 의 `WireError` enum에 추가 (`Closed` 바로 위):

```rust
    /// 요청이 deadline 안에 응답받지 못함 (KI-032).
    #[error("request timed out after {0:?}")]
    Timeout(std::time::Duration),
    /// 연결이 예기치 않게 종료됨.
    #[error("connection closed unexpectedly")]
    Closed,
```

- [ ] **Step 2: `WireClient`에 `request_timeout` 필드 + import 추가**

상단 import에 `use std::time::Duration;` 추가. struct 변경:

```rust
pub struct WireClient {
    stream: TcpStream,
    actor_id: String,
    accum: Vec<u8>,
    request_timeout: Duration,
}
```

`connect_as_ai`의 마지막 `return Ok(Self { stream, actor_id: ack.actor_id, accum });` 를:

```rust
                return Ok(Self {
                    stream,
                    actor_id: ack.actor_id,
                    accum,
                    request_timeout: Duration::from_secs(30),
                });
```

`actor_id()` 메서드 바로 아래에 setter 추가:

```rust
    /// 요청 deadline 변경 (기본 30s). 테스트·튜닝용.
    pub fn with_request_timeout(mut self, d: Duration) -> Self {
        self.request_timeout = d;
        self
    }
```

> 주의: `WireClient`를 생성하는 다른 connect 함수(`connect_as_compositor` 등)가 있으면 *동일하게* `request_timeout: Duration::from_secs(30)` 필드를 채워야 컴파일된다. `grep -n "Self {" ai-bridge/src/wire.rs`로 모든 생성 지점 확인.

- [ ] **Step 3: `request()`를 timeout으로 감쌈**

기존 `request()` 본문(line 87-104)을 교체:

```rust
    async fn request(&mut self, msg: &Value) -> WireResult<Value> {
        let expected_rid = msg.get("request_id").and_then(|v| v.as_str()).map(String::from);
        let body = serde_json::to_vec(msg)?;
        self.stream.write_all(&encode_frame(&body)).await?;
        let timeout = self.request_timeout;
        // expected_rid 있으면 일치할 때까지 frame skip; 없으면 첫 frame 반환.
        // 전체 루프를 deadline으로 감싸 서버 무응답/broadcast 폭주 시 hang 방지 (KI-032).
        let fut = async {
            loop {
                let frame = self.read_frame_json().await?;
                if let Some(rid) = &expected_rid {
                    let got = frame.get("request_id").and_then(|v| v.as_str());
                    if got == Some(rid.as_str()) {
                        return Ok(frame);
                    }
                    continue;
                }
                return Ok(frame);
            }
        };
        match tokio::time::timeout(timeout, fut).await {
            Ok(r) => r,
            Err(_) => Err(WireError::Timeout(timeout)),
        }
    }
```

- [ ] **Step 4: 회귀 테스트 작성**

`ai-bridge/tests/wire_timeout_test.rs` (Create). mock 서버가 Hello에는 HelloAck로 응답하지만 이후 get_object 요청에는 *침묵* → timeout 검증:

```rust
//! KI-032 회귀: wire request()가 서버 무응답 시 deadline으로 빠져나오는지.

use std::time::Duration;

use geulos_ai_bridge::wire::{WireClient, WireError};
use geulos_proto::{encode_frame, HelloAck};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn request_times_out_when_server_silent() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // mock 서버: Hello 프레임 1개 읽고 HelloAck 응답, 그 후 영원히 침묵.
    let server = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        // Hello 수신 (내용 무시).
        let _ = sock.read(&mut buf).await.unwrap();
        let ack = HelloAck { actor_id: "ai:test".to_string() };
        let body = serde_json::to_vec(&ack).unwrap();
        sock.write_all(&encode_frame(&body)).await.unwrap();
        // 이후 어떤 요청에도 응답하지 않음 — 연결만 유지.
        tokio::time::sleep(Duration::from_secs(5)).await;
    });

    let client = WireClient::connect_as_ai(&addr.to_string())
        .await
        .unwrap()
        .with_request_timeout(Duration::from_millis(200));
    let mut client = client;

    let start = std::time::Instant::now();
    let res = client.get_object("00000000-0000-0000-0000-000000000000").await;
    let elapsed = start.elapsed();

    assert!(matches!(res, Err(WireError::Timeout(_))), "expected Timeout, got {res:?}");
    assert!(elapsed < Duration::from_secs(2), "should fail fast, took {elapsed:?}");

    server.abort();
}
```

> `HelloAck`의 실제 필드명은 `geulos_proto`를 확인 (`grep -n "struct HelloAck" proto/src/`). actor_id 단일 필드가 아니면 그에 맞춰 조정.

- [ ] **Step 5: 테스트 실패 확인 (Step 1-3 전 상태 가정 시) → 통과 확인**

Run: `cargo test -p geulos-ai-bridge --test wire_timeout_test -- --nocapture`
Expected: PASS (Step 1-3 적용 후). 적용 전이면 200ms 안에 안 끝나고 5s 가까이 hang.

- [ ] **Step 6: 빌드·린트·커밋**

Run: `cargo build -p geulos-ai-bridge && cargo clippy -p geulos-ai-bridge --all-targets -- -D warnings`
Expected: 클린

```bash
git add ai-bridge/src/wire.rs ai-bridge/tests/wire_timeout_test.rs
git commit -m "fix(ai-bridge): wire request()에 30s deadline — 서버 무응답 hang 차단 (KI-032)"
```

---

### Task 2: KI-029 — host bridge 종료 시 자식 프로세스 cascade kill

**증상:** AI가 명시 `terminate` 호출 시엔 `taskkill /F /T`로 정상 cascade. 그러나 사용자가 VM 자체 종료(QEMU close / launch.ps1 Ctrl+C) 시 host bridge가 죽기 전 REGISTRY의 자식(npm/node 등)을 kill 안 함 → Windows에서 orphan 좀비로 잔존.

**해법:** `exec.rs`에 REGISTRY 전체를 cascade kill하는 `exec_stream_kill_all()` 추가. `main.rs`에서 `ctrlc` crate로 SIGINT/SIGTERM/Ctrl-Close 시 호출 후 종료.

**Files:**
- Modify: `crates/geulos-host-bridge/Cargo.toml` (deps)
- Modify: `crates/geulos-host-bridge/src/exec.rs` (kill 로직 ~217-245)
- Modify: `crates/geulos-host-bridge/src/main.rs` (main ~136)

- [ ] **Step 1: `ctrlc` 의존성 추가**

`crates/geulos-host-bridge/Cargo.toml` 의 `[dependencies]` 에 추가:

```toml
ctrlc = "3"
```

- [ ] **Step 2: `taskkill_pid` 헬퍼 추출 + `exec_stream_kill_all` 추가**

`crates/geulos-host-bridge/src/exec.rs` 의 `exec_stream_kill` 함수의 `#[cfg(windows)]` taskkill 블록을 private 헬퍼로 추출하고, kill_all을 추가한다. `exec_stream_kill`의 taskkill 본문을 다음 호출로 교체:

```rust
    #[cfg(windows)]
    {
        taskkill_pid(pid);
    }
```

그리고 파일 하단(`exec_stream_kill` 뒤)에 추가:

```rust
/// 단일 pid의 process tree를 cascade kill. Windows `taskkill /F /T`.
#[cfg(windows)]
fn taskkill_pid(pid: u32) {
    match Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(out) if !out.status.success() => {
            let err = String::from_utf8_lossy(&out.stderr).into_owned();
            eprintln!("[exec] taskkill /T /PID {} 부분 실패: {}", pid, err);
        }
        Ok(_) => {}
        Err(e) => eprintln!("[exec] taskkill spawn 실패 (pid {}): {}", pid, e),
    }
}

/// REGISTRY의 *모든* 스트림 프로세스를 cascade kill. 호스트 브리지 종료 훅에서 호출
/// (KI-029) — VM 종료 시 npm/node 손주가 orphan으로 잔존하던 문제 해소.
pub fn exec_stream_kill_all() {
    let entries: Vec<(String, u32)> = match REGISTRY.lock() {
        Ok(mut reg) => reg.drain().map(|(id, e)| (id, e.child.id())).collect(),
        Err(e) => {
            eprintln!("[exec] kill_all registry lock 실패: {}", e);
            return;
        }
    };
    for (id, pid) in entries {
        eprintln!("[exec] 종료 cleanup: stream {} pid {} kill", id, pid);
        #[cfg(windows)]
        taskkill_pid(pid);
        #[cfg(not(windows))]
        {
            let _ = pid;
        }
    }
}
```

> `exec_stream_kill`의 non-windows 분기(`entry.child` 직접 kill)는 그대로 둔다 — kill_all의 non-windows 동등(killpg)은 KI-027 v2 범위.

- [ ] **Step 3: 헬퍼 단위 테스트 추가**

`exec.rs` 하단 `#[cfg(test)] mod tests`에 (없으면 신설). Windows에서 장수 프로세스 spawn 후 kill_all이 REGISTRY를 비우는지:

```rust
#[cfg(test)]
mod kill_all_tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn kill_all_drains_registry() {
        // 장수 프로세스 spawn (ping -t는 무한, -n 100은 100초).
        let (sid, _pid) = exec_stream_start("cmd", &["/C".into(), "ping -n 100 127.0.0.1".into()], ".")
            .expect("spawn");
        assert!(REGISTRY.lock().unwrap().contains_key(&sid));
        exec_stream_kill_all();
        assert!(REGISTRY.lock().unwrap().is_empty(), "kill_all 후 REGISTRY 비어야 함");
    }
}
```

> `exec_stream_start`의 실제 시그니처(args 타입: `&[String]` vs `&[&str]`, cwd 타입)를 `grep -n "pub fn exec_stream_start" exec.rs`로 확인 후 호출 인자 맞춤.

- [ ] **Step 4: `main.rs`에 종료 훅 등록**

`crates/geulos-host-bridge/src/main.rs` 의 `fn main()` 에서 `auth::init_from_env();` 바로 다음에 추가:

```rust
    // KI-029: Ctrl+C / 종료 시 spawn된 자식 프로세스 전부 cascade kill.
    if let Err(e) = ctrlc::set_handler(|| {
        eprintln!("[host-bridge] 종료 신호 — 자식 프로세스 cleanup");
        exec::exec_stream_kill_all();
        std::process::exit(0);
    }) {
        eprintln!("[host-bridge] ctrlc handler 등록 실패: {}", e);
    }
```

- [ ] **Step 5: 빌드 + 테스트**

Run: `cargo test -p geulos-host-bridge && cargo clippy -p geulos-host-bridge --all-targets -- -D warnings`
Expected: PASS + 클린

- [ ] **Step 6: 수동 검증 절차 기록 + 커밋**

수동 검증(커밋 메시지/PR에 명시): VM 부팅 → AI가 `npm run dev` 스트리밍 → launch.ps1 Ctrl+C → 작업관리자에서 `node.exe` 잔존 0 확인. (CI 자동화 불가 — dev box 수동.)

```bash
git add crates/geulos-host-bridge/
git commit -m "fix(host-bridge): 종료 시 REGISTRY 자식 cascade kill — node 좀비 잔존 차단 (KI-029)"
```

---

### Task 3: KI-031 — ai-chat JSONL audit 로그 retention

**증상:** `~/.geulos/logs/ai-chat/<session>-<ts>.jsonl`이 세션마다 신규 파일 무제한 누적. 1년 장기 dev 머신에 ~1GB 압박.

**해법:** `ai_session.rs`에 `rotate_audit_logs()` — ai-chat 디렉터리에서 `.jsonl` 파일을 mtime 내림차순 정렬, `MAX_AUDIT_FILES`(500) 초과분 삭제. `start`/`load`에서 `ensure_audit_dir` 직후 호출.

**Files:**
- Modify: `apps/desktop-shell/src/ai_session.rs` (start ~45, load ~58, helper 영역 ~149-170)
- Test: `apps/desktop-shell/src/ai_session.rs` 내부 `#[cfg(test)] mod tests`

- [ ] **Step 1: 회귀 테스트 작성 (실패 확인용)**

`ai_session.rs` 의 `mod tests` 에 추가. 임시 디렉터리에 가짜 `.jsonl` N개 만들고 rotate 후 개수 검증:

```rust
    #[test]
    fn rotate_keeps_at_most_max_files() {
        let tmp = std::env::temp_dir().join(format!("geulos-rotate-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        // MAX_AUDIT_FILES + 5 개 생성.
        for i in 0..(MAX_AUDIT_FILES + 5) {
            let f = tmp.join(format!("sess-{:04}.jsonl", i));
            std::fs::write(&f, b"{}\n").unwrap();
        }
        rotate_audit_logs(&tmp);
        let count = std::fs::read_dir(&tmp)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().map(|x| x == "jsonl").unwrap_or(false))
            .count();
        assert_eq!(count, MAX_AUDIT_FILES, "rotate 후 {} 개 남아야 함", MAX_AUDIT_FILES);
        std::fs::remove_dir_all(&tmp).ok();
    }
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `cargo test -p geulos-desktop-shell rotate_keeps_at_most_max_files`
Expected: FAIL — `MAX_AUDIT_FILES`/`rotate_audit_logs` 미정의로 컴파일 에러.

- [ ] **Step 3: `rotate_audit_logs` + 상수 구현**

`ai_session.rs` 의 `ensure_audit_dir` 함수 바로 아래에 추가:

```rust
/// ai-chat audit JSONL 보관 상한. 초과분은 가장 오래된 것부터 삭제 (KI-031).
const MAX_AUDIT_FILES: usize = 500;

/// `dir` 안의 `*.jsonl`을 mtime 내림차순 정렬해 `MAX_AUDIT_FILES` 초과분(가장 오래된 것)
/// 삭제. best-effort — 읽기/삭제 실패는 log 후 무시 (audit retention이 세션 시작을 막으면 안 됨).
fn rotate_audit_logs(dir: &std::path::Path) {
    let mut files: Vec<(std::path::PathBuf, std::time::SystemTime)> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().map(|x| x == "jsonl").unwrap_or(false))
            .filter_map(|e| {
                let mtime = e.metadata().ok()?.modified().ok()?;
                Some((e.path(), mtime))
            })
            .collect(),
        Err(_) => return,
    };
    if files.len() <= MAX_AUDIT_FILES {
        return;
    }
    // 최신 우선 정렬 → 앞쪽 MAX개 유지, 나머지 삭제.
    files.sort_by(|a, b| b.1.cmp(&a.1));
    for (path, _) in files.into_iter().skip(MAX_AUDIT_FILES) {
        if let Err(e) = std::fs::remove_file(&path) {
            eprintln!("[ai-session] audit rotate 삭제 실패 ({}): {}", path.display(), e);
        }
    }
}
```

- [ ] **Step 4: `start`/`load`에서 rotate 호출**

`start`의 `ensure_audit_dir(&audit);` 다음 줄에:

```rust
        if let Some(dir) = audit.parent() {
            rotate_audit_logs(dir);
        }
```

`load`의 `ensure_audit_dir(&audit);` 다음에도 동일 블록 추가.

- [ ] **Step 5: 테스트 통과 확인**

Run: `cargo test -p geulos-desktop-shell rotate_keeps_at_most_max_files`
Expected: PASS

- [ ] **Step 6: 빌드·린트·커밋**

Run: `cargo clippy -p geulos-desktop-shell --all-targets -- -D warnings` (Task 5에서 잔여 경고 정리하므로 여기선 신규 경고만 없으면 OK)

```bash
git add apps/desktop-shell/src/ai_session.rs
git commit -m "feat(desktop-shell): ai-chat audit JSONL retention rotate (500개 상한, KI-031)"
```

---

### Task 4: KI-030 — 콘솔 스트리밍 polling 간격 500ms→100ms

**증상:** ConsoleWindow 스트리밍이 host bridge를 500ms 간격 polling. vite/webpack 초기 200+ line burst가 500ms 단위로 끊겨 보임.

**해법:** `shellrunner_methods.rs`의 polling 간격 2곳(인자 `500` + `sleep from_millis(500)`)을 명명 상수 `CONSOLE_POLL_MS = 100`으로 교체. host bridge 부하 5x이나 단일 사용자 dev 환경에서 무시할 수준.

**Files:**
- Modify: `apps/desktop-shell/src/handlers/shellrunner_methods.rs` (~533, ~652)

- [ ] **Step 1: 명명 상수 도입 + 적용**

`shellrunner_methods.rs` 상단(또는 함수 근처 적절한 위치)에 상수 추가:

```rust
/// ConsoleWindow 스트리밍 polling 간격 (ms). KI-030: 500→100으로 burst lag 완화.
const CONSOLE_POLL_MS: u64 = 100;
```

line ~533의 polling 간격 인자 `500,` → `CONSOLE_POLL_MS,` (해당 인자가 ms를 받는지 호출부 시그니처 확인 — 받는다면 직접 치환, 아니면 주변 맥락 맞춤).

line ~652의 `tokio::time::sleep(std::time::Duration::from_millis(500)).await;` →

```rust
                tokio::time::sleep(std::time::Duration::from_millis(CONSOLE_POLL_MS)).await;
```

> `grep -n "500" apps/desktop-shell/src/handlers/shellrunner_methods.rs`로 두 지점 정확 확인 후 치환. ring buffer 크기 `500`(line ~666 주석)은 *건드리지 말 것* — 무관한 상수.

- [ ] **Step 2: 빌드 확인**

Run: `cargo build -p geulos-desktop-shell`
Expected: 성공

- [ ] **Step 3: 커밋**

```bash
git add apps/desktop-shell/src/handlers/shellrunner_methods.rs
git commit -m "perf(desktop-shell): 콘솔 polling 500ms→100ms — dev server burst lag 완화 (KI-030)"
```

---

### Task 5: 컴파일 경고 7건 정리

**증상:** `cargo build -p geulos-desktop-shell`이 7 경고: `unreachable statement`, unused `stream`/`mounted_objects`/`req_seq`/`console_tx`/`cw_id`, dead fn `strip_ansi`.

**해법:** 각 경고를 *의미를 보존하며* 해소 — 진짜 unused면 `_` prefix 또는 제거, dead fn은 미래 재활용 의도면 `#[allow(dead_code)]` + 메모, 아니면 제거. `unreachable statement`는 로직 버그 신호일 수 있으니 *반드시 코드를 읽고* 판단.

**Files:**
- Modify: `apps/desktop-shell/src/handlers/shellrunner_methods.rs` (cw_id ~537, strip_ansi ~457)
- Modify: 나머지 경고 위치 (빌드 출력의 파일:라인으로 특정)

- [ ] **Step 1: 전체 경고 목록 + 위치 수집**

Run: `cargo build -p geulos-desktop-shell 2>&1 | grep -A3 "warning:"`
각 경고의 `파일:라인`을 기록한다 (7건).

- [ ] **Step 2: `unreachable statement` 먼저 — 코드 읽고 판단**

해당 위치를 Read로 열어 *왜* unreachable인지 확인. 위쪽에 무조건 `return`/`break`/`continue`가 있어 뒤 코드가 죽었는지 검사.
- 로직 버그(원래 실행돼야 할 코드)면 → 제어 흐름 수정.
- 의도된 죽은 코드면 → 죽은 문장 제거.
판단 근거를 커밋 메시지에 한 줄 남긴다.

- [ ] **Step 3: unused 변수 5건 처리**

각 변수(`stream`, `mounted_objects`, `req_seq`, `console_tx`, `cw_id`)에 대해:
- 의도적으로 안 쓰는 바인딩(패턴 매칭 등)이면 → `_` prefix (`let _cw_id = ...` 또는 `let _ = ...`).
- 원래 써야 하는데 빠진 거면 → 사용 코드 보강 (로직 검토 필요).
대부분 `cw_id`처럼 destructure 잔재면 `_` prefix가 정답.

- [ ] **Step 4: dead fn `strip_ansi` 처리**

`shellrunner_methods.rs:457` `strip_ansi`가 어디서도 안 쓰임. 결정:
- 콘솔 출력에서 ANSI 이스케이프를 제거해야 하는데 *연결이 빠진* 거라면(스트리밍 line에 raw ANSI가 섞여 보이는지 확인) → 적용 지점에 연결.
- 단순 미사용 잔재면 → 함수 제거 (git history로 복구 가능).
KI-030 콘솔 UX와 연관 가능성 있으니 *line 렌더 경로에서 ANSI가 보이는지* 먼저 확인 후 결정.

- [ ] **Step 5: 경고 0 확인**

Run: `cargo build -p geulos-desktop-shell 2>&1 | grep -c "warning:"`
Expected: `0`

Run: `cargo clippy -p geulos-desktop-shell --all-targets -- -D warnings`
Expected: 클린

- [ ] **Step 6: 커밋**

```bash
git add apps/desktop-shell/src/handlers/shellrunner_methods.rs
git commit -m "chore(desktop-shell): 컴파일 경고 7건 정리 (unused/dead/unreachable)"
```

---

## 최종 검증 (전체 Task 후)

- [ ] **Step 1: workspace 전체 그린**

Run: `cargo test --workspace`
Expected: 전체 PASS (~200 + 신규 3건)

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 클린

Run: `cargo fmt --check`
Expected: 클린

- [ ] **Step 2: known-issues.md 갱신**

`docs/known-issues.md`에서 KI-029/030/031/032를 *해소* 표시 (✅ + 날짜 + 변경 요약 1-2줄). "정기 검토 시점"의 "다음 작업 시 우선 검토" 목록에서 해소된 항목 제거.

```bash
git add docs/known-issues.md
git commit -m "docs(known-issues): KI-029/030/031/032 해소 — 견고성 하드닝 (2026-06-02)"
```

---

## Self-Review 메모

- **Spec 커버리지:** KI-032(Task1)/KI-029(Task2)/KI-031(Task3)/KI-030(Task4)/경고(Task5) — known-issues "우선 검토" 4건 + 위생 전부 매핑됨.
- **타입 일관성:** `WireError::Timeout(Duration)` — Task1 정의·테스트 일치. `MAX_AUDIT_FILES`/`rotate_audit_logs`/`CONSOLE_POLL_MS`/`exec_stream_kill_all`/`taskkill_pid` 단일 정의·사용.
- **미해결 가정 (실행 시 확인):** (a) `WireClient`의 다른 생성 지점에 `request_timeout` 필드 추가 필요, (b) `HelloAck` 실제 필드명, (c) `exec_stream_start` 인자 타입, (d) shellrunner polling 인자가 ms 단위인지. 각 Task 내 주석으로 명시.
- **범위 밖(의도적 연기):** KI-027(Unix killpg 동등) — kill_all non-windows는 no-op 유지. AI streaming(SSE)·보안 부채(KI-024/002/003)·M14 — 별 작업.
