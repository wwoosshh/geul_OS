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
        _ => 0, // Container는 자체 크기 없음 (자식의 합으로 계산)
    }
}

const PADDING: i32 = 16;
const SPACING: i32 = 8;

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
        // vstack: 자식들을 세로로 배치, 자기 높이는 자식 합 + padding
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
        out.push((id, Rect { x, y, w: avail_w, h: total_h }));
        total_h
    } else {
        let h = item_height(&obj.type_uri);
        out.push((id, Rect { x, y, w: avail_w, h }));
        h
    }
}

/// 전체 트리를 레이아웃. roots의 첫 객체가 윈도우 채움. 나머지 roots는 그 아래로.
pub fn layout(tree: &TreeModel, win_w: i32, win_h: i32) -> LayoutResult {
    let mut out = Vec::new();
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
