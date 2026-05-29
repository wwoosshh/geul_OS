//! GeulOS compositor library.

pub mod dispatch;
pub mod editor;
pub mod hit_test;
pub mod icons;
pub mod keyboard;
pub mod layout;
pub mod messages;
pub mod render;
pub mod server_client;
pub mod text;
pub mod theme;
pub mod tree_model;
pub mod vm_fb;
// DRM/KMS 디스플레이 백엔드 — drm 크레이트(Linux 전용)에 의존하므로 cfg 게이트.
#[cfg(target_os = "linux")]
pub mod vm_drm;
pub mod vm_input;
pub mod window_geom;
