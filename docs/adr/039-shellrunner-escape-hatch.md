# ADR-039 — ShellRunner@1 (생태계 도구 escape hatch)

- **상태:** Accepted
- **결정일:** 2026-05-26
- **부모 spec:** `docs/specs/2026-05-26-geulos-m12-shellrunner.md`
- **부모 plan:** `docs/plans/2026-05-26-geulos-m12-shellrunner.md`

## Context

M11.2까지 GeulOS는 "객체 method만, never shell" 정책으로 *생태계 도구* (git/npm/
cargo/docker) 사용 차단. 결과 AI가 react/vite 프로젝트 setup, 의존성 설치, 빌드,
배포 등 *2026 개발 ecosystem 99%*를 시도조차 못 함.

## Decision

Filesystem@1 escape hatch와 같은 패턴으로 *제한된 binary 실행*을 객체-네이티브
인터페이스에 통합:

1. `aios.builtin/ShellRunner@1` singleton — props.allowed_binaries (기본 11개:
   git/npm/yarn/pnpm/npx/cargo/rustc/docker/node/python/pip)
2. 단일 method `run(cmd, args, cwd)` — Rust tokio::process::Command (fork+execve)
   직접 호출. Node.js shell injection 위험과 무관.
3. AI sender면 Dialog 매 호출 동의. compositor sender면 즉시 실행.
4. 결과는 state.last_* 8 fields SetState — AI/사용자가 객체 tree로 결과 확인.

## 대안

- (A) Typed Process Objects (GitRepo@1 / NpmProject@1) 먼저: spec 정신 우선, 단
  도구별 wrap 큼. *M13+로 이월* — escape hatch가 작동 시 typed로 점진 회복.
- (B) Container 격리 환경 (Docker / M5.5 VM): production-grade 안전. 단 launcher
  구조 큰 변경. *M14+ 이월*.
- (C) shell 정책 그대로 유지: AI가 생태계 도구 영구 봉쇄. 비전과 충돌. 기각.

## Consequences

**Positive:**
- AI가 react/vite 프로젝트 setup, 의존성 설치, 빌드 등 표준 흐름 수행 가능
- audit log (M11.1 JSONL)에 *명령 + exit_code + stdout/stderr* 기록 — 사후 진단
- 화이트리스트로 rm/format 같은 위험 명령 *원천 차단*
- Dialog 동의가 *모든 명령마다* — 우회 불가 (KI-001 차단 유지)

**Negative:**
- 객체 모델 정신 *부분 약화* — 명령 string이 audit 단위 (M13+ typed로 회복)
- stdout/stderr 무제한 (v1) — 거대 출력 시 wire frame 큼 (v2 1MB cap)
- env vars process 상속 — 격리 X (v2 container)
- AI가 sudo / 임의 binary 사용하려면 사용자가 props.allowed_binaries SetState 필요

**Neutral:**
- M11.2 add_fs_object_acl과 무관 (별 helper)
- Filesystem@1과 자연 형제 — Desktop child + 둘 다 escape hatch
- compositor 직접 호출 (사용자 CLI에서 `! npm install` 같은 prefix) v2 가능

## 측정

- M12 acceptance 6 시나리오 (docs/manual-tests/m12-acceptance.md — T7에서)
- auto_react_project example로 npx create-vite → 실제 디렉터리/의존성 검증 (T7)
