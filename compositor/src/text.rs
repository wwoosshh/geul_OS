//! 텍스트 래스터화 (fontdue 기반).

use std::sync::OnceLock;

use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::Font;

const FONT_BYTES: &[u8] = include_bytes!("../fonts/font.ttf");
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
        let gy = y + glyph.y as i32 + (FONT_SIZE as i32);
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
