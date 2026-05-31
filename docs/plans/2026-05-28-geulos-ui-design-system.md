> **Status:** completed (2026-05-28)
> **Note:** UI 디자인 시스템 1단계 정식 마감 — theme.rs design token + fill_rect_rounded + zinc+blue 팔레트.

# UI 디자인 시스템 (모던 미니멀 light) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> **NEVER push** — controller가 끝에 batch push. subagent는 commit만.

**Goal:** 흩어진 `COLOR_*` 상수를 `theme.rs` design token(색 팔레트/spacing/radius)으로 체계화하고, compositor 전 패널에 모던 미니멀 light 룩(검증된 Tailwind/Radix zinc+blue 계열 + 표면 elevation 위계 + 둥근 모서리 + 약한 border)을 일괄 적용한다.

**Architecture:** `compositor/src/theme.rs` 신설 — 의미 기반 token(`SURFACE_PANEL`/`TEXT_SECONDARY`/`ACCENT` 등) `pub const`. render.rs/layout.rs가 `theme::` 참조. `fill_rect_rounded`(corner anti-alias) 신규 draw util로 Window/Console/Dialog/Button 둥근 모서리. geometry/hit_test 좌표 불변 — 색·radius·여백만 변경.

**Tech Stack:** Rust + fontdue + softbuffer (CPU 픽셀 렌더). 신규 의존성 없음. 디자인 원칙: 8pt spacing grid / WCAG AA 명도 대비 / 표면 elevation(app<panel<elevated) / 부드러운 accent + subtle 상태색. **fontdue/softbuffer라 launcher 실물 검증이 유일한 정확한 검증 — hex는 제안값, T7에서 튜닝.**

**Spec parent:** `docs/specs/2026-05-28-geulos-ui-design-system.md`

---

## File Structure

| 신규/수정 | 경로 | 책임 |
|---|---|---|
| Create | `compositor/src/theme.rs` | 모든 design token (색/spacing/radius) `pub const` + token unit test |
| Modify | `compositor/src/main.rs` 또는 `lib.rs` | `mod theme;` 등록 |
| Modify | `compositor/src/text.rs` | `blend_argb`를 `pub(crate)`로 노출 (fill_rect_rounded가 재사용) |
| Modify | `compositor/src/render.rs` | `fill_rect_rounded` 신설 + 모든 `COLOR_*` → `theme::` 교체 + radius/spacing 적용 + zebra 정리 |
| Modify | `compositor/src/layout.rs` | spacing 상수를 theme 정합 (`EXPLORER_ROW_H` 등 — 필요 시) |
| Modify | `docs/known-issues.md` | UI 디자인 시스템 1단계 마감 메모 |

---

## 진행 정책 공통

- Korean docs/comments + English identifiers
- 각 task TDD step (failing test → 구현 → pass → commit) — *시각 요소*라 unit test는 token 값/fill_rect_rounded 픽셀에 집중, 시각 자체는 T7 실물
- 각 commit 끝: `cargo build -p geulos-compositor` + `cargo clippy -p geulos-compositor --all-targets -- -D warnings` + `cargo fmt --all -- --check`
- compositor process 실행 중이면 rebuild 전 `Stop-Process -Name geulos,geulos-desktop-shell,geulos-compositor,geulosd -Force -ErrorAction SilentlyContinue`
- **geometry/hit_test 좌표 불변** — 색·radius·여백만. 여백 변경 시 layout.rs 상수도 함께 조정해 hit_test 정합 유지
- commit 메시지 한국어 + Co-Authored-By

---

# Stage A — design token 모듈 (1 task)

## Task 1: `theme.rs` 신설 — 전체 token 정의

**Files:**
- Create: `compositor/src/theme.rs`
- Modify: `compositor/src/main.rs` (또는 `lib.rs` — `mod theme;` 등록 위치 grep으로 확인)

- [ ] **Step 1.1: `theme.rs` 작성 (token + test)**

`compositor/src/theme.rs` 신규:

```rust
//! UI design token — 모던 미니멀 light. 흩어진 색/여백/radius 상수를 의미 기반으로 통일.
//!
//! 검증된 디자인 시스템(Tailwind/Radix zinc+blue) 계열 값 채용:
//! - 표면 elevation 위계: SURFACE_APP < PANEL < ELEVATED (명도 + border로 깊이)
//! - WCAG AA 대비: TEXT_PRIMARY(#18181B) on PANEL(#FFFFFF) = 16:1, SECONDARY = 4.6:1
//! - 8pt spacing grid: SPACE_* (4 배수)
//! - 부드러운 accent(blue-500) + subtle 상태(blue-50)
//!
//! 모든 값은 ARGB u32 (0xAA_RR_GG_BB). 후속 dark 테마 시 *값만* 교체 (이름 보존).
//! hex는 제안값 — fontdue/softbuffer 실물에서 미세 튜닝(T7).

// ─────────────── 표면 (elevation 위계) ───────────────
/// 최하단 desktop 배경 (zinc-50).
pub const SURFACE_APP: u32 = 0xFF_FA_FA_FA;
/// FileTree / Explorer 패널 (white).
pub const SURFACE_PANEL: u32 = 0xFF_FF_FF_FF;
/// Window / ConsoleWindow / Dialog (white + border로 분리).
pub const SURFACE_ELEVATED: u32 = 0xFF_FF_FF_FF;
/// 비활성/입력 영역 (zinc-100).
pub const SURFACE_SUNKEN: u32 = 0xFF_F4_F4_F5;

// ─────────────── 텍스트 위계 ───────────────
/// 본문·제목 (zinc-900).
pub const TEXT_PRIMARY: u32 = 0xFF_18_18_1B;
/// 보조·메타 (zinc-500).
pub const TEXT_SECONDARY: u32 = 0xFF_71_71_7A;
/// placeholder·비활성 (zinc-400).
pub const TEXT_TERTIARY: u32 = 0xFF_A1_A1_AA;
/// accent 배경 위 텍스트 (white).
pub const TEXT_ON_ACCENT: u32 = 0xFF_FF_FF_FF;

// ─────────────── accent (blue) ───────────────
/// 기본 강조 — 버튼·titlebar·focus (blue-500).
pub const ACCENT: u32 = 0xFF_3B_82_F6;
/// hover/active (blue-600).
pub const ACCENT_HOVER: u32 = 0xFF_25_63_EB;
/// selected row / 약한 강조 배경 (blue-50).
pub const ACCENT_SUBTLE: u32 = 0xFF_EF_F6_FF;

// ─────────────── border / 구분선 ───────────────
/// 패널·창 외곽 (zinc-200, 약하게).
pub const BORDER: u32 = 0xFF_E4_E4_E7;
/// 강조 구분 (zinc-300).
pub const BORDER_STRONG: u32 = 0xFF_D4_D4_D8;

// ─────────────── 상태색 (기존 정체성 유지) ───────────────
/// AI 강조 dot.
pub const STATUS_AI_DOT: u32 = 0xFF_FF_D5_00;
/// ConsoleWindow status: running (green-400).
pub const STATUS_RUNNING: u32 = 0xFF_4A_DE_80;
/// ConsoleWindow status: exited (gray).
pub const STATUS_EXITED: u32 = 0xFF_88_88_88;
/// ConsoleWindow status: terminated (red-500).
pub const STATUS_TERMINATED: u32 = 0xFF_EF_44_44;
/// ConsoleWindow status: error (amber-500).
pub const STATUS_ERROR: u32 = 0xFF_F5_9E_0B;
/// 닫기(X) 버튼 (red-500).
pub const CLOSE_BUTTON: u32 = 0xFF_EF_44_44;

// ─────────────── 단말 (CLI + Console 본문 — dark 정체성 유지) ───────────────
/// CLI 패널 + ConsoleWindow 본문 배경 (단말 dark).
pub const TERMINAL_BG: u32 = 0xFF_1E_1E_1E;
/// 단말 일반 텍스트 (stdout).
pub const TERMINAL_TEXT: u32 = 0xFF_E0_E0_E0;
/// 단말 stderr (red-300).
pub const TERMINAL_STDERR: u32 = 0xFF_FC_A5_A5;
/// CLI prompt (green).
pub const TERMINAL_PROMPT: u32 = 0xFF_6A_C9_6A;
/// CLI IME preedit / 회색 텍스트.
pub const TERMINAL_DIM: u32 = 0xFF_88_88_88;

// ─────────────── spacing scale (8pt grid 기반, 4 배수) ───────────────
pub const SPACE_XS: i32 = 4;
pub const SPACE_SM: i32 = 8;
pub const SPACE_MD: i32 = 12;
pub const SPACE_LG: i32 = 16;
pub const SPACE_XL: i32 = 24;

// ─────────────── radius ───────────────
/// 버튼 / selected row / 작은 요소.
pub const RADIUS_SM: i32 = 4;
/// Window / ConsoleWindow / Dialog 외곽.
pub const RADIUS_MD: i32 = 8;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_colors_are_opaque_argb() {
        // 모든 색 token은 alpha=0xFF (불투명) — 반투명 의도 없음.
        for c in [
            SURFACE_APP, SURFACE_PANEL, SURFACE_ELEVATED, SURFACE_SUNKEN,
            TEXT_PRIMARY, TEXT_SECONDARY, TEXT_TERTIARY, TEXT_ON_ACCENT,
            ACCENT, ACCENT_HOVER, ACCENT_SUBTLE, BORDER, BORDER_STRONG,
            STATUS_AI_DOT, STATUS_RUNNING, STATUS_EXITED, STATUS_TERMINATED,
            STATUS_ERROR, CLOSE_BUTTON, TERMINAL_BG, TERMINAL_TEXT,
            TERMINAL_STDERR, TERMINAL_PROMPT, TERMINAL_DIM,
        ] {
            assert_eq!(c >> 24, 0xFF, "token {:08X} alpha != 0xFF", c);
        }
    }

    #[test]
    fn spacing_scale_is_4px_grid() {
        for s in [SPACE_XS, SPACE_SM, SPACE_MD, SPACE_LG, SPACE_XL] {
            assert_eq!(s % 4, 0, "spacing {} not on 4px grid", s);
        }
        // 단조 증가.
        assert!(SPACE_XS < SPACE_SM && SPACE_SM < SPACE_MD);
        assert!(SPACE_MD < SPACE_LG && SPACE_LG < SPACE_XL);
    }

    #[test]
    fn elevation_hierarchy_app_lighter_than_panel_path() {
        // app(#FAFAFA)은 panel(#FFFFFF)보다 약간 어두워 패널이 떠 보임.
        // R 채널만 비교 (회색조라 R=G=B).
        let app_r = (SURFACE_APP >> 16) & 0xFF;
        let panel_r = (SURFACE_PANEL >> 16) & 0xFF;
        assert!(app_r < panel_r, "app가 panel보다 어두워야 elevation 성립");
    }
}
```

- [ ] **Step 1.2: 테스트 실행 — PASS 확인**

```
cargo test -p geulos-compositor --lib theme 2>&1 | Select-Object -Last 10
```

Expected: 3 test PASS (token 미정의면 컴파일 실패 → 정의 후 PASS).

- [ ] **Step 1.3: `mod theme;` 등록**

compositor의 mod 선언 위치 grep:
```
Get-ChildItem compositor/src -Filter "main.rs","lib.rs" | Select-String -Pattern "^mod |^pub mod " | Select-Object Path, LineNumber
```
적절한 파일(보통 main.rs)의 mod 선언부에 알파벳 위치로 추가:
```rust
mod theme;
```

- [ ] **Step 1.4: build + commit**

```
Stop-Process -Name geulos,geulos-desktop-shell,geulos-compositor,geulosd -Force -ErrorAction SilentlyContinue
cargo build -p geulos-compositor 2>&1 | Select-Object -Last 5
cargo fmt --all
git add compositor/src/theme.rs compositor/src/main.rs
git commit -m "$(cat <<'EOF'
feat(compositor): UI 디자인 시스템 T1 — theme.rs design token

흩어진 COLOR_*를 의미 기반 token으로 통일 (Tailwind/Radix zinc+blue 계열).
표면 elevation(app<panel<elevated) + 텍스트 위계(primary/secondary/tertiary) +
부드러운 accent(blue-500) + subtle(blue-50) + 약한 border(zinc-200) + 단말
dark(CLI/Console 공유) + 8pt spacing grid + radius(4/8). token unit test 3건
(opaque ARGB / 4px grid / elevation 위계).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

# Stage B — rounded rect 렌더 유틸 (1 task)

## Task 2: `fill_rect_rounded` + `blend_argb` 노출

**Files:**
- Modify: `compositor/src/text.rs` (`blend_argb` → `pub(crate)`)
- Modify: `compositor/src/render.rs` (`fill_rect_rounded` 신설 + test)

- [ ] **Step 2.1: `blend_argb`를 pub(crate)로 노출**

`compositor/src/text.rs`의 `fn blend_argb(bg: u32, fg: u32, alpha: u8) -> u32`를:
```rust
pub(crate) fn blend_argb(bg: u32, fg: u32, alpha: u8) -> u32 {
```

- [ ] **Step 2.2: `fill_rect_rounded` test 작성 (render.rs tests mod)**

`compositor/src/render.rs`의 `#[cfg(test)] mod tests`에 추가:

```rust
    #[test]
    fn fill_rect_rounded_radius_zero_fills_corners() {
        // radius=0 → 모든 corner 픽셀 불투명 (fill_rect와 동일).
        let w = 10usize;
        let h = 10usize;
        let mut buf = vec![0xFF_00_00_00u32; w * h];
        let rect = Rect { x: 0, y: 0, w: 10, h: 10 };
        fill_rect_rounded(&mut buf, w, h, &rect, 0, 0xFF_FF_FF_FF);
        // 좌상단 corner (0,0) 불투명 흰색.
        assert_eq!(buf[0], 0xFF_FF_FF_FF, "radius=0이면 corner도 채워져야");
    }

    #[test]
    fn fill_rect_rounded_clips_corner_pixel() {
        // radius=4인 10x10 사각형 — (0,0) corner 바깥 픽셀은 배경 유지.
        let w = 10usize;
        let h = 10usize;
        let bg = 0xFF_00_00_00u32;
        let mut buf = vec![bg; w * h];
        let rect = Rect { x: 0, y: 0, w: 10, h: 10 };
        fill_rect_rounded(&mut buf, w, h, &rect, 4, 0xFF_FF_FF_FF);
        // (0,0)은 corner 원(중심 (4,4), r=4) 바깥 (거리 ~5.6 > 4.5) → 배경 유지.
        assert_eq!(buf[0], bg, "corner 바깥 픽셀은 배경 유지");
        // 중앙 (5,5)는 불투명 흰색.
        assert_eq!(buf[5 * w + 5], 0xFF_FF_FF_FF, "중앙은 채워짐");
    }

    #[test]
    fn fill_rect_rounded_large_radius_no_panic() {
        // radius가 rect보다 커도 panic 없이 clamp.
        let w = 6usize;
        let h = 6usize;
        let mut buf = vec![0xFF_00_00_00u32; w * h];
        let rect = Rect { x: 0, y: 0, w: 6, h: 6 };
        fill_rect_rounded(&mut buf, w, h, &rect, 100, 0xFF_FF_FF_FF);
        // panic 없이 완료 — 중앙은 채워짐.
        assert_eq!(buf[3 * w + 3], 0xFF_FF_FF_FF);
    }
```

- [ ] **Step 2.3: 테스트 실행 — 실패 확인**

```
cargo test -p geulos-compositor --lib fill_rect_rounded 2>&1 | Select-Object -Last 10
```
Expected: 컴파일 실패 (fill_rect_rounded 미정의).

- [ ] **Step 2.4: `fill_rect_rounded` 구현**

`compositor/src/render.rs`의 `fill_rect` 함수 *직후* 추가:

```rust
/// 둥근 모서리 사각형. radius=0이면 fill_rect와 동일. 4 corner 영역만 픽셀별
/// 거리 판정 + anti-alias(blend_argb). 본체(corner 제외)는 통째로 채운다.
///
/// corner 중심에서 픽셀 거리 d:
/// - d <= r-0.5  → 불투명
/// - r-0.5 < d <= r+0.5 → alpha = (r+0.5-d) 비례 blend (AA edge)
/// - d > r+0.5  → skip (배경 유지)
pub fn fill_rect_rounded(buffer: &mut [u32], w: usize, h: usize, r: &Rect, radius: i32, color: u32) {
    // radius를 rect 절반으로 clamp (큰 radius panic/왜곡 방지).
    let radius = radius.clamp(0, (r.w.min(r.h) / 2).max(0));
    if radius == 0 {
        fill_rect(buffer, w, h, r, color);
        return;
    }
    let x0 = r.x.max(0);
    let y0 = r.y.max(0);
    let x1 = (r.x + r.w).min(w as i32);
    let y1 = (r.y + r.h).min(h as i32);
    // 4 corner 중심 (rect 안쪽으로 radius만큼).
    let cl = r.x + radius; // 좌측 corner 중심 x
    let cr = r.x + r.w - 1 - radius; // 우측
    let ct = r.y + radius; // 상단 corner 중심 y
    let cb = r.y + r.h - 1 - radius; // 하단
    for py in y0..y1 {
        for px in x0..x1 {
            // 이 픽셀이 어느 corner 영역인지 — 아니면 본체(불투명).
            let in_left = px < cl;
            let in_right = px > cr;
            let in_top = py < ct;
            let in_bottom = py > cb;
            let (cx, cy) = match (in_left, in_right, in_top, in_bottom) {
                (true, _, true, _) => (cl, ct),   // 좌상
                (true, _, _, true) => (cl, cb),   // 좌하
                (_, true, true, _) => (cr, ct),   // 우상
                (_, true, _, true) => (cr, cb),   // 우하
                _ => {
                    // 본체 — 불투명.
                    buffer[py as usize * w + px as usize] = color;
                    continue;
                }
            };
            let dx = (px - cx) as f32;
            let dy = (py - cy) as f32;
            let dist = (dx * dx + dy * dy).sqrt();
            let rf = radius as f32;
            let idx = py as usize * w + px as usize;
            if dist <= rf - 0.5 {
                buffer[idx] = color;
            } else if dist <= rf + 0.5 {
                // AA edge — alpha 0..255.
                let a = ((rf + 0.5 - dist) * 255.0).clamp(0.0, 255.0) as u8;
                buffer[idx] = crate::text::blend_argb(buffer[idx], color, a);
            }
            // else: 배경 유지.
        }
    }
}
```

- [ ] **Step 2.5: 테스트 PASS + build**

```
cargo test -p geulos-compositor --lib fill_rect_rounded 2>&1 | Select-Object -Last 10
cargo clippy -p geulos-compositor --all-targets -- -D warnings 2>&1 | Select-Object -Last 5
cargo fmt --all -- --check
```
Expected: 3 test PASS + clippy/fmt clean.

- [ ] **Step 2.6: commit**

```
git add compositor/src/text.rs compositor/src/render.rs
git commit -m "$(cat <<'EOF'
feat(compositor): UI 디자인 시스템 T2 — fill_rect_rounded (corner AA)

둥근 모서리 사각형 draw util. 4 corner 영역만 거리 판정 + anti-alias
(text::blend_argb 재사용, pub(crate) 노출). radius=0이면 fill_rect 위임,
큰 radius는 rect 절반으로 clamp. corner clip / radius-0 / large-radius
unit test 3건.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

# Stage C — token 일괄 교체 (1 task)

## Task 3: render.rs 모든 `COLOR_*` → `theme::` 교체 (geometry 불변)

**Files:**
- Modify: `compositor/src/render.rs`

색 token 매핑 (현 COLOR_* → theme::). **geometry/좌표/크기 불변 — 색 상수 참조만 교체.**

| 기존 상수 | 신규 token |
|---|---|
| `COLOR_BG` `#F5F5F5` | `theme::SURFACE_APP` |
| `COLOR_CONTAINER` `#E0E0E0` | `theme::SURFACE_SUNKEN` |
| `COLOR_BUTTON` `#4275E0` | `theme::ACCENT` |
| `COLOR_BUTTON_TEXT` | `theme::TEXT_ON_ACCENT` |
| `COLOR_TEXT` `#222` | `theme::TEXT_PRIMARY` |
| `COLOR_TREE_BG` `#F0F0F0` | `theme::SURFACE_PANEL` |
| `COLOR_CANVAS_BG` white | `theme::SURFACE_PANEL` |
| `COLOR_FOLDER_TEXT` | `theme::TEXT_PRIMARY` |
| `COLOR_FILE_TEXT` `#444` | `theme::TEXT_SECONDARY` |
| `COLOR_SELECTED_BG` `#D0E4FF` | `theme::ACCENT_SUBTLE` |
| `COLOR_ROW_BG` white | `theme::SURFACE_PANEL` |
| `COLOR_ROW_ALT_BG` `#F0F0F0` | `theme::SURFACE_APP` (zebra 약화 — T6에서 추가 조정) |
| `COLOR_ROW_SEPARATOR` `#D8D8D8` | `theme::BORDER` |
| `COLOR_PARENT_NAV_BG` `#DCE8FA` | `theme::ACCENT_SUBTLE` |
| `COLOR_AI_DOT` | `theme::STATUS_AI_DOT` |
| `COLOR_WINDOW_BG` | `theme::SURFACE_ELEVATED` |
| `COLOR_WINDOW_BORDER` `#999` | `theme::BORDER` |
| `COLOR_WINDOW_TITLE_BG` `#4275E0` | `theme::ACCENT` |
| `COLOR_WINDOW_TITLE_BG_FOCUSED` | `theme::ACCENT_HOVER` |
| `COLOR_WINDOW_TITLE_TEXT` | `theme::TEXT_ON_ACCENT` |
| `COLOR_WINDOW_CLOSE` `#E53E3E` | `theme::CLOSE_BUTTON` |
| `COLOR_WINDOW_RESIZE_HANDLE` `#CCC` | `theme::BORDER_STRONG` |
| `COLOR_PLACEHOLDER` `#999` | `theme::TEXT_TERTIARY` |
| `COLOR_CONSOLE_BG` | `theme::TERMINAL_BG` |
| `COLOR_CONSOLE_TEXT` | `theme::TERMINAL_TEXT` |
| `COLOR_CONSOLE_STDERR` | `theme::TERMINAL_STDERR` |
| `COLOR_STATUS_RUNNING/EXITED/TERMINATED/ERROR` | `theme::STATUS_*` |
| `COLOR_CLI_BG` `#1E1E1E` | `theme::TERMINAL_BG` |
| `COLOR_CLI_TEXT` | `theme::TERMINAL_TEXT` |
| `COLOR_CLI_CURSOR` | `theme::TERMINAL_TEXT` |
| `COLOR_CLI_PROMPT` | `theme::TERMINAL_PROMPT` |
| `COLOR_CLI_PREEDIT` | `theme::TERMINAL_DIM` |

**CLI는 dark 단말 유지** (TERMINAL_* — ConsoleWindow 본문과 일관, 셸 정체성). spec의 SURFACE_SUNKEN은 Container widget 등에만 적용. T7 실물에서 CLI light 전환 여부 최종 결정.

- [ ] **Step 3.1: render.rs 상단 `COLOR_*` const 블록 제거 + `use crate::theme;` 추가**

`compositor/src/render.rs` 상단 (line ~10-81)의 `const COLOR_*` / `const CLI_*` 색상 정의들을 제거. 단 *non-색상 상수* (`CLI_LINE_HEIGHT`/`CLI_PADDING_X`/`CLI_PADDING_Y`/`CLI_CURSOR_BLINK_MS`/`AI_HIGHLIGHT_MS`/`ICON_Y_OFFSET`)는 *유지* (spacing은 T5에서 theme 정합). 파일 상단 import에 추가:
```rust
use crate::theme;
```

- [ ] **Step 3.2: 위 매핑표대로 모든 색 참조 교체**

render.rs 본문의 `COLOR_X` 식별자를 매핑표의 `theme::Y`로 일괄 교체. `draw_explorer_row_bg`의 `COLOR_ROW_BG`/`COLOR_ROW_ALT_BG`/`COLOR_ROW_SEPARATOR`도 포함. grep으로 잔여 `COLOR_` 식별자 0 확인:
```
Get-ChildItem compositor/src/render.rs | Select-String -Pattern "COLOR_" | Select-Object LineNumber
```
→ 결과 0건 (전부 theme::로 교체).

- [ ] **Step 3.3: build + clippy + fmt**

```
Stop-Process -Name geulos,geulos-desktop-shell,geulos-compositor,geulosd -Force -ErrorAction SilentlyContinue
cargo build -p geulos-compositor 2>&1 | Select-Object -Last 10
cargo clippy -p geulos-compositor --all-targets -- -D warnings 2>&1 | Select-Object -Last 5
cargo fmt --all -- --check
```
Expected: 빌드 + clippy + fmt clean. (시각은 T7 실물.)

- [ ] **Step 3.4: commit**

```
git add compositor/src/render.rs
git commit -m "$(cat <<'EOF'
feat(compositor): UI 디자인 시스템 T3 — render.rs 색상 token 일괄 교체

흩어진 COLOR_* 30여 개를 theme:: token으로 교체 (geometry 불변). 표면
elevation(app/panel/elevated/sunken) + 텍스트 위계 + accent/subtle + 약한
border 적용. CLI는 TERMINAL_* (dark 단말 유지 — Console 본문과 일관). 색
상수 정의 블록 제거, non-색상 spacing 상수(CLI_PADDING 등)는 T5까지 유지.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

# Stage D — radius 적용 (1 task)

## Task 4: Window/Console/Dialog/Button 둥근 모서리 + border 약화

**Files:**
- Modify: `compositor/src/render.rs`

`fill_rect`로 그리던 외곽/배경을 `fill_rect_rounded`로 교체 (radius token). **geometry rect 불변 — fill 함수만 교체.**

- [ ] **Step 4.1: Window 외곽 radius**

`render_window`의 외곽 border + inner 배경 fill:
```rust
// 기존: fill_rect(buffer, w, h, rect, theme::BORDER);  (외곽)
//       fill_rect(buffer, w, h, &inner, theme::SURFACE_ELEVATED);
// 변경:
fill_rect_rounded(buffer, w, h, rect, theme::RADIUS_MD, theme::BORDER);
fill_rect_rounded(buffer, w, h, &inner, theme::RADIUS_MD, theme::SURFACE_ELEVATED);
```
titlebar는 상단만 둥글어야 자연스러우나 v1은 *titlebar는 사각 유지* (상단 corner는 외곽 radius가 cover — titlebar fill을 inner 안쪽에 그리면 외곽 둥근 부분이 살짝 보임). 단순화: titlebar `fill_rect` 유지, 외곽/inner만 rounded. close 버튼은 `fill_rect_rounded(.., theme::RADIUS_SM, theme::CLOSE_BUTTON)`.

- [ ] **Step 4.2: ConsoleWindow 외곽 radius**

`render_console_window`의 외곽 border + inner:
```rust
fill_rect_rounded(buffer, w, h, rect, theme::RADIUS_MD, theme::BORDER);
fill_rect_rounded(buffer, w, h, &inner, theme::RADIUS_MD, theme::TERMINAL_BG);
```
close 버튼 `RADIUS_SM`.

- [ ] **Step 4.3: Dialog 외곽 + 버튼 radius**

Dialog 렌더 함수(render.rs에서 Dialog 그리는 위치 grep — `aios.builtin/Dialog@1` 또는 dialog 관련 fill)에 외곽 `RADIUS_MD` + 액션 버튼 `RADIUS_SM` 적용. Dialog 배경은 `theme::SURFACE_ELEVATED`, 버튼은 `theme::ACCENT` + `RADIUS_SM`.

- [ ] **Step 4.4: Button widget radius**

`render_frame`의 Button 분기:
```rust
// 기존: fill_rect(buffer, width, height, &rect, theme::ACCENT);
fill_rect_rounded(buffer, width, height, &rect, theme::RADIUS_SM, theme::ACCENT);
```

- [ ] **Step 4.5: selected row radius (Explorer)**

Explorer/FileTree의 `COLOR_SELECTED_BG`(→ `theme::ACCENT_SUBTLE`) fill을 `fill_rect_rounded(.., theme::RADIUS_SM, theme::ACCENT_SUBTLE)`로. (row rect 안에 살짝 inset하면 더 예쁘나 v1은 rect 그대로 rounded.)

- [ ] **Step 4.6: build + clippy + fmt + commit**

```
Stop-Process -Name geulos,geulos-desktop-shell,geulos-compositor,geulosd -Force -ErrorAction SilentlyContinue
cargo build -p geulos-compositor 2>&1 | Select-Object -Last 5
cargo clippy -p geulos-compositor --all-targets -- -D warnings 2>&1 | Select-Object -Last 5
cargo fmt --all -- --check
git add compositor/src/render.rs
git commit -m "$(cat <<'EOF'
feat(compositor): UI 디자인 시스템 T4 — 둥근 모서리 + border 약화

Window/ConsoleWindow/Dialog 외곽 RADIUS_MD(8) + Button/selected/close
RADIUS_SM(4). fill_rect_rounded로 교체 (geometry rect 불변). 두꺼운 #999
border → 약한 BORDER(zinc-200). 미니멀 light 룩의 핵심 시각 변화.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

# Stage E — spacing 적용 (1 task)

## Task 5: 여백 token 적용 + layout 정합

**Files:**
- Modify: `compositor/src/render.rs` (CLI/창 본문 padding)
- Modify: `compositor/src/layout.rs` (EXPLORER_ROW_H 등 — 필요 시)

- [ ] **Step 5.1: CLI padding을 theme spacing으로**

render.rs의 `CLI_PADDING_X: i32 = 8` / `CLI_PADDING_Y: i32 = 6`을 theme 참조로:
```rust
// const CLI_PADDING_X: i32 = 8;  제거
// const CLI_PADDING_Y: i32 = 6;  제거
// 사용처에서 theme::SPACE_SM (8) / theme::SPACE_XS+2 대신:
// CLI_PADDING_X → theme::SPACE_SM (8)
// CLI_PADDING_Y → theme::SPACE_SM (8, 기존 6→8로 약간 넉넉히)
```
`CLI_LINE_HEIGHT`(22)는 폰트 의존이라 유지. CLI padding 사용처를 `theme::SPACE_SM`로 교체.

- [ ] **Step 5.2: 창 본문 inset을 theme spacing으로**

`render_window` / `render_console_window`의 본문 content 영역 padding 하드코딩(8 등)을 `theme::SPACE_SM`(8) 또는 `theme::SPACE_MD`(12)로. content inset을 `SPACE_MD`로 넉넉히 (가독성).

- [ ] **Step 5.3: Explorer row 높이/padding 정합**

`layout.rs`의 `EXPLORER_ROW_H: i32 = 28` 유지 (폰트 18 + 여백 — 8pt grid 근접). Explorer row 안 텍스트/아이콘 좌측 padding을 `theme::SPACE_MD`(12)로 (render.rs 사용처). row 높이 변경 시 hit_test 정합 — *변경 안 함* (28 유지).

- [ ] **Step 5.4: build + clippy + fmt + commit**

```
Stop-Process -Name geulos,geulos-desktop-shell,geulos-compositor,geulosd -Force -ErrorAction SilentlyContinue
cargo build -p geulos-compositor 2>&1 | Select-Object -Last 5
cargo clippy -p geulos-compositor --all-targets -- -D warnings 2>&1 | Select-Object -Last 5
cargo fmt --all -- --check
git add compositor/src/render.rs compositor/src/layout.rs
git commit -m "$(cat <<'EOF'
feat(compositor): UI 디자인 시스템 T5 — spacing token 적용

CLI padding + 창 본문 inset을 theme::SPACE_* (8pt grid)로. content inset
SPACE_MD(12)로 넉넉히, CLI padding SPACE_SM(8). EXPLORER_ROW_H(28)는
hit_test 정합 위해 유지 — 텍스트/아이콘 좌측 padding만 SPACE_MD.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

# Stage F — Explorer 행 정리 (1 task)

## Task 6: zebra 약화 + separator 정리

**Files:**
- Modify: `compositor/src/render.rs` (`draw_explorer_row_bg`)

- [ ] **Step 6.1: zebra 명도차 제거 + separator 약화**

`draw_explorer_row_bg`를 다음으로 변경 — 짝/홀 배경을 둘 다 `SURFACE_PANEL`에 가깝게(명도차 거의 제거) + separator를 약한 `BORDER`로:

```rust
fn draw_explorer_row_bg(buffer: &mut [u32], w: usize, h: usize, rect: &Rect) {
    // 미니멀: zebra 명도차 제거 — 모든 행 SURFACE_PANEL. 행 구분은 약한
    // separator(BORDER) + selected(ACCENT_SUBTLE, render 사용처)로.
    fill_rect(buffer, w, h, rect, crate::theme::SURFACE_PANEL);
    fill_rect(
        buffer,
        w,
        h,
        &Rect { x: rect.x, y: rect.y + rect.h - 1, w: rect.w, h: 1 },
        crate::theme::BORDER,
    );
}
```

`EXPLORER_ROW_H` import는 div_euclid 계산에 더 이상 불필요하면 제거 (zebra idx 계산 삭제) — 단 다른 곳에서 쓰면 유지. grep 확인 후 unused면 import 정리.

- [ ] **Step 6.2: build + clippy + fmt + commit**

```
Stop-Process -Name geulos,geulos-desktop-shell,geulos-compositor,geulosd -Force -ErrorAction SilentlyContinue
cargo build -p geulos-compositor 2>&1 | Select-Object -Last 5
cargo clippy -p geulos-compositor --all-targets -- -D warnings 2>&1 | Select-Object -Last 5
cargo fmt --all -- --check
git add compositor/src/render.rs
git commit -m "$(cat <<'EOF'
feat(compositor): UI 디자인 시스템 T6 — Explorer zebra 제거 + separator 약화

zebra 짝/홀 명도차(#FFF/#F0F0F0) 제거 — 모든 행 SURFACE_PANEL. 행 구분은
약한 BORDER separator + selected ACCENT_SUBTLE로. 미니멀 룩 (줄무늬 노이즈
제거). hover 피드백은 인터랙션 spec(C, 후속).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

# Stage G — 실물 검증 + 마감 (1 task)

## Task 7: launcher 시각 검증 + 튜닝 + known-issues

**Files:**
- Modify: `compositor/src/theme.rs` (실물 튜닝 — 필요 시)
- Modify: `docs/known-issues.md`

- [ ] **Step 7.1: 전체 빌드 + launcher 기동**

```
Stop-Process -Name geulos,geulos-desktop-shell,geulos-compositor,geulosd -Force -ErrorAction SilentlyContinue
cargo build --bin geulos 2>&1 | Select-Object -Last 5
D:/GeulOS/target/debug/geulos.exe
```
(controller가 background 기동 — subagent는 빌드만 확인하고 시각 검증은 controller/사용자에게 위임.)

- [ ] **Step 7.2: 시각 체크리스트 (controller + 사용자)**

launcher 화면에서 확인:
- [ ] desktop 배경 `SURFACE_APP`, 패널 `SURFACE_PANEL` — 패널이 살짝 떠 보임 (elevation)
- [ ] FileTree/Explorer 텍스트 위계 (폴더 primary / 파일 secondary)
- [ ] Explorer 행 — zebra 줄무늬 없이 깔끔, selected는 연한 파랑(ACCENT_SUBTLE) + 둥근 모서리
- [ ] Window/ConsoleWindow/Dialog — 둥근 모서리(RADIUS_MD) + 약한 border, titlebar accent
- [ ] CLI — dark 단말 유지, prompt 녹색
- [ ] 버튼 — accent + 둥근 모서리
- [ ] 기존 동작 회귀 없음: 클릭/드래그/스크롤/창 이동·리사이즈/X 닫기 좌표 정상

- [ ] **Step 7.3: 실물 튜닝 (필요 시)**

실물에서 대비/명도가 약하거나 강하면 `theme.rs` hex 미세 조정 (예: SURFACE_APP을 더 어둡게 elevation 강화, border 더 약하게). 변경 시 token unit test(elevation 위계) 여전히 통과 확인.

- [ ] **Step 7.4: known-issues 마감 메모**

`docs/known-issues.md` "마일스톤 종료 시점"에 추가:
```markdown
- **UI 디자인 시스템 1단계 마감 (2026-05-28):** compositor/src/theme.rs 신설로
  흩어진 COLOR_* 30여 개를 design token(Tailwind/Radix zinc+blue 계열)으로 통일.
  모던 미니멀 light — 표면 elevation(app<panel<elevated) + 텍스트 위계 +
  부드러운 accent(blue-500)/subtle(blue-50) + 약한 border(zinc-200) + 둥근
  모서리(fill_rect_rounded AA, RADIUS 4/8) + Explorer zebra 제거 + 8pt spacing.
  CLI/Console은 dark 단말 유지(정체성). geometry/hit_test 불변. fontdue CPU
  렌더라 launcher 실물 튜닝. 후속: 타이포 scale(2단계, draw_text size-aware +
  bold 폰트), 다크 테마(token 추상 마련), 아이콘 정비, 인터랙션/애니(C), 렌더
  성능(B, KI-007).
```

KI-017(word-wrap char 휴리스틱)는 본 spec 무관 — 유지.

- [ ] **Step 7.5: 최종 검증 + commit**

```
cargo test -p geulos-compositor --lib 2>&1 | Select-Object -Last 8
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | Select-Object -Last 5
cargo fmt --all -- --check
git add compositor/src/theme.rs docs/known-issues.md
git commit -m "$(cat <<'EOF'
docs+tune(compositor): UI 디자인 시스템 T7 — 실물 튜닝 + known-issues 마감

launcher 실물 검증 후 theme token 미세 조정. known-issues에 1단계 마감 메모
+ 후속(타이포/다크/아이콘/인터랙션/성능). geometry/hit_test 회귀 없음 확인.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review (controller 수행)

**1. Spec coverage:**
- theme.rs token (색/spacing/radius) → T1 ✓
- fill_rect_rounded (corner AA) → T2 ✓
- 색상 token 일괄 교체 → T3 ✓
- radius 적용 (Window/Console/Dialog/Button) → T4 ✓
- border 약화 → T3(색) + T4(radius와 함께) ✓
- spacing 적용 → T5 ✓
- Explorer zebra/separator 정리 → T6 ✓
- 실물 검증 + 튜닝 → T7 ✓
- 표면 elevation / 텍스트 위계 / accent → token 값(T1) + 적용(T3) ✓
- 빠짐: spec의 "CLI = SURFACE_SUNKEN"을 plan은 "CLI = TERMINAL dark 유지"로 변경 — *의도적 조정* (셸 정체성 + Console 일관). T3/T7에 근거 명시. spec과 불일치하나 plan이 더 정합 — T7 실물에서 최종 결정 + 필요 시 spec 동기화.

**2. Placeholder scan:** 모든 step에 구체 code/매핑/명령. radius corner 공식·token 값·매핑표 전부 명시. "필요 시"는 layout 정합/튜닝 등 *조건부 실측* 항목으로 acceptable.

**3. Type consistency:**
- `fill_rect_rounded(buffer, w, h, r, radius, color)` — T2 정의 / T4 사용 일치 ✓
- `theme::` token 이름 — T1 정의 / T3·T4·T5·T6 사용 일치 (SURFACE_*/TEXT_*/ACCENT*/BORDER*/STATUS_*/TERMINAL_*/SPACE_*/RADIUS_*) ✓
- `blend_argb` pub(crate) — T2 노출 / fill_rect_rounded 사용 ✓

자체 검토 통과. CLI 색 조정(spec→plan)만 T7 사용자 확인 대상으로 명시.

---

## Plan complete and saved to `docs/plans/2026-05-28-geulos-ui-design-system.md`.

Two execution options:

**1. Subagent-Driven (recommended)** — controller가 task별 implementer dispatch + spec/quality review (단순 token 교체 task는 review 간소화, fill_rect_rounded/radius 적용 같은 시각·로직 task는 review 유지).

**2. Inline Execution** — 현재 session에서 batch 실행 + checkpoint review.

Which approach?
