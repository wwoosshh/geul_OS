# UI 디자인 시스템 (모던 미니멀 light) — spec

**Date:** 2026-05-28
**Status:** Draft (사용자 review 대기)
**Parent:** UI/UX 최적화 이니셔티브 1단계 (A. 디자인 시스템)

## 동기

현재 GeulOS의 시각 디자인은 *"실행 구실만"* 수준 — render.rs에 `COLOR_*` 상수가 흩어져 있고, border가 두껍고 진하며(`#999`/`#D8D8D8`), 여백이 빠듯하고, 표면 계층(panel/elevated/sunken) 구분이 약해 평면적이다. OS는 *사람이 어려운 일을 편하게 하는 작업 환경*이며, 작업이 돌아가는 것뿐 아니라 **환경이 쾌적**해야 한다.

본 spec은 UI/UX 최적화 이니셔티브의 1단계 — **통일된 design token + 모던 미니멀 light 룩 전면 적용**. 색 팔레트·spacing·radius·border를 token으로 체계화하고 모든 패널에 일괄 적용해 즉각적 "보기 좋음"과 후속 작업(성능/인터랙션/레이아웃)의 기반을 만든다.

**렌더 환경 제약:** compositor는 fontdue + softbuffer 기반 **CPU 픽셀 렌더**. 웹 mockup은 실제 렌더와 다르게 보이므로, token 정의 → 전면 적용 → **launcher 실물 검증 루프**가 유일한 정확한 검증 방법이다. spec의 hex 값은 *제안값*이며 실물에서 미세 튜닝한다.

## 비-목표

- **타이포 scale (크기 위계)** — `draw_text`가 단일 18px 고정 + Noto Sans KR Regular만 임베드(bold 없음). size scale 도입은 `draw_text`/`measure_text_width`/cursor 좌표/word-wrap(KI-017) 전면 리팩터 동반 → 별 spec (2단계 후보).
- **다크 테마** — token을 theme 추상으로 만들되 light 단일 구현. dark 전환은 후속.
- **아이콘 정비** — Lucide 아이콘 세트 확장/정렬은 별 spec.
- **애니메이션/전환** — 인터랙션 spec(C)에서.
- **렌더 성능** (glyph 캐싱/부분 재그리기) — 성능 spec(B)에서. 단 본 spec의 `fill_rect_rounded`는 corner 픽셀만 추가 연산이라 성능 영향 미미.

## Architecture

### theme 모듈 (`compositor/src/theme.rs` 신설)

모든 design token을 한 모듈에 모은다. render.rs/layout.rs가 `theme::` 경로로 참조. 흩어진 `COLOR_*` 상수를 theme로 이전 + 의미 기반 이름 부여.

token 구성:
- 색 팔레트 (표면/텍스트/accent/border/상태)
- spacing scale 상수
- radius 상수

token은 `pub const`로 노출 (런타임 theme 전환은 비목표 — 단일 light). 의미 이름(`SURFACE_PANEL`, `TEXT_SECONDARY` 등)으로 후속 dark 테마 시 값만 교체 가능한 구조.

### 색 팔레트 (제안값 — 실물 튜닝)

**표면 (명도 차로 깊이 — 그림자 대신):**

| token | hex | 용도 |
|---|---|---|
| `SURFACE_APP` | `#FAFAFA` | 최하단 desktop 배경 |
| `SURFACE_PANEL` | `#FFFFFF` | FileTree / Explorer 패널 |
| `SURFACE_ELEVATED` | `#FFFFFF` | Window / ConsoleWindow / Dialog (border로 분리) |
| `SURFACE_SUNKEN` | `#F4F4F5` | CLI 입력 영역 / 비활성 |

**텍스트:**

| token | hex | 용도 |
|---|---|---|
| `TEXT_PRIMARY` | `#18181B` | 본문·제목 |
| `TEXT_SECONDARY` | `#71717A` | 보조·메타 |
| `TEXT_TERTIARY` | `#A1A1AA` | placeholder·비활성 |
| `TEXT_ON_ACCENT` | `#FFFFFF` | accent 배경 위 텍스트 |

**accent (부드러운 파랑):**

| token | hex | 용도 |
|---|---|---|
| `ACCENT` | `#3B82F6` | 기본 강조 (버튼·titlebar·focus) |
| `ACCENT_HOVER` | `#2563EB` | hover/active |
| `ACCENT_SUBTLE` | `#EFF6FF` | selected row / 약한 강조 배경 |

**border / 구분선:**

| token | hex | 용도 |
|---|---|---|
| `BORDER` | `#E4E4E7` | 패널·창 외곽 (약하게) |
| `BORDER_STRONG` | `#D4D4D8` | 강조 구분 (필요 시) |

**상태 (기존 유지):**

| token | hex | 용도 |
|---|---|---|
| `STATUS_AI_DOT` | `#FFD500` | AI 강조 dot (현 COLOR_AI_DOT) |
| `STATUS_RUNNING` | `#4ADE80` | ConsoleWindow running |
| `STATUS_EXITED` | `#888888` | ConsoleWindow exited |
| `STATUS_TERMINATED` | `#EF4444` | ConsoleWindow terminated |
| `STATUS_ERROR` | `#F59E0B` | ConsoleWindow error |
| `CONSOLE_BG` | `#1E1E1E` | ConsoleWindow 본문 (단말 — 유지) |
| `CONSOLE_TEXT` | `#E0E0E0` | console stdout |
| `CONSOLE_STDERR` | `#FCA5A5` | console stderr |

ConsoleWindow 본문은 *단말 정체성* 유지(짙은 배경) — 미니멀 light와 의도적 대비.

### spacing scale (`theme.rs`)

4px 기반:

| token | px |
|---|---|
| `SPACE_XS` | 4 |
| `SPACE_SM` | 8 |
| `SPACE_MD` | 12 |
| `SPACE_LG` | 16 |
| `SPACE_XL` | 24 |

현재 하드코딩된 padding(주로 8)을 scale 참조로 교체. Explorer 행 padding·창 본문 inset을 `SPACE_MD`~`SPACE_LG`로 넉넉하게. 기존 `EXPLORER_ROW_H` 등 layout 상수는 spacing scale과 정합하도록 조정(필요 시).

### radius + `fill_rect_rounded` (`render.rs` 또는 `text.rs` 옆 draw util)

현재 `fill_rect`는 사각형 전용. 둥근 모서리용 신규 함수:

```rust
/// 둥근 모서리 사각형. corner 영역 픽셀을 radius 원으로 clip + anti-alias.
/// radius=0이면 fill_rect와 동일. blend_argb(기존)로 corner edge alpha blend.
pub fn fill_rect_rounded(buffer, stride, height, rect: &Rect, radius: i32, color: u32)
```

구현:
- 본체는 fill_rect와 동일하게 채우되, 4 corner 정사각형(radius×radius) 영역만 픽셀별 처리
- corner 중심에서 픽셀 거리 `d = sqrt((cx-px)^2 + (cy-py)^2)`
  - `d <= radius - 0.5` → 불투명
  - `radius - 0.5 < d <= radius + 0.5` → alpha = (radius + 0.5 - d) 비례로 blend (AA)
  - `d > radius + 0.5` → skip (배경 유지)

radius token:

| token | px | 용도 |
|---|---|---|
| `RADIUS_SM` | 4 | 버튼 / selected row / 작은 요소 |
| `RADIUS_MD` | 8 | Window / ConsoleWindow / Dialog 외곽 |

### border 정리

미니멀 = border 최소화 + 약화:
- Window/Console/Dialog 외곽: 두꺼운 `#999` → `BORDER`(`#E4E4E7`) 1px + `RADIUS_MD`
- 패널 간 구분: 명도 차(SURFACE_APP vs SURFACE_PANEL) 우선, 필요 시 약한 1px `BORDER`
- Explorer zebra(`#FFFFFF`/`#F0F0F0` 교차): 짝/홀 명도차를 거의 없앰(둘 다 `SURFACE_PANEL`에 가깝게) + `ACCENT_SUBTLE` selected로 행 구분. hover 피드백은 인터랙션 spec(C, 비목표)이므로 본 spec에선 zebra 약화 + selected만.
- row separator(`#D8D8D8`): `BORDER`(`#E4E4E7`)로 약하게

### 적용 범위 (`compositor/src/render.rs` 전 함수)

모든 `COLOR_*` 직접 상수를 `theme::` token으로 교체 + radius/spacing 적용:

| 함수 | 변경 |
|---|---|
| `render_frame` (Container/Text/Button) | Button → `ACCENT` + `RADIUS_SM`. Text → `TEXT_PRIMARY`. bg → `SURFACE_APP` |
| FileTree 렌더 | `SURFACE_PANEL` + `TEXT_PRIMARY`/`SECONDARY` + 약한 border |
| Explorer 렌더 | zebra 약화 + `ACCENT_SUBTLE` selected + `SPACE_MD` padding + parent-nav 약하게 |
| `render_cli` | `SURFACE_SUNKEN` 입력 + `TEXT_*` |
| `render_window` | `SURFACE_ELEVATED` + `RADIUS_MD` 외곽 + `ACCENT` titlebar + 약한 `BORDER` + close 버튼 |
| `render_console_window` | 외곽 `RADIUS_MD` + titlebar `ACCENT` + 본문 `CONSOLE_BG` 유지 + status dot |
| Dialog 렌더 | `SURFACE_ELEVATED` + `RADIUS_MD` + 버튼 `ACCENT`/`RADIUS_SM` |

geometry/hit_test 좌표는 **불변** — 색·radius·여백만 변경(여백 변경 시 layout.rs 상수도 함께, hit_test 정합 유지). radius는 *시각만* 영향(hit_test는 사각형 rect 유지 — corner 클릭 약간의 시각/hit 불일치는 v1 허용, 미세).

## 테스트

**Unit (`compositor/`):**
- `theme.rs` token 존재/값 sanity (팔레트 상수 정의 + ARGB 형식)
- `fill_rect_rounded`:
  - radius=0이면 fill_rect와 동일 결과 (corner 픽셀 불투명)
  - corner 바깥 픽셀은 배경 유지 (skip)
  - corner edge 픽셀 alpha 0<a<255 (AA 동작)
  - rect보다 큰 radius는 clamp (panic 없음)

**시각 검증 (launcher 실물 — 자동 불가):**
- 모든 패널이 token 적용된 미니멀 룩
- Window/Console/Dialog 둥근 모서리 + 약한 border
- 텍스트 위계(primary/secondary/tertiary) 가독
- Explorer 행 구분 깔끔(zebra 약화 또는 selected/hover)
- 기존 기능 회귀 없음 (클릭/드래그/스크롤 좌표 정상)

## 작업 분류 (plan 단계 hint)

- **T1**: `theme.rs` 신설 — 색/spacing/radius token 전부 정의 + unit test
- **T2**: `fill_rect_rounded` 구현 + corner AA unit test
- **T3**: render.rs 색상 → theme token 일괄 교체 (geometry 불변)
- **T4**: radius 적용 (Window/Console/Dialog/Button) + border 약화
- **T5**: spacing 적용 (Explorer/CLI/창 본문 여백) + layout.rs 상수 정합
- **T6**: Explorer zebra/separator 정리 (selected `ACCENT_SUBTLE`)
- **T7**: 시각 검증 (launcher) + 미세 튜닝 + known-issues 갱신

## 알려진 한계 / 후속

- 타이포 위계 없음 (단일 18px) — 2단계 spec (draw_text size-aware + bold 폰트)
- 그림자(elevation) 없음 — radius + border로 대체. soft shadow는 후속(blur 비용)
- radius corner와 hit_test 사각형 불일치 (미세) — v2 정밀화
- 다크 테마 — token 추상 구조는 마련, 값 교체는 후속
