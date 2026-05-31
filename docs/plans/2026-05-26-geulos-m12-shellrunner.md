> **Status:** completed (2026-05-26)
> **Note:** M12 정식 마감 — ShellRunner@1 + 화이트리스트 binary + 120s timeout (ADR-039). 후속 M13 streaming + 후속 host routing으로 확장.

# M12 — ShellRunner@1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> **NEVER push** — controller가 마일스톤 끝에 batch push. subagent는 commit만.

**Goal:** AI/사용자가 화이트리스트 binary (git/npm/cargo/docker/...)를 Dialog 동의 후 실행할 수 있는 escape hatch — ShellRunner@1 singleton + run(cmd, args, cwd) method.

**Architecture:** Filesystem@1 패턴 답습. core std_types에 shellrunner() factory + run method sig. desktop-shell이 ShellRunner singleton mount + handle_run dispatch. AI sender면 PendingShellRun을 PendingMap에 등록 + Dialog mount. compositor가 Dialog.respond("허용") 보내면 execute_command가 tokio Command spawn + capture output + 7 state SetState broadcast.

**Tech Stack:** 기존 Rust workspace + tokio. 새 dependency 없음 (tokio::process::Command + tokio::time::timeout 이미 사용 중).

**Spec parent:** `docs/specs/2026-05-26-geulos-m12-shellrunner.md`

---

## File Structure

| 신규/수정 | 경로 | 책임 |
|---|---|---|
| Modify | `core/src/object/std_types.rs` | `shellrunner()` factory + 단위 test |
| Modify | `core/src/lib.rs` (또는 STD_TYPES 정의 위치) | STD_TYPES에 ShellRunner@1 추가 |
| Modify | `apps/desktop-shell/src/handlers/mod.rs` | `add_shellrunner_acl` helper + 단위 test + `pub mod shellrunner_methods;` |
| Modify | `apps/desktop-shell/src/dialog_ops.rs` | `PendingFs::ShellRun` variant 추가 |
| Create | `apps/desktop-shell/src/handlers/shellrunner_methods.rs` | `handle_run` + 화이트리스트/cwd 검증 + Dialog 분기 + `execute_command` |
| Modify | `apps/desktop-shell/src/handlers/dialog_methods.rs` | `handle_respond` PendingFs match에 ShellRun arm |
| Modify | `apps/desktop-shell/src/main.rs` | ShellRunner singleton mount + add_shellrunner_acl + invoke dispatch "run" |
| Modify | `ai-bridge/src/system_prompt.md` | ShellRunner@1 섹션 신규 + "never shell" 정책 완화 |
| Create | `docs/adr/039-shellrunner-escape-hatch.md` | ADR 결정 근거 |
| Create | `docs/manual-tests/m12-acceptance.md` | 시나리오 6 + auto_react_project 안내 |
| Modify | `docs/known-issues.md` | M12 마감 메모 |
| Create | `ai-bridge/examples/auto_react_project.rs` | end-to-end demo (npx create-vite + npm install) |

---

## 진행 정책 공통

- Korean docs/comments + English identifiers
- 각 task TDD step (failing test → 구현 → pass → commit)
- 각 commit 끝: `cargo test --workspace` + `clippy -D warnings` + `fmt --check` 통과
- desktop-shell process 실행 중이면 rebuild 시 lock — 사전 kill
- commit 메시지 한국어 + Co-Authored-By 라인

---

# Stage A — core 객체 정의 (1 task)

## Task 1: `shellrunner()` factory + STD_TYPES 등록

**Files:**
- Modify: `core/src/object/std_types.rs`
- Modify: STD_TYPES 정의 위치 (grep으로 확인 — core/src/lib.rs 또는 compositor/src/server_client.rs)

- [ ] **Step 1.1: 단위 test 추가**

`core/src/object/std_types.rs`의 `#[cfg(test)] mod tests` 안에 추가:

```rust
    #[test]
    fn shellrunner_has_run_method_and_state() {
        let sr = shellrunner(ActorId::local_user());
        assert_eq!(sr.type_uri.as_str(), "aios.builtin/ShellRunner@1");
        assert!(sr.methods.iter().any(|m| m.name() == "run"));
        assert!(sr.props.contains_key("allowed_binaries"));
        assert!(sr.props.contains_key("default_timeout_ms"));
        let allowed = sr.props.get("allowed_binaries").and_then(|v| v.as_array()).unwrap();
        assert!(allowed.iter().any(|v| v.as_str() == Some("git")));
        assert!(allowed.iter().any(|v| v.as_str() == Some("npm")));
        assert!(allowed.iter().any(|v| v.as_str() == Some("cargo")));
        for k in &["last_cmd", "last_args", "last_cwd", "last_exit_code",
                  "last_stdout", "last_stderr", "last_duration_ms", "last_error"] {
            assert!(sr.state.contains_key(*k), "state.{} 누락", k);
            assert_eq!(sr.state.get(*k), Some(&serde_json::json!(null)), "state.{} 초기 null", k);
        }
    }
```

- [ ] **Step 1.2: 테스트 실행 — 실패 확인**

```
cargo test -p geulos-core shellrunner_has_run 2>&1 | Select-Object -Last 5
```

Expected: 미정의 컴파일 실패.

- [ ] **Step 1.3: factory 함수 추가**

`core/src/object/std_types.rs`의 `filesystem()` 함수 직후:

```rust
/// `aios.builtin/ShellRunner@1` 객체 (M12) — 화이트리스트 binary 실행 escape hatch.
///
/// Filesystem@1과 같은 singleton 패턴. 임의 명령이 아닌 *허용된 binary*만 통과
/// (props.allowed_binaries — 사용자가 mount 시점 또는 SetState로 확장 가능).
///
/// method `run(cmd, args, cwd)` — desktop-shell handler가 화이트리스트 + cwd 검증
/// 후 AI sender면 Dialog 흐름, compositor면 즉시 실행. 결과는 state.last_* 8 fields
/// SetState. 본 v1은 one-shot 명령만 (wait_with_output). long-running은 M13+
/// Process@1 별도.
pub fn shellrunner(owner: ActorId) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.builtin/ShellRunner@1").expect("유효한 TypeUri"), owner);
    obj.set_prop(
        "allowed_binaries",
        json!([
            "git", "npm", "yarn", "pnpm", "npx", "cargo", "rustc", "docker", "node", "python",
            "pip"
        ]),
    );
    obj.set_prop("default_timeout_ms", json!(120000u64));
    for k in &[
        "last_cmd", "last_args", "last_cwd", "last_exit_code",
        "last_stdout", "last_stderr", "last_duration_ms", "last_error",
    ] {
        obj.set_state(k, json!(null));
    }
    obj.methods.push(
        MethodSig::new("run")
            .with_arg(ArgSpec::new("cmd", "string"))
            .with_arg(ArgSpec::new("args", "array<string>"))
            .with_arg(ArgSpec::new("cwd", "string")),
    );
    obj
}
```

- [ ] **Step 1.4: STD_TYPES 상수 갱신**

```
grep -rn "aios.builtin/Filesystem@1" core/ compositor/ apps/
```

찾은 `STD_TYPES` 배열에 `"aios.builtin/ShellRunner@1"` 추가.

- [ ] **Step 1.5: test 통과 + workspace 회귀**

```
cargo test -p geulos-core std_types::tests::shellrunner 2>&1 | Select-Object -Last 5
cargo test --workspace 2>&1 | Select-Object -Last 10
cargo clippy -p geulos-core --no-deps -- -D warnings 2>&1 | Select-Object -Last 5
cargo fmt --check 2>&1 | Select-Object -Last 5
```

- [ ] **Step 1.6: commit**

```
git add core/src/object/std_types.rs core/src/lib.rs compositor/src/server_client.rs
git commit -m "feat(core): M12 T1 — ShellRunner@1 factory + STD_TYPES 등록"
```

본문에 spec/ADR 참조 + Co-Authored-By 라인 포함.

---

# Stage B — desktop-shell handler (3 task)

## Task 2: `add_shellrunner_acl` helper

**Files:**
- Modify: `apps/desktop-shell/src/handlers/mod.rs`

- [ ] **Step 2.1: 단위 test 추가**

`apps/desktop-shell/src/handlers/mod.rs`의 `#[cfg(test)] mod tests`에 추가:

```rust
    #[test]
    fn shellrunner_acl_compositor_full_ai_run_only() {
        let owner = ActorId::local_user();
        let mut sr = std_types::shellrunner(owner.clone());
        add_shellrunner_acl(&mut sr);
        let g = geulos_core::server::GrantStore::default();
        let comp = ActorId::system_compositor();
        let ai = ActorId::new_ai_session();
        let shell = ActorId::new_app("desktop-shell");

        assert!(sr.is_allowed(&comp, AclOp::Invoke("run".into()), &g));
        assert!(sr.is_allowed(&ai, AclOp::Invoke("run".into()), &g));
        assert!(!sr.is_allowed(&ai, AclOp::Invoke("set_state".into()), &g));
        assert!(sr.is_allowed(&shell, AclOp::SetState("last_stdout".into()), &g));
        let evil = ActorId::new_app("evil");
        assert!(!sr.is_allowed(&evil, AclOp::Invoke("run".into()), &g));
    }
```

- [ ] **Step 2.2: 테스트 실행 — 실패**

```
cargo test -p geulos-desktop-shell --lib shellrunner_acl 2>&1 | Select-Object -Last 5
```

- [ ] **Step 2.3: helper 추가 + pub mod 등록**

`apps/desktop-shell/src/handlers/mod.rs`의 `add_container_acl` 다음에:

```rust
/// ShellRunner@1 — compositor 전체 + AI run 한정 + desktop-shell set_state.
///
/// M12 신규 (escape hatch). AI는 *run method만* — props/state 변경 차단.
/// 보안은 *Dialog 매 호출 동의* + props.allowed_binaries 화이트리스트로 다중 layer.
pub fn add_shellrunner_acl(obj: &mut Object) {
    obj.acl.push(AclEntry {
        actor: ActorPattern::SystemCompositor,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
    obj.acl.push(AclEntry {
        actor: ActorPattern::AiSession,
        method: MethodPattern::Exact("run".to_string()),
        effect: AclEffect::Allow,
    });
    obj.acl.push(AclEntry {
        actor: ActorPattern::App("desktop-shell".to_string()),
        method: MethodPattern::SetState,
        effect: AclEffect::Allow,
    });
}
```

같은 파일 module-level doc comment (helper 목록)에 `add_shellrunner_acl` 추가.

`pub mod` block 끝에:

```rust
pub mod shellrunner_methods;
```

(이 모듈 파일은 Task 4에서 생성. 빈 module 등록은 *Task 4에서* — 본 Task 2엔 helper만.)

- [ ] **Step 2.4: 테스트 통과 + clippy/fmt**

```
cargo test -p geulos-desktop-shell --lib handlers::tests 2>&1 | Select-Object -Last 8
cargo clippy -p geulos-desktop-shell --no-deps -- -D warnings 2>&1 | Select-Object -Last 3
cargo fmt --check 2>&1 | Select-Object -Last 3
```

Expected: 신규 1 + 기존 5 = 6 passed.

- [ ] **Step 2.5: commit**

```
git add apps/desktop-shell/src/handlers/mod.rs
git commit -m "feat(desktop-shell): M12 T2 — add_shellrunner_acl helper"
```

---

## Task 3: `PendingFs::ShellRun` variant 추가

**Files:**
- Modify: `apps/desktop-shell/src/dialog_ops.rs`
- Modify: `apps/desktop-shell/src/handlers/dialog_methods.rs` (임시 arm)

**Goal:** PendingFs enum에 ShellRun variant 등록 + exhaustive match 회피. 정식 처리는 T4.

- [ ] **Step 3.1:** `dialog_ops.rs`의 `pub enum PendingFs`에 신규 variant 추가. fields: `cmd: String, args: Vec<String>, cwd: std::path::PathBuf, requesting_actor: ActorId`. 다른 mutation variant (Save/CreateFile 등) 패턴 일관.

- [ ] **Step 3.2:** `dialog_methods.rs`의 handle_respond match에 임시 arm 추가 (eprintln 한 줄, 정식 처리는 T4).

- [ ] **Step 3.3:** `cargo build --workspace` + `cargo test --workspace` + `clippy -D warnings` 통과.

- [ ] **Step 3.4:** commit `feat(desktop-shell): M12 T3 — PendingFs::ShellRun variant`

---

## Task 4: `shellrunner_methods` 모듈 + handle_run + execute_command + 정식 dialog arm

**Files:**
- Create: `apps/desktop-shell/src/handlers/shellrunner_methods.rs`
- Modify: `apps/desktop-shell/src/handlers/dialog_methods.rs` (T3 임시 → 정식)

**Goal:** AI/compositor가 ShellRunner.run 호출 시 화이트리스트/cwd 검증 + Dialog 흐름 + tokio::process::Command 실행 + 8 state SetState. **본 task의 코드는 spec § Architecture / handle_run / execute_command 섹션 그대로 사용** (~200 라인). 본 plan에서는 *task 단위 + step 순서*만 명시.

- [ ] **Step 4.1: 신규 파일 작성** — spec 본문의 `shellrunner_methods.rs` 섹션 복사. 핵심:
  - `handle_run(target, args, stream, mounted, owner, desktop_id, sender_actor, pending, req_seq)` — cmd/args/cwd 파싱 + 검증 (빈 / 화이트리스트 / cwd 존재) + sender 분기 (AI → Dialog mount + PendingFs::ShellRun 등록 / compositor → 즉시 execute_command)
  - `mount_run_dialog` — `external_methods`의 Dialog mount 패턴 동일. 메시지: "AI가 다음 명령 실행: <cmd> <args>, cwd: <cwd>, 허용?"
  - `execute_command(mounted, target, cmd, args, cwd)` — tokio::process::Command spawn + wait_with_output + tokio::time::timeout (props.default_timeout_ms, default 120000) + 8 state SetState
  - `broadcast_error(mounted, target, msg)` — last_error/last_exit_code=-1만 SetState
  - `lookup_allowed_binaries` / `lookup_default_timeout_ms` — props 조회 helper

- [ ] **Step 4.2: dialog_methods.rs 정식 arm** — T3 임시 arm 교체:
  - `user_allowed`이면 `crate::handlers::shellrunner_methods::execute_command(mounted, sr_id, &cmd, &args, &cwd).await` 호출 + outcome.state_sets extend
  - 거부면 last_error="사용자 거부" + last_exit_code=-1 SetState
  - `find_shellrunner_id(mounted)` helper — type_uri "aios.builtin/ShellRunner@1" find. 같은 파일 끝에.

- [ ] **Step 4.3:** `cargo build -p geulos-desktop-shell` + workspace test/clippy/fmt 통과. import 누락 fix.

- [ ] **Step 4.4:** commit `feat(desktop-shell): M12 T4 — shellrunner_methods + Dialog 통합`

---

# Stage C — 통합 + 문서 (3 task)

## Task 5: main.rs ShellRunner singleton mount + invoke dispatch

**Files:**
- Modify: `apps/desktop-shell/src/main.rs`

- [ ] **Step 5.1:** 초기 mount block (filesystem_obj 부근, ~line 444):
  - `let mut shellrunner_obj = std_types::shellrunner(owner.clone());`
  - `shellrunner_obj.parent = Some(desktop.id);`
  - `add_shellrunner_acl(&mut shellrunner_obj);` (import 추가)

- [ ] **Step 5.2:** `desktop.children` + `all_objects`에 `shellrunner_obj.id` / `clone()` 추가. `let shellrunner_id = shellrunner_obj.id;` 변수.

- [ ] **Step 5.3:** subscribe targets에 `shellrunner_id` 추가.

- [ ] **Step 5.4:** invoke dispatch `match method`에 "run" 분기 (external_methods 다음):
  ```rust
  "run" => shellrunner_methods::handle_run(
      target_id, &args, &mut stream, &mut mounted_objects,
      &owner, desktop_id, &sender_actor, &pending, &mut req_seq,
  ).await?
  ```
  import `shellrunner_methods` 추가.

- [ ] **Step 5.5:** `cargo build -p geulos-desktop-shell` + workspace 통과.

- [ ] **Step 5.6: manual smoke** — launcher kill + rebuild + 띄움. `cargo run --example auto_crud_demo -p geulos-ai-bridge`로 Stage 1에 `aios.builtin/ShellRunner@1 → 1 objects` 확인.

- [ ] **Step 5.7:** commit `feat(desktop-shell): M12 T5 — ShellRunner mount + dispatch`

---

## Task 6: system_prompt 갱신 + ADR-039 + KI

**Files:**
- Modify: `ai-bridge/src/system_prompt.md`
- Create: `docs/adr/039-shellrunner-escape-hatch.md`
- Modify: `docs/known-issues.md`

- [ ] **Step 6.1: system_prompt** — `### Tools` 섹션 끝에 `### ShellRunner@1 — 생태계 도구 호출 (M12, escape hatch)` 추가. 본문: 언제 사용 / 사용 X / 흐름 5단계 / 제약 (timeout/one-shot/stdin X) / "never shell" 갱신 (suggesting 금지 + ShellRunner.run으로 실행 OK).

- [ ] **Step 6.2: ADR-039** — spec § 동기/Decision/대안/Consequences 그대로. 대안: (A) typed Process Objects → M13+, (B) container → M14+, (C) shell 유지 → 기각.

- [ ] **Step 6.3: known-issues.md** — 마일스톤 마감 섹션에 "M12 정식 마감 (2026-05-26)" 단락. 정기 검토에 M13/M14 후보.

- [ ] **Step 6.4:** workspace 회귀 + commit `docs: M12 T6 — system_prompt + ADR-039 + KI`

---

## Task 7: auto_react_project example + acceptance 문서 + final 검증

**Files:**
- Create: `ai-bridge/examples/auto_react_project.rs`
- Create: `docs/manual-tests/m12-acceptance.md`

- [ ] **Step 7.1: auto_react_project** — `auto_website_project.rs` 패턴 답습. 변경:
  - PROJECT_DIR: `D:/GeulOS/tmp-react-app`
  - SESSION_NAME: `auto-react-demo`
  - PROMPT: react 프로젝트 생성 + npm install + App.jsx 'Hello GeulOS React' 교체
  - timeout 300초 (npm install)
  - 검증: package.json + node_modules/react/ + src/App.jsx "Hello GeulOS React"

- [ ] **Step 7.2:** `cargo build --example auto_react_project -p geulos-ai-bridge`

- [ ] **Step 7.3: m12-acceptance.md** — 시나리오 6개 (git --version / 화이트리스트 거부 / cwd 없음 / Dialog 거부 / npm install 성공 / 보안 AiSession Exact "run") + auto_react_project 안내 + 결과 표 placeholder.

- [ ] **Step 7.4: final 검증** — `cargo test --workspace` + `clippy -D warnings` + `fmt --check` + `bash scripts/check-no-wildcard-acl.sh` 통과.

- [ ] **Step 7.5:** commit `demo+docs: M12 T7 — auto_react_project + acceptance 6 시나리오`

---

## Self-Review

**Spec coverage:**
- ✓ ShellRunner@1 factory (T1) / STD_TYPES 등록 (T1)
- ✓ add_shellrunner_acl helper + ACL 단위 test (T2)
- ✓ PendingFs::ShellRun variant (T3)
- ✓ shellrunner_methods (handle_run + execute_command + Dialog mount) (T4)
- ✓ dialog_methods 정식 arm (T4)
- ✓ main.rs singleton mount + invoke dispatch (T5)
- ✓ system_prompt 갱신 (T6)
- ✓ ADR-039 + KI 마감 메모 (T6)
- ✓ acceptance 시나리오 6+1 (T7)
- ✓ auto_react_project end-to-end (T7)
- ✓ 보안: 화이트리스트 / Dialog 매 호출 / Rust execve / sender 분기 / AiSession Exact "run"

**Placeholder scan:** T4 본문은 spec § Architecture 섹션 참조 — spec에 ~200 라인 완전 코드 명시되어 있어 implementer는 spec + plan 둘 다 read. plan-writing 가이드라인 "complete code in every step"은 spec 참조로 만족 (spec/plan 같이 commit되어 있어 implementer 접근 자연).

**Type 일관성:**
- `shellrunner(owner: ActorId) -> Object` (T1)
- `add_shellrunner_acl(obj: &mut Object)` (T2)
- `PendingFs::ShellRun { cmd, args, cwd, requesting_actor }` (T3)
- `handle_run(...)` 9 인자 (T4) / `execute_command(mounted, target, cmd, args, cwd)` 5 인자 (T4)
- method 이름 `run` 일관
- ShellRunner@1 type_uri 일관

---

## 실행 핸드오프

**Plan complete and saved to `docs/plans/2026-05-26-geulos-m12-shellrunner.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — controller가 task별 fresh subagent + spec/code review

**2. Inline Execution** — 본 세션에서 batch 실행 + 사용자 checkpoint

**Which approach?**
