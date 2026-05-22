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
        "aios.std/Folder@1" => 28,
        "aios.std/File@1" => 24,
        _ => 0, // Container는 자체 크기 없음 (자식의 합으로 계산)
    }
}

const PADDING: i32 = 16;
const SPACING: i32 = 8;
const INDENT: i32 = 16;

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

/// Desktop 루트 layout — M8 4분할 (좌 FileTree / 우 Explorer / 하 CLI / 오버레이 Window들).
///
/// 자식 구조: `[FileTree, Explorer, Cli, Window*...]` — Window는 z-order 오버레이.
/// CLI 없으면 (M7 호환) Cli/Window 분기 skip + 상단 풀높이 fallback.
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
    let left_w = (win_w as f32 * 0.25) as i32; // M8: 좌 25%
    let right_w = win_w - left_w;

    let has_cli = obj
        .children
        .iter()
        .any(|&cid| tree.get(cid).map(|o| o.type_uri.as_str()) == Some("aios.builtin/Cli@1"));
    let top_h = if has_cli { (win_h as f32 * 0.70) as i32 } else { win_h };
    let bottom_h = win_h - top_h;

    // 좌측: FileTree 패널 (상단 영역, 폴더만 — File 노드 skip).
    //
    // M8 T8.16: FileTree.state.scroll_y (라인 단위) 만큼 *전체 자손 y를 위로 밀어* clip.
    // *28px = Folder@1 행 단위* — `item_height(&TypeUri::parse("aios.std/Folder@1"))`로 정의됨.
    // Explorer의 24px (별 list 행)와 *다른 값* — FileTree는 폴더 트리라 item_height와 일관해야
    // 한 라인 스크롤 시 정확히 한 행이 사라짐. 음수 y rect는 fill_rect/draw_text가 자연 클립.
    if let Some(ft) = find_child_by_type(tree, obj, "aios.builtin/FileTree@1") {
        out.push((ft.id, Rect { x: 0, y: 0, w: left_w, h: top_h }, HitRole::Body));
        let expanded = extract_expanded(tree, ft.id);
        let scroll_y = ft.state.get("scroll_y").and_then(|v| v.as_i64()).unwrap_or(0).max(0) as i32;
        // FileTree의 자손 stride는 item_height(Folder@1)=28. Explorer의 24와 다른 값.
        let folder_row_height =
            item_height(&TypeUri::parse("aios.std/Folder@1").expect("Folder TypeUri"));
        let scroll_px = scroll_y * folder_row_height;
        let mut y = 4i32 - scroll_px;
        for &cid in &ft.children {
            y += layout_tree_node_folders_only(tree, &expanded, cid, 4, y, left_w - 8, out);
        }
    }

    // 우측: Explorer 패널 (상단 영역, active_folder 내용 list).
    //
    // M8 T8.16: Explorer.state.scroll_y (라인 단위) 만큼 *시작 y를 위로 밀어* clip.
    // `if y > top_h { break }`는 그대로 — scroll 적용 후에도 가시 영역 끝까지만 push해서 layout 비용 절감.
    if let Some(ex) = find_child_by_type(tree, obj, "aios.builtin/Explorer@1") {
        out.push((ex.id, Rect { x: left_w, y: 0, w: right_w, h: top_h }, HitRole::Body));
        let scroll_y = ex.state.get("scroll_y").and_then(|v| v.as_i64()).unwrap_or(0).max(0) as i32;
        let scroll_px = scroll_y * 24;

        // Parent nav row — active_folder가 설정된 경우만 첫 줄에 "/" 행을 push.
        // root(드라이브 일람)일 때는 active_folder가 없거나 빈 문자열이므로 row 없음.
        // scroll 영향 안 받음 — 항상 Explorer 상단 고정.
        let has_active_folder = ex
            .state
            .get("active_folder")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        let parent_row_h = if has_active_folder { 24 } else { 0 };
        if has_active_folder {
            out.push((
                ex.id,
                Rect { x: left_w + 4, y: 4, w: right_w - 8, h: 24 },
                HitRole::ExplorerParentNav,
            ));
        }

        // active_folder의 children을 24px 라인으로 layout — parent_row 아래부터 시작.
        //
        // **중요**: scroll_px > 0인 경우 child rect의 y가 ParentNav (y=4, h=24) 영역과
        // *겹칠 수 있음*. push 순서가 (Body, ParentNav, child1, child2, ...)인데 child rect가
        // ParentNav rect와 같은 좌표면 hit_test 역순에서 *child가 ParentNav를 가린다* — 사용자가
        // ParentNav를 클릭하려 해도 child가 매칭. 그래서 child rect의 *top이 parent_row 영역
        // 안에 들어가면 그 child는 layout에서 제외*한다 (스크롤로 사라진 행). 이렇게 하면
        // hit_test/render 모두 일관적으로 child를 무시한다.
        let kids = explorer_children(tree, ex);
        let visible_top = 4 + parent_row_h;
        let mut y = 4i32 + parent_row_h - scroll_px;
        for child_id in kids {
            if y >= visible_top {
                out.push((
                    child_id,
                    Rect { x: left_w + 4, y, w: right_w - 8, h: 24 },
                    HitRole::Body,
                ));
            }
            y += 24;
            if y > top_h {
                break;
            }
        }
    }

    // 하단: CLI 패널 (풀폭).
    if has_cli {
        if let Some(cli) = find_child_by_type(tree, obj, "aios.builtin/Cli@1") {
            out.push((cli.id, Rect { x: 0, y: top_h, w: win_w, h: bottom_h }, HitRole::Body));
        }
    }

    // Window 오버레이 — z 오름차순 정렬 → 마지막에 push (그리는 순서 = z).
    // state.destroyed=true는 close된 Window (T8.10) — layout/hit_test 모두에서 제외해
    // 시각적으로 사라지고 클릭도 안 됨. proto에 DestroyMsg가 없어 desktop-shell이
    // SetState로 우회한 결과 (KI-011 tombstone과 형식 일치).
    let mut windows: Vec<&geulos_core::Object> = obj
        .children
        .iter()
        .filter_map(|&id| tree.get(id))
        .filter(|o| o.type_uri.as_str() == "aios.builtin/Window@1")
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
    let mut kids: Vec<ObjectId> = folder.children.clone();
    // 폴더 먼저 (false < true), 그 다음 이름순.
    kids.sort_by_key(|id| {
        tree.get(*id)
            .map(|o| {
                let is_folder = o.type_uri.as_str() == "aios.std/Folder@1";
                let name = o.props.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
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

#[cfg(test)]
mod tests {
    use super::*;
    use geulos_core::{std_types, ActorId};
    use serde_json::json;

    /// Desktop + FileTree + Explorer 최소 트리에 active_folder를 set하면 layout이
    /// Explorer 상단에 ExplorerParentNav rect를 push해야 한다.
    #[test]
    fn explorer_parent_nav_row_pushed_when_active_folder_set() {
        let owner = ActorId::local_user();
        let mut desktop = std_types::desktop(owner.clone());
        let ft = std_types::file_tree(owner.clone(), "/");
        let mut ex = std_types::explorer(owner);
        let dummy_folder_id = ObjectId::new();
        ex.state.insert("active_folder".to_string(), json!(dummy_folder_id.to_string()));
        ex.parent = Some(desktop.id);
        let ft_with_parent = {
            let mut f = ft;
            f.parent = Some(desktop.id);
            f
        };
        desktop.children = vec![ft_with_parent.id, ex.id];
        let ex_id = ex.id;
        ex.parent = Some(desktop.id);

        let mut tree = TreeModel::new();
        tree.upsert(desktop);
        tree.upsert(ft_with_parent);
        tree.upsert(ex);

        let lay = layout(&tree, 1024, 768);
        let parent_nav = lay
            .rects
            .iter()
            .find(|(id, _, role)| *id == ex_id && *role == HitRole::ExplorerParentNav);
        assert!(parent_nav.is_some(), "active_folder set → ExplorerParentNav 행 push");
        let (_, rect, _) = parent_nav.unwrap();
        assert_eq!(rect.h, 24, "parent_row 높이 24px");
        assert_eq!(rect.y, 4, "Explorer 상단 첫 줄 (y=4)");
    }

    /// active_folder가 빈 string이거나 없으면 (드라이브 일람) ExplorerParentNav 행이 없다.
    #[test]
    fn explorer_no_parent_nav_row_at_root() {
        let owner = ActorId::local_user();
        let mut desktop = std_types::desktop(owner.clone());
        let mut ft = std_types::file_tree(owner.clone(), "/");
        let mut ex = std_types::explorer(owner);
        ft.parent = Some(desktop.id);
        ex.parent = Some(desktop.id);
        desktop.children = vec![ft.id, ex.id];
        let ex_id = ex.id;

        let mut tree = TreeModel::new();
        tree.upsert(desktop);
        tree.upsert(ft);
        tree.upsert(ex);

        let lay = layout(&tree, 1024, 768);
        let parent_nav = lay
            .rects
            .iter()
            .find(|(id, _, role)| *id == ex_id && *role == HitRole::ExplorerParentNav);
        assert!(parent_nav.is_none(), "active_folder 없음 → ExplorerParentNav 행 없음");
    }

    /// scroll_y로 child rect가 ParentNav (y=4, h=24) 영역과 겹치면 그 child는 layout에서
    /// 제외되어야 한다 — push되지 않아야 hit_test가 그 child를 잘못 매칭하지 않는다.
    #[test]
    fn explorer_child_skipped_when_scrolled_into_parent_nav_area() {
        let owner = ActorId::local_user();
        let mut desktop = std_types::desktop(owner.clone());
        let mut ft = std_types::file_tree(owner.clone(), "/");
        let mut ex = std_types::explorer(owner.clone());
        ft.parent = Some(desktop.id);
        ex.parent = Some(desktop.id);

        let mut active = std_types::folder(owner.clone(), "/x", "x", 0);
        let c1 = std_types::file(owner.clone(), "/x/a.txt", "a.txt", "text/plain", 0);
        let c2 = std_types::file(owner.clone(), "/x/b.txt", "b.txt", "text/plain", 0);
        let c3 = std_types::file(owner, "/x/c.txt", "c.txt", "text/plain", 0);
        let (c1_id, c2_id, c3_id) = (c1.id, c2.id, c3.id);
        active.children = vec![c1_id, c2_id, c3_id];
        ex.state.insert("active_folder".to_string(), json!(active.id.to_string()));
        // scroll_y=1 — 첫 child를 ParentNav 영역으로 밀어넣음.
        ex.state.insert("scroll_y".to_string(), json!(1));
        desktop.children = vec![ft.id, ex.id];

        let mut tree = TreeModel::new();
        tree.upsert(desktop);
        tree.upsert(ft);
        tree.upsert(ex);
        tree.upsert(active);
        tree.upsert(c1);
        tree.upsert(c2);
        tree.upsert(c3);

        let lay = layout(&tree, 1024, 768);
        // c1은 ParentNav 영역과 겹쳐 skip되어야 함.
        let c1_rect = lay.rects.iter().find(|(id, _, _)| *id == c1_id);
        assert!(c1_rect.is_none(), "scroll로 ParentNav 침범 → c1 skip");
        // c2/c3는 정상 visible (y=28, 52).
        let c2_rect = lay.rects.iter().find(|(id, _, _)| *id == c2_id).map(|(_, r, _)| *r).unwrap();
        let c3_rect = lay.rects.iter().find(|(id, _, _)| *id == c3_id).map(|(_, r, _)| *r).unwrap();
        assert_eq!(c2_rect.y, 28, "scroll 후 c2가 첫 가시 줄");
        assert_eq!(c3_rect.y, 52);
    }

    /// ExplorerParentNav가 있을 때 children rect들은 *24px 아래로* offset된다.
    /// (parent_row + 24만큼 첫 child y가 밀려야 겹치지 않음.)
    #[test]
    fn explorer_children_offset_by_parent_row_height() {
        let owner = ActorId::local_user();
        let mut desktop = std_types::desktop(owner.clone());
        let mut ft = std_types::file_tree(owner.clone(), "/");
        let mut ex = std_types::explorer(owner.clone());
        ft.parent = Some(desktop.id);
        ex.parent = Some(desktop.id);

        // active_folder = 실제 폴더 객체 (children 포함).
        let mut active = std_types::folder(owner.clone(), "/x", "x", 0);
        let child_file = std_types::file(owner, "/x/a.txt", "a.txt", "text/plain", 0);
        let child_id = child_file.id;
        active.children = vec![child_id];
        ex.state.insert("active_folder".to_string(), json!(active.id.to_string()));
        desktop.children = vec![ft.id, ex.id];

        let mut tree = TreeModel::new();
        tree.upsert(desktop);
        tree.upsert(ft);
        tree.upsert(ex);
        tree.upsert(active);
        tree.upsert(child_file);

        let lay = layout(&tree, 1024, 768);
        let child_rect =
            lay.rects.iter().find(|(id, _, _)| *id == child_id).map(|(_, r, _)| *r).unwrap();
        // parent_row 24 + 시작 padding 4 = 28부터 첫 자식.
        assert_eq!(child_rect.y, 28, "첫 자식 y = parent_row 24 + 4 padding");
    }
}
