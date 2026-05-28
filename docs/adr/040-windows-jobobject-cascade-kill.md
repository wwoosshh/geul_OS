# ADR-040 — Windows JobObject로 long-running process cascade kill

**Date:** 2026-05-28
**Status:** Accepted
**Parent:** ADR-039 (ShellRunner escape hatch)

## Context

M12 ShellRunner.run의 `tokio::Command::wait_with_output` 종료 시 child handle drop. tokio default `kill_on_drop=false` — Windows에서 `TerminateProcess`는 *부모만* kill → npm.cmd → node → esbuild 손주 process가 orphan화 → 사용자 시연에서 `Get-Process node`로 직접 정리하는 사태.

M13 ConsoleWindow의 terminate 요구: *모든 descendant* 동시 kill 보장.

## Decision

Windows JobObject + `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 플래그.

spawn 절차:
1. `CreateJobObjectW` + `SetInformationJobObject(JobObjectExtendedLimitInformation, KILL_ON_JOB_CLOSE)`
2. `tokio::Command::new(cmd).creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW)` spawn
3. `OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, pid)` → handle
4. `AssignProcessToJobObject(job, proc_handle)`
5. ToolHelp Snapshot으로 main thread 찾아 `ResumeThread`

terminate: `TerminateJobObject(job, 1)` — 모든 process exit code 1로 kill.
JobHandle drop 시 `CloseHandle` → KILL_ON_JOB_CLOSE 효력으로 cascade kill.

## Alternatives

| 대안 | 채택 안 한 이유 |
|---|---|
| PowerShell `taskkill /T /F` | 외부 명령 의존, 비동기 wait 까다로움, GeulOS 자체 시연 흐름 일관성 깨짐 |
| `psutil`-like crate (예: `sysinfo`) | 큰 의존성. process tree 탐색 후 개별 kill — race window 존재 (탐색 중 spawn된 손주 누락) |
| tokio `Command::kill_on_drop(true)` | 부모만 kill — orphan 문제 *그대로* |
| Unix fork만 활용 (Windows 후순위) | dev box가 Windows. 사용자 시연 즉시 차단됨 |

## Consequences

**좋음:**
- Win32 직접 호출 — 가장 신뢰성 + 의존성 최소화 (windows-sys만)
- KILL_ON_JOB_CLOSE 효력으로 *handle drop만으로도* 보장 (방어층 2개: 명시 TerminateJobObject + Drop)
- M12에서 본 *정확한* 문제 (orphan node/esbuild) 영구 해소

**비용:**
- `windows-sys` 새 의존성 (cfg windows target 한정 — Unix CI green 유지)
- Unix v1 미지원 → KI-027 등록. v2에서 nix crate의 setsid + Pid::from_raw(-pgid) + killpg(SIGTERM) → 3초 → killpg(SIGKILL)
- CREATE_SUSPENDED + ResumeThread 절차 추가 — assign 누락 race window 차단의 대가

**연결:**
- ADR-039 (ShellRunner escape hatch) — run_streamed가 본 ADR 패턴 적용
- KI-027 — Unix JobObject 동등 구현
