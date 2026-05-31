> **Status:** completed (2026-05-29)
> **Note:** SP1 데스크톱 셸 환경 정식 마감 — top_bar/dock/desktop_icon/file_manager factory 안착, AI=사용자2 동일 명령표면 유지.

# SP1 데스크톱 셸 환경 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 고정 3패널 데스크톱을 윈도잉 데스크톱 환경(바탕화면 + 상단 네비바 + 우측 독 + 바탕화면 아이콘 + 떠있는 파일관리자 창 + 상하 리사이즈 CLI)으로 재설계하되, 모든 affordance를 AI도 invoke 가능한 객체 메서드로 노출(AI=사용자2).

**Architecture:** 접근 A — 크롬을 객체로 트리에 추가하고 `layout_desktop`을 확장. 파일관리자 창은 기존 `FileTree`+`Explorer`를 창으로 래핑(렌더·메서드 재사용). 컴포지터 클릭/드래그는 객체 메서드 Invoke로만 동작 → AI가 동일 Invoke로 사용자 2가 됨.

**Tech Stack:** Rust (musl via `cargo zigbuild`), 객체 서버(geulosd) + 와이어 프로토콜, 컴포지터(DRM 렌더 + evdev), `serde_json`. 빌드/부팅은 `boot/build.ps1` + `boot/qemu/launch.ps1`(Windows PowerShell).

**핵심 불변식:** 새 UI 동작을 컴포지터 전용으로 만들지 말 것. 모든 동작 = 트리 객체의 메서드(Invoke). 컴포지터는 클릭→Invoke 변환만. (참고 메모리: AI=사용자2 동일 명령표면.)

---

## 사전 환경 (모든 빌드/부팅 공통)

```powershell
$env:PATH = [System.Environment]::GetEnvironmentVariable("Path","Machine") + ";" + `
            [System.Environment]::GetEnvironmentVariable("Path","User") + ";" + `
            "$env:USERPROFILE\.cargo\bin"
# 호스트 단위 테스트:   cargo test -p <crate>
# 크로스컴파일 검증:    cargo zigbuild --target x86_64-unknown-linux-musl -p <crate>
# 이미지 빌드:          & .\boot\build.ps1 -Release   (재빌드 전 QEMU 종료)
# 부팅(그래픽):         & .\boot\qemu\launch.ps1 -Graphics   (직렬로그 boot/serial.log)
Get-Process qemu-system-x86_64 -ErrorAction SilentlyContinue | Stop-Process -Force
```

**검증 철학:** 순수 로직(객체 factory, 레이아웃 영역 계산, launch 디스패치, hit-test 매핑)은 호스트 `cargo test`로 TDD. 렌더/부팅 의존부는 `cargo zigbuild` 통과 + VM 부팅 시각 확인(사용자) + **AI 패리티 테스트**(서버 Invoke로 동일 결과).

---

## 파일 구조 (생성/수정 맵)

| 파일 | 책임 / 변경 |
|---|---|
| `core/src/object/std_types.rs` | **신규 factory**: `top_bar`, `dock`, `desktop_icon`, `file_manager`. **`desktop` 확장**: `wallpaper`/`cli_height` state + `launch`/`set_wallpaper`/`set_cli_height` 메서드. |
| `compositor/src/layout.rs` | `HitRole`에 `DesktopIcon`/`DockItem`/`TopBarItem`/`CliResizeHandle` 추가. **순수 영역 계산 함수 `desktop_regions()` 신규**(테스트 대상). `layout_desktop` 확장: 상단/우측/하단/중앙 배치 + 아이콘 + (M2)FileManager 창. |
| `compositor/src/render.rs` | 바탕화면 배경 + `TopBar`/`Dock`/`DesktopIcon`/(M2)`FileManager` 렌더. 기존 `render_window`/icons/text 재사용. |
| `compositor/src/hit_test.rs` | 새 HitRole 영역 hit 매핑(기존 역순 z 로직 유지). |
| `compositor/src/bin/geulos-vm-compositor.rs` | 새 HitRole→`UiAction::Invoke` 매핑(open/launch/activate/set_cli_height). (M3)CLI 리사이즈 드래그 상태. |
| `apps/desktop-shell/src/applauncher.rs` (신규) | **앱 레지스트리**: `app_id` → 창 구성 객체들. `resolve_launch()` 순수 디스패치(테스트 대상). |
| `apps/desktop-shell/src/handlers/shell_methods.rs` (신규) | `Desktop.launch`/`set_wallpaper`/`set_cli_height`, `Dock.launch`, `DesktopIcon.open`, `TopBar.activate` invoke 핸들러. |
| `apps/desktop-shell/src/main.rs` | 시작 mount에 Desktop(wallpaper/cli_height) + TopBar + Dock + DesktopIcon들 추가 + 구독/ACL. (M2)FileTree/Explorer 고정 mount 제거. |

---

## M1 — 크롬 + 실행 모델

### Task 1: 새 객체 타입 factory + Desktop 확장

**Files:**
- Modify: `core/src/object/std_types.rs`
- Test: `core/src/object/std_types.rs` (인라인 `#[cfg(test)]`)

- [ ] **Step 1: 실패 테스트 작성** — `std_types.rs` 기존 `#[cfg(test)] mod tests`(없으면 추가) 끝에:

```rust
#[cfg(test)]
mod sp1_tests {
    use super::*;
    use crate::object::identity::ActorId;

    fn owner() -> ActorId { ActorId::new() }

    #[test]
    fn top_bar_has_activate_method() {
        let o = top_bar(owner());
        assert_eq!(o.type_uri.as_str(), "aios.builtin/TopBar@1");
        assert!(o.methods.iter().any(|m| m.name == "activate"));
        assert!(o.state.contains_key("items"));
    }

    #[test]
    fn dock_has_launch_method() {
        let o = dock(owner());
        assert_eq!(o.type_uri.as_str(), "aios.builtin/Dock@1");
        assert!(o.methods.iter().any(|m| m.name == "launch"));
    }

    #[test]
    fn desktop_icon_carries_app_and_open() {
        let o = desktop_icon(owner(), "file_manager", "파일관리자", "folder", 40, 40);
        assert_eq!(o.type_uri.as_str(), "aios.builtin/DesktopIcon@1");
        assert_eq!(o.props.get("app").unwrap(), "file_manager");
        assert_eq!(o.state.get("x").unwrap(), 40);
        assert!(o.methods.iter().any(|m| m.name == "open"));
    }

    #[test]
    fn file_manager_is_window_like() {
        let o = file_manager(owner(), 100, 80, 700, 460, 1);
        assert_eq!(o.type_uri.as_str(), "aios.builtin/FileManager@1");
        assert_eq!(o.state.get("w").unwrap(), 700);
        for m in ["move", "resize", "focus", "close"] {
            assert!(o.methods.iter().any(|s| s.name == m), "missing {m}");
        }
    }

    #[test]
    fn desktop_has_launch_and_chrome_state() {
        let o = desktop(owner());
        assert!(o.methods.iter().any(|m| m.name == "launch"));
        assert!(o.methods.iter().any(|m| m.name == "set_cli_height"));
        assert!(o.state.contains_key("wallpaper"));
        assert!(o.state.contains_key("cli_height"));
    }
}
```

(NOTE: `o.props`/`o.state`는 `Object`의 필드 — 기존 다른 테스트에서 접근 방식 확인. `MethodSig`의 이름 필드가 `.name`이 아니면 해당 필드명으로 교정. `ActorId::new()`가 없으면 기존 테스트의 owner 생성 방식을 따른다.)

- [ ] **Step 2: 테스트 실패 확인** — `cargo test -p geulos-core sp1_tests` → factory 함수 미정의로 FAIL.

- [ ] **Step 3: factory 구현** — `std_types.rs`에 추가 (기존 `desktop()` L133-135는 교체):

```rust
/// 데스크톱 루트 셸. 바탕화면 + 떠있는 창 + 하단 CLI 호스트.
/// state: wallpaper(배경 색/그라데이션 토큰), cli_height(하단 CLI 높이 px).
/// 메서드: launch(app) 실행 진입점, set_wallpaper(v), set_cli_height(px).
pub fn desktop(owner: ActorId) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.builtin/Desktop@1").expect("유효한 TypeUri"), owner);
    obj.set_state("wallpaper", json!("#1E2A3A"));
    obj.set_state("cli_height", json!(220));
    obj.methods.push(MethodSig::new("launch").with_arg(ArgSpec::new("app", "string")));
    obj.methods.push(MethodSig::new("set_wallpaper").with_arg(ArgSpec::new("v", "string")));
    obj.methods.push(MethodSig::new("set_cli_height").with_arg(ArgSpec::new("px", "i32")));
    obj
}

/// 상단 네비게이션 바. items=[{id,label}], clock=컴포지터 자동 표시.
pub fn top_bar(owner: ActorId) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.builtin/TopBar@1").expect("유효한 TypeUri"), owner);
    obj.set_state("items", json!([{"id":"geulos","label":"GeulOS"}]));
    obj.set_state("clock", json!(""));
    obj.methods.push(MethodSig::new("activate").with_arg(ArgSpec::new("item_id", "string")));
    obj
}

/// 우측 퀵런치 독. items=[{app,label,icon}].
pub fn dock(owner: ActorId) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.builtin/Dock@1").expect("유효한 TypeUri"), owner);
    obj.set_state("items", json!([]));
    obj.methods.push(MethodSig::new("launch").with_arg(ArgSpec::new("item_id", "string")));
    obj
}

/// 바탕화면 아이콘(다중). open()=해당 app 실행.
pub fn desktop_icon(owner: ActorId, app: &str, label: &str, icon: &str, x: i32, y: i32) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.builtin/DesktopIcon@1").expect("유효한 TypeUri"), owner);
    obj.set_prop("app", json!(app));
    obj.set_prop("label", json!(label));
    obj.set_prop("icon", json!(icon));
    obj.set_state("x", json!(x));
    obj.set_state("y", json!(y));
    obj.methods.push(MethodSig::new("open"));
    obj
}

/// 파일관리자 창. FileTree+Explorer를 자식으로 감싸는 떠있는 창(Window 동형).
pub fn file_manager(owner: ActorId, x: i32, y: i32, w: i32, h: i32, z: i32) -> Object {
    let mut obj = Object::new(TypeUri::parse("aios.builtin/FileManager@1").expect("유효한 TypeUri"), owner);
    obj.set_state("x", json!(x));
    obj.set_state("y", json!(y));
    obj.set_state("w", json!(w));
    obj.set_state("h", json!(h));
    obj.set_state("z", json!(z));
    obj.set_state("focused", json!(true));
    obj.methods.push(MethodSig::new("move").with_arg(ArgSpec::new("x", "i32")).with_arg(ArgSpec::new("y", "i32")));
    obj.methods.push(MethodSig::new("resize").with_arg(ArgSpec::new("w", "i32")).with_arg(ArgSpec::new("h", "i32")));
    obj.methods.push(MethodSig::new("focus"));
    obj.methods.push(MethodSig::new("close"));
    obj
}
```

- [ ] **Step 4: 테스트 통과 확인** — `cargo test -p geulos-core sp1_tests` → 5개 PASS. (필드명/생성자 불일치 시 교정 후 통과.)

- [ ] **Step 5: 커밋**
```powershell
git add core/src/object/std_types.rs
git commit -m "feat(core): SP1 객체 타입 — TopBar/Dock/DesktopIcon/FileManager + Desktop 확장"
```

---

### Task 2: 레이아웃 영역 계산 (순수, TDD)

**Files:**
- Modify: `compositor/src/layout.rs` (상단에 순수 함수 + HitRole 추가)
- Test: `compositor/src/layout.rs` (인라인 테스트)

- [ ] **Step 1: HitRole 확장** — `layout.rs` L33-37 `enum HitRole`에 variant 추가:

```rust
pub enum HitRole {
    Body,
    ExpandToggle,
    ExplorerParentNav,
    DesktopIcon,      // 바탕화면 아이콘 → open()
    DockItem,         // 독 항목 → Dock.launch(item)
    TopBarItem,       // 네비바 항목 → TopBar.activate(item)
    CliResizeHandle,  // CLI 상단 리사이즈 핸들 → Desktop.set_cli_height
}
```

- [ ] **Step 2: 영역 계산 실패 테스트** — `layout.rs` 테스트 모듈에:

```rust
#[cfg(test)]
mod region_tests {
    use super::*;
    #[test]
    fn regions_partition_screen() {
        // 1280x800, 상단바 30, 독폭 56, cli높이 220
        let r = desktop_regions(1280, 800, 220);
        assert_eq!(r.topbar, Rect { x: 0, y: 0, w: 1280, h: 30 });
        assert_eq!(r.dock, Rect { x: 1280 - 56, y: 30, w: 56, h: 800 - 30 - 220 });
        assert_eq!(r.cli, Rect { x: 0, y: 800 - 220, w: 1280, h: 220 });
        assert_eq!(r.cli_handle, Rect { x: 0, y: 800 - 220 - 6, w: 1280, h: 6 });
        // 중앙(바탕화면) = 상단바 아래, 독 왼쪽, CLI 위
        assert_eq!(r.desktop, Rect { x: 0, y: 30, w: 1280 - 56, h: 800 - 30 - 220 });
    }

    #[test]
    fn cli_height_clamped() {
        // 과대 cli_height는 화면 높이-상단바보다 작게 clamp
        let r = desktop_regions(1280, 800, 100_000);
        assert!(r.cli.h <= 800 - 30 - 40); // 최소 데스크톱 영역 40 보장
    }
}
```

- [ ] **Step 3: 영역 계산 구현** — `layout.rs`에 상수 + 순수 함수 추가:

```rust
pub const TOPBAR_H: i32 = 30;
pub const DOCK_W: i32 = 56;
pub const CLI_HANDLE_H: i32 = 6;
pub const CLI_MIN_H: i32 = 60;
pub const DESKTOP_MIN_H: i32 = 40;

pub struct DesktopRegions {
    pub topbar: Rect,
    pub dock: Rect,
    pub cli: Rect,
    pub cli_handle: Rect,
    pub desktop: Rect, // 바탕화면 + 떠있는 창 영역
}

/// 화면 크기 + cli_height에서 데스크톱 영역들을 계산 (순수).
pub fn desktop_regions(win_w: i32, win_h: i32, cli_height: i32) -> DesktopRegions {
    let max_cli = (win_h - TOPBAR_H - DESKTOP_MIN_H).max(CLI_MIN_H);
    let cli_h = cli_height.clamp(CLI_MIN_H, max_cli);
    let mid_h = win_h - TOPBAR_H - cli_h;
    DesktopRegions {
        topbar: Rect { x: 0, y: 0, w: win_w, h: TOPBAR_H },
        dock: Rect { x: win_w - DOCK_W, y: TOPBAR_H, w: DOCK_W, h: mid_h },
        cli: Rect { x: 0, y: win_h - cli_h, w: win_w, h: cli_h },
        cli_handle: Rect { x: 0, y: win_h - cli_h - CLI_HANDLE_H, w: win_w, h: CLI_HANDLE_H },
        desktop: Rect { x: 0, y: TOPBAR_H, w: win_w - DOCK_W, h: mid_h },
    }
}
```

- [ ] **Step 4: 테스트 통과** — `cargo test -p geulos-compositor region_tests` → 2개 PASS.

- [ ] **Step 5: 커밋**
```powershell
git add compositor/src/layout.rs
git commit -m "feat(compositor): SP1 데스크톱 영역 계산 + HitRole 확장 (순수, TDD)"
```

---

### Task 3: 앱 레지스트리 + launch 디스패치 (순수, TDD)

**Files:**
- Create: `apps/desktop-shell/src/applauncher.rs`
- Modify: `apps/desktop-shell/src/lib.rs` (mod 선언)
- Test: `applauncher.rs` 인라인

- [ ] **Step 1: 실패 테스트 + 구현** — `applauncher.rs` 생성:

```rust
//! 앱 레지스트리 — app_id를 "어떤 창을 구성할지"로 해석 (순수 디스패치).
//! Desktop.launch / Dock.launch / DesktopIcon.open 이 공통으로 사용 → 사용자·AI 동일 경로.

/// app_id가 알려진 앱이면 그 종류 반환. 알 수 없으면 None.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum AppKind {
    FileManager,
    // SP3: Notepad, ...
}

pub fn resolve_app(app_id: &str) -> Option<AppKind> {
    match app_id {
        "file_manager" => Some(AppKind::FileManager),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_app_resolves() {
        assert_eq!(resolve_app("file_manager"), Some(AppKind::FileManager));
    }
    #[test]
    fn unknown_app_is_none() {
        assert_eq!(resolve_app("nope"), None);
    }
}
```

- [ ] **Step 2: lib.rs에 mod 추가** — `apps/desktop-shell/src/lib.rs`에 `pub mod applauncher;`

- [ ] **Step 3: 테스트 통과** — `cargo test -p geulos-desktop-shell applauncher` → 2개 PASS.

- [ ] **Step 4: 커밋**
```powershell
git add apps/desktop-shell/src/applauncher.rs apps/desktop-shell/src/lib.rs
git commit -m "feat(desktop-shell): SP1 앱 레지스트리 + launch 디스패치 (순수, TDD)"
```

---

### Task 4: launch / 크롬 메서드 핸들러 (desktop-shell)

**Files:**
- Create: `apps/desktop-shell/src/handlers/shell_methods.rs`
- Modify: `apps/desktop-shell/src/handlers/mod.rs` (dispatch 연결)

> 참고 패턴: `apps/desktop-shell/src/handlers/window_methods.rs`(focus/close/move batch SetState), `explorer_methods.rs`(open_file이 창 mount하는 흐름), `window_ops.rs`(max_z/next_window_position/dedup). M1에서 `launch`는 **앱 레지스트리로 종류 판정 후 로그 + (M2에서 실제 창 mount)**. M1 단계 목표는 "클릭/AI Invoke가 launch 핸들러에 도달"까지 검증.

- [ ] **Step 1: 핸들러 작성** — `shell_methods.rs` 생성:

```rust
//! Desktop/TopBar/Dock/DesktopIcon 크롬 메서드 핸들러.
//! 모든 동작은 여기로 — 컴포지터 클릭과 AI Invoke가 동일하게 도달.

use geulos_core::ObjectId;
use crate::applauncher::{resolve_app, AppKind};

/// launch 결과 — 호출자(invoke 루프)가 mount/SetState로 반영.
pub enum LaunchOutcome {
    OpenFileManager,     // M2: FileManager 창 mount
    AlreadyOpenFocus(ObjectId), // 기존 창 focus
    Unknown(String),     // 알 수 없는 app — no-op + 로그
}

/// app_id로 무엇을 할지 결정. (이미 열린 FileManager 있으면 focus.)
pub fn handle_launch(app_id: &str, mounted: &[geulos_core::Object]) -> LaunchOutcome {
    match resolve_app(app_id) {
        Some(AppKind::FileManager) => {
            if let Some(existing) = mounted.iter()
                .find(|o| o.type_uri.as_str() == "aios.builtin/FileManager@1")
            {
                LaunchOutcome::AlreadyOpenFocus(existing.id)
            } else {
                LaunchOutcome::OpenFileManager
            }
        }
        None => LaunchOutcome::Unknown(app_id.to_string()),
    }
}
```

(`set_cli_height`는 Desktop.state.cli_height를 clamp 후 SetState — clamp는 layout의 상수 재사용. M1에선 핸들러만, 실제 드래그는 M3.)

- [ ] **Step 2: dispatch 연결** — `handlers/mod.rs`에서 invoke target type별 분기에 추가: `TopBar.activate`/`Dock.launch`/`DesktopIcon.open`/`Desktop.launch`를 `shell_methods::handle_launch`로 라우팅(DesktopIcon.open은 `props.app`을, Dock.launch는 `items[item_id].app`을 app_id로). 기존 분기(window_methods 등) 패턴을 그대로 따른다.

- [ ] **Step 3: 단위 테스트** — `shell_methods.rs`에 `handle_launch`(알려진/미지/이미열림) 테스트 추가 → `cargo test -p geulos-desktop-shell` PASS.

- [ ] **Step 4: 크로스컴파일 + 커밋**
```powershell
cargo zigbuild --target x86_64-unknown-linux-musl -p geulos-desktop-shell
git add apps/desktop-shell/src/handlers/shell_methods.rs apps/desktop-shell/src/handlers/mod.rs
git commit -m "feat(desktop-shell): SP1 크롬 메서드 핸들러 (launch/activate/open) + dedup"
```

---

### Task 5: 컴포지터 — 크롬 렌더 + hit-test + Invoke 매핑

**Files:**
- Modify: `compositor/src/layout.rs` (`layout_desktop` 확장), `render.rs`, `hit_test.rs`, `bin/geulos-vm-compositor.rs`

> 참고: `layout_desktop`(layout.rs L191-342), `render_frame`(render.rs L33+), `hit_test`(hit_test.rs L24-64), 컴포지터 입력 루프(geulos-vm-compositor.rs의 클릭→Invoke 분기).

- [ ] **Step 1: `layout_desktop` 확장** — `desktop_regions()`로 영역 계산 후: TopBar rect(TopBarItem role), Dock rect + 각 item rect(DockItem role), 바탕화면 영역에 DesktopIcon들을 `state.x/y` 위치 + 아이콘 크기(예 64x72)로 push(DesktopIcon role), cli_handle rect(CliResizeHandle role) push, Cli는 `cli` rect로. 기존 FileTree/Explorer 패널 push는 **M1에선 유지**(중앙 영역 안에 배치하거나 임시로 기존 위치). 떠있는 창(Window/FileManager) z-정렬 push는 기존 로직 유지.

- [ ] **Step 2: 렌더** — `render.rs` `render_frame` match에 분기 추가: 바탕화면 배경(Desktop.state.wallpaper 색으로 desktop region fill), `TopBar@1`(바 배경 + items 텍스트 + 우측 clock = chrono now), `Dock@1`(우측 패널 배경 + items 아이콘 `draw_icon`), `DesktopIcon@1`(아이콘 + 라벨 텍스트). 색은 `theme.rs` 토큰 사용. 텍스트/아이콘은 기존 `draw_text`/`draw_icon` 재사용.

- [ ] **Step 3: hit-test** — `hit_test.rs`는 layout rects를 역순 순회하므로 새 role은 자동 매칭. 새 role을 컨테이너처럼 skip하지 않도록 확인(Desktop/FileTree만 skip 유지).

- [ ] **Step 4: Invoke 매핑** — `geulos-vm-compositor.rs` 클릭 분기에 role별 추가:
  - `DesktopIcon` → `Invoke{target=icon, method:"open", args:null}`
  - `DockItem` → `Invoke{target=dock, method:"launch", args:{item_id}}` (item_id는 layout이 rect에 실어주거나 dock state 인덱스로)
  - `TopBarItem` → `Invoke{target=topbar, method:"activate", args:{item_id}}`
  - `CliResizeHandle` → (M3) 드래그 시작
  기존 Window 분기(close/move/resize/focus)·Explorer·Cli 분기는 그대로.

- [ ] **Step 5: 크로스컴파일** — `cargo zigbuild --target x86_64-unknown-linux-musl -p geulos-compositor --bin geulos-vm-compositor` 통과.

- [ ] **Step 6: 커밋**
```powershell
git add compositor/src/layout.rs compositor/src/render.rs compositor/src/hit_test.rs compositor/src/bin/geulos-vm-compositor.rs
git commit -m "feat(compositor): SP1 크롬 렌더(네비바/독/아이콘/바탕화면) + hit-test + Invoke 매핑"
```

---

### Task 6: desktop-shell 시작 시 크롬 mount

**Files:**
- Modify: `apps/desktop-shell/src/main.rs`

> 참고: 기존 mount 시퀀스(main.rs L391-540) — Desktop/FileTree/Explorer/Cli mount + ACL(`add_container_acl`/`add_ui_object_acl`) + Subscribe. 동일 패턴으로 새 객체 추가.

- [ ] **Step 1: 객체 생성·mount·구독** — startup에서: `top_bar(owner)`, `dock(owner)`(items=[{app:"file_manager",label:"파일관리자",icon:"folder"}, …]), `desktop_icon(owner,"file_manager","파일관리자","folder",40,40)` 등 생성. Desktop.children에 추가. 기존 ACL 헬퍼로 ACL 부여. mount + Invoke 구독(TopBar/Dock/DesktopIcon/Desktop). **M1에선 기존 FileTree/Explorer/Cli mount 유지**(회귀 최소화).

- [ ] **Step 2: 크로스컴파일 + 커밋**
```powershell
cargo zigbuild --target x86_64-unknown-linux-musl -p geulos-desktop-shell
git add apps/desktop-shell/src/main.rs
git commit -m "feat(desktop-shell): SP1 시작 시 TopBar/Dock/DesktopIcon mount + 구독"
```

---

### Task 7: M1 부팅 검증 (사용자 + AI 패리티)

- [ ] **Step 1: 빌드 + 부팅**
```powershell
Get-Process qemu-system-x86_64 -ErrorAction SilentlyContinue | Stop-Process -Force
& .\boot\build.ps1 -Release
& .\boot\qemu\launch.ps1 -Graphics
Get-Content boot/serial.log -Tail 40
```
Expected: 부팅 + desktop-shell이 7+개 객체 mount(TopBar/Dock/DesktopIcon 포함) 로그. **시각(사용자)**: 상단 네비바 + 우측 독 + 바탕화면 아이콘이 보인다(기존 패널은 아직 공존).

- [ ] **Step 2: 클릭 동작** — 바탕화면 아이콘/독 클릭 → 직렬로그에 launch 핸들러 도달(`OpenFileManager` 로그). (실제 창은 M2.)

- [ ] **Step 3: AI 패리티** — 호스트에서 ai-bridge/테스트 클라이언트로 서버에 `Invoke(Desktop, "launch", {app:"file_manager"})` 전송 → 동일 launch 핸들러 도달 로그 확인. = 명령 표면 통일 1차 입증.

---

## M2 — FileManager 창 (FileTree+Explorer 래핑)

> M1 인프라(launch 핸들러, 영역 계산, 크롬) 위에 구축. 기존 `FileTree`/`Explorer` 객체·렌더·메서드를 **창 본문**으로 재배치.

### Task 8: launch가 FileManager 창을 실제 mount
- `shell_methods::handle_launch`의 `OpenFileManager`를 실제 구현: `file_manager(owner, x,y,w,h, max_z+1)` + 자식 `file_tree`/`explorer` mount, FileManager.children=[ft,ex], Desktop.children에 추가, ACL+구독, focus batch(기존 window_methods 패턴). `AlreadyOpenFocus`는 focus invoke.

### Task 9: 컴포지터가 FileManager 창 렌더 + 본문 레이아웃
- `layout_desktop`: FileManager 창을 z-정렬 떠있는 창으로 push(기존 Window처럼 타이틀바/리사이즈 핸들 — `window_geom` 상수 재사용). 창 본문 rect를 좌(FileTree ~30%)/우(Explorer ~70%)로 분할해 **기존 FileTree/Explorer 레이아웃 함수에 origin 오프셋 전달**(렌더/레이아웃 함수에 base x/y 인자 추가 필요 — 현재 화면 절대좌표 가정 시).
- `render.rs`: FileManager = `render_window` 류 창틀 + 본문은 기존 FileTree/Explorer 렌더 위임(오프셋 적용).
- 입력: 창 타이틀 드래그=move, 모서리=resize, 닫기=close(기존 Window 분기 재사용). 본문 클릭=기존 FileTree/Explorer dispatch.

### Task 10: 고정 패널 제거 → 클린 바탕화면
- `main.rs`: 시작 시 FileTree/Explorer 고정 mount 제거(파일관리자 실행 시에만 창 자식으로 mount). `layout_desktop`에서 고정 좌/우 패널 분기 제거.
- 부팅 시 바탕화면+크롬만, 아이콘 클릭→파일관리자 창. **AI 패리티 테스트**: `Desktop.launch` Invoke로 동일 창 등장 + 트리에 FileManager+FileTree+Explorer 확인.
- 빌드+부팅 시각 확인(사용자).

---

## M3 — CLI 상하 리사이즈

### Task 11: set_cli_height + 드래그 핸들
- `shell_methods`: `Desktop.set_cli_height(px)` → clamp(layout 상수) 후 Desktop.state.cli_height SetState.
- 컴포지터: `CliResizeHandle` role 클릭→드래그 상태 시작, 드래그 중 포인터 y로 새 높이 계산, release(또는 드래그 중 throttle) 시 `Invoke(Desktop,"set_cli_height",{px})`. layout이 `Desktop.state.cli_height`를 `desktop_regions`에 전달(이미 인자).
- 빌드+부팅: CLI 상단 핸들 드래그로 높이 변경 확인(사용자). AI도 `set_cli_height` Invoke로 동일.

---

## Self-Review (작성자 점검)

**1. 스펙 커버리지:** 네비바/독/아이콘/바탕화면 → Task 1,5,6. 파일관리자 창=FileTree/Explorer 래핑 → Task 8,9,10. CLI 리사이즈 → Task 11. 명령표면 불변식(모든 동작=메서드) → Task 1(메서드 정의)+4(핸들러)+5(클릭=Invoke)+AI 패리티(Task 7,10). 앱 레지스트리/launch → Task 3,4. **누락 없음.**

**2. Placeholder 스캔:** M1은 완전 코드(factory/영역계산/레지스트리/핸들러 + 테스트). M2/M3는 작업 윤곽 — 각 마일스톤이 동작 산출물이며 M1 인프라+기존 코드 재사용을 명시. 실행 시 M2/M3 task는 기존 패턴(window_methods/render_window/layout_desktop) 참조로 구체화. **모호 동작 지시 없음**(에러처리=dedup/clamp/no-op 명시).

**3. 타입/시그니처 일관성:** `desktop_regions(win_w,win_h,cli_height)→DesktopRegions` (Task2) → layout_desktop/Task11에서 동일 사용. `resolve_app(&str)→Option<AppKind>` (Task3) → `handle_launch`(Task4)에서 사용. `HitRole` 새 variant(Task2) → 렌더/hit/Invoke(Task5)에서 사용. factory 시그니처(Task1) → main.rs mount(Task6)·launch mount(Task8)에서 사용. **일관.**

**알려진 리스크:** FileTree/Explorer 렌더가 화면 절대좌표 가정 시 창 본문 오프셋 보정 필요(Task9 명시). desktop-shell ACL/구독 누락 시 mount/invoke 실패(기존 헬퍼 패턴 준수). 한글 IME 미해결로 네비바/독 라벨은 보이나 텍스트 입력은 SP4 전까지 US-QWERTY.
