//! Window 오버레이 영역 상수 — render와 입력 처리가 공유.
//!
//! M8 T8.9에서 분리. render.rs와 main.rs가 같은 상수를 참조해야 *시각 영역*과
//! *클릭 영역*이 어긋나지 않는다 (title bar 24px, [x] 16px, resize 10px 등).

/// Window title bar 높이 (px).
pub const WINDOW_TITLE_H: i32 = 24;
/// 우하 resize handle 한 변 (px).
pub const WINDOW_RESIZE_HANDLE: i32 = 10;
/// [x] 닫기 버튼 한 변 (px).
pub const WINDOW_CLOSE_BTN: i32 = 16;
/// resize 시 최소 폭 (px).
pub const WINDOW_MIN_W: i32 = 200;
/// resize 시 최소 높이 (px).
pub const WINDOW_MIN_H: i32 = 120;
