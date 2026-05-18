# ADR-026 — 멀티-윈도우 객체 모델 (M8)

**Status:** Accepted (2026-05-18)

## Context
M8에서 사용자가 파일을 클릭하면 *floating viewer 창*으로 띄우고, 여러 파일을 동시에 열어볼 수 있어야 한다 (사용자 발언: "파일을 선택하면 해당 파일을 창형태로 불러오는것"). 윈도우 모델 선택지:

- **컴포지터-local 윈도우** — 컴포지터가 내부 Vec<Window>로 관리. 서버 객체 트리에는 없음.
  *장점*: 구현 단순, invoke 라운드트립 없음.
  *단점*: AI가 `query type`으로 열린 윈도우를 못 봄. ADR-009(AI 기본 불신) + ADR-020(셸의 상호작용 요소는 1급 객체) 정신에 반함.
- **1급 객체 `aios.builtin/Window@1`** — Desktop의 자식으로 mount, server tree에 존재.
  *장점*: AI 가시성, query 가능, 사용자/AI가 같은 모델 공유.
  *단점*: focus·move·resize 시 컴포지터→desktop-shell invoke 필요.

## Decision
`aios.builtin/Window@1`을 1급 객체로 도입. Desktop의 자식으로 mount되며 server tree의 일부.

- **네임스페이스:** `aios.builtin/Window@1` — ADR-020의 셸 빌트인 정책 일관.
- **props:** `title: String`, `file_id: ObjectId` (단방향 참조).
- **state:** `x, y, w, h: i32`, `z: i32` (z-order, 큰 값이 위), `focused: bool`.
- **methods:** `move(x, y)` / `resize(w, h)` / `focus()` / `close()`.
- **Lifecycle:** `Explorer.open_file` invoke → desktop-shell이 Window mount. `close()` 또는 `[x]` 클릭 → `emit_destroyed` (KI-011 tombstone 메커니즘 재사용).
- **Z-order:** focus 시 현존 최대 z + 1 (단조 증가, 음수/오버플로 무시 — 솔로 dogfooding 범위에서 충분).
- **입력 라우팅:** 컴포지터가 마우스 좌표로 hit-test 후, drag move/resize는 *drag end 시점*에 한 번만 invoke (매 mouse move 마다 invoke X — latency·broadcast 비용 회피). 컴포지터 local state로 중간 위치를 보여줌.
- **AI 가시성:** `query type aios.builtin/Window@1`로 현재 열린 윈도우 일람 가능. AI가 "지금 열려있는 창을 닫아줘" 같은 명령을 수행할 근거.

## Consequences
- AI와 사용자가 *같은 윈도우 모델*을 본다 — ADR-009/ADR-020 일관.
- Window mutation은 desktop-shell이 단일 라이터 (ADR-003 보존).
- focus/move/resize invoke가 server tree 갱신 + StateSet broadcast를 트리거 — drag 중에는 컴포지터 local 표시로 회피하므로 commit 시점에만 발생.
- Window는 KI-011 tombstone 메커니즘으로 안전하게 close 가능.
- Window가 *키보드 input*을 받는 시점이 오면 focus 시스템 본격 도입 필요 (M8은 read-only라 사실상 무용 — 모델 일관성용).
- 같은 파일을 두 번 open할 때의 정책 (기존 윈도우 focus vs 새 윈도우)은 spec/plan에서 정의 (M8 결정: 기존 윈도우 focus + 최상위).

## 참고
- 관련 ADR: ADR-009 (AI 기본 불신), ADR-020 (데스크톱 셸), ADR-003 (단일 라이터), KI-011 (emit_destroyed 안전 close)
- 관련 spec: `docs/specs/2026-05-18-geulos-m8-multi-window-explorer.md` §4.1, §11
