//! softbuffer 픽셀 버퍼에 객체 트리 그리기.

use crate::keyboard::CliLocalState;
use crate::layout::{LayoutResult, Rect};
use crate::text::{draw_text, measure_text_width};
use crate::tree_model::TreeModel;
use crate::window_geom::{WINDOW_CLOSE_BTN, WINDOW_RESIZE_HANDLE, WINDOW_TITLE_H};

const COLOR_BG: u32 = 0xFF_F5_F5_F5;
const COLOR_CONTAINER: u32 = 0xFF_E0_E0_E0;
const COLOR_BUTTON: u32 = 0xFF_42_75_E0;
const COLOR_TEXT: u32 = 0xFF_22_22_22;
const COLOR_BUTTON_TEXT: u32 = 0xFF_FF_FF_FF;
const COLOR_TREE_BG: u32 = 0xFF_F0_F0_F0;
const COLOR_CANVAS_BG: u32 = 0xFF_FF_FF_FF;
const COLOR_FOLDER_TEXT: u32 = 0xFF_22_22_22;
const COLOR_FILE_TEXT: u32 = 0xFF_44_44_44;
const COLOR_SELECTED_BG: u32 = 0xFF_D0_E4_FF;
const COLOR_AI_DOT: u32 = 0xFF_FF_D5_00;
const AI_HIGHLIGHT_MS: i64 = 5000;

// T8.8: Window 오버레이 색상 + 치수.
const COLOR_WINDOW_BG: u32 = 0xFF_FA_FA_FA;
const COLOR_WINDOW_BORDER: u32 = 0xFF_99_99_99;
const COLOR_WINDOW_TITLE_BG: u32 = 0xFF_42_75_E0;
const COLOR_WINDOW_TITLE_BG_FOCUSED: u32 = 0xFF_22_55_C0;
const COLOR_WINDOW_TITLE_TEXT: u32 = 0xFF_FF_FF_FF;
const COLOR_WINDOW_CLOSE: u32 = 0xFF_E5_3E_3E;
const COLOR_WINDOW_RESIZE_HANDLE: u32 = 0xFF_CC_CC_CC;
/// "(미리보기 없음)" 등 placeholder 텍스트 색 (T8.4에서 제거됐던 것, T8.8 Window 본문에서 재사용).
const COLOR_PLACEHOLDER: u32 = 0xFF_99_99_99;
// WINDOW_TITLE_H / WINDOW_RESIZE_HANDLE / WINDOW_CLOSE_BTN은 T8.9에서 window_geom 모듈로
// 분리됨 (render와 main.rs 입력 처리가 같은 상수를 공유해야 click 영역이 어긋나지 않는다).

// T7.5: 하단 CLI 패널 색상.
const COLOR_CLI_BG: u32 = 0xFF_1E_1E_1E;
const COLOR_CLI_TEXT: u32 = 0xFF_F0_F0_F0;
const COLOR_CLI_CURSOR: u32 = 0xFF_F0_F0_F0;
const COLOR_CLI_PROMPT: u32 = 0xFF_6A_C9_6A;
/// T7.6 (ADR-029): IME 조합 중 텍스트 색 — 회색으로 *commit 전* 임을 시각적으로 구분.
const COLOR_CLI_PREEDIT: u32 = 0xFF_88_88_88;
/// CLI 한 줄 픽셀 높이 (폰트 18pt + 약간의 여유).
const CLI_LINE_HEIGHT: i32 = 22;
/// CLI 텍스트 좌측 여백.
const CLI_PADDING_X: i32 = 8;
/// CLI 텍스트 상단 여백.
const CLI_PADDING_Y: i32 = 6;
/// 커서 깜빡임 주기 (ms) — 1초 (500ms on / 500ms off).
const CLI_CURSOR_BLINK_MS: i64 = 1000;

/// 한 프레임을 그린다.
///
/// `cli_state`는 컴포지터-사이드 CLI 입력 버퍼/커서. Cli 객체가 layout에 있을 때만 사용된다.
pub fn render_frame(
    tree: &TreeModel,
    layout: &LayoutResult,
    buffer: &mut [u32],
    width: usize,
    height: usize,
    cli_state: &CliLocalState,
) {
    // 배경
    fill_rect(
        buffer,
        width,
        height,
        &Rect { x: 0, y: 0, w: width as i32, h: height as i32 },
        COLOR_BG,
    );

    let now_ms = chrono::Utc::now().timestamp_millis();
    let selected_id = find_selected_in_file_tree(tree);

    for (id, rect, _role) in layout.iter() {
        let obj = match tree.get(id) {
            Some(o) => o,
            None => continue,
        };
        // T8.10 방어 가드 — layout_desktop이 destroyed Window를 이미 제외하지만,
        // 다른 경로(예: 비-Desktop 루트)나 향후 다른 객체가 tombstone될 수 있어 동일 skip.
        if obj.state.get("destroyed").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        match obj.type_uri.as_str() {
            "aios.builtin/Desktop@1" => {
                // 배경만 — 자식 FileTree/Canvas가 윈도우를 덮음.
            }
            "aios.builtin/FileTree@1" => {
                fill_rect(buffer, width, height, &rect, COLOR_TREE_BG);
            }
            "aios.builtin/Explorer@1" => {
                // M8: 흰 배경. 자식 (Folder/File) line rect들은 layout이 직접 push하므로
                // 각 자식은 자기 type_uri 분기에서 그려진다 — 여기서는 별도 자식 iteration 불필요.
                fill_rect(buffer, width, height, &rect, COLOR_CANVAS_BG);
            }
            "aios.builtin/Cli@1" => {
                render_cli(buffer, width, height, &rect, obj, cli_state, now_ms);
            }
            "aios.builtin/Window@1" => {
                let focused = obj.state.get("focused").and_then(|v| v.as_bool()).unwrap_or(false);
                render_window(buffer, width, height, &rect, tree, obj, focused);
            }
            "aios.std/Folder@1" => {
                let is_sel = selected_id == Some(id);
                if is_sel {
                    fill_rect(buffer, width, height, &rect, COLOR_SELECTED_BG);
                }
                let name = obj.props.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let prefix = if is_folder_expanded(tree, id) { "[-]" } else { "[+]" };
                let label = format!("{} {}", prefix, name);
                draw_text(buffer, width, height, &label, rect.x + 4, rect.y + 6, COLOR_FOLDER_TEXT);
                draw_ai_dot_if_recent(buffer, width, height, &rect, obj, now_ms);
            }
            "aios.std/File@1" => {
                let is_sel = selected_id == Some(id);
                if is_sel {
                    fill_rect(buffer, width, height, &rect, COLOR_SELECTED_BG);
                }
                let name = obj.props.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let label = format!("  {}", name);
                draw_text(buffer, width, height, &label, rect.x + 4, rect.y + 4, COLOR_FILE_TEXT);
                draw_ai_dot_if_recent(buffer, width, height, &rect, obj, now_ms);
            }
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

/// FileTree.state["selected"]가 가리키는 객체 ID를 추출.
fn find_selected_in_file_tree(tree: &TreeModel) -> Option<geulos_core::ObjectId> {
    for id in tree.ids() {
        if let Some(o) = tree.get(id) {
            if o.type_uri.as_str() == "aios.builtin/FileTree@1" {
                if let Some(s) = o.state.get("selected").and_then(|v| v.as_str()) {
                    if let Ok(u) = uuid::Uuid::parse_str(s) {
                        return Some(geulos_core::ObjectId::from_uuid(u));
                    }
                }
            }
        }
    }
    None
}

/// FileTree.state["expanded"]에 folder_id가 포함되어 있는지 확인.
fn is_folder_expanded(tree: &TreeModel, folder_id: geulos_core::ObjectId) -> bool {
    for id in tree.ids() {
        if let Some(o) = tree.get(id) {
            if o.type_uri.as_str() == "aios.builtin/FileTree@1" {
                if let Some(arr) = o.state.get("expanded").and_then(|v| v.as_array()) {
                    for v in arr {
                        if let Some(s) = v.as_str() {
                            if let Ok(u) = uuid::Uuid::parse_str(s) {
                                if geulos_core::ObjectId::from_uuid(u) == folder_id {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

/// AI가 최근(5초 이내) 변경한 객체인지 판정 — 픽셀과 분리된 순수 함수.
fn is_ai_recent(actor: &str, last_change_ms: i64, now_ms: i64) -> bool {
    actor == "ai" && now_ms - last_change_ms < AI_HIGHLIGHT_MS && now_ms >= last_change_ms
}

/// AI가 최근(5초 이내) 변경한 객체면 우측에 노란 점.
fn draw_ai_dot_if_recent(
    buffer: &mut [u32],
    w: usize,
    h: usize,
    rect: &Rect,
    obj: &geulos_core::Object,
    now_ms: i64,
) {
    let actor = obj.state.get("last_change_actor").and_then(|v| v.as_str()).unwrap_or("");
    let ts = obj.state.get("last_change_ms").and_then(|v| v.as_i64()).unwrap_or(0);
    if !is_ai_recent(actor, ts, now_ms) {
        return;
    }
    let dot_x = rect.x + rect.w - 16;
    let dot_y = rect.y + rect.h / 2 - 4;
    fill_rect(buffer, w, h, &Rect { x: dot_x, y: dot_y, w: 8, h: 8 }, COLOR_AI_DOT);
}

/// 하단 CLI 패널 렌더 (T7.5).
///
/// - 검정 배경.
/// - `state.lines` 마지막 N라인을 위에서 아래로 그림 (rect에 들어가는 만큼만).
/// - 마지막에 입력 라인 `> {input_buffer}` + 깜빡이는 cursor.
/// - 출력 라인이 너무 많으면 위쪽이 잘려나가고 가장 최근 라인이 항상 입력 위에 보임.
#[allow(clippy::too_many_arguments)]
fn render_cli(
    buffer: &mut [u32],
    w: usize,
    h: usize,
    rect: &Rect,
    obj: &geulos_core::Object,
    cli_state: &CliLocalState,
    now_ms: i64,
) {
    fill_rect(buffer, w, h, rect, COLOR_CLI_BG);

    // 가용 텍스트 영역 (padding 제외).
    let text_x = rect.x + CLI_PADDING_X;
    let text_top = rect.y + CLI_PADDING_Y;
    let text_bottom = rect.y + rect.h - CLI_PADDING_Y;
    let avail_h = (text_bottom - text_top).max(0);
    let total_lines_capacity = (avail_h / CLI_LINE_HEIGHT).max(1) as usize;
    if total_lines_capacity == 0 {
        return;
    }

    // 입력 라인은 항상 마지막. 출력 라인은 그 위에 capacity-1개까지.
    let history_capacity = total_lines_capacity.saturating_sub(1);
    let lines: Vec<&str> = obj
        .state
        .get("lines")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let start = lines.len().saturating_sub(history_capacity);
    let visible = &lines[start..];

    let mut y = text_top;
    for line in visible {
        draw_text(buffer, w, h, line, text_x, y, COLOR_CLI_TEXT);
        y += CLI_LINE_HEIGHT;
    }

    // 입력 라인 — rect 하단에 고정 (출력이 없어도 prompt는 보임).
    // T7.8 (ADR-031): Cli.state.mode가 "ai"면 prompt = `[ai:<session_name>] > `, 그 외 `> `.
    // T7.9 (ADR-032): "awaiting_api_key"는 `[API key 입력] > ` — 사용자가 명령이 아닌 키를
    //   입력 중임을 시각적으로 명시.
    let prompt_y = text_bottom - CLI_LINE_HEIGHT;
    let mode = obj.state.get("mode").and_then(|v| v.as_str()).unwrap_or("shell");
    let prompt = match mode {
        "ai" => match obj.state.get("session_name").and_then(|v| v.as_str()) {
            Some(name) => format!("[ai:{}] > ", name),
            None => "[ai] > ".to_string(),
        },
        "awaiting_api_key" => "[API key 입력] > ".to_string(),
        _ => "> ".to_string(),
    };
    draw_text(buffer, w, h, &prompt, text_x, prompt_y, COLOR_CLI_PROMPT);
    let prompt_width = measure_text_width(&prompt);
    let input_x = text_x + prompt_width;
    draw_text(buffer, w, h, &cli_state.input_buffer, input_x, prompt_y, COLOR_CLI_TEXT);

    // T7.6 (ADR-029): IME 조합 중 텍스트(preedit)를 input_buffer 끝에 회색으로.
    // v1 단순화 — preedit는 cursor 위치와 무관하게 input_buffer *전체* 뒤에 그린다.
    // 사용자가 cursor를 중간으로 옮긴 채 IME 입력해도 preedit는 끝에 표시 (UX 약점, v2).
    if !cli_state.preedit_text.is_empty() {
        let input_full_width = measure_text_width(&cli_state.input_buffer);
        let preedit_x = input_x + input_full_width;
        draw_text(buffer, w, h, &cli_state.preedit_text, preedit_x, prompt_y, COLOR_CLI_PREEDIT);
    }

    // 깜빡이는 커서 — 500ms on / 500ms off.
    let blink_on = (now_ms.rem_euclid(CLI_CURSOR_BLINK_MS)) < (CLI_CURSOR_BLINK_MS / 2);
    if blink_on {
        // 커서 위치 = input_x + (cursor_pos까지의 prefix 폭).
        // cursor_pos는 항상 char boundary 위에 있으므로 multi-byte UTF-8 한글에도 안전.
        let cursor_pos = cli_state.cursor_pos.min(cli_state.input_buffer.len());
        let input_text = &cli_state.input_buffer[..cursor_pos];
        let input_width = measure_text_width(input_text);
        let cur_x = input_x + input_width;
        let cur_rect = Rect { x: cur_x, y: prompt_y + 2, w: 2, h: CLI_LINE_HEIGHT - 4 };
        fill_rect(buffer, w, h, &cur_rect, COLOR_CLI_CURSOR);
    }
}

/// Window 오버레이 렌더 — 외곽 border + title bar (focus 색 구분) + 본문 preview + [x] + resize handle.
///
/// T8.8: layout이 Window rect를 z 오름차순 마지막에 push하므로 그 위에 그려진다.
/// 본문 preview는 T7 모델 그대로 — `file.state.preview` (첫 512 bytes). M9에서 full read.
#[allow(clippy::too_many_arguments)]
fn render_window(
    buffer: &mut [u32],
    w: usize,
    h: usize,
    rect: &Rect,
    tree: &TreeModel,
    obj: &geulos_core::Object,
    focused: bool,
) {
    // 외곽 border (1px) — rect 전체를 border 색으로 칠한 뒤 inner를 BG로 덮음.
    // rect.w/h가 2 미만이면 inner의 w/h가 음수 → fill_rect가 clip하므로 안전.
    fill_rect(buffer, w, h, rect, COLOR_WINDOW_BORDER);
    let inner = Rect { x: rect.x + 1, y: rect.y + 1, w: rect.w - 2, h: rect.h - 2 };
    fill_rect(buffer, w, h, &inner, COLOR_WINDOW_BG);

    // Title bar (높이 WINDOW_TITLE_H, focus 시 짙은 파랑).
    let title_rect = Rect { x: inner.x, y: inner.y, w: inner.w, h: WINDOW_TITLE_H };
    let title_bg = if focused { COLOR_WINDOW_TITLE_BG_FOCUSED } else { COLOR_WINDOW_TITLE_BG };
    fill_rect(buffer, w, h, &title_rect, title_bg);
    let title = obj.props.get("title").and_then(|v| v.as_str()).unwrap_or("(window)");
    draw_text(buffer, w, h, title, title_rect.x + 8, title_rect.y + 4, COLOR_WINDOW_TITLE_TEXT);

    // [x] 닫기 버튼 (title bar 우상단 16×16 빨간 사각형 + "x").
    let close_rect = Rect {
        x: title_rect.x + title_rect.w - WINDOW_CLOSE_BTN - 4,
        y: title_rect.y + 4,
        w: WINDOW_CLOSE_BTN,
        h: WINDOW_CLOSE_BTN,
    };
    fill_rect(buffer, w, h, &close_rect, COLOR_WINDOW_CLOSE);
    draw_text(buffer, w, h, "x", close_rect.x + 4, close_rect.y, COLOR_WINDOW_TITLE_TEXT);

    // Content 영역 (title bar 아래 8px 패딩).
    // inner.h가 title+padding보다 작으면 content_rect.h가 음수 → 아래 max_lines = 0이 되어 텍스트 없음.
    let content_rect = Rect {
        x: inner.x + 8,
        y: inner.y + WINDOW_TITLE_H + 8,
        w: inner.w - 16,
        h: inner.h - WINDOW_TITLE_H - 16,
    };
    // file_id로 File 객체 lookup → preview 출력.
    if let Some(file_id_str) = obj.props.get("file_id").and_then(|v| v.as_str()) {
        if let Ok(uuid) = uuid::Uuid::parse_str(file_id_str) {
            let file_id = geulos_core::ObjectId::from_uuid(uuid);
            if let Some(file) = tree.get(file_id) {
                let preview = file.state.get("preview").and_then(|v| v.as_str()).unwrap_or("");
                if preview.is_empty() {
                    draw_text(
                        buffer,
                        w,
                        h,
                        "(미리보기 없음 — M9에서 full read)",
                        content_rect.x,
                        content_rect.y,
                        COLOR_PLACEHOLDER,
                    );
                } else {
                    let mut y = content_rect.y;
                    // content_rect.h가 음수면 0 lines.
                    let max_lines = (content_rect.h / 20).max(0) as usize;
                    for line in preview.lines().take(max_lines) {
                        if y + 16 > content_rect.y + content_rect.h {
                            break;
                        }
                        draw_text(buffer, w, h, line, content_rect.x, y, COLOR_TEXT);
                        y += 20;
                    }
                }
            }
        }
    }

    // Resize handle (우하 10×10 회색 사각형).
    let resize_rect = Rect {
        x: inner.x + inner.w - WINDOW_RESIZE_HANDLE,
        y: inner.y + inner.h - WINDOW_RESIZE_HANDLE,
        w: WINDOW_RESIZE_HANDLE,
        h: WINDOW_RESIZE_HANDLE,
    };
    fill_rect(buffer, w, h, &resize_rect, COLOR_WINDOW_RESIZE_HANDLE);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_recent_within_5s_returns_true() {
        // 4.5초 전 변경 → 강조 대상.
        let now = 10_000;
        let ts = now - 4_500;
        assert!(is_ai_recent("ai", ts, now));
    }

    #[test]
    fn ai_recent_at_5s_boundary_returns_false() {
        // 정확히 5초 → false (< 비교).
        let now = 10_000;
        let ts = now - 5_000;
        assert!(!is_ai_recent("ai", ts, now));
    }

    #[test]
    fn ai_recent_over_5s_returns_false() {
        let now = 10_000;
        let ts = now - 5_001;
        assert!(!is_ai_recent("ai", ts, now));
    }

    #[test]
    fn ai_recent_non_ai_actor_returns_false() {
        let now = 10_000;
        let ts = now - 100;
        assert!(!is_ai_recent("user", ts, now));
        assert!(!is_ai_recent("", ts, now));
    }

    #[test]
    fn ai_recent_future_timestamp_returns_false() {
        // 시계 어긋남(미래) — 음수 차이로 인한 오탐 방지.
        let now = 10_000;
        let ts = now + 1_000;
        assert!(!is_ai_recent("ai", ts, now));
    }
}
