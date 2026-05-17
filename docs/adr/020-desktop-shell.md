# ADR-020 — 데스크톱 셸 아키텍처

**Status:** Accepted (2026-05-18)

## Context
M7에서 *바탕화면*이 필요. 옵션 검토:
- 컴포지터에 내장 → 컴포지터가 너무 큼, 셸 교체 불가
- builtin 라이브러리 → 단일 라이터 이벤트 루프 위배
- **별 프로세스 (desktop-shell)** → 컴포지터·서버와 동등

## Decision
desktop-shell은 별 프로세스. 부팅 시 server-host 다음으로 시작. Desktop/FileTree/Canvas는
`aios.builtin/*` 네임스페이스 — `aios.std/*`(앱이 자유로이 게시)와 구분.

## Consequences
- 단일 라이터 보존 (ADR-003)
- 셸 교체 가능 (다른 desktop-shell 구현 가능)
- 부팅 시 server-host → desktop-shell → compositor 순서 의존성 (M7.5에서 geulosd가 자동 supervise)
