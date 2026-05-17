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

#[test]
fn hit_test_finds_button_not_container() {
    use geulos_compositor::hit_test::hit_test;

    let mut tm = TreeModel::new();
    let owner = ActorId::local_user();
    let mut c = std_types::container(owner.clone());
    let mut text = std_types::text(owner.clone(), "x");
    let mut button = std_types::button(owner, "press me");

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
    let btn_rect = r.get(button_id).unwrap();
    let cx = btn_rect.x + 10;
    let cy = btn_rect.y + 10;
    let hit = hit_test(&tm, &r, cx, cy);
    assert_eq!(hit, Some(button_id), "Button을 hit해야 함, Container를 지나가야 함");
}

// ───────────────────────── M7 T4: Desktop 좌/우 분할 레이아웃 ─────────────────────────

fn owner() -> ActorId {
    ActorId::local_user()
}

#[test]
fn desktop_splits_left_thirty_right_seventy() {
    let mut desktop = std_types::desktop(owner());
    let mut ft = std_types::file_tree(owner(), "/tmp");
    let mut cv = std_types::canvas(owner());
    ft.parent = Some(desktop.id);
    cv.parent = Some(desktop.id);
    desktop.children = vec![ft.id, cv.id];
    let (ft_id, cv_id) = (ft.id, cv.id);
    let mut tm = TreeModel::new();
    tm.upsert(desktop);
    tm.upsert(ft);
    tm.upsert(cv);
    let lay = layout(&tm, 1000, 600);
    let ft_rect = lay.get(ft_id).expect("ft rect");
    let cv_rect = lay.get(cv_id).expect("cv rect");
    assert_eq!(ft_rect.x, 0);
    assert_eq!(ft_rect.w, 300);
    assert_eq!(cv_rect.x, 300);
    assert_eq!(cv_rect.w, 700);
    assert_eq!(ft_rect.h, 600);
    assert_eq!(cv_rect.h, 600);
}

#[test]
fn file_tree_lists_top_level_children_vertically() {
    let mut desktop = std_types::desktop(owner());
    let mut ft = std_types::file_tree(owner(), "/tmp");
    let cv = std_types::canvas(owner());
    let mut f1 = std_types::folder(owner(), "/tmp/a", "a", 0);
    let mut f2 = std_types::file(owner(), "/tmp/b.txt", "b.txt", "text/plain", 0);
    ft.parent = Some(desktop.id);
    f1.parent = Some(ft.id);
    f2.parent = Some(ft.id);
    ft.children = vec![f1.id, f2.id];
    desktop.children = vec![ft.id, cv.id];
    let (f1_id, f2_id) = (f1.id, f2.id);
    let mut tm = TreeModel::new();
    tm.upsert(desktop);
    tm.upsert(ft);
    tm.upsert(cv);
    tm.upsert(f1);
    tm.upsert(f2);
    let lay = layout(&tm, 1000, 600);
    let r1 = lay.get(f1_id).expect("f1");
    let r2 = lay.get(f2_id).expect("f2");
    assert!(r1.x >= 0 && r1.x < 300);
    assert!(r2.y > r1.y);
}

#[test]
fn expanded_folder_shows_children_indented() {
    let mut desktop = std_types::desktop(owner());
    let mut ft = std_types::file_tree(owner(), "/tmp");
    let cv = std_types::canvas(owner());
    let mut f1 = std_types::folder(owner(), "/tmp/a", "a", 0);
    let mut nested = std_types::file(owner(), "/tmp/a/n.txt", "n.txt", "text/plain", 0);
    nested.parent = Some(f1.id);
    f1.children = vec![nested.id];
    f1.parent = Some(ft.id);
    ft.children = vec![f1.id];
    desktop.children = vec![ft.id, cv.id];
    ft.state.insert("expanded".into(), serde_json::json!([f1.id.to_string()]));
    let (f1_id, n_id) = (f1.id, nested.id);
    let mut tm = TreeModel::new();
    tm.upsert(desktop);
    tm.upsert(ft);
    tm.upsert(cv);
    tm.upsert(f1);
    tm.upsert(nested);
    let lay = layout(&tm, 1000, 600);
    let f1_rect = lay.get(f1_id).expect("f1");
    let n_rect = lay.get(n_id).expect("n");
    assert!(n_rect.x > f1_rect.x, "nested should be indented");
    assert!(n_rect.y > f1_rect.y);
}
