> **Status:** adopted (2026-05-31)
> **Note:** M12 정식 채택 (2026-05-26 마감), ADR-039. ShellRunner@1 + 화이트리스트 binary + 120s timeout + Dialog 흐름 정착. M13에서 host bridge로 routing 확장.

# M12 — ShellRunner@1 (생태계 도구 escape hatch)

**Date:** 2026-05-26
**Status:** Draft (사용자 review 대기)
**Parent:** M11.2 (mutation 흐름 안정화) 후속

## 동기

M11.2까지 AI는 *객체 모델 안*의 동작 (Folder/File CRUD)만 가능. 그러나 *생태계 표준 도구* (git/npm/cargo/docker/npx 등) 사용은 *원천 차단* (system_prompt "never shell" 정책). 결과:

- AI가 "react 프로젝트 만들어줘" 요청 받으면 *코드 작성*은 하나 *npm install* / *vite build* 불가
- 사용자가 *수동으로 명령 실행*해야 → "AI가 사용자 PC에서 작업" 비전 약화
- 2026년 개발 ecosystem 도구 99% 사용 불가

M12는 *escape hatch 패턴* (Filesystem@1과 동일 디자인)으로 *임의 binary 실행*을 도입. 추후 M13+에서 *typed Process Objects* (GitRepo@1 / NpmProject@1 등)로 *spec 정신 복원*.

**보안 노트**: Rust `tokio::process::Command`는 *fork+execve* 직접 사용 (shell 거치지 않음). Node.js child_process의 shell injection 위험과 *무관*. binary와 args가 *별 인자*라 quote/escape 불필요.

## 비-목표

- typed Process Objects (GitRepo@1 등) — *M13+*
- container 격리 환경 — *M14+*
- Long-running process / log stream — v2 (Process@1 객체 별도)
- stdin 입력 (interactive REPL) — 미지원
- 명령 chain / pipe — AI가 *여러 invoke로 표현*

## 범위

**핵심 1 객체 + 1 method + Dialog 흐름**:

- 신규 객체: `aios.builtin/ShellRunner@1` (singleton, Filesystem@1 형제)
- 신규 method: `run(cmd, args, cwd)` — Rust Command::new(cmd).args(args).current_dir(cwd)에 해당
- 결과: SetState로 last_cmd / last_args / last_cwd / last_exit_code / last_stdout / last_stderr / last_duration_ms broadcast
- Dialog: 매 호출 동의
- 화이트리스트: props.allowed_binaries (기본 git/npm/yarn/pnpm/npx/cargo/rustc/docker/node/python/pip)
- Timeout: 기본 120초 (긴 명령 `npm install` 등 고려). 초과 시 kill + last_exit_code = -1

## Architecture

### 객체 정의 (core/src/object/std_types.rs)

shellrunner(owner) factory function. props에 allowed_binaries Vec<String>과 default_timeout_ms u64. state는 last_* 8 fields 모두 null 초기화. methods는 run 하나.

### desktop-shell mount

main.rs 초기 mount에 ShellRunner singleton 추가 (Filesystem과 같은 위치, Desktop의 child). add_shellrunner_acl 호출.

### ACL helper (handlers/mod.rs 신설)

ShellRunner 정책:
- SystemCompositor / Wildcard / Allow — 사용자 직접 호출
- AiSession / Exact("run") / Allow — AI는 run만 (다른 method 없음)
- App("desktop-shell") / SetState / Allow

### run invoke handler (handlers/shellrunner_methods.rs 신설)

흐름:
1. args 파싱: cmd / args / cwd
2. cmd 검증: 빈 또는 화이트리스트 외 → last_error SetState + 종료
3. cwd 검증: 절대 path + 존재 확인
4. sender_actor가 ai: 접두사이면 Dialog mount + PendingShellRun에 보관
5. 그 외 (system:compositor) → 즉시 execute_and_broadcast

### Dialog respond 분기 확장 (dialog_methods.rs)

handle_respond의 PendingFs match에 PendingShellRun variant 추가:
- 허용 → execute_and_broadcast 호출
- 거부 → last_error="사용자 거부" SetState

### execute_and_broadcast 함수

Rust 코드 흐름:
- Instant::now() started
- tokio::process::Command::new(&cmd).args(&args).current_dir(&cwd)
- .stdout/.stderr piped
- .spawn() then wait_with_output() wrapped in tokio::time::timeout(default_timeout)
- 결과: exit_code i32, stdout String, stderr String, duration_ms u64
- 7건 SetState broadcast (cmd/args/cwd/exit_code/stdout/stderr/duration_ms)

### Wire 흐름

AI -> server invoke(ShellRunner.run({cmd, args, cwd}))
  -> ACL pass (AiSession Exact "run" Allow)
  -> desktop-shell handle_run
  -> 화이트리스트 검증 + cwd 검증
  -> sender=AI이므로 Dialog mount + PendingShellRun 등록
  -> 사용자 응답 대기
  -> compositor Dialog.respond("action":"허용")
  -> handle_respond PendingShellRun.take + execute_and_broadcast
  -> tokio::process::Command 실행 (수십초)
  -> SetState 7건
  -> AI가 subscribe/get_object로 결과 확인

## 결정 사항 (default + 근거)

| 결정 | 기본값 | 근거 |
|---|---|---|
| 화이트리스트 binary | git/npm/yarn/pnpm/npx/cargo/rustc/docker/node/python/pip | 2026 일반 개발 ecosystem. rm/format/cp/mv는 파일 동작이라 객체 모델 사용. shell escape 의도 분리. |
| Dialog 정책 | 매 호출 동의 (grant 캐시 X v1) | shell 명령은 위험. grant 캐시는 v2 검토 (cmd 패턴별 한 번 동의 후 후속 통과 등) |
| Timeout | 120초 | npm install 평균 30-60초 + 여유. 초과 시 kill + last_exit_code=-1 |
| stdout/stderr 크기 | 무제한 (v1) | 큰 출력은 wire frame 큼. v2에 cap (예: 1MB) 도입 |
| env vars | process 상속 | 격리는 M14+ container. v1 단순 |
| cwd 검증 | 절대 path + 존재 확인. granted_dirs 검사 X | shell은 명령 자체가 위험. Dialog 동의가 안전망. cwd 어디든 명령 텍스트로 사용자 판단 |
| 화이트리스트 외 binary | 즉시 거부 + last_error SetState | 사용자가 props.allowed_binaries 통해 확장 가능 |
| 권한 모델 | ACL = AiSession Exact "run" Allow + Dialog | M11 패턴 일관 |
| stdin | 미지원 (v1) | interactive 안 됨. AI는 non-interactive 명령만 |
| pipe / chain | AI가 여러 invoke로 표현 | bash -c "cmd1 && cmd2" 같은 wrap도 가능하나 권장 X |

## ai-bridge system_prompt 갱신

기존 "never shell" 강한 정책을 조건부 허용으로 변경. ShellRunner@1 섹션 신규 추가:

aios.builtin/ShellRunner@1 — 제한된 binary 실행. props.allowed_binaries (기본 git/npm/cargo/docker/...)만 통과. method: run(cmd, args, cwd).

반드시 객체 모델 우선 시도 — 파일 작업은 Folder/File method, 코드 작성은 File.save. ShellRunner.run은 생태계 도구 (의존성 설치 / git / 빌드) 한정.

매 호출 사용자 Dialog 동의. 결과는 state.last_exit_code/stdout/stderr. exit_code=0이면 성공, 그 외는 stderr 확인 후 재시도.

## 회귀 안전성

- M11.2 add_fs_object_acl과 무관 (별 helper)
- 기존 PendingFs enum에 PendingShellRun variant 추가 — dialog_methods.rs handle_respond match 깨지지 않게 exhaustive 갱신
- Filesystem@1 자체는 변경 없음

## 검증 시나리오 (M12 acceptance)

1. AI가 ShellRunner.run("git", ["--version"], "D:/") → Dialog → 허용 → state.last_stdout = "git version 2.x.x"
2. AI가 화이트리스트 외 ShellRunner.run("rm", ["-rf", "/"], ...) → last_error="화이트리스트 외" + 실행 0
3. AI가 ShellRunner.run("npm", ["install", "react"], "D:/proj") → Dialog → 허용 → 60초 후 react 설치 + node_modules 폴더 생성 검증
4. compositor 직접 호출 (system:compositor) → Dialog 없이 즉시 실행
5. timeout 시나리오 — 별도 sleep 명령
6. react 프로젝트 end-to-end: npx create-vite my-app --template react → 자동 허용 → 디렉터리 생성 + 의존성 설치

## M12 범위 외 (M13+ 후보)

- Long-running process (vite dev server) — Process@1 객체 별도 (v2)
- typed Process Objects (GitRepo@1 등)
- container 격리
- stdin/interactive
- 명령 chain
- 결과 streaming (현재는 wait_with_output 완료 시 한 번에 SetState)

## 측정 통과 기준

- `cargo test --workspace` 통과
- `clippy -D warnings` / `fmt --check` 클린
- M11 wildcard ACL guard 통과
- M12 acceptance 6 시나리오 — auto_react_project example로 자동 검증
