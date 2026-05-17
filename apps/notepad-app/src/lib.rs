//! notepad-app 라이브러리: 메모 트리 구성 + 편집 로직.
//!
//! T3에서 본격 구현. T2 시점에는 진입점만.

use geulos_core::{std_types, ActorId, Object, ObjectId};

/// notepad의 초기 UI 트리를 만든다.
///
/// 트리 구조:
/// ```text
/// MemoList (root)
///   └─ TextArea (활성 메모용 — 처음엔 bound_memo가 nil ObjectId)
/// ```
///
/// 실제 Memo 객체들은 fs::load_all()이 디스크에서 발견해 *후속 mount*.
///
/// T2 단계에서는 *최소 구조*만. T3에서 활성 메모 추적 + Memo 생성 로직 추가.
pub fn build_initial_tree(owner: ActorId) -> (Object, Object) {
    let memo_list = std_types::memo_list(owner.clone());
    // bound_memo는 일단 nil ObjectId (TextArea가 비활성 상태 표시).
    // 실제 활성 메모가 결정되면 set_active로 갱신.
    let text_area = std_types::text_area(owner, ObjectId::nil());
    (memo_list, text_area)
}
