//! 디스크 동기화 시 "보존할 경로 vs 시스템 파일" 분류 (순수 — 호스트 테스트).
//!
//! B 모델: initramfs `/payload/*`를 디스크로 덮어쓰되, 사용자 데이터 디렉터리
//! (`root`, `home`)는 절대 건드리지 않는다. 입력은 디스크 루트 기준 상대경로
//! ("root", "root/notes.txt", "bin/geulosd" 등, 선행 슬래시 없음).

/// 동기화 시 보존(=덮어쓰기 금지)해야 하는 사용자 데이터 경로면 true.
/// `root`·`home` 자신과 그 하위 전부.
pub fn should_preserve(rel_path: &str) -> bool {
    let p = rel_path.trim_start_matches('/');
    let first = p.split('/').next().unwrap_or("");
    matches!(first, "root" | "home")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_dirs_preserved() {
        assert!(should_preserve("root"));
        assert!(should_preserve("root/notes.txt"));
        assert!(should_preserve("home"));
        assert!(should_preserve("home/geul/a.txt"));
        assert!(should_preserve("/root/x")); // 선행 슬래시 허용
    }

    #[test]
    fn system_dirs_not_preserved() {
        assert!(!should_preserve("bin/geulosd"));
        assert!(!should_preserve("sbin/init"));
        assert!(!should_preserve("lib/modules/6.12/ext4.ko"));
        assert!(!should_preserve("etc/geulos/marker"));
    }

    #[test]
    fn lookalike_not_preserved() {
        assert!(!should_preserve("rootfs")); // 'root'로 시작하지만 다른 디렉터리명
        assert!(!should_preserve("homework"));
    }
}
