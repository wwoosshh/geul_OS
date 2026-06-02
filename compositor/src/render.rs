//! softbuffer 픽셀 버퍼에 객체 트리 그리기.

use crate::editor::EditorState;
use crate::keyboard::CliLocalState;
use crate::layout::{HitRole, LayoutResult, Rect};
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

/// "#RRGGBB" 색 문자열 → ARGB u32 (alpha 0xFF). 파싱 실패 시 None.
/// Desktop.state.wallpaper 같은 hex 색 토큰 파싱용. "#" 접두는 선택.
fn parse_hex_color(s: &str) -> Option<u32> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 {
        return None;
    }
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some(0xFF_00_00_00 | rgb)
}
/// 커서 깜빡임 주기 (ms) — 1초 (500ms on / 500ms off).
const CLI_CURSOR_BLINK_MS: i64 = 1000;

/// F2 인라인 이름변경 오버레이 — selected row 위에 흰 입력박스 + buffer + 캐럿.
///
/// 컴포지터 main이 [Rename] 클릭 시 채우고, Enter/Esc로 비운다. layout의 target_id Body rect
/// 위에 덮어 그려 Windows 탐색기와 같은 인라인 편집 UX 제공.
pub struct RenameOverlay {
    pub target_id: geulos_core::ObjectId,
    pub buffer: String,
}

/// 한 프레임을 그린다.
///
/// `cli_state`는 컴포지터-사이드 CLI 입력 버퍼/커서. Cli 객체가 layout에 있을 때만 사용된다.
/// `editor`는 M9 T7: edit_mode Window의 컴포지터 측 editor state. Some이고 그 window_id가
/// layout에 있으면 render_window 안에서 cursor 막대(2×18px)를 그린다.
/// `rename`은 F2 인라인 이름변경 — Some이면 target_id의 Body rect를 흰 박스+텍스트로 덮는다.
#[allow(clippy::too_many_arguments)]
pub fn render_frame(
    tree: &TreeModel,
    layout: &LayoutResult,
    buffer: &mut [u32],
    width: usize,
    height: usize,
    cli_state: &CliLocalState,
    editor: Option<&EditorState>,
    rename: Option<&RenameOverlay>,
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
    let explorer_selected_id = find_selected_in_explorer(tree);

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
                // SP1: 중앙 바탕화면 영역(TopBar 아래 ~ CLI 위, 독 제외)을 wallpaper 색으로 채움.
                // 색 = Desktop.state.wallpaper("#RRGGBB"). 파싱 실패 시 theme bg 폴백.
                let cli_height = obj
                    .state
                    .get("cli_height")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(crate::layout::CLI_DEFAULT_H as i64)
                    as i32;
                let has_cli = obj.children.iter().any(|&cid| {
                    tree.get(cid).map(|o| o.type_uri.as_str()) == Some("aios.builtin/Cli@1")
                });
                let regions =
                    crate::layout::desktop_regions(width as i32, height as i32, cli_height);
                // CLI 없으면 중앙 영역을 화면 하단까지 (layout_desktop의 mid 계산과 일치).
                let desk = if has_cli {
                    regions.desktop
                } else {
                    Rect {
                        x: regions.desktop.x,
                        y: regions.desktop.y,
                        w: regions.desktop.w,
                        h: height as i32 - crate::layout::TOPBAR_H,
                    }
                };
                let bg = obj
                    .state
                    .get("wallpaper")
                    .and_then(|v| v.as_str())
                    .and_then(parse_hex_color)
                    .unwrap_or(theme::SURFACE_APP);
                fill_rect(buffer, width, height, &desk, bg);
            }
            "aios.builtin/TopBar@1" => match role {
                // 항목 칸 — TopBarItem rect 위에 라벨. 배경은 Body 분기가 이미 칠함.
                HitRole::TopBarItem => {
                    // TopBar item은 x=0에서 좌→우로 TOPBAR_ITEM_W 칸. idx = rect.x / W.
                    let idx = (rect.x.max(0) / crate::layout::TOPBAR_ITEM_W) as usize;
                    let label = obj
                        .state
                        .get("items")
                        .and_then(|v| v.as_array())
                        .and_then(|items| items.get(idx))
                        .and_then(|it| it.get("label").and_then(|l| l.as_str()))
                        .unwrap_or("");
                    draw_text(
                        buffer,
                        width,
                        height,
                        label,
                        rect.x + theme::SPACE_MD,
                        rect.y + 6,
                        theme::TEXT_PRIMARY,
                    );
                }
                // 바 배경 + "GeulOS [활성앱]" 좌측 + 우측 정렬 시계.
                _ => {
                    fill_rect(buffer, width, height, &rect, theme::SURFACE_PANEL);
                    // 하단 1px separator.
                    fill_rect(
                        buffer,
                        width,
                        height,
                        &Rect { x: rect.x, y: rect.y + rect.h - 1, w: rect.w, h: 1 },
                        theme::BORDER,
                    );
                    // 활성앱(포커스된 부유 창) 제목 — TopBar item 칸들(x=0부터 TOPBAR_ITEM_W씩)
                    // *뒤에* 표시. "GeulOS" 브랜드는 item 라벨(TopBarItem 분기)이 이미 그리므로
                    // 여기서 다시 그리지 않는다(중복 방지). macOS 메뉴바처럼 현재 활성 앱만 추가.
                    let n_items = obj
                        .state
                        .get("items")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.len())
                        .unwrap_or(0) as i32;
                    let text_y = rect.y + 6;
                    if let Some(app_title) = find_focused_window_title(tree) {
                        let after_items_x =
                            rect.x + n_items * crate::layout::TOPBAR_ITEM_W + theme::SPACE_MD;
                        let sep = "|  ";
                        draw_text(buffer, width, height, sep, after_items_x, text_y, theme::TEXT_TERTIARY);
                        let title_x = after_items_x + measure_text_width(sep);
                        draw_text(buffer, width, height, app_title, title_x, text_y, theme::TEXT_PRIMARY);
                    }
                    let clock = obj.state.get("clock").and_then(|v| v.as_str()).unwrap_or("");
                    if !clock.is_empty() {
                        let cw = measure_text_width(clock);
                        draw_text(
                            buffer,
                            width,
                            height,
                            clock,
                            rect.x + rect.w - cw - theme::SPACE_MD,
                            rect.y + 6,
                            theme::TEXT_SECONDARY,
                        );
                    }
                }
            },
            "aios.builtin/Dock@1" => match role {
                // 항목 칸 — DockItem rect 중앙에 아이콘.
                HitRole::DockItem => {
                    let idx = (((rect.y - crate::layout::TOPBAR_H).max(0))
                        / crate::layout::DOCK_ITEM_H) as usize;
                    let icon_name = obj
                        .state
                        .get("items")
                        .and_then(|v| v.as_array())
                        .and_then(|items| items.get(idx))
                        .and_then(|it| it.get("icon").and_then(|i| i.as_str()))
                        .unwrap_or("");
                    let kind = crate::icons::icon_kind_for_name(icon_name);
                    // 16×16 아이콘을 칸 중앙에 정렬.
                    let ix = rect.x + (rect.w - crate::icons::ICON_SIZE as i32) / 2;
                    let iy = rect.y + (rect.h - crate::icons::ICON_SIZE as i32) / 2;
                    crate::icons::blit_icon_at(buffer, width, height, ix, iy, kind);
                }
                // 독 배경 패널.
                _ => {
                    fill_rect(buffer, width, height, &rect, theme::SURFACE_PANEL);
                    // 좌측 1px separator (바탕화면과 경계).
                    fill_rect(
                        buffer,
                        width,
                        height,
                        &Rect { x: rect.x, y: rect.y, w: 1, h: rect.h },
                        theme::BORDER,
                    );
                }
            },
            "aios.builtin/DesktopIcon@1" => {
                // OS풍 바탕화면 아이콘: 큰 아이콘(nearest-neighbor 확대) + 가로 중앙 라벨.
                //
                // 박스: ICON_BOX_W(64) × ICON_BOX_H(72).
                // 아이콘: 40×40 nearest-neighbor 확대, 박스 상단 중앙 정렬.
                //   - 상단 여백 = SPACE_SM(8) → iy = rect.y + 8
                //   - 수평 중앙 = rect.x + (64 - 40) / 2 = rect.x + 12
                // 라벨: 아이콘 아래 4px 간격, 가로 중앙 정렬.
                //   - 어두운 바탕화면 위 가독성: 흰 텍스트(TEXT_ON_ACCENT) + 1px 그림자 효과
                //   - 라벨이 박스보다 넓으면 오른쪽 끝에서 clamp (truncate approximation).
                let icon_name = obj.props.get("icon").and_then(|v| v.as_str()).unwrap_or("");
                let label = obj.props.get("label").and_then(|v| v.as_str()).unwrap_or("");
                let kind = crate::icons::icon_kind_for_name(icon_name);

                const DESKTOP_ICON_SIZE: i32 = 40;
                const DESKTOP_ICON_TOP_PAD: i32 = theme::SPACE_SM; // 8px

                let ix = rect.x + (rect.w - DESKTOP_ICON_SIZE) / 2;
                let iy = rect.y + DESKTOP_ICON_TOP_PAD;
                // 어두운 바탕화면 위에서 어두운 선화 아이콘이 묻히므로 밝게 틴트.
                crate::icons::blit_icon_scaled_tinted(
                    buffer,
                    width,
                    height,
                    ix,
                    iy,
                    kind,
                    DESKTOP_ICON_SIZE,
                    theme::TEXT_ON_ACCENT,
                );

                // 라벨 — 아이콘 아래 SPACE_XS(4px) 간격, 박스 가로 중앙.
                if !label.is_empty() {
                    let lw = measure_text_width(label);
                    // 라벨이 박스를 넘으면 시작 x를 rect.x + 2로 clamp.
                    let lx = if lw >= rect.w {
                        rect.x + 2
                    } else {
                        rect.x + (rect.w - lw) / 2
                    };
                    let ly = iy + DESKTOP_ICON_SIZE + theme::SPACE_XS;
                    // 1px 어두운 그림자 (가독성): 검은 텍스트를 (lx+1, ly+1)에 먼저.
                    draw_text(
                        buffer,
                        width,
                        height,
                        label,
                        lx + 1,
                        ly + 1,
                        0xFF_00_00_00, // 검정 shadow
                    );
                    // 실제 텍스트: 흰색 (어두운 바탕화면 위 고대비).
                    draw_text(buffer, width, height, label, lx, ly, theme::TEXT_ON_ACCENT);
                }
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
            // SP1: FileManager@1 — 창 프레임(테두리/타이틀바/닫기/리사이즈 + 본문 배경)만 그린다.
            // 좌측 FileTree / 우측 Explorer 컬럼 및 각 Folder/File 행은 layout이 push한 rect를 따라
            // 각자의 type_uri 분기에서 (이 창 arm 이후 정순으로) 그려진다.
            "aios.builtin/FileManager@1" => {
                // FM은 Body + 4 toolbar role로 layout에 push됨 (4 toolbar는 hit_test용).
                // chrome은 Body 한 번만 그림 — 안 그러면 4번 chrome 중첩 렌더 버그.
                if role == crate::layout::HitRole::Body {
                    let focused =
                        obj.state.get("focused").and_then(|v| v.as_bool()).unwrap_or(false);
                    render_file_manager(buffer, width, height, &rect, obj, focused);
                }
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
                let is_explorer_sel = explorer_selected_id == Some(id);
                let name = obj.props.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let is_expanded = is_folder_expanded(tree, id);

                // FileTree 영역 vs Explorer 영역 판정.
                // SP1: FileTree/Explorer는 FileManager@1 창 본문(좌 30%/우 70%)에만 존재한다.
                // layout과 동일한 분할식(file_panel_split_x)을 공유하는 point_in_file_tree_column으로
                // 이 행이 어느 창의 어느 컬럼인지 판정 — 창이 화면 임의 위치에 떠 있어도 정확하다.
                // 어떤 FileManager 창에도 안 드는 경우(이론상 없음)는 Explorer 스타일로 폴백.
                let in_filetree = crate::layout::point_in_file_tree_column(
                    tree,
                    rect.x + rect.w / 2,
                    rect.y + rect.h / 2,
                )
                .unwrap_or(false);

                // Explorer 행은 단색 배경 + BORDER separator로 행 영역 명확화 (T6: zebra 제거).
                // FileTree 행은 indent로 이미 구조 표시 — 별도 처리 없음.
                if !in_filetree {
                    draw_explorer_row_bg(buffer, width, height, &rect);
                }
                // Explorer.selected_item 하이라이트 (단일클릭 select, v1.5) — 텍스트/아이콘보다 먼저.
                if !in_filetree && is_explorer_sel {
                    fill_rect(buffer, width, height, &rect, theme::ACCENT_SUBTLE);
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
                let is_explorer_sel = explorer_selected_id == Some(id);
                // 항상 Explorer 영역 (FileTree는 File skip) — 단색 배경 + BORDER separator (T6).
                draw_explorer_row_bg(buffer, width, height, &rect);
                // Explorer.selected_item 하이라이트 (단일클릭 select, v1.5) — 텍스트/아이콘보다 먼저.
                if is_explorer_sel {
                    fill_rect(buffer, width, height, &rect, theme::ACCENT_SUBTLE);
                }
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

    // F2 인라인 rename 오버레이 — *마지막에* 그려서 selected row 위를 완전히 덮는다.
    // layout에서 target_id의 Body rect를 찾고, 흰 박스 + buffer + 캐럿(_)을 그린다.
    if let Some(ov) = rename {
        if let Some((_, rect, _)) = layout
            .iter()
            .find(|(id, _, role)| *id == ov.target_id && *role == HitRole::Body)
        {
            // 흰 배경 + 파랑 1px 테두리 (포커스 표시).
            const WHITE: u32 = 0xFF_FF_FF_FF;
            const FOCUS_BORDER: u32 = 0xFF_00_7A_CC;
            fill_rect(buffer, width, height, &rect, WHITE);
            // 1px 테두리 (상/하/좌/우 4번).
            fill_rect(buffer, width, height, &Rect { x: rect.x, y: rect.y, w: rect.w, h: 1 }, FOCUS_BORDER);
            fill_rect(buffer, width, height, &Rect { x: rect.x, y: rect.y + rect.h - 1, w: rect.w, h: 1 }, FOCUS_BORDER);
            fill_rect(buffer, width, height, &Rect { x: rect.x, y: rect.y, w: 1, h: rect.h }, FOCUS_BORDER);
            fill_rect(buffer, width, height, &Rect { x: rect.x + rect.w - 1, y: rect.y, w: 1, h: rect.h }, FOCUS_BORDER);
            // 텍스트 + 캐럿(끝에 |). draw_text로 텍스트 그리고 그 우측에 1px 막대.
            let text_x = rect.x + 4;
            let text_y = rect.y + 6;
            draw_text(buffer, width, height, &ov.buffer, text_x, text_y, theme::TEXT_PRIMARY);
            let caret_x = text_x + measure_text_width(&ov.buffer);
            fill_rect(buffer, width, height, &Rect { x: caret_x, y: text_y, w: 2, h: 18 }, theme::TEXT_PRIMARY);
        }
    }
}

/// 현재 포커스된 부유 창의 제목을 반환.
///
/// 부유 창 종류: `Window@1`, `FileManager@1`, `ConsoleWindow@1`.
/// 포커스 판정: `state["focused"] == true` (ConsoleWindow는 focused 없으므로 skip).
///
/// 제목 우선순위:
/// - `FileManager@1` → `props["title"]` else `state["title"]` else "파일관리자"
/// - `Window@1`      → `props["title"]` else `state["title"]` else "(window)"
/// - `ConsoleWindow@1` → focused 없음 (항상 포커스 없는 것으로 취급)
///
/// 이 함수는 render-only — hit 영역이나 state를 변경하지 않는다.
fn find_focused_window_title<'a>(tree: &'a TreeModel) -> Option<&'a str> {
    for id in tree.ids() {
        let obj = match tree.get(id) {
            Some(o) => o,
            None => continue,
        };
        if obj.state.get("destroyed").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        let focused = obj.state.get("focused").and_then(|v| v.as_bool()).unwrap_or(false);
        if !focused {
            continue;
        }
        match obj.type_uri.as_str() {
            "aios.builtin/Window@1" => {
                let title = obj
                    .props
                    .get("title")
                    .and_then(|v| v.as_str())
                    .or_else(|| obj.state.get("title").and_then(|v| v.as_str()))
                    .unwrap_or("(window)");
                return Some(title);
            }
            "aios.builtin/FileManager@1" => {
                let title = obj
                    .props
                    .get("title")
                    .and_then(|v| v.as_str())
                    .or_else(|| obj.state.get("title").and_then(|v| v.as_str()))
                    .unwrap_or("파일관리자");
                return Some(title);
            }
            _ => {}
        }
    }
    None
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

/// Explorer.state["selected_item"]가 가리키는 객체 ID를 추출.
/// 단일클릭 select 동작(v1.5)이 설정하는 필드 — FileTree.state.selected과 별개.
fn find_selected_in_explorer(tree: &TreeModel) -> Option<geulos_core::ObjectId> {
    for id in tree.ids() {
        if let Some(o) = tree.get(id) {
            if o.type_uri.as_str() == "aios.builtin/Explorer@1" {
                if let Some(s) = o.state.get("selected_item").and_then(|v| v.as_str()) {
                    if !s.is_empty() {
                        if let Ok(u) = uuid::Uuid::parse_str(s) {
                            return Some(geulos_core::ObjectId::from_uuid(u));
                        }
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

/// CLI 입력 라인 프롬프트 문자열 (mode에 따라). render_cli + 컴포지터 마우스 hit가 공유.
pub fn cli_prompt_text(obj: &geulos_core::Object) -> String {
    let mode = obj.state.get("mode").and_then(|v| v.as_str()).unwrap_or("shell");
    match mode {
        "ai" => match obj.state.get("session_name").and_then(|v| v.as_str()) {
            Some(name) => format!("[ai:{}] > ", name),
            None => "[ai] > ".to_string(),
        },
        "awaiting_api_key" => "[API key 입력] > ".to_string(),
        _ => "> ".to_string(),
    }
}

/// CLI 입력 라인 기하: `(input_x, prompt_y, line_height)`.
///
/// 마우스 클릭 → 문자 offset 매핑(컴포지터)과 선택 하이라이트 렌더가 *동일* 좌표를
/// 쓰도록 render_cli와 공유한다 (시각·hit 일관성).
pub fn cli_input_geometry(rect: &Rect, obj: &geulos_core::Object) -> (i32, i32, i32) {
    let text_x = rect.x + theme::SPACE_SM;
    let text_bottom = rect.y + rect.h - theme::SPACE_SM;
    let prompt_y = text_bottom - CLI_LINE_HEIGHT;
    let input_x = text_x + measure_text_width(&cli_prompt_text(obj));
    (input_x, prompt_y, CLI_LINE_HEIGHT)
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
    // CLI_PADDING_X/Y 상수 제거 → theme::SPACE_SM(8)으로 통일 (8pt grid).
    // 기존 CLI_PADDING_X=8은 SPACE_SM과 동일, CLI_PADDING_Y=6→SPACE_SM=8로 약간 넉넉히.
    // 텍스트 위치만 바뀌며 hit_test와 무관 (CLI는 클릭 영역 판정 없음).
    let text_x = rect.x + theme::SPACE_SM;
    let text_top = rect.y + theme::SPACE_SM;
    let text_bottom = rect.y + rect.h - theme::SPACE_SM;
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
    let cli_wrap_w = (rect.x + rect.w - theme::SPACE_SM - text_x - 4).max(20);
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

    // AI streaming v1: 라이브 누적 텍스트 (확정 lines 아래, 입력 라인 위) + 블록 커서.
    let streaming_active =
        obj.state.get("streaming_active").and_then(|v| v.as_bool()).unwrap_or(false);
    if streaming_active {
        let stream_text = obj.state.get("streaming_text").and_then(|v| v.as_str()).unwrap_or("");
        let cursor_text = format!("{}█", stream_text);
        for vline in crate::editor::wrap_by_pixel_width(&cursor_text, cli_wrap_w) {
            if y + CLI_LINE_HEIGHT > text_bottom {
                break;
            }
            draw_text(buffer, w, h, &vline.text, text_x, y, theme::TERMINAL_TEXT);
            y += CLI_LINE_HEIGHT;
        }
    }

    // 입력 라인 — rect 하단에 고정 (출력이 없어도 prompt는 보임).
    // T7.8 (ADR-031): Cli.state.mode가 "ai"면 prompt = `[ai:<session_name>] > `, 그 외 `> `.
    // T7.9 (ADR-032): "awaiting_api_key"는 `[API key 입력] > ` — 사용자가 명령이 아닌 키를
    //   입력 중임을 시각적으로 명시.
    let prompt_y = text_bottom - CLI_LINE_HEIGHT;
    let prompt = cli_prompt_text(obj);
    draw_text(buffer, w, h, &prompt, text_x, prompt_y, theme::TERMINAL_PROMPT);
    let prompt_width = measure_text_width(&prompt);
    let input_x = text_x + prompt_width;

    // SP4: 선택 영역 하이라이트 — 입력 텍스트 *아래*(먼저) 그려 텍스트가 위에 보이게.
    if let Some((sel_s, sel_e)) = cli_state.selection_range() {
        let buf_len = cli_state.input_buffer.len();
        let sel_s = sel_s.min(buf_len);
        let sel_e = sel_e.min(buf_len);
        let x0 = input_x + measure_text_width(&cli_state.input_buffer[..sel_s]);
        let x1 = input_x + measure_text_width(&cli_state.input_buffer[..sel_e]);
        let sel_rect = Rect { x: x0, y: prompt_y, w: (x1 - x0).max(0), h: CLI_LINE_HEIGHT };
        fill_rect(buffer, w, h, &sel_rect, theme::TERMINAL_SELECTION);
    }
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

        // selection 하이라이트 — 텍스트 그리기 *전*에 배경으로 깔아야 글자가 위에 보임.
        // 한 line의 selection 시작/끝 byte를 prefix measure로 픽셀 폭 변환.
        if editor_active {
            if let Some(ed) = editor {
                if let Some((sel_s, sel_e)) = ed.selection_range() {
                    const SEL_BG: u32 = 0xFF_B3_D7_FF; // Windows 메모장풍 파랑.
                    let mut y_sel = content_rect.y;
                    for line in &wrapped[start..end] {
                        let line_s = line.start_byte;
                        let line_e = line_s + line.text.len();
                        let lo = sel_s.max(line_s);
                        let hi = sel_e.min(line_e);
                        if lo < hi {
                            let pre_len = lo - line_s;
                            let mid_len = hi - line_s;
                            let prefix = &line.text[..pre_len.min(line.text.len())];
                            let middle = &line.text[..mid_len.min(line.text.len())];
                            let x0 = content_rect.x + measure_text_width(prefix);
                            let x1 = content_rect.x + measure_text_width(middle);
                            fill_rect(
                                buffer,
                                w,
                                h,
                                &Rect { x: x0, y: y_sel, w: (x1 - x0).max(1), h: LINE_HEIGHT },
                                SEL_BG,
                            );
                        }
                        y_sel += LINE_HEIGHT;
                    }
                }
            }
        }

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

/// FileManager@1 오버레이 렌더 — Window@1 chrome mirror + 본문 배경.
///
/// SP1: 창 프레임만 그린다 (border + 타이틀바 + [x] + resize handle + 본문 배경).
/// 본문의 좌측 FileTree / 우측 Explorer 컬럼과 각 Folder/File 행은 layout_file_panels가
/// push한 rect를 따라 *각자의 type_uri 분기*에서 이 arm 이후(정순) 그려지므로 여기서는
/// 본문 영역을 패널 배경색으로 칠하기만 한다 (행이 없는 빈 영역도 흰 패널로 보이도록).
///
/// 타이틀: props/state의 "title"이 있으면 그것, 없으면 "파일관리자". dirty 개념 없음 (저장 상태 X).
fn render_file_manager(
    buffer: &mut [u32],
    w: usize,
    h: usize,
    rect: &Rect,
    obj: &geulos_core::Object,
    focused: bool,
) {
    // 외곽 border (1px) + 내부 배경 — Window@1과 동일 RADIUS_MD 패턴.
    fill_rect_rounded(buffer, w, h, rect, theme::RADIUS_MD, theme::BORDER);
    let inner = Rect { x: rect.x + 1, y: rect.y + 1, w: rect.w - 2, h: rect.h - 2 };
    fill_rect_rounded(buffer, w, h, &inner, theme::RADIUS_MD, theme::SURFACE_ELEVATED);

    // 본문(타이틀바 아래) 영역을 패널 배경색으로 — 트리/탐색기 행이 없는 빈 공간도 흰 패널.
    let body_rect = Rect {
        x: inner.x,
        y: inner.y + WINDOW_TITLE_H,
        w: inner.w,
        h: inner.h - WINDOW_TITLE_H,
    };
    fill_rect(buffer, w, h, &body_rect, theme::SURFACE_PANEL);

    // FM 툴바 (28px) — body_rect 최상단에 그린다.
    {
        let toolbar_rect = Rect { x: body_rect.x, y: body_rect.y, w: body_rect.w, h: 28 };
        fill_rect(buffer, w, h, &toolbar_rect, theme::SURFACE_ELEVATED);
        let labels = ["+ New File", "+ New Folder", "Rename", "Delete"];
        let mut bx = body_rect.x + 4;
        for label in labels.iter() {
            let btn = Rect { x: bx, y: body_rect.y + 2, w: 100, h: 24 };
            fill_rect(buffer, w, h, &btn, theme::SURFACE_PANEL);
            draw_text(buffer, w, h, label, btn.x + 4, btn.y + 4, theme::TEXT_PRIMARY);
            bx += 104;
        }
    }

    // Title bar (높이 WINDOW_TITLE_H, focus 시 짙은 파랑) — Window@1 동형.
    let title_rect = Rect { x: inner.x, y: inner.y, w: inner.w, h: WINDOW_TITLE_H };
    let title_bg = if focused { theme::ACCENT_HOVER } else { theme::ACCENT };
    fill_rect(buffer, w, h, &title_rect, title_bg);
    let title = obj
        .props
        .get("title")
        .and_then(|v| v.as_str())
        .or_else(|| obj.state.get("title").and_then(|v| v.as_str()))
        .unwrap_or("파일관리자");
    draw_text(buffer, w, h, title, title_rect.x + 8, title_rect.y + 4, theme::TEXT_ON_ACCENT);

    // [x] 닫기 버튼 — Window@1과 동일 위치/크기.
    let close_rect = Rect {
        x: title_rect.x + title_rect.w - WINDOW_CLOSE_BTN - 4,
        y: title_rect.y + 4,
        w: WINDOW_CLOSE_BTN,
        h: WINDOW_CLOSE_BTN,
    };
    fill_rect_rounded(buffer, w, h, &close_rect, theme::RADIUS_SM, theme::CLOSE_BUTTON);
    draw_text(buffer, w, h, "x", close_rect.x + 4, close_rect.y, theme::TEXT_ON_ACCENT);

    // Resize handle (우하 10×10) — Window@1과 동일.
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

    // Content 영역 — SPACE_MD(12)로 넉넉한 여백 (가독성 향상).
    // ConsoleWindow 본문은 표시 전용 (클릭→편집 없음) — content_rect 변경이 hit_test에 영향 없음.
    // main.rs scroll clamp식(content_h = h-2-WINDOW_TITLE_H-16)과 6px 차이가 생기나
    // ConsoleWindow scroll은 lines 개수 기반이고 render 좌표에 의존하지 않아 안전.
    let content_rect = Rect {
        x: inner.x + theme::SPACE_MD,
        y: inner.y + WINDOW_TITLE_H + theme::SPACE_MD,
        w: inner.w - theme::SPACE_MD * 2,
        h: inner.h - WINDOW_TITLE_H - theme::SPACE_MD * 2,
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

/// Explorer 자식 행 배경 — 미니멀: zebra 명도차 제거 + 약한 BORDER separator.
/// 모든 행 단색 SURFACE_PANEL. 행 구분은 1px 하단 BORDER로.
/// 선택 행은 호출부에서 ACCENT_SUBTLE fill_rect_rounded로 덮어씀 — 별개 처리.
fn draw_explorer_row_bg(buffer: &mut [u32], w: usize, h: usize, rect: &Rect) {
    // 미니멀: zebra 명도차 제거 — 모든 행 SURFACE_PANEL. 행 구분은 약한
    // separator(BORDER) + selected(ACCENT_SUBTLE, render 사용처)로.
    fill_rect(buffer, w, h, rect, theme::SURFACE_PANEL);
    fill_rect(
        buffer,
        w,
        h,
        &Rect { x: rect.x, y: rect.y + rect.h - 1, w: rect.w, h: 1 },
        theme::BORDER,
    );
}

pub fn fill_rect(buffer: &mut [u32], w: usize, h: usize, r: &Rect, color: u32) {
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

    // ─── find_focused_window_title 테스트 ───────────────────────────────────

    fn make_tree_with_window(
        type_uri: &str,
        focused: bool,
        props_title: Option<&str>,
        state_title: Option<&str>,
    ) -> TreeModel {
        use geulos_core::{ActorId, Object, ObjectId, TypeUri};
        let mut obj = Object {
            id: ObjectId::new(),
            type_uri: TypeUri::parse(type_uri).unwrap(),
            parent: None,
            children: Vec::new(),
            props: Default::default(),
            state: Default::default(),
            methods: Vec::new(),
            owner: ActorId::local_user(),
            acl: Vec::new(),
            destroyed: false,
        };
        obj.state
            .insert("focused".to_string(), serde_json::Value::Bool(focused));
        if let Some(t) = props_title {
            obj.props.insert(
                "title".to_string(),
                serde_json::Value::String(t.to_string()),
            );
        }
        if let Some(t) = state_title {
            obj.state.insert(
                "title".to_string(),
                serde_json::Value::String(t.to_string()),
            );
        }
        let mut tree = TreeModel::new();
        tree.upsert(obj);
        tree
    }

    #[test]
    fn focused_window_returns_props_title() {
        let tree =
            make_tree_with_window("aios.builtin/Window@1", true, Some("메모장"), None);
        assert_eq!(find_focused_window_title(&tree), Some("메모장"));
    }

    #[test]
    fn focused_window_falls_back_to_state_title() {
        let tree =
            make_tree_with_window("aios.builtin/Window@1", true, None, Some("메모장2"));
        assert_eq!(find_focused_window_title(&tree), Some("메모장2"));
    }

    #[test]
    fn focused_window_default_title_when_no_title() {
        let tree = make_tree_with_window("aios.builtin/Window@1", true, None, None);
        assert_eq!(find_focused_window_title(&tree), Some("(window)"));
    }

    #[test]
    fn unfocused_window_returns_none() {
        let tree =
            make_tree_with_window("aios.builtin/Window@1", false, Some("메모장"), None);
        assert_eq!(find_focused_window_title(&tree), None);
    }

    #[test]
    fn focused_file_manager_returns_default_title() {
        let tree =
            make_tree_with_window("aios.builtin/FileManager@1", true, None, None);
        assert_eq!(find_focused_window_title(&tree), Some("파일관리자"));
    }

    #[test]
    fn focused_file_manager_with_props_title() {
        let tree = make_tree_with_window(
            "aios.builtin/FileManager@1",
            true,
            Some("파일 탐색기"),
            None,
        );
        assert_eq!(find_focused_window_title(&tree), Some("파일 탐색기"));
    }

    #[test]
    fn console_window_never_focused_so_returns_none() {
        // ConsoleWindow@1은 focused state가 없으므로 결과는 None.
        let tree =
            make_tree_with_window("aios.builtin/ConsoleWindow@1", true, Some("cmd"), None);
        // ConsoleWindow는 find_focused_window_title가 무시 — type_uri 매칭 없음.
        assert_eq!(find_focused_window_title(&tree), None);
    }

    #[test]
    fn empty_tree_returns_none() {
        let tree = TreeModel::new();
        assert_eq!(find_focused_window_title(&tree), None);
    }

    #[test]
    fn destroyed_window_is_skipped() {
        use geulos_core::{ActorId, Object, ObjectId, TypeUri};
        let mut obj = Object {
            id: ObjectId::new(),
            type_uri: TypeUri::parse("aios.builtin/Window@1").unwrap(),
            parent: None,
            children: Vec::new(),
            props: Default::default(),
            state: Default::default(),
            methods: Vec::new(),
            owner: ActorId::local_user(),
            acl: Vec::new(),
            destroyed: false,
        };
        obj.state
            .insert("focused".to_string(), serde_json::Value::Bool(true));
        obj.state
            .insert("destroyed".to_string(), serde_json::Value::Bool(true));
        obj.props.insert(
            "title".to_string(),
            serde_json::Value::String("좀비창".to_string()),
        );
        let mut tree = TreeModel::new();
        tree.upsert(obj);
        assert_eq!(find_focused_window_title(&tree), None);
    }

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
    fn parse_hex_color_parses_rrggbb() {
        assert_eq!(parse_hex_color("#1E2A3A"), Some(0xFF_1E_2A_3A));
        assert_eq!(parse_hex_color("1E2A3A"), Some(0xFF_1E_2A_3A)); // # 생략 허용
        assert_eq!(parse_hex_color("#FFFFFF"), Some(0xFF_FF_FF_FF));
    }

    #[test]
    fn parse_hex_color_rejects_invalid() {
        assert_eq!(parse_hex_color("#FFF"), None); // 3자리 단축형 미지원
        assert_eq!(parse_hex_color(""), None);
        assert_eq!(parse_hex_color("#GGGGGG"), None); // 비-hex
        assert_eq!(parse_hex_color("#1E2A3A00"), None); // 8자리 미지원
    }

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
