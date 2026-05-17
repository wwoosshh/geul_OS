# ADR-021 — 워크스페이스 단방향 동기화 (M7)

**Status:** Accepted (2026-05-18)

## Context
GeulOS 객체 ↔ 호스트 디스크 동기화 방향:
- 단방향(객체→디스크): 단순, GeulOS 안 변경만 디스크에 기록
- 양방향(+FS watcher): 강력, Windows 탐색기 변경도 GeulOS에 반영. 충돌 처리 필요.

## Decision
M7은 *단방향*. 워크스페이스 루트는 `%USERPROFILE%\GeulOS\workspace` (env `GEULOS_WORKSPACE`로
override). 외부 변경은 desktop-shell 재시작 또는 명시적 `FileTree.refresh()` 호출 시만 반영.
양방향은 M9+.

## Consequences
- M7 스코프 7주 유지
- 사용자가 Windows 탐색기로 워크스페이스를 *읽고 편집*하는 것은 가능하지만, 편집 결과가
  실시간으로 트리에 안 나타남 — refresh 또는 재시작 필요. 명시적 한계로 README/매뉴얼에 문서화.
