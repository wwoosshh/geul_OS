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

/// 레이아웃 결과: ObjectId → Rect 매핑.
#[derive(Debug, Default)]
pub struct LayoutResult {
    pub rects: Vec<(ObjectId, Rect)>,
}

impl LayoutResult {
    pub fn get(&self, id: ObjectId) -> Option<Rect> {
        self.rects.iter().find(|(i, _)| *i == id).map(|(_, r)| *r)
    }

    pub fn iter(&self) -> impl Iterator<Item = (ObjectId, Rect)> + '_ {
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
    out: &mut Vec<(ObjectId, Rect)>,
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
        out.insert(container_idx, (id, Rect { x, y, w: avail_w, h: total_h }));
        total_h
    } else {
        let h = item_height(&obj.type_uri);
        out.push((id, Rect { x, y, w: avail_w, h }));
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
    out: &mut Vec<(ObjectId, Rect)>,
) -> i32 {
    let obj = match tree.get(id) {
        Some(o) => o,
        None => return 0,
    };
    let mut cur_y = y;
    let h = item_height(&obj.type_uri);
    out.push((id, Rect { x, y: cur_y, w: avail_w, h }));
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
    out: &mut Vec<(ObjectId, Rect)>,
) {
    let obj = match tree.get(id) {
        Some(o) => o,
        None => return,
    };
    // Desktop 자체 rect (윈도우 전체).
    out.push((id, Rect { x: 0, y: 0, w: win_w, h: win_h }));
    let left_w = (win_w as f32 * 0.25) as i32; // M8: 좌 25%
    let right_w = win_w - left_w;

    let has_cli = obj
        .children
        .iter()
        .any(|&cid| tree.get(cid).map(|o| o.type_uri.as_str()) == Some("aios.builtin/Cli@1"));
    let top_h = if has_cli { (win_h as f32 * 0.70) as i32 } else { win_h };
    let bottom_h = win_h - top_h;

    // 좌측: FileTree 패널 (상단 영역, 폴더만 — File 노드 skip).
    if let Some(ft) = find_child_by_type(tree, obj, "aios.builtin/FileTree@1") {
        out.push((ft.id, Rect { x: 0, y: 0, w: left_w, h: top_h }));
        let expanded = extract_expanded(tree, ft.id);
        let mut y = 4i32;
        for &cid in &ft.children {
            y += layout_tree_node_folders_only(tree, &expanded, cid, 4, y, left_w - 8, out);
        }
    }

    // 우측: Explorer 패널 (상단 영역, active_folder 내용 list).
    if let Some(ex) = find_child_by_type(tree, obj, "aios.builtin/Explorer@1") {
        out.push((ex.id, Rect { x: left_w, y: 0, w: right_w, h: top_h }));
        // active_folder의 children을 24px 라인으로 layout.
        let kids = explorer_children(tree, ex);
        let mut y = 4i32;
        for child_id in kids {
            out.push((child_id, Rect { x: left_w + 4, y, w: right_w - 8, h: 24 }));
            y += 24;
            if y > top_h {
                break;
            }
        }
    }

    // 하단: CLI 패널 (풀폭).
    if has_cli {
        if let Some(cli) = find_child_by_type(tree, obj, "aios.builtin/Cli@1") {
            out.push((cli.id, Rect { x: 0, y: top_h, w: win_w, h: bottom_h }));
        }
    }

    // Window 오버레이 — z 오름차순 정렬 → 마지막에 push (그리는 순서 = z).
    let mut windows: Vec<&geulos_core::Object> = obj
        .children
        .iter()
        .filter_map(|&id| tree.get(id))
        .filter(|o| o.type_uri.as_str() == "aios.builtin/Window@1")
        .collect();
    windows.sort_by_key(|w| w.state.get("z").and_then(|v| v.as_i64()).unwrap_or(0));
    for w in windows {
        let x = w.state.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let y = w.state.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let wid = w.state.get("w").and_then(|v| v.as_i64()).unwrap_or(600) as i32;
        let hgt = w.state.get("h").and_then(|v| v.as_i64()).unwrap_or(400) as i32;
        out.push((w.id, Rect { x, y, w: wid, h: hgt }));
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
#[allow(clippy::too_many_arguments)]
fn layout_tree_node_folders_only(
    tree: &TreeModel,
    expanded: &[ObjectId],
    id: ObjectId,
    x: i32,
    y: i32,
    avail_w: i32,
    out: &mut Vec<(ObjectId, Rect)>,
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
    out.push((id, Rect { x, y: cur_y, w: avail_w, h }));
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
