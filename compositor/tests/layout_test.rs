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
    use geulos_compositor::layout::HitRole;
    assert_eq!(
        hit,
        Some((button_id, HitRole::Body)),
        "Button을 hit해야 함, Container를 지나가야 함"
    );
}

// ───────────────────────── M7 T4: Desktop 좌/우 분할 레이아웃 ─────────────────────────

fn owner() -> ActorId {
    ActorId::local_user()
}

#[test]
fn desktop_splits_left_twenty_five_right_seventy_five() {
    // M8 T8.4: Canvas → Explorer, 좌 30/70 → 25/75.
    let mut desktop = std_types::desktop(owner());
    let mut ft = std_types::file_tree(owner(), "/tmp");
    let mut ex = std_types::explorer(owner());
    ft.parent = Some(desktop.id);
    ex.parent = Some(desktop.id);
    desktop.children = vec![ft.id, ex.id];
    let (ft_id, ex_id) = (ft.id, ex.id);
    let mut tm = TreeModel::new();
    tm.upsert(desktop);
    tm.upsert(ft);
    tm.upsert(ex);
    let lay = layout(&tm, 1000, 600);
    let ft_rect = lay.get(ft_id).expect("ft rect");
    let ex_rect = lay.get(ex_id).expect("ex rect");
    assert_eq!(ft_rect.x, 0);
    assert_eq!(ft_rect.w, 250);
    assert_eq!(ex_rect.x, 250);
    assert_eq!(ex_rect.w, 750);
    assert_eq!(ft_rect.h, 600);
    assert_eq!(ex_rect.h, 600);
}

#[test]
fn file_tree_lists_top_level_children_vertically() {
    // M8: 좌측은 폴더만 — 파일은 우측 Explorer로.
    let mut desktop = std_types::desktop(owner());
    let mut ft = std_types::file_tree(owner(), "/tmp");
    let ex = std_types::explorer(owner());
    let mut f1 = std_types::folder(owner(), "/tmp/a", "a", 0);
    let mut f2 = std_types::folder(owner(), "/tmp/b", "b", 0);
    ft.parent = Some(desktop.id);
    f1.parent = Some(ft.id);
    f2.parent = Some(ft.id);
    ft.children = vec![f1.id, f2.id];
    desktop.children = vec![ft.id, ex.id];
    let (f1_id, f2_id) = (f1.id, f2.id);
    let mut tm = TreeModel::new();
    tm.upsert(desktop);
    tm.upsert(ft);
    tm.upsert(ex);
    tm.upsert(f1);
    tm.upsert(f2);
    let lay = layout(&tm, 1000, 600);
    let r1 = lay.get(f1_id).expect("f1");
    let r2 = lay.get(f2_id).expect("f2");
    assert!(r1.x >= 0 && r1.x < 250);
    assert!(r2.y > r1.y);
}

// ───────────────────────── M7 T7.5: 하단 CLI 패널 (3분할) ─────────────────────────

#[test]
fn desktop_with_cli_splits_top_seventy_bottom_thirty() {
    // M8 T8.4: Canvas → Explorer, 좌 30/70 → 25/75.
    let mut desktop = std_types::desktop(owner());
    let mut ft = std_types::file_tree(owner(), "/tmp");
    let mut ex = std_types::explorer(owner());
    let mut cli = std_types::cli(owner());
    ft.parent = Some(desktop.id);
    ex.parent = Some(desktop.id);
    cli.parent = Some(desktop.id);
    desktop.children = vec![ft.id, ex.id, cli.id];
    let (ft_id, ex_id, cli_id) = (ft.id, ex.id, cli.id);
    let mut tm = TreeModel::new();
    tm.upsert(desktop);
    tm.upsert(ft);
    tm.upsert(ex);
    tm.upsert(cli);
    let lay = layout(&tm, 1000, 600);
    let ft_rect = lay.get(ft_id).expect("ft rect");
    let ex_rect = lay.get(ex_id).expect("ex rect");
    let cli_rect = lay.get(cli_id).expect("cli rect");
    // 상단 영역: 70% 높이 = 420.
    assert_eq!(ft_rect.h, 420, "FileTree 높이는 win_h * 0.7");
    assert_eq!(ex_rect.h, 420, "Explorer 높이는 win_h * 0.7");
    // 좌/우 분할 (M8 25% / 75%).
    assert_eq!(ft_rect.x, 0);
    assert_eq!(ft_rect.w, 250);
    assert_eq!(ex_rect.x, 250);
    assert_eq!(ex_rect.w, 750);
    // CLI: 하단 30% 높이, 풀폭.
    assert_eq!(cli_rect.x, 0, "CLI는 x=0부터");
    assert_eq!(cli_rect.w, 1000, "CLI는 윈도우 풀폭");
    assert_eq!(cli_rect.y, 420, "CLI는 상단 70% 밑에 위치");
    assert_eq!(cli_rect.h, 180, "CLI 높이는 win_h * 0.3");
}

#[test]
fn desktop_without_cli_falls_back_to_full_height_panels() {
    // 자식이 [FileTree, Explorer]만이면 상하 분할 없음 — 풀높이 panel.
    let mut desktop = std_types::desktop(owner());
    let mut ft = std_types::file_tree(owner(), "/tmp");
    let mut ex = std_types::explorer(owner());
    ft.parent = Some(desktop.id);
    ex.parent = Some(desktop.id);
    desktop.children = vec![ft.id, ex.id];
    let (ft_id, ex_id) = (ft.id, ex.id);
    let mut tm = TreeModel::new();
    tm.upsert(desktop);
    tm.upsert(ft);
    tm.upsert(ex);
    let lay = layout(&tm, 1000, 600);
    assert_eq!(lay.get(ft_id).unwrap().h, 600);
    assert_eq!(lay.get(ex_id).unwrap().h, 600);
}

// ───────────────────────── M8 T8.4: Explorer 4분할 + Window 오버레이 ─────────────────────────

#[test]
fn layout_desktop_renders_explorer_in_right_top() {
    let mut tree = TreeModel::new();
    let owner = ActorId::local_user();
    let mut desktop = std_types::desktop(owner.clone());
    let ft = std_types::file_tree(owner.clone(), "/");
    let ex = std_types::explorer(owner.clone());
    let cli = std_types::cli(owner.clone());
    desktop.children = vec![ft.id, ex.id, cli.id];
    let (ft_id, ex_id, cli_id) = (ft.id, ex.id, cli.id);
    tree.upsert(desktop);
    tree.upsert(ft);
    tree.upsert(ex);
    tree.upsert(cli);

    let lay = layout(&tree, 1000, 600);
    let ft_rect = lay.get(ft_id).unwrap();
    let ex_rect = lay.get(ex_id).unwrap();
    let cli_rect = lay.get(cli_id).unwrap();
    assert_eq!(ft_rect.w, 250, "25% × 1000 = 250");
    assert_eq!(ex_rect.x, 250);
    assert_eq!(ex_rect.w, 750);
    assert_eq!(ex_rect.h, 420, "70% × 600 = 420");
    assert_eq!(cli_rect.y, 420);
    assert_eq!(cli_rect.h, 180);
    assert_eq!(cli_rect.w, 1000);
}

#[test]
fn layout_desktop_overlays_windows_in_z_order() {
    use geulos_core::ObjectId;
    let mut tree = TreeModel::new();
    let owner = ActorId::local_user();
    let mut desktop = std_types::desktop(owner.clone());
    let ft = std_types::file_tree(owner.clone(), "/");
    let ex = std_types::explorer(owner.clone());
    let cli = std_types::cli(owner.clone());
    let fid = ObjectId::new();
    let mut w1 = std_types::window(owner.clone(), "a", fid, 10, 10, 200, 100);
    w1.set_state("z", serde_json::json!(1));
    let mut w2 = std_types::window(owner.clone(), "b", fid, 50, 50, 200, 100);
    w2.set_state("z", serde_json::json!(2));
    desktop.children = vec![ft.id, ex.id, cli.id, w1.id, w2.id];
    let (w1_id, w2_id) = (w1.id, w2.id);
    for o in [desktop, ft, ex, cli, w1, w2] {
        tree.upsert(o);
    }
    let lay = layout(&tree, 1000, 600);
    let r1_pos = lay.rects.iter().position(|(id, _, _)| *id == w1_id).unwrap();
    let r2_pos = lay.rects.iter().position(|(id, _, _)| *id == w2_id).unwrap();
    assert!(r1_pos < r2_pos, "z 낮은 윈도우가 먼저 (밑에) 그려져야");
}

// ───────────────────────── M8 회귀 fix #2: HitRole 폴더 클릭 분리 ─────────────────────────

#[test]
fn folder_in_tree_has_separate_toggle_and_body_rects() {
    // 좌측 트리의 폴더는 동일 ObjectId에 두 rect — Body + ExpandToggle — 가 push되어야.
    // toggle은 row 시작에서 36px 좁은 영역, body는 row 전체.
    use geulos_compositor::layout::HitRole;
    use geulos_core::std_types;
    let mut tree = TreeModel::new();
    let owner = ActorId::local_user();
    let mut desktop = std_types::desktop(owner.clone());
    let mut ft = std_types::file_tree(owner.clone(), "/");
    let ex = std_types::explorer(owner.clone());
    let cli = std_types::cli(owner.clone());
    let folder = std_types::folder(owner.clone(), "/x", "x", 0);
    ft.children = vec![folder.id];
    desktop.children = vec![ft.id, ex.id, cli.id];
    let folder_id = folder.id;
    for o in [desktop, ft, ex, cli, folder] {
        tree.upsert(o);
    }
    let lay = layout(&tree, 1000, 600);
    // 좌측 트리 영역(x < 250)의 folder rects만 — Explorer 우측에도 같은 folder가 line으로
    // 등장하므로 (active_folder 미설정 시 FileTree.children fallback) 그건 제외.
    let folder_rects: Vec<_> =
        lay.rects.iter().filter(|(i, r, _)| *i == folder_id && r.x < 250).collect();
    assert_eq!(folder_rects.len(), 2, "좌측 트리 폴더는 Body + ExpandToggle 두 rect");
    let toggle = folder_rects.iter().find(|(_, _, r)| *r == HitRole::ExpandToggle).unwrap();
    let body = folder_rects.iter().find(|(_, _, r)| *r == HitRole::Body).unwrap();
    assert!(toggle.1.w < body.1.w, "toggle은 body보다 좁음");
    assert_eq!(toggle.1.x, body.1.x, "둘 다 같은 x에서 시작");
    assert_eq!(toggle.1.y, body.1.y, "둘 다 같은 y");
    assert_eq!(toggle.1.h, body.1.h, "둘 다 같은 h");
}

#[test]
fn hit_test_returns_expand_toggle_for_left_36px_of_folder_row() {
    // 사용자 클릭이 폴더 행의 좌측 36px 이내 → ExpandToggle 반환.
    // 그 외 행 영역 → Body 반환.
    use geulos_compositor::hit_test::hit_test;
    use geulos_compositor::layout::HitRole;
    use geulos_core::std_types;
    let mut tree = TreeModel::new();
    let owner = ActorId::local_user();
    let mut desktop = std_types::desktop(owner.clone());
    let mut ft = std_types::file_tree(owner.clone(), "/");
    let ex = std_types::explorer(owner.clone());
    let cli = std_types::cli(owner.clone());
    let folder = std_types::folder(owner.clone(), "/x", "x", 0);
    ft.children = vec![folder.id];
    desktop.children = vec![ft.id, ex.id, cli.id];
    let folder_id = folder.id;
    for o in [desktop, ft, ex, cli, folder] {
        tree.upsert(o);
    }
    let lay = layout(&tree, 1000, 600);
    let body = lay.get(folder_id).expect("folder body rect");
    // 행 좌측 8px (toggle 영역) → ExpandToggle.
    let hit = hit_test(&tree, &lay, body.x + 8, body.y + body.h / 2);
    assert_eq!(hit, Some((folder_id, HitRole::ExpandToggle)), "좌측 8px = toggle");
    // 행 50px (toggle 36px 밖) → Body.
    let hit2 = hit_test(&tree, &lay, body.x + 50, body.y + body.h / 2);
    assert_eq!(hit2, Some((folder_id, HitRole::Body)), "x=50 = body (toggle 영역 밖)");
}

// ───────────────────────── M8 T8.16: FileTree + Explorer scroll_y offset ─────────────────────────

#[test]
fn file_tree_with_scroll_y_skips_first_lines() {
    use geulos_core::std_types;
    let mut tree = TreeModel::new();
    let owner = geulos_core::ActorId::local_user();
    let mut desktop = std_types::desktop(owner.clone());
    let mut ft = std_types::file_tree(owner.clone(), "/");
    let ex = std_types::explorer(owner.clone());
    let cli = std_types::cli(owner.clone());
    ft.set_state("scroll_y", serde_json::json!(2));
    let f1 = std_types::folder(owner.clone(), "/a", "a", 0);
    let f2 = std_types::folder(owner.clone(), "/b", "b", 0);
    let f3 = std_types::folder(owner.clone(), "/c", "c", 0);
    ft.children = vec![f1.id, f2.id, f3.id];
    desktop.children = vec![ft.id, ex.id, cli.id];
    for o in [desktop, ft.clone(), ex, cli, f1.clone(), f2.clone(), f3.clone()] {
        tree.upsert(o);
    }
    let lay = layout(&tree, 1000, 600);
    // scroll_y=2 → 첫 2 라인 (f1, f2)가 위로 밀려 y가 음수 또는 비가시
    // f3는 (시작 위치 4 - 2*24 = -44 + 위치 보정 후) 보일 것
    let f1_rect = lay.get(f1.id);
    let f3_rect = lay.get(f3.id);
    assert!(f1_rect.is_none() || f1_rect.unwrap().y < 0, "f1은 scroll로 음수 y 또는 미존재");
    assert!(f3_rect.is_some(), "f3는 layout에 있어야");
}

#[test]
fn explorer_with_scroll_y_skips_first_lines() {
    use geulos_core::std_types;
    let mut tree = TreeModel::new();
    let owner = geulos_core::ActorId::local_user();
    let mut desktop = std_types::desktop(owner.clone());
    let mut ft = std_types::file_tree(owner.clone(), "/");
    let mut ex = std_types::explorer(owner.clone());
    let cli = std_types::cli(owner.clone());
    ex.set_state("scroll_y", serde_json::json!(3));
    let drives: Vec<_> = (0..5)
        .map(|i| std_types::folder(owner.clone(), &format!("/d{}", i), &format!("d{}", i), 0))
        .collect();
    ft.children = drives.iter().map(|d| d.id).collect();
    desktop.children = vec![ft.id, ex.id, cli.id];
    for o in [desktop, ft.clone(), ex.clone(), cli.clone()] {
        tree.upsert(o);
    }
    for d in &drives {
        tree.upsert(d.clone());
    }
    let lay = layout(&tree, 1000, 600);
    // drives는 FileTree.children인 동시에 explorer_children fallback이라 *양쪽 패널 모두*
    // 같은 ObjectId의 rect를 push한다. 좌측(x < 250)은 FileTree (scroll 미적용), 우측은
    // Explorer (scroll_y=3 적용). 우측 rect만 골라 검사.
    let left_w = 250;
    let d0_right: Vec<_> =
        lay.rects.iter().filter(|(i, r, _)| *i == drives[0].id && r.x >= left_w).collect();
    let d3_right: Vec<_> =
        lay.rects.iter().filter(|(i, r, _)| *i == drives[3].id && r.x >= left_w).collect();
    // d0은 scroll로 위 (y < 4). d3는 가시 영역 첫 줄.
    assert!(d0_right.is_empty() || d0_right[0].1.y < 4, "d0은 scroll로 위로 밀림");
    assert!(!d3_right.is_empty(), "d3는 우측 패널에 보여야");
}

#[test]
fn expanded_folder_shows_children_indented() {
    // M8: 좌측은 폴더만 보임 — nested 자식도 폴더여야 트리에 표시됨.
    let mut desktop = std_types::desktop(owner());
    let mut ft = std_types::file_tree(owner(), "/tmp");
    let ex = std_types::explorer(owner());
    let mut f1 = std_types::folder(owner(), "/tmp/a", "a", 0);
    let mut nested = std_types::folder(owner(), "/tmp/a/n", "n", 0);
    nested.parent = Some(f1.id);
    f1.children = vec![nested.id];
    f1.parent = Some(ft.id);
    ft.children = vec![f1.id];
    desktop.children = vec![ft.id, ex.id];
    ft.state.insert("expanded".into(), serde_json::json!([f1.id.to_string()]));
    let (f1_id, n_id) = (f1.id, nested.id);
    let mut tm = TreeModel::new();
    tm.upsert(desktop);
    tm.upsert(ft);
    tm.upsert(ex);
    tm.upsert(f1);
    tm.upsert(nested);
    let lay = layout(&tm, 1000, 600);
    let f1_rect = lay.get(f1_id).expect("f1");
    let n_rect = lay.get(n_id).expect("n");
    assert!(n_rect.x > f1_rect.x, "nested should be indented");
    assert!(n_rect.y > f1_rect.y);
}
