//! softbuffer 픽셀 버퍼에 객체 트리 그리기.

use crate::editor::EditorState;
use crate::keyboard::CliLocalState;
use crate::layout::{HitRole, LayoutResult, Rect, EXPLORER_ROW_H};
use crate::text::{draw_text, measure_text_width};
use crate::theme;
use crate::tree_model::TreeModel;
use crate::window_geom::{WINDOW_CLOSE_BTN, WINDOW_RESIZE_HANDLE, WINDOW_TITLE_H};

const AI_HIGHLIGHT_MS: i64 = 5000;

/// Lucide 16×16 아이콘을 텍스트 baseline 시각 중심에 정렬하기 위한 y offset
/// (text_y → icon_y). fontdue 18pt 텍스트는 y_top에서 ~22px 시각 height로 그려져
/// 시각 중심이 y_top+11 부근이므로, icon (16px) top을 y_top+4에 두면 두 중심이 거의 맞는다.
/// 이 상수 없이 icon과 text를 동일 y에 두면 icon이 텍스트 baseline보다 위로 떠 보임 (사용자 보고).
const ICON_Y_OFFSET: i32 = 4;

// WINDOW_TITLE_H / WINDOW_RESIZE_HANDLE / WINDOW_CLOSE_BTN은 T8.9에서 window_geom 모듈로
// 분리됨 (render와 main.rs 입력 처리가 같은 상수를 공유해야 click 영역이 어긋나지 않는다).

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
/// `editor`는 M9 T7: edit_mode Window의 컴포지터 측 editor state. Some이고 그 window_id가
/// layout에 있으면 render_window 안에서 cursor 막대(2×18px)를 그린다.
#[allow(clippy::too_many_arguments)]
pub fn render_frame(
    tree: &TreeModel,
    layout: &LayoutResult,
    buffer: &mut [u32],
    width: usize,
    height: usize,
    cli_state: &CliLocalState,
    editor: Option<&EditorState>,
) {
    // 배경
    fill_rect(
        buffer,
        width,
        height,
        &Rect { x: 0, y: 0, w: width as i32, h: height as i32 },
        theme::SURFACE_APP,
    );

    let now_ms = chrono::Utc::now().timestamp_millis();
    let selected_id = find_selected_in_file_tree(tree);

    for (id, rect, role) in layout.iter() {
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
                fill_rect(buffer, width, height, &rect, theme::SURFACE_PANEL);
            }
            "aios.builtin/Explorer@1" => match role {
                // 상단 parent-nav 행 — active_folder 설정 시 layout이 push.
                // "/" 텍스트 + folder-open 아이콘 + 안내 문구. 약한 하늘색 배경으로 일반 폴더 행과 즉시 구분.
                HitRole::ExplorerParentNav => {
                    fill_rect(buffer, width, height, &rect, theme::ACCENT_SUBTLE);
                    // 하단 separator — 자식 행들과 시각 경계.
                    fill_rect(
                        buffer,
                        width,
                        height,
                        &Rect { x: rect.x, y: rect.y + rect.h - 1, w: rect.w, h: 1 },
                        theme::BORDER,
                    );
                    let icon = crate::icons::icon_for_file("aios.std/Folder@1", "..", "", true);
                    crate::icons::blit_icon_at(
                        buffer,
                        width,
                        height,
                        rect.x + 4,
                        rect.y + 6 + ICON_Y_OFFSET,
                        icon,
                    );
                    draw_text(
                        buffer,
                        width,
                        height,
                        "/",
                        rect.x + 24,
                        rect.y + 6,
                        theme::TEXT_PRIMARY,
                    );
                    draw_text(
                        buffer,
                        width,
                        height,
                        "(상위 폴더)",
                        rect.x + 48,
                        rect.y + 6,
                        theme::TEXT_TERTIARY,
                    );
                }
                // M8: 흰 배경. 자식 (Folder/File) line rect들은 layout이 직접 push하므로
                // 각 자식은 자기 type_uri 분기에서 그려진다 — 여기서는 별도 자식 iteration 불필요.
                _ => {
                    fill_rect(buffer, width, height, &rect, theme::SURFACE_PANEL);
                }
            },
            "aios.builtin/Cli@1" => {
                render_cli(buffer, width, height, &rect, obj, cli_state, now_ms);
            }
            "aios.builtin/Window@1" => {
                let focused = obj.state.get("focused").and_then(|v| v.as_bool()).unwrap_or(false);
                render_window(buffer, width, height, &rect, tree, obj, focused, editor);
            }
            // M13 T9: ConsoleWindow@1 — floating panel (Window@1과 동형 UI, 본문은 콘솔 로그).
            "aios.builtin/ConsoleWindow@1" => {
                render_console_window(buffer, width, height, &rect, obj);
            }
            "aios.builtin/Dialog@1" => {
                render_dialog(buffer, width, height, &rect, obj);
            }
            "aios.std/Folder@1" => {
                let is_sel = selected_id == Some(id);
                let name = obj.props.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let is_expanded = is_folder_expanded(tree, id);

                // FileTree 영역 (좌 25% 미만) vs Explorer 영역 — width 기준 휴리스틱
                // (layout::layout_desktop의 left_w = width*0.25와 일관).
                let ft_threshold = (width as f32 * 0.25) as i32;
                let in_filetree = rect.x < ft_threshold;

                // Explorer 행은 zebra + separator로 클릭 영역 명확화 (사용자 요청).
                // FileTree 행은 indent로 이미 구조 표시 — 별도 처리 없음.
                if !in_filetree {
                    draw_explorer_row_bg(buffer, width, height, &rect);
                }
                if is_sel {
                    // T4: 선택 행 RADIUS_SM 둥근 모서리. (parent-nav ACCENT_SUBTLE은 사각 유지)
                    fill_rect_rounded(
                        buffer,
                        width,
                        height,
                        &rect,
                        theme::RADIUS_SM,
                        theme::ACCENT_SUBTLE,
                    );
                }

                let icon = crate::icons::icon_for_file("aios.std/Folder@1", name, "", is_expanded);

                if in_filetree {
                    // FileTree: [+]/[-] (ExpandToggle 36px 영역) + icon (rect.x+40) + name (rect.x+60)
                    let prefix = if is_expanded { "[-]" } else { "[+]" };
                    draw_text(
                        buffer,
                        width,
                        height,
                        prefix,
                        rect.x + 4,
                        rect.y + 6,
                        theme::TEXT_PRIMARY,
                    );
                    crate::icons::blit_icon_at(
                        buffer,
                        width,
                        height,
                        rect.x + 40,
                        rect.y + 6 + ICON_Y_OFFSET,
                        icon,
                    );
                    draw_text(
                        buffer,
                        width,
                        height,
                        name,
                        rect.x + 60,
                        rect.y + 6,
                        theme::TEXT_PRIMARY,
                    );
                } else {
                    // Explorer: icon (rect.x+4) + name (rect.x+24). prefix 없음.
                    // text는 rect.y+6, icon은 baseline 정렬 위해 rect.y+6+ICON_Y_OFFSET.
                    crate::icons::blit_icon_at(
                        buffer,
                        width,
                        height,
                        rect.x + 4,
                        rect.y + 6 + ICON_Y_OFFSET,
                        icon,
                    );
                    draw_text(
                        buffer,
                        width,
                        height,
                        name,
                        rect.x + 24,
                        rect.y + 6,
                        theme::TEXT_PRIMARY,
                    );
                }
                draw_ai_dot_if_recent(buffer, width, height, &rect, obj, now_ms);
            }
            "aios.std/File@1" => {
                let is_sel = selected_id == Some(id);
                // 항상 Explorer 영역 (FileTree는 File skip) — zebra + separator.
                draw_explorer_row_bg(buffer, width, height, &rect);
                if is_sel {
                    // T4: 선택 행 RADIUS_SM 둥근 모서리.
                    fill_rect_rounded(
                        buffer,
                        width,
                        height,
                        &rect,
                        theme::RADIUS_SM,
                        theme::ACCENT_SUBTLE,
                    );
                }
                let name = obj.props.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let mime = obj
                    .props
                    .get("mime")
                    .and_then(|v| v.as_str())
                    .unwrap_or("application/octet-stream");
                let icon = crate::icons::icon_for_file("aios.std/File@1", name, mime, false);

                // File은 layout_tree_node_folders_only가 좌측에서 skip (T8.4) — Explorer 영역만.
                // text는 rect.y+6 (Folder와 통일), icon은 baseline 정렬 위해 rect.y+6+ICON_Y_OFFSET.
                crate::icons::blit_icon_at(
                    buffer,
                    width,
                    height,
                    rect.x + 4,
                    rect.y + 6 + ICON_Y_OFFSET,
                    icon,
                );
                draw_text(
                    buffer,
                    width,
                    height,
                    name,
                    rect.x + 24,
                    rect.y + 6,
                    theme::TEXT_SECONDARY,
                );
                draw_ai_dot_if_recent(buffer, width, height, &rect, obj, now_ms);
            }
            "aios.std/Container@1" => {
                fill_rect(buffer, width, height, &rect, theme::SURFACE_SUNKEN);
            }
            "aios.std/Text@1" => {
                fill_rect(buffer, width, height, &rect, theme::SURFACE_APP);
                let content =
                    obj.state.get("content").and_then(|v| v.as_str()).unwrap_or("(empty)");
                draw_text(
                    buffer,
                    width,
                    height,
                    content,
                    rect.x + 8,
                    rect.y + 8,
                    theme::TEXT_PRIMARY,
                );
            }
            "aios.std/Button@1" => {
                // T4: Button 위젯 RADIUS_SM 둥근 모서리.
                fill_rect_rounded(buffer, width, height, &rect, theme::RADIUS_SM, theme::ACCENT);
                let label = obj.state.get("label").and_then(|v| v.as_str()).unwrap_or("(button)");
                draw_text(
                    buffer,
                    width,
                    height,
                    label,
                    rect.x + 16,
                    rect.y + 16,
                    theme::TEXT_ON_ACCENT,
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
                    theme::TEXT_ON_ACCENT,
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
    fill_rect(buffer, w, h, &Rect { x: dot_x, y: dot_y, w: 8, h: 8 }, theme::STATUS_AI_DOT);
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
    fill_rect(buffer, w, h, rect, theme::TERMINAL_BG);

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
    // scroll_offset만큼 bottom에서 위로 스크롤 (이전 출력 확인용). 초과 시 자연 clamp.
    let history_capacity = total_lines_capacity.saturating_sub(1);
    let lines: Vec<&str> = obj
        .state
        .get("lines")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    // 긴 라인 자동 wrap (사용자 보고: AI 답변이 가로로 잘림). 각 logical line을
    // CLI 텍스트 폭에 맞춰 visual line으로 분해 — Window 본문과 동일한 fontdue 기반.
    // 4px 우측 margin으로 우측 끝 글자가 패널 경계에 닿지 않게.
    let cli_wrap_w = (rect.x + rect.w - CLI_PADDING_X - text_x - 4).max(20);
    let wrapped_lines: Vec<String> = lines
        .iter()
        .flat_map(|line| {
            crate::editor::wrap_by_pixel_width(line, cli_wrap_w).into_iter().map(|w| w.text)
        })
        .collect();
    // wrap된 visual line 기준 scroll + bottom-trim.
    let scroll = cli_state.scroll_offset.min(wrapped_lines.len().saturating_sub(history_capacity));
    let end = wrapped_lines.len().saturating_sub(scroll);
    let start = end.saturating_sub(history_capacity);
    let visible = &wrapped_lines[start..end];

    let mut y = text_top;
    for line in visible {
        draw_text(buffer, w, h, line, text_x, y, theme::TERMINAL_TEXT);
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
    draw_text(buffer, w, h, &prompt, text_x, prompt_y, theme::TERMINAL_PROMPT);
    let prompt_width = measure_text_width(&prompt);
    let input_x = text_x + prompt_width;
    draw_text(buffer, w, h, &cli_state.input_buffer, input_x, prompt_y, theme::TERMINAL_TEXT);

    // T7.6 (ADR-029): IME 조합 중 텍스트(preedit)를 input_buffer 끝에 회색으로.
    // v1 단순화 — preedit는 cursor 위치와 무관하게 input_buffer *전체* 뒤에 그린다.
    // 사용자가 cursor를 중간으로 옮긴 채 IME 입력해도 preedit는 끝에 표시 (UX 약점, v2).
    if !cli_state.preedit_text.is_empty() {
        let input_full_width = measure_text_width(&cli_state.input_buffer);
        let preedit_x = input_x + input_full_width;
        draw_text(buffer, w, h, &cli_state.preedit_text, preedit_x, prompt_y, theme::TERMINAL_DIM);
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
        fill_rect(buffer, w, h, &cur_rect, theme::TERMINAL_TEXT);
    }
}

/// Window 오버레이 렌더 — 외곽 border + title bar (focus 색 구분) + 본문 text + [x] + resize handle.
///
/// T8.8: layout이 Window rect를 z 오름차순 마지막에 push하므로 그 위에 그려진다.
/// M8 T8.15 (ADR-033): 본문은 더 이상 `file.state.preview`가 아니라 *Window.state.content*에서
/// 직접 읽는다. desktop-shell이 open_file 시점에 file_read::read_file_for_window 결과를
/// Window mount 페이로드에 채워 보내므로 (T8.14), 컴포지터는 file_id로 File을 lookup할 필요 없다.
/// 라인 단위 `scroll_y`로 가시 영역 clip + 긴 줄은 word wrap (T8.19).
#[allow(clippy::too_many_arguments)]
fn render_window(
    buffer: &mut [u32],
    w: usize,
    h: usize,
    rect: &Rect,
    _tree: &TreeModel, // M8 T8.15: file_id로 File lookup 더 이상 안 함. signature는 안정 위해 보존.
    obj: &geulos_core::Object,
    focused: bool,
    editor: Option<&EditorState>,
) {
    // 외곽 border (1px) — rect 전체를 border 색으로 칠한 뒤 inner를 BG로 덮음.
    // rect.w/h가 2 미만이면 inner의 w/h가 음수 → fill_rect가 clip하므로 안전.
    // T4: RADIUS_MD 둥근 모서리 — border(외곽)와 inner 모두 같은 radius로 1px 감싸기 형태 유지.
    fill_rect_rounded(buffer, w, h, rect, theme::RADIUS_MD, theme::BORDER);
    let inner = Rect { x: rect.x + 1, y: rect.y + 1, w: rect.w - 2, h: rect.h - 2 };
    fill_rect_rounded(buffer, w, h, &inner, theme::RADIUS_MD, theme::SURFACE_ELEVATED);

    // Title bar (높이 WINDOW_TITLE_H, focus 시 짙은 파랑).
    let title_rect = Rect { x: inner.x, y: inner.y, w: inner.w, h: WINDOW_TITLE_H };
    let title_bg = if focused { theme::ACCENT_HOVER } else { theme::ACCENT };
    fill_rect(buffer, w, h, &title_rect, title_bg);
    let dirty = obj.state.get("dirty").and_then(|v| v.as_bool()).unwrap_or(false);
    let raw_title = obj.props.get("title").and_then(|v| v.as_str()).unwrap_or("(window)");
    let title = if dirty { format!("* {}", raw_title) } else { raw_title.to_string() };
    draw_text(buffer, w, h, &title, title_rect.x + 8, title_rect.y + 4, theme::TEXT_ON_ACCENT);

    // [x] 닫기 버튼 (title bar 우상단 16×16 빨간 사각형 + "x").
    let close_rect = Rect {
        x: title_rect.x + title_rect.w - WINDOW_CLOSE_BTN - 4,
        y: title_rect.y + 4,
        w: WINDOW_CLOSE_BTN,
        h: WINDOW_CLOSE_BTN,
    };
    // T4: close 버튼 RADIUS_SM 둥근 모서리.
    fill_rect_rounded(buffer, w, h, &close_rect, theme::RADIUS_SM, theme::CLOSE_BUTTON);
    draw_text(buffer, w, h, "x", close_rect.x + 4, close_rect.y, theme::TEXT_ON_ACCENT);

    // Content 영역 (title bar 아래 8px 패딩).
    // inner.h가 title+padding보다 작으면 content_rect.h가 음수 → 아래 visible_lines = 0이 되어 텍스트 없음.
    let content_rect = Rect {
        x: inner.x + 8,
        y: inner.y + WINDOW_TITLE_H + 8,
        w: inner.w - 16,
        h: inner.h - WINDOW_TITLE_H - 16,
    };

    // editor가 이 Window의 활성 editor면 *editor.content를 우선 source*로 사용 (local-master).
    // 키 입력 시 매번 wire 갱신 없이도 화면 즉시 반영. editor가 없거나 다른 Window면
    // server-side Window.state.content fallback (viewer/AI write 결과 등).
    let editor_active = editor.map(|e| e.window_id == obj.id).unwrap_or(false);
    let server_content = obj.state.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let content: &str = if editor_active {
        editor.map(|e| e.content.as_str()).unwrap_or(server_content)
    } else {
        server_content
    };
    let too_large = obj.state.get("content_too_large").and_then(|v| v.as_bool()).unwrap_or(false);
    let scroll_y = obj.state.get("scroll_y").and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;

    const LINE_HEIGHT: i32 = 20;
    let visible_lines = (content_rect.h / LINE_HEIGHT).max(0) as usize;

    // fontdue advance 기반 wrap — char width 휴리스틱(14)이 한글에서 어긋나 cursor/render
    // 불일치를 일으켜 *editor.rs의 wrap_by_pixel_width로 통일*. render와 click이 동일 wrap을
    // 사용하므로 cursor 시각 위치와 click hit가 정확히 일치.
    //
    // wrap_by_pixel_width가 measure_text_width로 *정확*하게 wrap하므로 margin 4px이면 충분.
    let wrap_w = (content_rect.w - 4).max(20);
    let wrapped = crate::editor::wrap_by_pixel_width(content, wrap_w);

    if content.is_empty() && !editor_active {
        draw_text(
            buffer,
            w,
            h,
            "(빈 파일 또는 viewer 미지원)",
            content_rect.x,
            content_rect.y,
            theme::TEXT_TERTIARY,
        );
    } else {
        let total = wrapped.len();
        let start = scroll_y.min(total.saturating_sub(visible_lines));
        let end = (start + visible_lines).min(total);

        let mut y = content_rect.y;
        for line in &wrapped[start..end] {
            draw_text(buffer, w, h, &line.text, content_rect.x, y, theme::TEXT_PRIMARY);
            y += LINE_HEIGHT;
        }

        // 1MB 초과 안내 — content 끝까지 스크롤한 경우만.
        if too_large && end == total && y + LINE_HEIGHT <= content_rect.y + content_rect.h {
            draw_text(
                buffer,
                w,
                h,
                "[파일이 1MB 초과 — 일부만 표시]",
                content_rect.x,
                y,
                theme::TEXT_TERTIARY,
            );
        }
    }

    // Window는 *항상 편집 가능* (메모장 UX) — focused Window가 editor 대상이면 cursor 그림.
    // cursor x는 *그 line의 prefix를 fontdue로 measure*해서 산출 — 한글 mix에서도 정확.
    let _ = focused;
    if editor_active {
        if let Some(ed) = editor {
            let (line_idx, in_line_byte) =
                crate::editor::line_and_byte_in_line(&wrapped, ed.cursor);
            let visible_line = line_idx as i32 - scroll_y as i32;
            let visible_capacity = (content_rect.h / LINE_HEIGHT).max(0);
            if visible_line >= 0 && visible_line < visible_capacity {
                let prefix =
                    &wrapped[line_idx].text[..in_line_byte.min(wrapped[line_idx].text.len())];
                let cx_px = content_rect.x + measure_text_width(prefix);
                let cy_px = content_rect.y + visible_line * LINE_HEIGHT + 2;
                fill_rect(
                    buffer,
                    w,
                    h,
                    &Rect { x: cx_px, y: cy_px, w: 2, h: 18 },
                    theme::TEXT_PRIMARY,
                );
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
    fill_rect(buffer, w, h, &resize_rect, theme::BORDER_STRONG);
}

/// ConsoleWindow@1 오버레이 렌더 — Window@1 패턴 mirror + 콘솔 로그 본문.
///
/// M13 T9:
/// - geometry: state.x/y/w/h (Window@1과 동일 — ConsoleWindow는 state에 geometry 저장).
/// - titlebar: props.title (desktop-shell이 "cmd args — dir" 형식으로 생성) + status dot.
/// - status dot: state.status에 따른 색상 (running=초록, exited=회색, terminated=빨강, error=주황).
/// - 본문: state.lines 배열을 monospace 줄 단위 렌더. "[stderr] " 접두 줄은 연한 빨강.
/// - scroll_y: state.scroll_y offset 적용.
/// - X 버튼: Window@1과 동일 위치/크기.
/// - resize handle: Window@1과 동일.
fn render_console_window(
    buffer: &mut [u32],
    w: usize,
    h: usize,
    rect: &Rect,
    obj: &geulos_core::Object,
) {
    // 외곽 border (1px) + 내부 배경 (단말 색).
    // T4: RADIUS_MD 둥근 모서리 — Window@1과 동일 패턴.
    fill_rect_rounded(buffer, w, h, rect, theme::RADIUS_MD, theme::BORDER);
    let inner = Rect { x: rect.x + 1, y: rect.y + 1, w: rect.w - 2, h: rect.h - 2 };
    fill_rect_rounded(buffer, w, h, &inner, theme::RADIUS_MD, theme::TERMINAL_BG);

    // Title bar — Window@1과 동일 높이(WINDOW_TITLE_H). focused state는 ConsoleWindow엔 없으므로
    // 항상 unfocused 색 사용 (짙은 blue 고정 — v1 단순화).
    let title_rect = Rect { x: inner.x, y: inner.y, w: inner.w, h: WINDOW_TITLE_H };
    fill_rect(buffer, w, h, &title_rect, theme::ACCENT);

    // props.title — desktop-shell이 "cmd args — dir" 형식으로 mount 시 설정.
    let title = obj.props.get("title").and_then(|v| v.as_str()).unwrap_or("(console)");
    draw_text(buffer, w, h, title, title_rect.x + 8, title_rect.y + 4, theme::TEXT_ON_ACCENT);

    // status dot — title bar 우측에 8×8 사각형.
    // X 버튼보다 왼쪽에 배치 (WINDOW_CLOSE_BTN + 4 + 8 + 4 = 32px 여백).
    let status = obj.state.get("status").and_then(|v| v.as_str()).unwrap_or("running");
    let dot_color = match status {
        "running" => theme::STATUS_RUNNING,
        "exited" => theme::STATUS_EXITED,
        "terminated" => theme::STATUS_TERMINATED,
        "error" => theme::STATUS_ERROR,
        _ => theme::STATUS_EXITED,
    };
    let dot_size = 8i32;
    let dot_x = title_rect.x + title_rect.w - WINDOW_CLOSE_BTN - 4 - dot_size - 6;
    let dot_y = title_rect.y + (WINDOW_TITLE_H - dot_size) / 2;
    fill_rect(buffer, w, h, &Rect { x: dot_x, y: dot_y, w: dot_size, h: dot_size }, dot_color);

    // [x] 닫기 버튼 — Window@1과 동일 위치/크기.
    let close_rect = Rect {
        x: title_rect.x + title_rect.w - WINDOW_CLOSE_BTN - 4,
        y: title_rect.y + 4,
        w: WINDOW_CLOSE_BTN,
        h: WINDOW_CLOSE_BTN,
    };
    // T4: close 버튼 RADIUS_SM 둥근 모서리.
    fill_rect_rounded(buffer, w, h, &close_rect, theme::RADIUS_SM, theme::CLOSE_BUTTON);
    draw_text(buffer, w, h, "x", close_rect.x + 4, close_rect.y, theme::TEXT_ON_ACCENT);

    // Content 영역 (title bar 아래 8px 패딩).
    let content_rect = Rect {
        x: inner.x + 8,
        y: inner.y + WINDOW_TITLE_H + 8,
        w: inner.w - 16,
        h: inner.h - WINDOW_TITLE_H - 16,
    };

    // state.lines 배열에서 줄 목록 추출.
    let lines: Vec<&str> = obj
        .state
        .get("lines")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    const CONSOLE_LINE_H: i32 = 20;
    let scroll_y = obj.state.get("scroll_y").and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
    // .max(1) — content_h < CONSOLE_LINE_H 시 visible_lines=0으로 빈 화면 방지.
    // Window@1의 visible_lines max(0) (line 559) 및 max_scroll_y_for ConsoleWindow visible max(1)과 일관.
    let visible_lines = (content_rect.h / CONSOLE_LINE_H).max(1) as usize;

    // scroll_y offset 적용 — lines[scroll_y..scroll_y+visible_lines].
    let total = lines.len();
    let start = scroll_y.min(total.saturating_sub(visible_lines));
    let end = (start + visible_lines).min(total);

    if lines.is_empty() {
        draw_text(
            buffer,
            w,
            h,
            "(출력 없음)",
            content_rect.x,
            content_rect.y,
            theme::TEXT_TERTIARY,
        );
    } else {
        let mut y = content_rect.y;
        for line in &lines[start..end] {
            // "[stderr] " 접두 줄은 연한 빨강, 그 외 일반 색.
            let color = if line.starts_with("[stderr] ") {
                theme::TERMINAL_STDERR
            } else {
                theme::TERMINAL_TEXT
            };
            draw_text(buffer, w, h, line, content_rect.x, y, color);
            y += CONSOLE_LINE_H;
        }
    }

    // Resize handle (우하 10×10 회색 사각형) — Window@1과 동일.
    let resize_rect = Rect {
        x: inner.x + inner.w - WINDOW_RESIZE_HANDLE,
        y: inner.y + inner.h - WINDOW_RESIZE_HANDLE,
        w: WINDOW_RESIZE_HANDLE,
        h: WINDOW_RESIZE_HANDLE,
    };
    fill_rect(buffer, w, h, &resize_rect, theme::BORDER_STRONG);
}

/// Dialog@1 모달 렌더 — 화면 중앙 박스 + title + message + buttons.
///
/// rect는 layout이 산출한 *Dialog 자체 rect* (예: 화면 중앙 400×200). 클릭 hit는 main의
/// 자체 영역 분석으로 처리 (Window 패턴과 동일) — 여기서는 그리기만.
fn render_dialog(buffer: &mut [u32], w: usize, h: usize, rect: &Rect, obj: &geulos_core::Object) {
    // 외곽 border + 내부 BG (Window 박스 패턴 재사용).
    // T4: RADIUS_MD 둥근 모서리 — Window@1/ConsoleWindow@1과 동일 패턴.
    fill_rect_rounded(buffer, w, h, rect, theme::RADIUS_MD, theme::BORDER);
    let inner = Rect { x: rect.x + 1, y: rect.y + 1, w: rect.w - 2, h: rect.h - 2 };
    fill_rect_rounded(buffer, w, h, &inner, theme::RADIUS_MD, theme::SURFACE_ELEVATED);

    let title = obj.props.get("title").and_then(|v| v.as_str()).unwrap_or("(dialog)");
    draw_text(buffer, w, h, title, inner.x + 12, inner.y + 12, theme::TEXT_PRIMARY);

    let message = obj.props.get("message").and_then(|v| v.as_str()).unwrap_or("");
    draw_text(buffer, w, h, message, inner.x + 12, inner.y + 44, theme::TEXT_PRIMARY);

    let actions = obj.props.get("actions").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let n = actions.len().max(1);
    let btn_w = 100i32;
    let btn_h = 32i32;
    let gap = 12i32;
    let total_w = n as i32 * btn_w + (n as i32 - 1) * gap;
    let mut bx = inner.x + (inner.w - total_w) / 2;
    let by = inner.y + inner.h - btn_h - 12;
    for a in &actions {
        let label = a.as_str().unwrap_or("?");
        let br = Rect { x: bx, y: by, w: btn_w, h: btn_h };
        // T4: Dialog 액션 버튼 RADIUS_SM 둥근 모서리.
        fill_rect_rounded(buffer, w, h, &br, theme::RADIUS_SM, theme::ACCENT);
        draw_text(buffer, w, h, label, br.x + 12, br.y + 6, theme::TEXT_ON_ACCENT);
        bx += btn_w + gap;
    }
}

/// Explorer 자식 행 배경 — zebra (짝/홀수 행 alternating) + 1px 하단 separator.
/// 사용자가 *어디까지가 한 행*인지 즉시 파악할 수 있도록 하기 위한 시각 보조.
///
/// rect.y가 음수일 수 있어 (scroll), `div_euclid`/`rem_euclid`로 안전한 modulo 계산.
/// stride는 `layout::EXPLORER_ROW_H` — 두 값이 어긋나면 zebra가 행 단위가 아닌 픽셀 단위로 깜빡임.
fn draw_explorer_row_bg(buffer: &mut [u32], w: usize, h: usize, rect: &Rect) {
    let idx = rect.y.div_euclid(EXPLORER_ROW_H).rem_euclid(2);
    let bg = if idx == 0 { theme::SURFACE_PANEL } else { theme::SURFACE_APP };
    fill_rect(buffer, w, h, rect, bg);
    fill_rect(
        buffer,
        w,
        h,
        &Rect { x: rect.x, y: rect.y + rect.h - 1, w: rect.w, h: 1 },
        theme::BORDER,
    );
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

/// 둥근 모서리 사각형. radius=0이면 fill_rect와 동일. 4 corner 영역만 픽셀별
/// 거리 판정 + anti-alias(blend_argb). 본체(corner 제외)는 통째로 채운다.
///
/// corner 중심에서 픽셀 거리 d:
/// - d <= r-0.5  → 불투명
/// - r-0.5 < d <= r+0.5 → alpha = (r+0.5-d) 비례 blend (AA edge)
/// - d > r+0.5  → skip (배경 유지)
pub fn fill_rect_rounded(
    buffer: &mut [u32],
    w: usize,
    h: usize,
    r: &Rect,
    radius: i32,
    color: u32,
) {
    let radius = radius.clamp(0, (r.w.min(r.h) / 2).max(0));
    if radius == 0 {
        fill_rect(buffer, w, h, r, color);
        return;
    }
    let x0 = r.x.max(0);
    let y0 = r.y.max(0);
    let x1 = (r.x + r.w).min(w as i32);
    let y1 = (r.y + r.h).min(h as i32);
    let cl = r.x + radius;
    let cr = r.x + r.w - 1 - radius;
    let ct = r.y + radius;
    let cb = r.y + r.h - 1 - radius;
    for py in y0..y1 {
        for px in x0..x1 {
            let in_left = px < cl;
            let in_right = px > cr;
            let in_top = py < ct;
            let in_bottom = py > cb;
            let (cx, cy) = match (in_left, in_right, in_top, in_bottom) {
                (true, _, true, _) => (cl, ct),
                (true, _, _, true) => (cl, cb),
                (_, true, true, _) => (cr, ct),
                (_, true, _, true) => (cr, cb),
                _ => {
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
                let a = ((rf + 0.5 - dist) * 255.0).clamp(0.0, 255.0) as u8;
                buffer[idx] = crate::text::blend_argb(buffer[idx], color, a);
            }
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

    // cursor_pixel_pos는 fontdue-기반 wrap_by_pixel_width + line_and_byte_in_line으로 대체됨
    // (editor.rs). 해당 테스트도 editor::tests로 이동.

    #[test]
    fn fill_rect_rounded_radius_zero_fills_corners() {
        let w = 10usize;
        let h = 10usize;
        let mut buf = vec![0xFF_00_00_00u32; w * h];
        let rect = Rect { x: 0, y: 0, w: 10, h: 10 };
        fill_rect_rounded(&mut buf, w, h, &rect, 0, 0xFF_FF_FF_FF);
        assert_eq!(buf[0], 0xFF_FF_FF_FF, "radius=0이면 corner도 채워져야");
    }

    #[test]
    fn fill_rect_rounded_clips_corner_pixel() {
        let w = 10usize;
        let h = 10usize;
        let bg = 0xFF_00_00_00u32;
        let mut buf = vec![bg; w * h];
        let rect = Rect { x: 0, y: 0, w: 10, h: 10 };
        fill_rect_rounded(&mut buf, w, h, &rect, 4, 0xFF_FF_FF_FF);
        assert_eq!(buf[0], bg, "corner 바깥 픽셀은 배경 유지");
        assert_eq!(buf[5 * w + 5], 0xFF_FF_FF_FF, "중앙은 채워짐");
    }

    #[test]
    fn fill_rect_rounded_large_radius_no_panic() {
        let w = 6usize;
        let h = 6usize;
        let mut buf = vec![0xFF_00_00_00u32; w * h];
        let rect = Rect { x: 0, y: 0, w: 6, h: 6 };
        fill_rect_rounded(&mut buf, w, h, &rect, 100, 0xFF_FF_FF_FF);
        assert_eq!(buf[3 * w + 3], 0xFF_FF_FF_FF);
    }

    #[test]
    fn fill_rect_rounded_aa_band_blends() {
        // AA 밴드 회귀 가드 — corner edge 픽셀이 *부분 blend*인지 검증.
        // 10x10 radius=4 → 좌상 corner 중심 (4,4). (1,1)은 dist=√18≈4.24 ∈ (3.5, 4.5]
        // → AA blend (alpha≈66). 공식 (rf+0.5-dist)가 (rf-dist)로 바뀌면 dist>rf라 skip되어
        // bg(검정, R=0) 유지 → 이 test 실패로 회귀 탐지.
        let w = 10usize;
        let h = 10usize;
        let bg = 0xFF_00_00_00u32;
        let mut buf = vec![bg; w * h];
        let rect = Rect { x: 0, y: 0, w: 10, h: 10 };
        fill_rect_rounded(&mut buf, w, h, &rect, 4, 0xFF_FF_FF_FF);
        let r = (buf[w + 1] >> 16) & 0xFF; // (1,1) 픽셀의 R 채널
        assert!(r > 0 && r < 0xFF, "AA 밴드 픽셀은 부분 blend (0<R<255), got R={:#X}", r);
    }
}
