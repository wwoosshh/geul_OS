//! 좌표 → 클릭 가능한 ObjectId (가장 안쪽).

use geulos_core::ObjectId;

use crate::layout::LayoutResult;
use crate::tree_model::TreeModel;

/// 주어진 좌표에 있는 클릭 가능한 객체를 반환.
///
/// "클릭 가능"의 정의: methods가 비어있지 않은 객체 (즉 Button 등).
/// Container는 클릭 통과시킴.
pub fn hit_test(tree: &TreeModel, layout: &LayoutResult, px: i32, py: i32) -> Option<ObjectId> {
    // layout.rects는 부모-자식 순서로 출력되므로 *뒤에서부터* 검사 (자식이 위)
    let mut candidates: Vec<_> = layout.iter().collect();
    candidates.reverse();
    for (id, rect) in candidates {
        if rect.contains(px, py) {
            if let Some(obj) = tree.get(id) {
                if !obj.methods.is_empty() {
                    return Some(id);
                }
            }
        }
    }
    None
}
