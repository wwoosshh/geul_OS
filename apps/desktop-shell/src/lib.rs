//! GeulOS 데스크톱 셸 라이브러리.
//!
//! M7: 워크스페이스 루트 스캔 → Desktop/FileTree/Canvas + Folder/File 트리 mount.
//! 단방향 동기화 — 객체 변경만 디스크에 기록 (FS watcher는 M9+).

pub mod workspace;
