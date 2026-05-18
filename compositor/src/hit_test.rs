//! 좌표 → 클릭 가능한 ObjectId (가장 안쪽).

use geulos_core::ObjectId;

use crate::layout::LayoutResult;
use crate::tree_model::TreeModel;

/// 주어진 좌표에 있는 클릭 가능한 객체를 반환.
///
/// 역순 iterate — `layout`이 부모를 먼저 push (Container는 자식보다 앞에 insert,
/// Window는 z 오름차순으로 마지막에 push)하므로, 뒤에서부터 검사하면 가장 위에 그려진
/// 객체가 우선 매칭된다. M8: Window z가 큰 것부터 hit.
///
/// 컨테이너성 타입(Desktop, FileTree)은 hit 무시 — 실제 클릭 대상은 자식 (Folder/File).
/// Explorer/Cli는 skip 안 함 — 빈 영역 클릭 시 자체가 target이 되어 dispatch_click의
/// fallback 분기(첫 메서드, args=null)로 흘러가지만 안전(noop). M3 echo-app 호환을 위해
/// `methods.is_empty()` 필터는 두지 않는다(Container는 methods 비어있어도 reverse 순서로
/// 자식이 먼저 매칭되어 자연 skip된다).
pub fn hit_test(tree: &TreeModel, layout: &LayoutResult, px: i32, py: i32) -> Option<ObjectId> {
    // `LayoutResult::iter()`가 DoubleEndedIterator를 노출하지 않으므로 underlying Vec를
    // 직접 역순 순회.
    for (id, rect) in layout.rects.iter().rev() {
        if !rect.contains(px, py) {
            continue;
        }
        if let Some(obj) = tree.get(*id) {
            let uri = obj.type_uri.as_str();
            // 컨테이너성 타입은 hit 무시 (자식이 진짜 target).
            if matches!(uri, "aios.builtin/Desktop@1" | "aios.builtin/FileTree@1") {
                continue;
            }
        }
        return Some(*id);
    }
    None
}
