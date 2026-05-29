//! 좌표 → 클릭 가능한 (ObjectId, HitRole) (가장 안쪽).

use geulos_core::ObjectId;

use crate::layout::{HitRole, LayoutResult};
use crate::tree_model::TreeModel;

/// 주어진 좌표에 있는 클릭 가능한 객체 + 역할을 반환.
///
/// 역순 iterate — `layout`이 부모를 먼저 push (Container는 자식보다 앞에 insert,
/// Window는 z 오름차순으로 마지막에 push)하므로, 뒤에서부터 검사하면 가장 위에 그려진
/// 객체가 우선 매칭된다. M8: Window z가 큰 것부터 hit.
///
/// **HitRole 우선순위** (M8 회귀 fix #2): 좌측 트리의 폴더는 동일 ObjectId에 두 rect
/// (Body + ExpandToggle)를 push하고, ExpandToggle이 *나중에* push되므로 역순에서 먼저
/// 검사된다. 사용자 클릭이 toggle 영역(36px)에 들면 `(id, ExpandToggle)`이 반환되고, 그
/// 외 영역(폴더명)에서는 toggle이 contains() 검사 실패 → Body가 매칭되어 `(id, Body)`.
///
/// 컨테이너성 타입(Desktop, FileTree)은 hit 무시 — 실제 클릭 대상은 자식 (Folder/File).
/// Explorer/Cli는 skip 안 함 — 빈 영역 클릭 시 자체가 target이 되어 dispatch_click의
/// fallback 분기(첫 메서드, args=null)로 흘러가지만 안전(noop). M3 echo-app 호환을 위해
/// `methods.is_empty()` 필터는 두지 않는다(Container는 methods 비어있어도 reverse 순서로
/// 자식이 먼저 매칭되어 자연 skip된다).
pub fn hit_test(
    tree: &TreeModel,
    layout: &LayoutResult,
    px: i32,
    py: i32,
) -> Option<(ObjectId, HitRole)> {
    // M9 T7: Dialog가 떠 있으면 *modal* — Dialog rect 안만 매칭, 밖이면 None.
    // 다른 Window/CLI/Explorer로 클릭이 흘러가지 못하게 차단해 confirm/warn 흐름이 깨지지
    // 않게 한다. layout이 Dialog를 *마지막에* push하므로 역순 lookup으로 즉시 찾는다.
    let dialog_rect = layout.rects.iter().rev().find_map(|(id, r, _role)| {
        let obj = tree.get(*id)?;
        if obj.type_uri.as_str() == "aios.builtin/Dialog@1"
            && !obj.state.get("destroyed").and_then(|v| v.as_bool()).unwrap_or(false)
            && obj.state.get("result").map(|v| v.is_null()).unwrap_or(true)
        {
            Some((*id, *r))
        } else {
            None
        }
    });
    if let Some((dialog_id, dr)) = dialog_rect {
        return if dr.contains(px, py) { Some((dialog_id, HitRole::Body)) } else { None };
    }

    // `LayoutResult::iter()`가 DoubleEndedIterator를 노출하지 않으므로 underlying Vec를
    // 직접 역순 순회.
    for (id, rect, role) in layout.rects.iter().rev() {
        if !rect.contains(px, py) {
            continue;
        }
        if let Some(obj) = tree.get(*id) {
            let uri = obj.type_uri.as_str();
            // 컨테이너성 타입은 hit 무시 (자식이 진짜 target) — 단 Desktop이 소유한
            // CliResizeHandle은 *실제 클릭 대상*이므로 예외 (set_cli_height invoke 경로).
            if matches!(uri, "aios.builtin/Desktop@1" | "aios.builtin/FileTree@1")
                && *role != HitRole::CliResizeHandle
            {
                continue;
            }
        }
        return Some((*id, *role));
    }
    None
}
