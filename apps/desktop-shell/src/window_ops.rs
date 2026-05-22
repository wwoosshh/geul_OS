//! Window 객체 lifecycle 헬퍼 (M8 T8.7).
//!
//! Explorer.open_file 처리 시 *기존 Window 중복 검출 / 새 Window geometry 결정 /
//! 새 Window 객체 구축* 같은 순수 로직만 분리. 디스크/네트워크 부수효과는 호출부
//! (`main.rs`)가 담당. 모든 함수는 `mounted_objects` 슬라이스만 보고 결론을 내므로
//! 단위 테스트에서 자유롭게 호출 가능.

use geulos_core::{std_types, ActorId, Object, ObjectId};
use serde_json::json;

use crate::invoke_handler::InvokeOutcome;

/// 같은 파일을 이미 열어둔 Window가 있으면 그 ID. 없으면 None.
///
/// 비교 기준은 Window.props.file_id (string). `std_types::window`가
/// `set_prop("file_id", json!(file_id))`로 직렬화하므로 ObjectId Display와 동일한 UUID
/// 문자열. 같은 파일을 두 번 open했을 때 새 Window를 만들지 *기존 것을 focus만 할지*
/// 결정하는 데 사용.
pub fn find_window_for_file(mounted_objects: &[Object], file_id: ObjectId) -> Option<ObjectId> {
    let key = file_id.to_string();
    mounted_objects
        .iter()
        .find(|o| {
            o.type_uri.as_str() == "aios.builtin/Window@1"
                && o.props.get("file_id").and_then(|v| v.as_str()) == Some(key.as_str())
        })
        .map(|o| o.id)
}

/// 현재 mounted Window들 중 최대 z. 없으면 0.
///
/// 첫 Window는 max_z(0) + 1 = 1로 시작. 매 focus마다 +1씩 단조 증가하므로 i32
/// overflow는 사실상 발생 X (2^31번 클릭이 필요).
pub fn max_z(mounted_objects: &[Object]) -> i32 {
    mounted_objects
        .iter()
        .filter(|o| o.type_uri.as_str() == "aios.builtin/Window@1")
        .filter_map(|o| o.state.get("z").and_then(|v| v.as_i64()))
        .max()
        .map(|z| z as i32)
        .unwrap_or(0)
}

/// Cascade 위치 — 마지막 Window의 위치 + (30, 30). 첫 Window는 default.
///
/// `last()` 기준은 mount 순서(=Vec 삽입 순). z-order와 분리 — 가장 *최근에 생성된* 윈도우
/// 옆에 새 윈도우가 떨어진다. 사용자가 같은 파일을 여러 번 열어도 시각적으로 겹치지
/// 않도록 +30 px 오프셋. 화면 밖까지 cascade되는 wrap 로직은 M9+에서 도입.
pub fn next_window_position(mounted_objects: &[Object], default: (i32, i32)) -> (i32, i32) {
    // 뒤에서부터 첫 Window — DoubleEndedIterator라 O(1). filter().last() 대신 rfind를 쓰는
    // 이유는 clippy::filter_next/double_ended_iterator_last lint 회피.
    let last = mounted_objects.iter().rfind(|o| o.type_uri.as_str() == "aios.builtin/Window@1");
    match last {
        Some(w) => {
            let x = w.state.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32 + 30;
            let y = w.state.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32 + 30;
            (x, y)
        }
        None => default,
    }
}

/// 새 Window 객체 (Desktop 자식, focused, max z + 1).
///
/// 호출부에서 ACL 부여 / mount 송신 / mounted_objects 삽입을 따로 수행. 이 함수는
/// 순수 객체 구축만 책임.
///
/// 7개 인자 — Window를 *완전한 초기 상태*(owner / parent / 표시 파일 / 제목 / 위치 /
/// 크기 / z)로 한 번에 정의. 구조체로 묶으면 호출부 가독성이 오히려 떨어져 `std_types::window`와
/// 동일하게 clippy 한정 허용.
#[allow(clippy::too_many_arguments)]
pub fn build_new_window(
    owner: &ActorId,
    desktop_id: ObjectId,
    file_id: ObjectId,
    title: &str,
    pos: (i32, i32),
    size: (i32, i32),
    new_z: i32,
) -> Object {
    let mut w = std_types::window(owner.clone(), title, file_id, pos.0, pos.1, size.0, size.1);
    w.parent = Some(desktop_id);
    w.set_state("z", serde_json::json!(new_z));
    w.set_state("focused", serde_json::json!(true));
    w
}

/// Window.toggle_edit — `edit_mode` 상태를 flip (M9 / ADR-035).
///
/// 순수 함수: 현재 값을 받아 *반대* 값을 SetState로 반환. main.rs는 이 함수 호출 전후로
/// `mounted_objects`의 사본도 동기 갱신해 다음 invoke 처리 시점에 일관된 값이 보이도록 한다.
/// dirty 처리는 별도 — toggle 자체는 dirty를 건드리지 않는다 (사용자가 아직 *입력*하지 않았으므로).
pub fn handle_toggle_edit(window_id: ObjectId, current_edit_mode: bool) -> InvokeOutcome {
    let new_mode = !current_edit_mode;
    InvokeOutcome { state_sets: vec![(window_id, "edit_mode".to_string(), json!(new_mode))] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn find_window_for_file_matches_existing() {
        let owner = ActorId::local_user();
        let fid = ObjectId::new();
        let w = std_types::window(owner, "x", fid, 0, 0, 100, 100);
        let wid = w.id;
        let mounted = vec![w];
        assert_eq!(find_window_for_file(&mounted, fid), Some(wid));
    }

    #[test]
    fn max_z_returns_zero_when_empty() {
        assert_eq!(max_z(&[]), 0);
    }

    #[test]
    fn max_z_finds_highest() {
        let owner = ActorId::local_user();
        let fid = ObjectId::new();
        let mut w1 = std_types::window(owner.clone(), "a", fid, 0, 0, 1, 1);
        w1.set_state("z", json!(3));
        let mut w2 = std_types::window(owner, "b", fid, 0, 0, 1, 1);
        w2.set_state("z", json!(7));
        assert_eq!(max_z(&[w1, w2]), 7);
    }

    #[test]
    fn next_position_cascades_30_30() {
        let owner = ActorId::local_user();
        let fid = ObjectId::new();
        let w = std_types::window(owner, "a", fid, 100, 80, 1, 1);
        assert_eq!(next_window_position(&[w], (50, 40)), (130, 110));
    }

    #[test]
    fn next_position_uses_default_when_empty() {
        assert_eq!(next_window_position(&[], (50, 40)), (50, 40));
    }

    #[test]
    fn toggle_edit_flips_value() {
        let id = ObjectId::new();
        let o = handle_toggle_edit(id, false);
        assert_eq!(o.state_sets.len(), 1);
        assert_eq!(o.state_sets[0].0, id);
        assert_eq!(o.state_sets[0].1, "edit_mode");
        assert_eq!(o.state_sets[0].2, json!(true));

        let o2 = handle_toggle_edit(id, true);
        assert_eq!(o2.state_sets[0].2, json!(false));
    }
}
