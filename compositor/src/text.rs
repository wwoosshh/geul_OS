//! 텍스트 래스터화 (fontdue 기반).

use std::sync::OnceLock;

use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::Font;

// Noto Sans KR Regular (SIL OFL 1.1) — 한글/라틴 모두 커버.
// Source: https://github.com/notofonts/noto-cjk (Sans/SubsetOTF/KR)
// License: compositor/fonts/LICENSE-NotoSansKR
// 옵션 A 선택: 단일 폰트로 한글+라틴 동시 처리. 별도 fallback 불필요.
const FONT_BYTES: &[u8] = include_bytes!("../fonts/NotoSansKR-Regular.otf");
const FONT_SIZE: f32 = 18.0;

static FONT: OnceLock<Font> = OnceLock::new();

fn font() -> &'static Font {
    FONT.get_or_init(|| {
        Font::from_bytes(FONT_BYTES, fontdue::FontSettings::default()).expect("font load")
    })
}

/// 텍스트를 ARGB 픽셀 버퍼에 그리는 유틸.
///
/// `buffer`: ARGB u32 픽셀 버퍼 (softbuffer 호환). `stride`는 한 행의 픽셀 수.
/// `(x, y)`는 텍스트 left-top 위치.
/// `color`: ARGB u32 (예: 0xFF_00_00_00 검정).
#[allow(clippy::too_many_arguments)]
pub fn draw_text(
    buffer: &mut [u32],
    stride: usize,
    height: usize,
    text: &str,
    x: i32,
    y: i32,
    color: u32,
) {
    let f = font();
    let fonts = [f];
    let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
    layout.reset(&LayoutSettings::default());
    layout.append(&fonts, &TextStyle::new(text, FONT_SIZE, 0));
    for glyph in layout.glyphs() {
        let (metrics, bitmap) = f.rasterize(glyph.parent, FONT_SIZE);
        let gx = x + glyph.x as i32;
        // fontdue 0.9.x PositiveYDown: `GlyphPosition.y`는 *글리프 bbox 상단*의
        // 픽셀 좌표(layout 원점 = LayoutSettings.{x,y} = (0,0) 기준). finalize() 내부에서
        // `baseline_y = max_ascent`로 잡힌 뒤 `glyph.y += baseline_y`되므로,
        // 결과적으로 layout 원점에서 글리프 top까지의 거리(픽셀)이다.
        // 따라서 `+ FONT_SIZE`를 더하면 안 됨 — 그 보정은 baseline 기준 좌표일 때나
        // 필요했고, font.ttf(좁은 ASCII) 시절 우연히 맞아 보였을 뿐. Noto Sans KR로
        // 바꾸면서 텍스트가 한 행 아래로 밀려 hit_test와 시각 어긋남 (T6 후속 버그).
        let gy = y + glyph.y as i32;
        for row in 0..metrics.height {
            for col in 0..metrics.width {
                let px = gx + col as i32;
                let py = gy + row as i32;
                if px < 0 || py < 0 || px >= stride as i32 || py >= height as i32 {
                    continue;
                }
                let alpha = bitmap[row * metrics.width + col];
                if alpha == 0 {
                    continue;
                }
                let idx = (py as usize) * stride + (px as usize);
                let bg = buffer[idx];
                buffer[idx] = blend_argb(bg, color, alpha);
            }
        }
    }
}

fn blend_argb(bg: u32, fg: u32, alpha: u8) -> u32 {
    let a = alpha as u32;
    let inv = 255 - a;
    let bg_r = (bg >> 16) & 0xFF;
    let bg_g = (bg >> 8) & 0xFF;
    let bg_b = bg & 0xFF;
    let fg_r = (fg >> 16) & 0xFF;
    let fg_g = (fg >> 8) & 0xFF;
    let fg_b = fg & 0xFF;
    let r = (bg_r * inv + fg_r * a) / 255;
    let g = (bg_g * inv + fg_g * a) / 255;
    let b = (bg_b * inv + fg_b * a) / 255;
    0xFF_00_00_00 | (r << 16) | (g << 8) | b
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 한글 글리프가 실제로 폰트에 존재하는지 회귀 테스트.
    /// 기존 font.ttf(ASCII만)로 회귀하면 이 테스트가 실패함.
    #[test]
    fn korean_glyph_present_in_font() {
        let f = font();
        // "한" — Hangul Syllable HAN (U+D55C). 빈 사각형(tofu)이면 0 반환.
        let idx = f.lookup_glyph_index('한');
        assert!(idx != 0, "Korean glyph '한' missing from bundled font");
        // placeholder의 핵심 글자도 확인.
        for c in "파일을선택하세요".chars() {
            assert!(f.lookup_glyph_index(c) != 0, "Korean glyph '{c}' missing from bundled font");
        }
    }

    /// 라틴 글리프도 같은 폰트로 처리되는지 확인 — Option A의 핵심 가정.
    #[test]
    fn latin_glyphs_present_in_font() {
        let f = font();
        for c in "ABCabc012()".chars() {
            assert!(f.lookup_glyph_index(c) != 0, "Latin glyph '{c}' missing from bundled font");
        }
    }
}
