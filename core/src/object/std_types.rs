//! 표준 객체 팩토리 함수.
//!
//! 모든 GeulOS 앱이 기본적으로 사용하게 되는 표준 객체 타입:
//! `Container`, `Text`, `Button`, `Toggle` (M3),
//! `Memo`, `TextArea`, `MemoList` (M7 — 메모장 도그푸딩).

use serde_json::json;

use super::identity::{ActorId, ObjectId, TypeUri};
use super::method::{ArgSpec, MethodSig};
use super::Object;

/// 레이아웃 컨테이너. 자식 객체를 담는 용도.
pub fn container(owner: ActorId) -> Object {
    Object::new(TypeUri::parse("aios.std/Container@1").expect("유효한 TypeUri"), owner)
}

/// 텍스트 표시 객체.
pub fn text(owner: ActorId, content: &str) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.std/Text@1").expect("유효한 TypeUri"), owner);
    obj.set_state("content", json!(content));
    obj
}

/// 누를 수 있는 버튼.
pub fn button(owner: ActorId, label: &str) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.std/Button@1").expect("유효한 TypeUri"), owner);
    obj.set_state("label", json!(label));
    obj.methods.push(MethodSig::new("press"));
    obj
}

/// 켜고 끄는 토글.
pub fn toggle(owner: ActorId, initial: bool) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.std/Toggle@1").expect("유효한 TypeUri"), owner);
    obj.set_state("on", json!(initial));
    obj.methods.push(MethodSig::new("toggle"));
    obj.methods.push(MethodSig::new("set"));
    obj
}

// ───────────────────────── M7: 메모장 타입 ─────────────────────────

/// 메모 한 건.
///
/// state:
/// - `title: String` — 메모 제목
/// - `body: String` — 메모 본문 (UTF-8, byte index 기반 편집)
/// - `created_at: i64` — Unix ms 생성 시각
/// - `updated_at: i64` — Unix ms 마지막 수정 시각
/// - `tags: [String]` — 사용자 또는 AI가 부여한 태그
///
/// 메서드:
/// - `insert_text(at: usize, text: String)` — body의 byte index `at`에 `text` 삽입
/// - `delete_range(from: usize, to: usize)` — body의 [from, to) byte 범위 삭제
/// - `set_title(title: String)` — 제목 변경
/// - `set_tags(tags: [String])` — 태그 교체 (병합 아님)
/// - `save()` — 영속 저장 (notepad-app이 fs로 flush)
pub fn memo(owner: ActorId, title: &str, created_at_ms: i64) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.std/Memo@1").expect("유효한 TypeUri"), owner);
    obj.set_state("title", json!(title));
    obj.set_state("body", json!(""));
    obj.set_state("created_at", json!(created_at_ms));
    obj.set_state("updated_at", json!(created_at_ms));
    obj.set_state("tags", json!([] as [&str; 0]));

    obj.methods.push(
        MethodSig::new("insert_text")
            .with_arg(ArgSpec::new("at", "usize"))
            .with_arg(ArgSpec::new("text", "string")),
    );
    obj.methods.push(
        MethodSig::new("delete_range")
            .with_arg(ArgSpec::new("from", "usize"))
            .with_arg(ArgSpec::new("to", "usize")),
    );
    obj.methods.push(MethodSig::new("set_title").with_arg(ArgSpec::new("title", "string")));
    obj.methods.push(MethodSig::new("set_tags").with_arg(ArgSpec::new("tags", "[string]")));
    obj.methods.push(MethodSig::new("save"));
    obj
}

/// 편집 가능한 텍스트 위젯. *컴포지터가 직접 다루며 와이어 메서드는 노출하지 않음*.
///
/// props:
/// - `bound_memo: ObjectId` — 이 TextArea가 보여주는 Memo 객체
///
/// state (compositor가 갱신):
/// - `cursor_pos: usize` — body 안 커서 위치 (byte index)
/// - `selection: Option<[usize, usize]>` — 선택 영역
/// - `focused: bool` — 키보드 입력 수신 여부
///
/// 사용자/AI가 body를 *직접* 변경하지 않고 bound_memo의 메서드를 호출 — 그래야 단일
/// 라이터 이벤트 루프와 영속성 모델이 흐트러지지 않는다.
pub fn text_area(owner: ActorId, bound_memo: ObjectId) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.std/TextArea@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("bound_memo", json!(bound_memo));
    obj.set_state("cursor_pos", json!(0));
    obj.set_state("selection", json!(null));
    obj.set_state("focused", json!(false));
    obj
}

/// 메모 목록 컨테이너. notepad-app이 루트로 mount, 자식이 Memo 객체들.
///
/// state:
/// - `active_memo: Option<ObjectId>` — 현재 편집 중인 메모
///
/// 메서드:
/// - `create_memo(title: String)` — 새 Memo 생성 + 자식으로 추가
/// - `delete_memo(id: ObjectId)` — Memo destroy + fs 파일 제거
/// - `set_active(id: ObjectId)` — TextArea의 bound_memo를 갱신
pub fn memo_list(owner: ActorId) -> Object {
    let mut obj =
        Object::new(TypeUri::parse("aios.std/MemoList@1").expect("유효한 TypeUri"), owner);
    obj.set_state("active_memo", json!(null));

    obj.methods.push(MethodSig::new("create_memo").with_arg(ArgSpec::new("title", "string")));
    obj.methods.push(MethodSig::new("delete_memo").with_arg(ArgSpec::new("id", "ObjectId")));
    obj.methods.push(MethodSig::new("set_active").with_arg(ArgSpec::new("id", "ObjectId")));
    obj
}
