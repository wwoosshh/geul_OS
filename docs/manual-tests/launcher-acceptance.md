# GeulOS Launcher Acceptance

**Binary:** `geulos-launcher` (executable name `geulos`)

## 사전 조건
- `cargo build --workspace` 통과
- `target/debug/geulos.exe`, `geulosd.exe` (server-host), `geulos-desktop-shell.exe`, `geulos-compositor.exe` 모두 존재
  - **NOTE**: server-host crate의 binary 이름은 `geulosd`이지 `geulos-server-host`가 아님

## 시나리오 P — 통합 spawn
1. `cargo run -p geulos-launcher`
2. 콘솔에 `[geulos] server-host spawn` → `[geulos] server-host ready` → `[geulos] desktop-shell spawn` → `[geulos] desktop-shell ready` → `[geulos] compositor spawn (GUI)` 순으로 보임
3. compositor GUI 창 등장 — M9/M10 기능 모두 동작 확인 (FileTree, Window, AI Dialog, file watcher 등)
4. `~/.geulos/logs/{server,shell,compositor}.log` 세 파일 생성 + 각 자식의 stdout/stderr 누적

## 시나리오 Q — Cleanup
5. compositor 창 [x] 닫기 → 콘솔에 `[geulos] compositor 종료` + `[geulos] desktop-shell cleanup` + `[geulos] server-host cleanup` 순서로 보임
6. `ps`/`Get-Process`로 geulos-* 프로세스 모두 사라졌는지 확인
7. (다시 실행 후) Ctrl+C로 종료 → 동일 cleanup 흐름

## 통과 조건
- P/Q 모두 정확
- 자식 process가 leak 없이 정리
- 사용자가 *단일 명령*으로 GeulOS 시작/종료 가능

## 알려진 한계
- locate_bin이 launcher와 같은 dir의 자식 binary만 찾음 — 다른 PATH 위치 미지원
- ready 폴링은 timeout 후 강제 종료 — 실제 부팅 실패는 로그 확인 필요
- Linux/macOS는 빌드만 가능 (시각 미검증)
- 단일 프로세스 통합 (in-process mpsc)은 v2
- Windows에서 `Ctrl+C`는 console control event로 전달되며 자식 process도 같은 console group을 공유하므로 자식이 *먼저* signal을 받고 즉시 종료될 수 있음 — launcher의 cleanup 흐름은 정상적으로 진행되나 `kill()`이 이미 죽은 process에 대해 호출됨 (무해)
- log file write race: stdout/stderr를 line 단위로 동일 file handle에 append — `tokio::select!`로 직렬화되므로 line 섞임 없음 (단, append flag로 OS-level interleave는 가능하나 단일 task이므로 무문제)

## 후속
- Phase B: MSI/exe installer (wix)
- Phase C: 단일 프로세스 통합 (큰 리팩토링)
- 자동 업데이트
