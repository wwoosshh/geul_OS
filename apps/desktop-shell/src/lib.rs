//! GeulOS 데스크톱 셸 라이브러리.
//!
//! M7: 워크스페이스 루트 스캔 → Desktop/FileTree/Canvas + Folder/File 트리 mount.
//! 단방향 동기화 — 객체 변경만 디스크에 기록 (FS watcher는 M9+).
//! T7.5: 하단 CLI 패널 — Cli@1 객체 mount + cli_handler dispatch.

pub mod ai_session;
pub mod host_bridge_client;
pub mod applauncher;
pub mod cli_handler;
pub mod dialog_ops;
pub mod drives;
pub mod explorer_ops;
pub mod file_ops;
pub mod file_read;
pub mod file_write;
pub mod folder_ops;
pub mod fs_ops;
pub mod fs_watcher;
pub mod granted_dirs;
pub mod handlers;
pub mod invoke_handler;
pub mod job_object;
pub mod lazy_mount;
pub mod permission;
pub mod process_registry;
pub mod scan;
pub mod window_ops;
pub mod workspace;
