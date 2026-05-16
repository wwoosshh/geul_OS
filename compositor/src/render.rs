//! softbuffer 픽셀 버퍼에 객체 트리 그리기.

use crate::layout::{LayoutResult, Rect};
use crate::text::draw_text;
use crate::tree_model::TreeModel;

const COLOR_BG: u32 = 0xFF_F5_F5_F5;
const COLOR_CONTAINER: u32 = 0xFF_E0_E0_E0;
const COLOR_BUTTON: u32 = 0xFF_42_75_E0;
const COLOR_TEXT: u32 = 0xFF_22_22_22;
const COLOR_BUTTON_TEXT: u32 = 0xFF_FF_FF_FF;

/// 한 프레임을 그린다.
pub fn render_frame(
    tree: &TreeModel,
    layout: &LayoutResult,
    buffer: &mut [u32],
    width: usize,
    height: usize,
) {
    // 배경
    fill_rect(
        buffer,
        width,
        height,
        &Rect { x: 0, y: 0, w: width as i32, h: height as i32 },
        COLOR_BG,
    );

    for (id, rect) in layout.iter() {
        let obj = match tree.get(id) {
            Some(o) => o,
            None => continue,
        };
        match obj.type_uri.as_str() {
            "aios.std/Container@1" => {
                fill_rect(buffer, width, height, &rect, COLOR_CONTAINER);
            }
            "aios.std/Text@1" => {
                fill_rect(buffer, width, height, &rect, COLOR_BG);
                let content =
                    obj.state.get("content").and_then(|v| v.as_str()).unwrap_or("(empty)");
                draw_text(buffer, width, height, content, rect.x + 8, rect.y + 8, COLOR_TEXT);
            }
            "aios.std/Button@1" => {
                fill_rect(buffer, width, height, &rect, COLOR_BUTTON);
                let label = obj.state.get("label").and_then(|v| v.as_str()).unwrap_or("(button)");
                draw_text(
                    buffer,
                    width,
                    height,
                    label,
                    rect.x + 16,
                    rect.y + 16,
                    COLOR_BUTTON_TEXT,
                );
            }
            "aios.std/Toggle@1" => {
                let on = obj.state.get("on").and_then(|v| v.as_bool()).unwrap_or(false);
                let color = if on { 0xFF_4C_AF_50 } else { 0xFF_9E_9E_9E };
                fill_rect(buffer, width, height, &rect, color);
                draw_text(
                    buffer,
                    width,
                    height,
                    if on { "ON" } else { "OFF" },
                    rect.x + 16,
                    rect.y + 8,
                    COLOR_BUTTON_TEXT,
                );
            }
            _ => {}
        }
    }
}

fn fill_rect(buffer: &mut [u32], w: usize, h: usize, r: &Rect, color: u32) {
    let x0 = r.x.max(0) as usize;
    let y0 = r.y.max(0) as usize;
    let x1 = ((r.x + r.w).max(0) as usize).min(w);
    let y1 = ((r.y + r.h).max(0) as usize).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            buffer[y * w + x] = color;
        }
    }
}
