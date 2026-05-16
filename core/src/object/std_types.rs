//! 표준 객체 팩토리 함수.
//!
//! 모든 GeulOS 앱이 기본적으로 사용하게 되는 4가지 기본 객체 타입:
//! `Container`, `Text`, `Button`, `Toggle`.

use serde_json::json;

use super::identity::{ActorId, TypeUri};
use super::method::MethodSig;
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
