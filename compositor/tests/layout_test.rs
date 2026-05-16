use geulos_compositor::layout::layout;
use geulos_compositor::tree_model::TreeModel;
use geulos_core::{std_types, ActorId};

#[test]
fn empty_tree_yields_empty_layout() {
    let tm = TreeModel::new();
    let r = layout(&tm, 800, 600);
    assert_eq!(r.rects.len(), 0);
}

#[test]
fn single_text_assigned_height_40() {
    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    let t = std_types::text(owner, "hi");
    let id = t.id;
    tm.upsert(t);
    let r = layout(&tm, 800, 600);
    let rect = r.get(id).unwrap();
    assert_eq!(rect.h, 40);
}

#[test]
fn container_with_text_and_button_vstacks() {
    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    let mut c = std_types::container(owner.clone());
    let mut text = std_types::text(owner.clone(), "count: 0");
    let mut button = std_types::button(owner, "+1");

    let c_id = c.id;
    let text_id = text.id;
    let button_id = button.id;

    text.parent = Some(c_id);
    button.parent = Some(c_id);
    c.children.push(text_id);
    c.children.push(button_id);

    tm.upsert(c);
    tm.upsert(text);
    tm.upsert(button);

    let r = layout(&tm, 800, 600);
    let trect = r.get(text_id).unwrap();
    let brect = r.get(button_id).unwrap();
    // text는 button 위에 있어야 함
    assert!(trect.y < brect.y);
    // padding/spacing 적용된 만큼 x도 16 이상
    assert!(trect.x >= 16);
}

#[test]
fn click_hit_test_via_rect_contains() {
    use geulos_compositor::layout::Rect;
    let r = Rect { x: 10, y: 20, w: 30, h: 40 };
    assert!(r.contains(15, 25));
    assert!(!r.contains(5, 25));
    assert!(!r.contains(15, 65));
}
