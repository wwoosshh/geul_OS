//! AI가 부여받은 *디렉터리 grant* in-memory 캐시 (M10 Phase 1 / ADR-036).
//!
//! 한 dir에 대해 [허용] Dialog를 한 번 처리하면 그 dir 안 후속 write/create/rename은
//! confirm 없이 통과. 세션 = desktop-shell process 한 번 실행 — AI 채팅 세션 (`/ai start
//! ... /exit`) 무관. process 종료 시 자연 reset.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Default)]
pub struct GrantedDirs {
    inner: Mutex<HashSet<PathBuf>>,
}

impl GrantedDirs {
    pub fn new() -> Self {
        Self::default()
    }

    /// 특정 dir에 대한 grant 여부.
    pub fn contains(&self, dir: &Path) -> bool {
        self.inner.lock().expect("GrantedDirs poisoned").contains(dir)
    }

    /// dir grant 추가. 이미 있으면 무동작.
    pub fn insert(&self, dir: PathBuf) {
        self.inner.lock().expect("GrantedDirs poisoned").insert(dir);
    }

    /// 현재 grant된 모든 dir 목록 (UI 표시·테스트용).
    pub fn list(&self) -> Vec<PathBuf> {
        let g = self.inner.lock().expect("GrantedDirs poisoned");
        g.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("GrantedDirs poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_then_contains() {
        let g = GrantedDirs::new();
        let d = PathBuf::from("/tmp/x");
        assert!(!g.contains(&d));
        g.insert(d.clone());
        assert!(g.contains(&d));
    }

    #[test]
    fn insert_duplicate_is_no_op() {
        let g = GrantedDirs::new();
        let d = PathBuf::from("/tmp/x");
        g.insert(d.clone());
        g.insert(d.clone());
        assert_eq!(g.len(), 1);
    }

    #[test]
    fn different_dirs_independent() {
        let g = GrantedDirs::new();
        g.insert(PathBuf::from("/a"));
        g.insert(PathBuf::from("/b"));
        assert!(g.contains(Path::new("/a")));
        assert!(g.contains(Path::new("/b")));
        assert!(!g.contains(Path::new("/c")));
        assert_eq!(g.len(), 2);
    }
}
