# M13 — ConsoleWindow@1 (long-running process 시각화 + 제어)

**Date:** 2026-05-28
**Status:** Draft (사용자 review 대기)
**Parent:** M12 (ShellRunner@1 one-shot) 후속

## 동기

M12 ShellRunner@1은 *one-shot* 명령만 지원 (`tokio::Command::wait_with_output` 종료까지 block → 결과 8 state SetState 1회 + 종료). long-running process (`npm run dev`, `npm test --watch`, `node server.js`, watcher)는 다음 한계로 *실용 불가*:

- 120초 default timeout에 *강제 kill*되거나
- spawn 후 *결과 SetState가 영원히 안 옴* → 객체 모델에서 *존재 자체 미관측*
- Windows `TerminateProcess`는 *부모만* kill → npm.cmd → node → esbuild 손주 process *orphan화* → 사용자가 *Task Manager로도 찾기 어려움*

**M12.2 사용자 시연 (2026-05-28)**: AI가 vite dev server를 띄움. ShellRunner.run의 timeout이 hit되어 wait abort, child handle drop. 하지만 npm/node/esbuild process tree는 orphan으로 살아남음. 사용자가 "현재 개발서버가 OS 내부에서 숨어 실행되고 있고 이것을 확인할 방법이 없어"라 보고. 결국 PowerShell `Stop-Process`로 강제 정리 — *GeulOS 비전 정면 위배*.

**비전과의 충돌**: GeulOS는 *AI를 OS-level에서 지원하는 것*이 강점. AI가 띄운 dev server가 *시각화*되지 않고 *제어 불가*하면 Windows의 Claude Code/OpenInterpreter 대비 *작업성·사용감* 강점 상실.

M13은 long-running process를 **GeulOS 객체 트리에 시각화 + 제어 가능한 1급 객체**로 도입.

## 비-목표

- typed Process Objects (GitRepo@1 / NpmProject@1 / NpmProject.run_dev) — *M14+*
- container 격리 환경 — *M15+*
- stdin 입력 (사용자가 `r` 키로 vite hot reload trigger 등) — *v2*
- AI restart/pause/SIGINT 분리 method — terminate 하나로 통합
- Unix (Linux/macOS) JobObject 동등 구현 — *v2* (cfg(unix) placeholder + KI)
- Console buffer를 디스크 full log file로 백업 — *v2* (현재는 ring buffer 500 line만)

## 범위

**핵심 1 객체 + 1 method + Dialog 흐름 + Windows JobObject + UI 패널**:

- 신규 객체: `aios.builtin/ConsoleWindow@1` (per-process, multi-instance)
- ShellRunner@1 확장: 신규 method `run_streamed(cmd, args, cwd)` → ConsoleWindow id 반환
- Stream pipeline: stdout/stderr line별 SetState 즉시 broadcast (ring buffer 500 line)
- terminate method: 사용자 + AI 호출 가능. AI는 Dialog 동의 필수.
- Windows JobObject (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`): 손주 process까지 cascade kill 보장
- UI: Window@1-유사 floating panel + X 닫기 = terminate
- AI prompt: run vs run_streamed 가이드

## Architecture

### 객체 정의 (`core/src/object/std_types.rs`)

신규 factory `console_window(owner, cmd, args, cwd, title)`:

**props** (불변):
- `cmd: String` — 실행한 binary 이름 (예: "npm")
- `args: Vec<String>` — 인자 (예: ["run", "dev"])
- `cwd: String` — 절대 경로
- `pid: u32` — OS process id (디버그 + 외부 진단)
- `title: String` — UI titlebar 표시 ("npm run dev — tmp-react-app" 등 desktop-shell 자동 생성)
- `x, y, w, h: i32` — Window@1과 동일한 geometry. 초기값은 desktop-shell이 cascade 위치 계산.

**state**:
- `lines: Vec<String>` — stdout + stderr interleaved. 각 line은 prefix 포함:
  - stdout: 원본 그대로
  - stderr: `"[stderr] "` 접두 (사용자가 시각적 구분 + AI도 parse 가능)
  - ring buffer max 500. overflow 시 가장 오래된 line pop_front.
  - **순서**: stdout reader와 stderr reader는 *별 tokio task*. 두 stream이 *동시* 출력 시 mpsc 도착 순서가 *실제 wall-clock과 약간 reorder 가능*. v1은 mpsc 도착 순서 그대로 (대다수 시연에서 시각적으로 충분 — 시간 차이 ms 단위). strict 시간순은 v2 (BufRead poll + 단일 reader로 통합).
- `line_count: u64` — 총 누적 line 수 (truncation 추적). UI는 "showing last N of M" 표시.
- `status: String` — `"running"` | `"exited"` | `"terminated"` | `"error"`
- `exit_code: Option<i64>` — null=running, 0=정상 종료, 그 외=오류, -1=signal/error
- `started_at: String` — ISO 8601 timestamp (mount 시점)
- `ended_at: Option<String>` — exit/terminated 시점
- `scroll_y: i32` — UI scroll 위치 (사용자 변경)

**methods**:
- `terminate()` — 사용자/AI 호출. AI는 Dialog 동의 필수.
- `move(x, y)`, `resize(w, h)`, `focus()`, `scroll(y)`, `close()` — Window@1과 동일 UI 메서드. `close()`는 `terminate()`의 alias (UI 호환 — X 버튼 클릭 시 compositor가 close 또는 terminate 어느 쪽 invoke해도 동일 결과).

**parent**: desktop_id (Window@1과 동일 — 라이프사이클 함께).

### ShellRunner@1 확장 (`core/src/object/std_types.rs`)

기존 `shellrunner()` factory의 methods 배열에 `run_streamed` 추가:

```rust
obj.set_methods(vec!["run", "run_streamed"]);
```

props (allowed_binaries / default_timeout_ms)는 *그대로 공유* — long-running용 별 list 안 만듦. 사용자가 부적절한 명령으로 run_streamed 호출하면 (e.g. `git status`) 그냥 정상 종료되어 status="exited"로 즉시 표시 — 정상 동작.

### ACL helper (`apps/desktop-shell/src/handlers/mod.rs`)

기존 `add_shellrunner_acl`은 변경 없음 (run + run_streamed 모두 AiSession Exact("run")...로는 부족 → 변경 필요):

기존:
```
AiSession / Exact("run") / Allow
```

변경:
```
AiSession / Exact("run") / Allow
AiSession / Exact("run_streamed") / Allow
```

신규 `add_console_window_acl(obj: &mut Object)`:
- SystemCompositor / Wildcard / Allow — compositor가 X 닫기 / move / resize / focus / scroll 직접 호출
- AiSession / Exact("terminate") / Allow — AI는 terminate만 (Dialog 동의는 handler에서 처리)
- App("desktop-shell") / SetState / Allow — stream pipeline의 SetState

### Stream event 정의 (`apps/desktop-shell/src/handlers/shellrunner_methods.rs`)

신규 enum (ShellRunResult 옆):

```rust
#[derive(Debug)]
pub enum ConsoleEvent {
    Line { target_id: ObjectId, kind: LineKind, text: String },
    Exit { target_id: ObjectId, exit_code: i64, status: String },
}

#[derive(Debug, Clone, Copy)]
pub enum LineKind { Stdout, Stderr }
```

main loop의 select! arm에 `console_rx` 신규 — `mpsc::channel::<ConsoleEvent>(256)`.

### run_streamed handler (`shellrunner_methods.rs::handle_run_streamed`)

handle_run과 동일한 검증 (allowed_binaries + cwd) 후:

**AI sender 분기** (sender_actor가 `ai:` 접두사):
1. Dialog mount (`AI ShellRunner.run_streamed: <cmd> <args>` 메시지)
2. `PendingFs::ShellStream { cmd, args, cwd, requesting_actor }` 등록
3. Dialog [허용] → dialog_methods가 `spawn_streamed`를 호출 (아래)
4. 거부 → 끝

**compositor 직접 분기**:
- 즉시 `spawn_streamed` 호출

### `spawn_streamed` 함수 (`shellrunner_methods.rs`)

```
spawn_streamed(
    stream: &mut TcpStream,
    mounted_objects: &mut Vec<Object>,
    owner: ActorId,
    desktop_id: ObjectId,
    req_seq: &mut u64,
    cmd: String,
    args: Vec<String>,
    cwd: PathBuf,
    console_tx: mpsc::Sender<ConsoleEvent>,
) -> Result<ObjectId, Box<dyn Error>>
```

흐름:
1. ConsoleWindow@1 객체 생성 + add_console_window_acl + state.status="running"/started_at/lines=[]
2. **Windows JobObject 생성** (아래 섹션)
3. tokio::Command spawn:
   - `.stdin(Stdio::null())` (M12와 동일 — interactive prompt 회피)
   - `.stdout(Stdio::piped())`, `.stderr(Stdio::piped())`
   - Windows: `.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW)` — assign-then-resume 패턴 필수
4. spawn 직후 → AssignProcessToJobObject → ResumeThread
5. ConsoleWindow.props.pid = child.id() 채움
6. mount + subscribe (compositor Lifecycle 자동 도착) + mounted_objects.push
7. tokio::spawn 3 task:
   - **stdout reader**: `BufReader::new(child.stdout).lines()` loop → `ConsoleEvent::Line { kind: Stdout, text }` → console_tx.send
   - **stderr reader**: 동일, kind=Stderr
   - **exit waiter**: `child.wait().await` → `ConsoleEvent::Exit { exit_code, status: "exited" }` → console_tx.send. JobObject handle은 *exit waiter task가 owns* — task 종료 시 drop → job close → 이미 process 죽었으므로 cascade kill no-op.
8. ConsoleWindow id 반환 (InvokeOutcome에 event_id로)

### main loop select! arm (`apps/desktop-shell/src/main.rs`)

기존 shellrun_rx와 같은 패턴:

```rust
ev = console_rx.recv() => {
    match ev {
        Some(ConsoleEvent::Line { target_id, kind, text }) => {
            apply_console_line(&mut mounted_objects, &mut stream, &mut req_seq,
                                target_id, kind, text).await;
        }
        Some(ConsoleEvent::Exit { target_id, exit_code, status }) => {
            apply_console_exit(&mut mounted_objects, &mut stream, &mut req_seq,
                                target_id, exit_code, status).await;
        }
        None => break,
    }
}
```

**`apply_console_line`**:
- mounted_objects에서 target 찾음 → state.lines push (overflow 시 pop_front) → state.line_count++
- 2건 SetState broadcast (lines + line_count) — line별 즉시

**`apply_console_exit`**:
- state.status = status / exit_code = Some(code) / ended_at = now()
- 3건 SetState broadcast

### terminate method handler (`apps/desktop-shell/src/handlers/console_window_methods.rs` 신설)

`handle_terminate`:
1. sender_actor가 `ai:` 접두 → Dialog mount + `PendingFs::ConsoleTerminate { target_id, requesting_actor }`
2. 그 외 (system:compositor) → 즉시 terminate 실행

terminate 실행:
- target ConsoleWindow의 JobObject handle 조회 (아래 ProcessRegistry 참조)
- `TerminateJobObject(job, 1)` — 즉시 모든 descendant kill
- exit waiter task가 wake → `ConsoleEvent::Exit { status: "terminated", exit_code: -1 }` 자동 송신
- → 정상 cleanup 경로로 합류

### `move/resize/focus/scroll/close` handler (`console_window_methods.rs`)

Window@1의 동등 handler를 그대로 따라감 (state.x/y/w/h/scroll_y 갱신 + SetState). `close`는 `terminate` 호출로 위임.

### ProcessRegistry (`apps/desktop-shell/src/process_registry.rs` 신설)

ConsoleWindow id ↔ JobObject handle 매핑. desktop-shell process 안의 in-memory HashMap.

```rust
pub struct ProcessRegistry {
    inner: Arc<Mutex<HashMap<ObjectId, JobHandle>>>,
}
```

JobHandle은 `Send`해야 (mpsc + handler 간 이동). Windows의 HANDLE은 Send 가능 — `unsafe impl Send`.

handle_terminate / exit waiter 모두 registry 통해 JobObject 조회. exit waiter 정상 종료 시 registry에서 제거.

### Windows JobObject 구현 (`apps/desktop-shell/src/job_object.rs` 신설)

`windows-sys` crate (workspace에 추가 — 의존성 추가 비용 낮음).

API:
```rust
pub struct JobHandle(HANDLE);

impl JobHandle {
    pub fn create() -> std::io::Result<Self>;
    pub fn assign_process(&self, pid: u32) -> std::io::Result<()>;
    pub fn terminate(&self) -> std::io::Result<()>;
}
impl Drop for JobHandle { /* CloseHandle */ }
unsafe impl Send for JobHandle {}
```

구현 핵심:
- `CreateJobObjectW(null, null)` → HANDLE
- `SetInformationJobObject(handle, JobObjectExtendedLimitInformation, &mut info, ...)`
  with `info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`
- `OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid)` → process handle
- `AssignProcessToJobObject(job, proc)` → 성공/실패 (E.g. UAC 권한)
- `TerminateJobObject(job, 1)` — 모든 process exit code 1로 kill

Unix (`#[cfg(unix)]`): `pub fn create() -> Result<Self> { unimplemented!("M13 v1 Windows only") }` — 빌드는 통과, runtime에 panic. v2에서 setsid + killpg(SIGTERM) + 3초 후 killpg(SIGKILL)로 구현. KI-027 등록.

### Wire 흐름 (run_streamed 예시)

```
AI                                    desktop-shell                    server-host
 │  Invoke{run_streamed, cmd, args, cwd}
 ├─────────────────────────────────────►
 │                                     │ Dialog mount (PendingFs::ShellStream)
 │                                     ├─────────────────────────────────►
 │   InvokeAck (event_id)
 ◄─────────────────────────────────────┤
 │                                     │ (사용자/bg Dialog [허용])
 │                                     │ Dialog.respond → spawn_streamed
 │                                     │ ConsoleWindow Mount + Job create + spawn
 │                                     ├─────────────────────────────────►
 │   Lifecycle(Created ConsoleWindow)
 ◄─────────────────────────────────────────────────────────────────────────┤
 │  Get/Subscribe(ConsoleWindow, ["StateSet"])
 ├─────────────────────────────────────────────────────────────────────────►
 │                                     │ stdout/stderr reader → ConsoleEvent::Line
 │                                     │ main loop → SetState{lines, line_count}
 │                                     ├─────────────────────────────────►
 │   StateSet event
 ◄─────────────────────────────────────────────────────────────────────────┤
 │  (반복)
 │
 │  Invoke{terminate} on ConsoleWindow
 ├─────────────────────────────────────►
 │                                     │ Dialog mount
 │                                     │ (사용자 [허용]) → TerminateJobObject
 │                                     │ exit waiter → ConsoleEvent::Exit
 │                                     │ main loop → SetState{status, exit_code, ended_at}
 │                                     ├─────────────────────────────────►
 │   StateSet event
 ◄─────────────────────────────────────────────────────────────────────────┤
```

### UI (compositor)

ConsoleWindow는 compositor에서 Window@1과 *동일한 z-stack*에 배치. STD_TYPES에 ConsoleWindow type-level subscribe 추가 (M8 KI-004 패턴).

**렌더링** (`compositor/src/render/`):
- Window@1과 동일한 floating panel layout (border + titlebar + 본문)
- titlebar: `"<cmd> <args>"` + status dot (running=초록 #4ade80 / exited=회색 #888 / terminated=빨강 #ef4444 / error=주황 #f59e0b) + X 닫기 버튼
- 본문: monospace 폰트, line별 표시. stderr line은 `[stderr] ` 접두 색상 약간 다르게 (옵션, v1엔 단색 OK).
- scroll_y에 따라 wrapping window의 윗부분 cut
- 본문 영역 좌우 padding 8px

**hit_test** (`compositor/src/hit_test.rs`):
- Window@1과 동일한 titlebar drag / resize edge / X 클릭 처리
- X 클릭 → compositor가 `invoke close()` on ConsoleWindow → desktop-shell이 terminate로 위임

**multi-window**:
- compositor.startup에서 STD_TYPES 추가에 ConsoleWindow 포함
- desktop-shell이 새 ConsoleWindow mount → Lifecycle Created → compositor 자동 ID-subscribe + 렌더 트리 push
- 무제한. cascade 배치 (M8 Window 패턴).

### AI prompt 변경 (`ai-bridge/src/system_prompt.md`)

ShellRunner@1 섹션 method 목록에 `run_streamed` 추가 + 가이드:

> **`run(cmd, args, cwd)`** — *one-shot* 명령 (cargo build / npm install / git commit). 1초~몇 분, 결과 1회. state.last_* 8 fields.
>
> **`run_streamed(cmd, args, cwd)`** — *long-running* 명령 (npm run dev / npm test --watch / node server.js). ConsoleWindow@1 객체 mount + 그 id 반환. AI는:
>   1. event_id로 InvokeAck 수신 (run과 동일 ack-only)
>   2. `list_objects_by_type("aios.builtin/ConsoleWindow@1")`로 방금 생성된 객체 발견 (props.cmd/cwd 매칭). Dialog 동의 전엔 mount 안 됨 — 1~2초 polling.
>   3. `subscribe(<cw_id>, ["StateSet"])` + drain — state.lines 실시간 stream.
>   4. **drain empty 시 `get_object(<cw_id>)`로 state.lines 폴백 확인** (KI-026 동일 race — subscribe 이전 도착한 line 놓칠 수 있음). 1초 간격 ~5회 polling.
>   5. dev server URL은 보통 처음 ~20 line 안에 등장 (vite: `"Local:   http://localhost:5173/"`) — 발견 시 *사용자에게 즉시 안내*.
>   6. 작업 완료 시 `invoke_method(<cw_id>, "terminate", {})` — 사용자에게 *별 Dialog 동의* 필수.
>
> **언제 어느 method 쓸지:**
>   - 명령이 *명백히 종료*되는 것 (build/install/commit/test 1회 실행) → `run`
>   - 명령이 *사용자가 닫을 때까지 살아있어야* 하는 것 (dev server / watcher / REPL) → `run_streamed`
>   - 헷갈리면 `run`이 안전 (timeout 후 cleanup 보장).

### 테스트

**Unit** (`core/tests/`):
- `console_window_test.rs::console_window_factory_creates_with_acl_methods` — factory 호출 → props/state/methods 검증
- `console_window_test.rs::ring_buffer_overflow_pops_oldest` — apply_console_line을 600회 → lines.len() == 500, line_count == 600

**Integration** (`apps/desktop-shell/tests/` 또는 `server-host/tests/m13_acceptance.rs`):
- `m13_run_streamed_emits_lines_and_exits` — compositor actor가 `node -e "console.log('a');console.log('b')"` run_streamed → ConsoleWindow mount → SetState lines 도착 (a/b) → SetState status="exited" exit_code=0
- `m13_terminate_kills_cascade_process_tree` (Windows only) — `node -e "setInterval(()=>{},1000)"` spawn → child PID 확인 → terminate invoke → Get-Process node 0 확인
- `m13_ai_dialog_required_for_terminate` — AI actor가 terminate invoke → Dialog mount 확인 (실행 안 됨) → Dialog.respond("허용") → 실제 terminate

**Manual acceptance** (`docs/manual-tests/m13-acceptance.md`):
1. AI가 vite dev server 띄움
2. ConsoleWindow가 desktop에 floating panel로 표시 (cascade 위치)
3. titlebar status dot이 초록 (running)
4. 본문에 vite 시작 로그 줄줄이 stream (Local URL 등장)
5. AI가 사용자에게 "http://localhost:5173에서 확인하세요" 안내
6. 사용자가 X 클릭 → 1초 안에 status가 빨강 (terminated) → 모든 node process 종료 (Get-Process node 0)
7. (별 시나리오) AI가 작업 완료 후 자동 terminate 시도 → Dialog 표시 → 사용자 [허용] → cleanup

## 보안 / ACL

ConsoleWindow@1 ACL:
- SystemCompositor / Wildcard / Allow — compositor가 close/move/resize/focus/scroll 자유
- AiSession / Exact("terminate") / Allow — AI는 terminate만 (Dialog 동의는 handler가 처리)
- App("desktop-shell") / SetState / Allow — stream pipeline의 SetState

AI는 *terminate만 가능, move/resize 등 UI 조작 불가*. 사용자가 직접 조작하거나 compositor가 처리.

terminate Dialog 동의는 *세션마다 매번*. M11 dir-grant 같은 영속 grant 없음 — process kill은 중대한 변경이므로 매번 확인.

ShellRunner.run_streamed 자체의 Dialog도 *매번*. run과 동일.

## Windows-only 한계

- Unix (Linux/macOS) 빌드는 통과하나 spawn_streamed runtime panic — KI-027 등록
- v2에서 nix crate의 `setsid` + `Pid::from_raw(-pgid)` + `killpg(SIGTERM)` → 3초 grace → `killpg(SIGKILL)`로 구현
- 우리 dev box (Windows 11)에서 모든 시연 가능 → v1 사용성 충분

## Migration / 호환성

- 기존 `ShellRunner@1` 객체의 wire 호환성 *보존* (props/state 변경 없음, methods에 항목 추가만 — 기존 client는 무시)
- 기존 `add_shellrunner_acl`에 `Exact("run_streamed")` 추가 — 기존 ACL 평가 무영향
- 새 객체 타입 추가는 *기존 코드 영향 없음* — compositor STD_TYPES에 1줄 추가만

## 알려진 한계 / 후속 (M14+)

- **stdin 미지원**: vite의 `r` (restart) / `q` (quit) 키 안 됨. 사용자는 terminate + 재spawn으로 우회.
- **scroll auto-bottom**: 사용자가 명시 scroll 안 했으면 새 line 도착 시 자동 bottom으로. 명시 scroll 후엔 그 위치 유지. v2.
- **terminate timeout**: TerminateJobObject 후 cleanup 시간이 길어지는 process (e.g. graceful shutdown handler가 있는 server) 대응 — 5초 wait + force. v2.
- **stderr 색상 구분**: v1엔 단색 (prefix만). v2에 색상.
- **scrollback search**: 본문 검색 (Ctrl+F) — v2.
- **typed Process Objects**: NpmProject@1 / GitRepo@1 등으로 cmd/args 추상화 — M14+.
- **Unix JobObject 동등**: setsid + process group — v2 (KI-027).

## 작업 분류 (plan 단계 hint)

- Phase 1 (객체 + factory + ACL): 객체 정의 / std_types::console_window / add_console_window_acl
- Phase 2 (JobObject + spawn pipeline): job_object.rs / process_registry.rs / shellrunner_methods::handle_run_streamed / spawn_streamed
- Phase 3 (event 흐름): ConsoleEvent enum / main loop select! arm / apply_console_line / apply_console_exit
- Phase 4 (terminate + Dialog): console_window_methods::handle_terminate / PendingFs::ConsoleTerminate / dialog_methods 분기 추가
- Phase 5 (UI): compositor STD_TYPES 추가 / render ConsoleWindow / hit_test X 닫기
- Phase 6 (AI 통합): system_prompt.md 갱신 / auto_react_project example 확장 (npm run dev 띄우고 URL 확인)
- Phase 7 (테스트 + 문서): unit + integration + manual acceptance + ADR-040

## ADR 후보

ADR-040 — "Windows JobObject로 long-running process 관리". 결정: tokio child handle 직접 kill 대신 JobObject로 묶어 cascade kill. 대안: PowerShell `taskkill /T` (외부 의존), psutil-like crate (별 의존성). 선택 이유: Win32 직접 호출 가장 신뢰성 + 우리 환경에 정확히 적합.
