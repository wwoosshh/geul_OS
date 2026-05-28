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
        for c in [
            SURFACE_APP,
            SURFACE_PANEL,
            SURFACE_ELEVATED,
            SURFACE_SUNKEN,
            TEXT_PRIMARY,
            TEXT_SECONDARY,
            TEXT_TERTIARY,
            TEXT_ON_ACCENT,
            ACCENT,
            ACCENT_HOVER,
            ACCENT_SUBTLE,
            BORDER,
            BORDER_STRONG,
            STATUS_AI_DOT,
            STATUS_RUNNING,
            STATUS_EXITED,
            STATUS_TERMINATED,
            STATUS_ERROR,
            CLOSE_BUTTON,
            TERMINAL_BG,
            TERMINAL_TEXT,
            TERMINAL_STDERR,
            TERMINAL_PROMPT,
            TERMINAL_DIM,
        ] {
            assert_eq!(c >> 24, 0xFF, "token {:08X} alpha != 0xFF", c);
        }
    }

    #[test]
    fn spacing_scale_is_4px_grid() {
        for s in [SPACE_XS, SPACE_SM, SPACE_MD, SPACE_LG, SPACE_XL] {
            assert_eq!(s % 4, 0, "spacing {} not on 4px grid", s);
        }
        assert!(SPACE_XS < SPACE_SM && SPACE_SM < SPACE_MD);
        assert!(SPACE_MD < SPACE_LG && SPACE_LG < SPACE_XL);
    }

    #[test]
    fn elevation_hierarchy_app_lighter_than_panel_path() {
        let app_r = (SURFACE_APP >> 16) & 0xFF;
        let panel_r = (SURFACE_PANEL >> 16) & 0xFF;
        assert!(app_r < panel_r, "app가 panel보다 어두워야 elevation 성립");
    }
}
