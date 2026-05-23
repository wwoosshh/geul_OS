# ADR-036 — 객체-네이티브 파일시스템 (Approach E 하이브리드)

- **상태:** Accepted
- **결정일:** 2026-05-23
- **부모 spec:** `docs/specs/2026-05-23-geulos-m10-object-native-filesystem.md`

## Context

M9까지 AI는 사용자가 열어둔 File만 invoke 가능. 큰 프로젝트나 작업 이어가기는 불가.
일반적 해결 (Claude Code/Cursor)은 path-based fs API 도구 — *AI가 보는 정보가 사용자 GUI와
다름*. 사용자가 매번 상태 설명/캡처 필요.

GeulOS는 *모든 게 객체화*되어 AI auto-context가 가능해야 한다는 차별성 (README §3·4).

## Decision

Approach E — 하이브리드:
1. cwd 안 = 객체-네이티브. Folder/File에 create/delete/rename 메서드. desktop-shell이
   notify-rs로 외부 변경 감지 → 객체 state 자동 갱신.
2. cwd 밖 = path-API escape hatch (`aios.builtin/Filesystem@1.read_external/write_external`).
   사용자 Dialog 매번.
3. 권한: cwd 안 디렉터리 단위 grant + 삭제 항상 confirm. cwd 밖 모든 작업 confirm.

## Alternatives 거부

- **C (path-API wrapper)** — Claude Code 모방. 객체 모델의 강점 0. AI auto-context X. 거부.
- **D (cwd 밖 일체 금지)** — 외부 import/조회 불가능. 실용성 낮음. 거부.

## Consequences

- desktop-shell 부담 증가: file watcher 인프라 + granted_dirs 정책 + 3 phase 구현
- AI tooling 비약적 개선 — subscribe만으로 cwd 상태 실시간 인지
- M9의 Dialog/permission 인프라 그대로 활용
- 새 의존성 1개 (notify-rs 7.x — Phase 2에 도입)

## Trade-offs

- 큰 cwd (node_modules 포함)는 mount 폭발 가능 — v2에서 .gitignore + lazy + LRU
- granted_dirs 세션 영속 X — 재실행 시 reset (v2)
- Bash/glob/grep 도구 보류 — v2/M11

## 참고

- ADR-035 (M9 권한 Dialog)
- README §3·4 (객체 OS 차별성)
- T8.7 (lazy_mount 패턴)
