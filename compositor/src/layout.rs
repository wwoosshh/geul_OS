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
#[allow(clippy::too_many_arguments)]
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

/// Desktop 루트를 좌(30%) / 우(70%)로 분할 배치.
/// 자식 순서는 [FileTree, Canvas]로 가정.
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
    let left_w = (win_w as f32 * 0.30) as i32;
    let right_w = win_w - left_w;

    // 좌측: FileTree 패널.
    if let Some(&ft_id) = obj.children.first() {
        out.push((ft_id, Rect { x: 0, y: 0, w: left_w, h: win_h }));
        let expanded = extract_expanded(tree, ft_id);
        if let Some(ft) = tree.get(ft_id) {
            let mut y = 4i32;
            for &cid in &ft.children {
                y += layout_tree_node(tree, &expanded, cid, 4, y, left_w - 8, out);
            }
        }
    }

    // 우측: Canvas 패널.
    if let Some(&cv_id) = obj.children.get(1) {
        out.push((cv_id, Rect { x: left_w, y: 0, w: right_w, h: win_h }));
        if let Some(cv) = tree.get(cv_id) {
            if let Some(active_app) = cv.state.get("active_app").and_then(|v| v.as_str()) {
                if let Ok(uuid) = uuid::Uuid::parse_str(active_app) {
                    let app_id = ObjectId::from_uuid(uuid);
                    layout_object(tree, app_id, left_w, 0, right_w, out);
                }
            }
        }
    }
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
