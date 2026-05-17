# ADR-022 — AI 작업 시각화 v1: 노란 점 + 5초 페이드

**Status:** Accepted (2026-05-18)

## Context
"AI가 일하는 모습이 실시간으로 보이는 파일 시스템" — 사용자 비전의 핵심. 시각 언어 선택지가
많음(글로우/배지/색상/세션 그룹 강조). 1차에는 *가장 단순한 것*, 향후 사용자 설정으로.

## Decision
v1: 각 File/Folder의 `state.last_change_actor` (`ai|user|system`) + `state.last_change_ms`
(Unix ms). 컴포지터 렌더가 매 프레임 `now - last_change_ms`를 계산:
- `< 5000ms && actor == "ai"`: 파일명 우측 8px에 8×8 노란 사각 점 ●
- 그 외: 점 없음
페이드는 시간 비교만으로 자동 (별도 타이머·이벤트 없음). 5초 후 다음 redraw에서 사라짐.

## Consequences
- 구현 비용 매우 낮음 (state 2개 + 렌더 분기 1개)
- 사용자가 자리 비웠을 때 변경 누락 인지 가능 → M8+에서 *누적 카운터* 추가 가능
- 향후 사용자 설정으로 시각 언어 교체 가능 (글로우/색상/배지 등) — ADR 따로 갱신
