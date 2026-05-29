//! 단순 레이아웃 엔진.
//!
//! Container = 세로 stack (vstack). Text/Button/Toggle = 자식 없는 직사각형 box.
//! 루트 컨테이너가 윈도우 전체를 채움.

use geulos_core::{ObjectId, TypeUri};

use crate::tree_model::TreeModel;

/// 한 객체의 화면상 사각형.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

/// hit rect의 *역할*. 한 ObjectId가 여러 hit rect를 가질 수 있으므로 click 분기에 사용.
///
/// - `Body`: 객체의 기본 본문 hit. dispatch_click의 type별 분기를 그대로 적용.
/// - `ExpandToggle`: 좌측 트리 폴더의 `[+]`/`[-]` 표식 영역. dispatch_click이 expand/collapse만
///   보내고 navigate_to는 보내지 않는다 (M8 회귀 fix #2 — UX 분리).
/// - `ExplorerParentNav`: 우측 Explorer 상단 첫 줄 — active_folder의 부모로 navigate.
///   active_folder가 설정된 경우만 push되며, 클릭 시 main이 부모 id를 산출해 navigate_to 발송.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitRole {
    Body,
    ExpandToggle,
    ExplorerParentNav,
    DesktopIcon,     // 바탕화면 아이콘 → open()
    DockItem,        // 독 항목 → Dock.launch
    TopBarItem,      // 네비바 항목 → TopBar.activate
    CliResizeHandle, // CLI 상단 리사이즈 핸들 → Desktop.set_cli_height
}

pub const TOPBAR_H: i32 = 30;
pub const DOCK_W: i32 = 56;
pub const CLI_HANDLE_H: i32 = 6;
pub const CLI_MIN_H: i32 = 60;
pub const DESKTOP_MIN_H: i32 = 40;

/// Desktop.state.cli_height 부재 시 기본값 (factory 기본 220과 일치).
pub const CLI_DEFAULT_H: i32 = 220;

// ── SP1 크롬 per-item geometry ──
// layout / render / bin 클릭 핸들러가 *같은 상수*를 공유해야 클릭 영역과 그려진 위치가
// 어긋나지 않는다 (window_geom 패턴과 동일 의도). LayoutResult는 (ObjectId, Rect, HitRole)만
// 운반하므로 "어느 item을 눌렀나"는 bin이 이 상수 + desktop_regions로 *y/x 위치에서 역산*한다.
/// TopBar item 한 칸 가로 폭 (좌→우 배치).
pub const TOPBAR_ITEM_W: i32 = 96;
/// Dock item 한 칸 세로 높이 (상→하 stack, DOCK_W 정사각형에 가깝게).
pub const DOCK_ITEM_H: i32 = 56;
/// DesktopIcon 클릭/렌더 박스 크기.
pub const ICON_BOX_W: i32 = 64;
pub const ICON_BOX_H: i32 = 72;

/// 데스크톱 영역 묶음.
pub struct DesktopRegions {
    pub topbar: Rect,
    pub dock: Rect,
    pub cli: Rect,
    pub cli_handle: Rect,
    pub desktop: Rect, // 바탕화면 + 떠있는 창 영역
}

/// 화면 크기 + cli_height에서 데스크톱 영역들을 계산 (순수).
pub fn desktop_regions(win_w: i32, win_h: i32, cli_height: i32) -> DesktopRegions {
    let max_cli = (win_h - TOPBAR_H - DESKTOP_MIN_H).max(CLI_MIN_H);
    let cli_h = cli_height.clamp(CLI_MIN_H, max_cli);
    let mid_h = win_h - TOPBAR_H - cli_h;
    DesktopRegions {
        topbar: Rect { x: 0, y: 0, w: win_w, h: TOPBAR_H },
        dock: Rect { x: win_w - DOCK_W, y: TOPBAR_H, w: DOCK_W, h: mid_h },
        cli: Rect { x: 0, y: win_h - cli_h, w: win_w, h: cli_h },
        cli_handle: Rect { x: 0, y: win_h - cli_h - CLI_HANDLE_H, w: win_w, h: CLI_HANDLE_H },
        desktop: Rect { x: 0, y: TOPBAR_H, w: win_w - DOCK_W, h: mid_h },
    }
}

/// 레이아웃 결과: ObjectId → Rect 매핑 (+ role).
///
/// 동일 ObjectId가 여러 row를 가질 수 있다 (예: 좌측 트리 폴더는 Body + ExpandToggle 두 개).
/// hit_test가 *역순 iterate*하므로, 폴더에서는 Body를 먼저 push하고 ExpandToggle을 나중에
/// push해야 toggle이 우선 매칭된다 (사용자 클릭이 [+] 영역에 들면 toggle, 폴더명 영역이면 body).
#[derive(Debug, Default)]
pub struct LayoutResult {
    pub rects: Vec<(ObjectId, Rect, HitRole)>,
}

impl LayoutResult {
    /// 주어진 ObjectId의 Body rect 반환 (기존 API 호환 — render.rs 등 단일 rect 가정 호출처).
    /// Body가 없으면 첫 번째 rect 반환 (single-push 케이스 안전).
    pub fn get(&self, id: ObjectId) -> Option<Rect> {
        self.rects
            .iter()
            .find(|(i, _, role)| *i == id && *role == HitRole::Body)
            .map(|(_, r, _)| *r)
            .or_else(|| self.rects.iter().find(|(i, _, _)| *i == id).map(|(_, r, _)| *r))
    }

    pub fn iter(&self) -> impl Iterator<Item = (ObjectId, Rect, HitRole)> + '_ {
        self.rects.iter().copied()
    }
}

/// 객체 타입별 정해진 높이 (단순 모델).
fn item_height(type_uri: &TypeUri) -> i32 {
    match type_uri.as_str() {
        "aios.std/Text@1" => 40,
        "aios.std/Button@1" => 60,
        "aios.std/Toggle@1" => 40,
        // Folder/File 둘 다 28 — 18pt 한글 텍스트 (시각 height ~22)에 여유 4px씩.
        // 이전엔 File=24였으나 한글 텍스트가 행 경계를 넘어 zebra/separator 시각 구분이 모호 (사용자 보고).
        "aios.std/Folder@1" => 28,
        "aios.std/File@1" => 28,
        _ => 0, // Container는 자체 크기 없음 (자식의 합으로 계산)
    }
}

const PADDING: i32 = 16;
const SPACING: i32 = 8;
const INDENT: i32 = 16;
/// Explorer 자식 행 + ParentNav 행의 픽셀 stride. FileTree Folder@1 item_height와 동일.
/// render.rs draw_explorer_row_bg, main.rs max_scroll_y_for의 추정과도 일치해야 한다.
pub const EXPLORER_ROW_H: i32 = 28;

/// 한 객체와 그 자손을 레이아웃해서 사각형 목록을 반환.
fn layout_object(
    tree: &TreeModel,
    id: ObjectId,
    x: i32,
    y: i32,
    avail_w: i32,
    out: &mut Vec<(ObjectId, Rect, HitRole)>,
) -> i32 {
    let obj = match tree.get(id) {
        Some(o) => o,
        None => return 0,
    };
    if obj.type_uri.as_str() == "aios.std/Container@1" {
        // vstack: 자식들을 세로로 배치, 자기 높이는 자식 합 + padding.
        //
        // z-order: Container는 자식들보다 *먼저* 그려져야 하므로(자식이 위에 보임)
        // `out`에 자식보다 앞 슬롯에 들어가야 한다. 그러나 자기 크기는 자식 처리 후에야
        // 결정되므로, 자식을 추가하기 전 인덱스를 기억해뒀다가 마지막에 그 자리에 insert.
        let container_idx = out.len();
        let mut cur_y = y + PADDING;
        let inner_x = x + PADDING;
        let inner_w = avail_w - 2 * PADDING;
        let mut content_h = 0i32;
        for &child_id in &obj.children {
            let used = layout_object(tree, child_id, inner_x, cur_y, inner_w, out);
            cur_y += used + SPACING;
            content_h += used + SPACING;
        }
        // SPACING 마지막 제거
        if content_h > 0 {
            content_h -= SPACING;
        }
        let total_h = content_h + 2 * PADDING;
        out.insert(container_idx, (id, Rect { x, y, w: avail_w, h: total_h }, HitRole::Body));
        total_h
    } else {
        let h = item_height(&obj.type_uri);
        out.push((id, Rect { x, y, w: avail_w, h }, HitRole::Body));
        h
    }
}

/// FileTree 자식 한 개 + (Folder이면 expanded 시) 자손들을 들여쓰기 재귀로 배치.
/// 사용한 세로 공간을 반환 (자식 + 자손 누적).
///
/// M8 T8.4부터 dead — `layout_tree_node_folders_only`로 대체됨. M7 회귀 가능성 대비로
/// *유지* 중이며 T8.12에서 최종 정리 예정.
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
fn layout_tree_node(
    tree: &TreeModel,
    expanded: &[ObjectId],
    id: ObjectId,
    x: i32,
    y: i32,
    avail_w: i32,
    out: &mut Vec<(ObjectId, Rect, HitRole)>,
) -> i32 {
    let obj = match tree.get(id) {
        Some(o) => o,
        None => return 0,
    };
    let mut cur_y = y;
    let h = item_height(&obj.type_uri);
    out.push((id, Rect { x, y: cur_y, w: avail_w, h }, HitRole::Body));
    cur_y += h;
    let is_folder = obj.type_uri.as_str() == "aios.std/Folder@1";
    if is_folder && expanded.contains(&id) {
        for &child_id in &obj.children {
            cur_y += layout_tree_node(
                tree,
                expanded,
                child_id,
                x + INDENT,
                cur_y,
                avail_w - INDENT,
                out,
            );
        }
    }
    cur_y - y
}

/// FileTree.state["expanded"] (UUID 문자열 배열) → ObjectId 목록.
/// SP1: layout_desktop에서 더 이상 직접 호출되지 않음 — FileManager@1 창 구현 시 재사용 예정.
#[allow(dead_code)]
fn extract_expanded(tree: &TreeModel, ft_id: ObjectId) -> Vec<ObjectId> {
    let ft = match tree.get(ft_id) {
        Some(o) => o,
        None => return vec![],
    };
    let arr = match ft.state.get("expanded").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return vec![],
    };
    arr.iter()
        .filter_map(|v| v.as_str())
        .filter_map(|s| uuid::Uuid::parse_str(s).ok())
        .map(ObjectId::from_uuid)
        .collect()
}

/// Desktop 루트 layout — SP1 바탕화면 (TopBar / Dock / CLI / DesktopIcons / 오버레이 Window들).
///
/// 자식 구조: `[TopBar, Dock, Cli, DesktopIcon*, Window*...]`
/// FileTree/Explorer는 더 이상 고정 패널이 아니다 — FileManager@1 창 안에서 표시된다(후속 작업).
fn layout_desktop(
    tree: &TreeModel,
    id: ObjectId,
    win_w: i32,
    win_h: i32,
    out: &mut Vec<(ObjectId, Rect, HitRole)>,
) {
    let obj = match tree.get(id) {
        Some(o) => o,
        None => return,
    };
    // Desktop 자체 rect (윈도우 전체).
    out.push((id, Rect { x: 0, y: 0, w: win_w, h: win_h }, HitRole::Body));

    let has_cli = obj
        .children
        .iter()
        .any(|&cid| tree.get(cid).map(|o| o.type_uri.as_str()) == Some("aios.builtin/Cli@1"));

    // SP1 크롬 영역 — Desktop.state.cli_height(없으면 기본 220) 기준 desktop_regions로 분할.
    // CLI가 없는 트리(echo-app 등 비-Desktop은 layout() fallback이라 여기 안 옴; Desktop이지만
    // Cli 자식이 아직 mount 안 된 부팅 초기 상태)에서는 중앙 영역을 화면 하단까지 확장한다.
    let cli_height = obj.state.get("cli_height").and_then(|v| v.as_i64()).unwrap_or(CLI_DEFAULT_H as i64) as i32;
    let r = desktop_regions(win_w, win_h, cli_height);

    // SP1: FileTree/Explorer는 더 이상 고정 패널이 아니다 — 파일관리자 창(FileManager@1)
    // 안에서만 표시된다(후속 작업). 바탕화면 중앙(r.desktop)은 wallpaper + DesktopIcon만.

    // ── TopBar 스트립 ── (Desktop의 자식 또는 트리 전역에서 첫 TopBar@1)
    if let Some(tb) = find_topbar(tree, obj) {
        // 바 배경 (Body) — render가 type_uri로 그림.
        out.push((tb.id, r.topbar, HitRole::Body));
        // 각 item을 좌→우로 TOPBAR_ITEM_W 칸 배치. 클릭은 모두 TopBarItem role (같은 TopBar id).
        // bin이 px 위치에서 item index를 역산해 items[idx].id를 activate.
        let n = tb.state.get("items").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
        for i in 0..n as i32 {
            let ix = r.topbar.x + i * TOPBAR_ITEM_W;
            if ix >= r.topbar.x + r.topbar.w {
                break; // 바를 넘치면 더 안 그림
            }
            out.push((
                tb.id,
                Rect { x: ix, y: r.topbar.y, w: TOPBAR_ITEM_W, h: r.topbar.h },
                HitRole::TopBarItem,
            ));
        }
    }

    // ── Dock 스트립 (우측 세로) ──
    if let Some(dk) = find_dock(tree, obj) {
        out.push((dk.id, r.dock, HitRole::Body));
        let n = dk.state.get("items").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
        for i in 0..n as i32 {
            let iy = r.dock.y + i * DOCK_ITEM_H;
            if iy >= r.dock.y + r.dock.h {
                break;
            }
            out.push((
                dk.id,
                Rect { x: r.dock.x, y: iy, w: r.dock.w, h: DOCK_ITEM_H },
                HitRole::DockItem,
            ));
        }
    }

    // ── CLI 리사이즈 핸들 (M3 드래그용 hit; M1 bin은 no-op) ──
    // r.cli_handle(CLI 위 6px 띠)에 Desktop id로 push.
    if has_cli {
        out.push((id, r.cli_handle, HitRole::CliResizeHandle));
    }

    // 하단: CLI 패널 — r.cli 영역 (cli_height 반영, TopBar/Dock 제외 풀폭).
    if has_cli {
        if let Some(cli) = find_child_by_type(tree, obj, "aios.builtin/Cli@1") {
            out.push((cli.id, r.cli, HitRole::Body));
        }
    }

    // ── DesktopIcons (중앙 바탕화면, state.x/y 오프셋) ──
    // r.desktop 영역에 wallpaper + 아이콘만 표시. floating 창은 아래 Window 오버레이에서 처리.
    for &cid in &obj.children {
        let icon = match tree.get(cid) {
            Some(o) if o.type_uri.as_str() == "aios.builtin/DesktopIcon@1" => o,
            _ => continue,
        };
        if icon.state.get("destroyed").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        let ix = icon.state.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let iy = icon.state.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        out.push((
            icon.id,
            Rect { x: r.desktop.x + ix, y: r.desktop.y + iy, w: ICON_BOX_W, h: ICON_BOX_H },
            HitRole::DesktopIcon,
        ));
    }

    // Window 오버레이 — z 오름차순 정렬 → 마지막에 push (그리는 순서 = z).
    // state.destroyed=true는 close된 Window (T8.10) — layout/hit_test 모두에서 제외해
    // 시각적으로 사라지고 클릭도 안 됨. proto에 DestroyMsg가 없어 desktop-shell이
    // SetState로 우회한 결과 (KI-011 tombstone과 형식 일치).
    //
    // M13 T9: ConsoleWindow@1도 floating panel로 Window@1과 동일한 z-sort 오버레이.
    // geometry는 state.x/y/w/h에서 읽음 (Window@1과 동일).
    let mut windows: Vec<&geulos_core::Object> = obj
        .children
        .iter()
        .filter_map(|&id| tree.get(id))
        .filter(|o| {
            matches!(o.type_uri.as_str(), "aios.builtin/Window@1" | "aios.builtin/ConsoleWindow@1")
        })
        .filter(|o| !o.state.get("destroyed").and_then(|v| v.as_bool()).unwrap_or(false))
        .collect();
    windows.sort_by_key(|w| w.state.get("z").and_then(|v| v.as_i64()).unwrap_or(0));
    for w in windows {
        let x = w.state.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let y = w.state.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let wid = w.state.get("w").and_then(|v| v.as_i64()).unwrap_or(600) as i32;
        let hgt = w.state.get("h").and_then(|v| v.as_i64()).unwrap_or(400) as i32;
        out.push((w.id, Rect { x, y, w: wid, h: hgt }, HitRole::Body));
    }

    // M9 T7: Dialog 오버레이 — Window 보다 z 위 (modal). 화면 중앙 400×200 고정.
    //
    // 응답 완료(state.result != null)된 Dialog는 desktop-shell이 곧 destroyed=true로 마크해
    // 사라지지만, destroy 도착 전 일시 상태(result만 set)는 *modal에서 빠진다* — 사용자가
    // 클릭 응답 직후 다른 영역을 즉시 클릭할 수 있도록. destroyed=true는 Window와 동일하게
    // 제외 (이미 닫힌 객체).
    let dialogs: Vec<&geulos_core::Object> = obj
        .children
        .iter()
        .filter_map(|&cid| tree.get(cid))
        .filter(|o| o.type_uri.as_str() == "aios.builtin/Dialog@1")
        .filter(|o| !o.state.get("destroyed").and_then(|v| v.as_bool()).unwrap_or(false))
        .filter(|o| o.state.get("result").map(|v| v.is_null()).unwrap_or(true))
        .collect();
    for d in dialogs {
        let dw = 400i32;
        let dh = 200i32;
        let dx = (win_w - dw) / 2;
        let dy = (win_h - dh) / 2;
        out.push((d.id, Rect { x: dx, y: dy, w: dw, h: dh }, HitRole::Body));
    }
}

/// 자식 중 첫 번째로 주어진 type_uri 매칭 Object를 반환.
fn find_child_by_type<'a>(
    tree: &'a TreeModel,
    parent: &'a geulos_core::Object,
    type_uri: &str,
) -> Option<&'a geulos_core::Object> {
    parent
        .children
        .iter()
        .filter_map(|&cid| tree.get(cid))
        .find(|o| o.type_uri.as_str() == type_uri)
}

/// TopBar@1 객체를 찾는다 — Desktop 자식 우선, 없으면 트리 전역 fallback.
/// (mount race로 parent.children 연결이 늦을 수 있어 전역 탐색 fallback을 둔다.)
fn find_topbar<'a>(
    tree: &'a TreeModel,
    desktop: &'a geulos_core::Object,
) -> Option<&'a geulos_core::Object> {
    find_child_by_type(tree, desktop, "aios.builtin/TopBar@1").or_else(|| {
        tree.ids()
            .filter_map(|id| tree.get(id))
            .find(|o| o.type_uri.as_str() == "aios.builtin/TopBar@1")
    })
}

/// Dock@1 객체를 찾는다 — Desktop 자식 우선, 없으면 트리 전역 fallback.
fn find_dock<'a>(
    tree: &'a TreeModel,
    desktop: &'a geulos_core::Object,
) -> Option<&'a geulos_core::Object> {
    find_child_by_type(tree, desktop, "aios.builtin/Dock@1").or_else(|| {
        tree.ids()
            .filter_map(|id| tree.get(id))
            .find(|o| o.type_uri.as_str() == "aios.builtin/Dock@1")
    })
}

/// FileTree 트리 자식 layout — `layout_tree_node`와 같지만 *File 노드는 skip* (M8: 좌측은 폴더만).
///
/// M8 회귀 fix #2 (HitRole): 폴더 한 줄당 **두 개의 hit rect**를 push 한다.
///
/// 1. `Body` — 전체 행 (폴더명 + 표식 포함). 클릭 시 navigate_to.
/// 2. `ExpandToggle` — 행 좌측 `[+]`/`[-]` 표식 영역 (~36px). 클릭 시 expand/collapse만.
///
/// **push 순서가 중요**: `hit_test`가 *역순* iterate하므로 마지막 push가 우선 매칭된다.
/// `Body` 먼저, `ExpandToggle` 나중에 push → 사용자 클릭 좌표가 toggle 영역에 들면 toggle이
/// 먼저 매칭되고, 그 외 영역(폴더명 부분)에서는 toggle이 contains() 검사에서 탈락하고 Body가
/// 매칭된다.
///
/// `toggle_w`는 폰트 ~10px/char × "[+] " 4 char ≈ 40px의 안전 마진으로 36px 하드코드.
/// `measure_text_width`를 쓰지 않은 이유: text 모듈이 ab_glyph PxScale 계산을 매번 수행하는데,
/// (a) 모든 폴더에 대해 일정한 값이고, (b) 폰트가 바뀌지 않는 한 변하지 않으므로 컴파일 타임
/// 상수로 충분. M9에서 폰트 metric API가 안정되면 measure_text_width("[+] ")로 교체 검토.
///
/// SP1: layout_desktop에서 더 이상 직접 호출되지 않음 — FileManager@1 창 구현 시 재사용 예정.
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
fn layout_tree_node_folders_only(
    tree: &TreeModel,
    expanded: &[ObjectId],
    id: ObjectId,
    x: i32,
    y: i32,
    avail_w: i32,
    out: &mut Vec<(ObjectId, Rect, HitRole)>,
) -> i32 {
    let obj = match tree.get(id) {
        Some(o) => o,
        None => return 0,
    };
    if obj.type_uri.as_str() != "aios.std/Folder@1" {
        return 0; // 파일은 좌측 트리에서 안 보임
    }
    let mut cur_y = y;
    let h = item_height(&obj.type_uri);
    let row_rect = Rect { x, y: cur_y, w: avail_w, h };
    let toggle_w = 36.min(row_rect.w);
    let toggle_rect = Rect { x, y: cur_y, w: toggle_w, h };
    // Body 먼저 push (역순 hit_test에서 후순위) — 폴더명 영역 클릭 시 매칭.
    out.push((id, row_rect, HitRole::Body));
    // ExpandToggle 나중에 push (역순 hit_test에서 우선) — [+]/[-] 영역 클릭 시 매칭.
    out.push((id, toggle_rect, HitRole::ExpandToggle));
    cur_y += h;
    if expanded.contains(&id) {
        for &child_id in &obj.children {
            // M10 Phase 2: destroyed=true 자식은 skip — 외부 rename으로 옛 이름 객체가
            // destroyed marker만 있고 트리에 잔존하는 경우 visual 깔끔 유지.
            if tree
                .get(child_id)
                .map(|o| o.state.get("destroyed").and_then(|v| v.as_bool()).unwrap_or(false))
                .unwrap_or(false)
            {
                continue;
            }
            cur_y += layout_tree_node_folders_only(
                tree,
                expanded,
                child_id,
                x + INDENT,
                cur_y,
                avail_w - INDENT,
                out,
            );
        }
    }
    cur_y - y
}

/// Explorer가 보여줄 자식 ObjectId 목록 — active_folder의 children, 폴더 먼저 + 이름순 정렬.
/// active_folder=None이면 FileTree의 children (드라이브 일람).
/// SP1: layout_desktop에서 더 이상 직접 호출되지 않음 — FileManager@1 창 구현 시 재사용 예정.
#[allow(dead_code)]
fn explorer_children(tree: &TreeModel, ex: &geulos_core::Object) -> Vec<ObjectId> {
    let active = ex.state.get("active_folder").and_then(|v| v.as_str());
    let folder_id = match active {
        Some(s) if !s.is_empty() => match uuid::Uuid::parse_str(s) {
            Ok(u) => Some(ObjectId::from_uuid(u)),
            Err(_) => None,
        },
        _ => {
            // None → FileTree.children (드라이브 일람)
            return tree
                .ids()
                .find(|id| {
                    tree.get(*id)
                        .map(|o| o.type_uri.as_str() == "aios.builtin/FileTree@1")
                        .unwrap_or(false)
                })
                .and_then(|ft_id| tree.get(ft_id).map(|ft| ft.children.clone()))
                .unwrap_or_default();
        }
    };
    let folder_id = match folder_id {
        Some(id) => id,
        None => return vec![],
    };
    let folder = match tree.get(folder_id) {
        Some(o) => o,
        None => return vec![],
    };
    // M10 Phase 2: destroyed=true 자식은 사전 필터 — 외부 rename/remove 후 옛 객체가
    // marker만 가지고 트리에 잔존하는 경우 Explorer 목록에 안 보이게.
    let mut kids: Vec<ObjectId> = folder
        .children
        .iter()
        .copied()
        .filter(|cid| {
            tree.get(*cid)
                .map(|o| !o.state.get("destroyed").and_then(|v| v.as_bool()).unwrap_or(false))
                .unwrap_or(false)
        })
        .collect();
    // 폴더 먼저 (false < true), 그 다음 *case-insensitive* 이름순.
    // ASCII 정렬 (대문자 < 소문자)을 쓰면 한 폴더 안에 "Program Files"와 "app_build"가 두 그룹으로
    // 분리되어 보임 (사용자 보고) — FileTree의 native 정렬과도 어긋남. to_lowercase()로 통일.
    kids.sort_by_key(|id| {
        tree.get(*id)
            .map(|o| {
                let is_folder = o.type_uri.as_str() == "aios.std/Folder@1";
                let name =
                    o.props.get("name").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                (!is_folder, name)
            })
            .unwrap_or((true, String::new()))
    });
    kids
}

/// 전체 트리를 레이아웃.
///
/// Desktop@1 루트가 있으면 좌/우 분할 셸 경로. 없으면 기존 vstack 폴백
/// (echo-app 등 M3 호환).
pub fn layout(tree: &TreeModel, win_w: i32, win_h: i32) -> LayoutResult {
    let mut out = Vec::new();
    for &root in tree.roots() {
        let obj = match tree.get(root) {
            Some(o) => o,
            None => continue,
        };
        if obj.type_uri.as_str() == "aios.builtin/Desktop@1" {
            layout_desktop(tree, root, win_w, win_h, &mut out);
            return LayoutResult { rects: out };
        }
    }
    // Desktop 루트가 없으면 기존 동작 (echo-app 호환).
    let mut y = 0i32;
    for &root in tree.roots() {
        let used = layout_object(tree, root, 0, y, win_w, &mut out);
        y += used;
        if y >= win_h {
            break;
        }
    }
    LayoutResult { rects: out }
}

// SP1: Explorer/FileTree 패널 layout 테스트 제거됨 — 고정 패널이 바탕화면에서 사라짐.
// 4개 테스트 삭제:
//   - explorer_parent_nav_row_pushed_when_active_folder_set
//   - explorer_no_parent_nav_row_at_root
//   - explorer_child_skipped_when_scrolled_into_parent_nav_area
//   - explorer_children_offset_by_parent_row_height
// 이 동작들은 FileManager@1 창 구현 시 새 테스트로 복원 예정.

#[cfg(test)]
mod region_tests {
    use super::*;
    #[test]
    fn regions_partition_screen() {
        let r = desktop_regions(1280, 800, 220);
        assert_eq!(r.topbar, Rect { x: 0, y: 0, w: 1280, h: 30 });
        assert_eq!(r.dock, Rect { x: 1280 - 56, y: 30, w: 56, h: 800 - 30 - 220 });
        assert_eq!(r.cli, Rect { x: 0, y: 800 - 220, w: 1280, h: 220 });
        assert_eq!(r.cli_handle, Rect { x: 0, y: 800 - 220 - 6, w: 1280, h: 6 });
        assert_eq!(r.desktop, Rect { x: 0, y: 30, w: 1280 - 56, h: 800 - 30 - 220 });
    }
    #[test]
    fn cli_height_clamped() {
        let r = desktop_regions(1280, 800, 100_000);
        assert!(r.cli.h <= 800 - 30 - 40);
    }
}

#[cfg(test)]
mod sp1_chrome_tests {
    use super::*;
    use geulos_core::{std_types, ActorId};
    use serde_json::json;

    /// Desktop + TopBar + Dock + DesktopIcon 트리를 layout하면 각 크롬 객체의 rect/role이
    /// desktop_regions 영역에 맞게 push된다.
    #[test]
    fn chrome_rects_pushed_with_correct_roles_and_regions() {
        let owner = ActorId::local_user();
        let mut desktop = std_types::desktop(owner.clone());
        let mut tb = std_types::top_bar(owner.clone()); // items=[{id:"geulos",label:"GeulOS"}]
        let mut dk = std_types::dock(owner.clone());
        dk.state.insert(
            "items".to_string(),
            json!([{"app":"file_manager","label":"파일관리자","icon":"folder"}]),
        );
        let mut ic = std_types::desktop_icon(owner.clone(), "file_manager", "파일관리자", "folder", 40, 50);
        tb.parent = Some(desktop.id);
        dk.parent = Some(desktop.id);
        ic.parent = Some(desktop.id);
        let (tb_id, dk_id, ic_id) = (tb.id, dk.id, ic.id);
        desktop.children = vec![tb.id, dk.id, ic.id];

        let mut tree = TreeModel::new();
        tree.upsert(desktop);
        tree.upsert(tb);
        tree.upsert(dk);
        tree.upsert(ic);

        let (w, h) = (1280, 800);
        let r = desktop_regions(w, h, CLI_DEFAULT_H);
        let lay = layout(&tree, w, h);

        // TopBar Body = r.topbar.
        let tb_body = lay
            .rects
            .iter()
            .find(|(id, _, role)| *id == tb_id && *role == HitRole::Body)
            .map(|(_, rc, _)| *rc);
        assert_eq!(tb_body, Some(r.topbar), "TopBar Body = r.topbar");
        // TopBar item 1개 (TopBarItem role), x=0부터.
        let tb_item = lay
            .rects
            .iter()
            .find(|(id, _, role)| *id == tb_id && *role == HitRole::TopBarItem)
            .map(|(_, rc, _)| *rc)
            .expect("TopBarItem 1개");
        assert_eq!(tb_item.x, 0);
        assert_eq!(tb_item.w, TOPBAR_ITEM_W);

        // Dock Body = r.dock.
        let dk_body = lay
            .rects
            .iter()
            .find(|(id, _, role)| *id == dk_id && *role == HitRole::Body)
            .map(|(_, rc, _)| *rc);
        assert_eq!(dk_body, Some(r.dock), "Dock Body = r.dock");
        // Dock item 1개, y=r.dock.y부터 DOCK_ITEM_H.
        let dk_item = lay
            .rects
            .iter()
            .find(|(id, _, role)| *id == dk_id && *role == HitRole::DockItem)
            .map(|(_, rc, _)| *rc)
            .expect("DockItem 1개");
        assert_eq!(dk_item.x, r.dock.x);
        assert_eq!(dk_item.y, r.dock.y);
        assert_eq!(dk_item.h, DOCK_ITEM_H);

        // DesktopIcon rect = r.desktop offset + state x/y.
        let ic_rect = lay
            .rects
            .iter()
            .find(|(id, _, role)| *id == ic_id && *role == HitRole::DesktopIcon)
            .map(|(_, rc, _)| *rc)
            .expect("DesktopIcon rect");
        assert_eq!(ic_rect.x, r.desktop.x + 40);
        assert_eq!(ic_rect.y, r.desktop.y + 50);
        assert_eq!(ic_rect.w, ICON_BOX_W);
        assert_eq!(ic_rect.h, ICON_BOX_H);
    }

    /// Cli가 있으면 CliResizeHandle rect가 r.cli_handle에 Desktop id로 push된다.
    #[test]
    fn cli_resize_handle_pushed_when_cli_present() {
        let owner = ActorId::local_user();
        let mut desktop = std_types::desktop(owner.clone());
        let desk_id = desktop.id;
        let mut cli = std_types::cli(owner.clone());
        cli.parent = Some(desktop.id);
        desktop.children = vec![cli.id];

        let mut tree = TreeModel::new();
        tree.upsert(desktop);
        tree.upsert(cli);

        let (w, h) = (1280, 800);
        let r = desktop_regions(w, h, CLI_DEFAULT_H);
        let lay = layout(&tree, w, h);
        let handle = lay
            .rects
            .iter()
            .find(|(id, _, role)| *id == desk_id && *role == HitRole::CliResizeHandle)
            .map(|(_, rc, _)| *rc);
        assert_eq!(handle, Some(r.cli_handle), "CliResizeHandle = r.cli_handle");
    }

    // SP1: desktop_icon_pushed_after_panels_for_hit_priority 삭제 —
    // 패널 제거로 FileTree/Explorer Body rect가 더 이상 push되지 않아 test obsolete.
}
