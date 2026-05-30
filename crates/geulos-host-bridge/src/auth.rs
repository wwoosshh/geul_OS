//! per-launch 토큰 인증. launch.ps1이 GEULOS_BRIDGE_TOKEN env로 전달 → main이 startup
//! 시 1회 로드. 토큰 미설정이면 인증 비활성(개발용 fallback) — v1.5 정상 운영은 항상 설정.

use std::sync::OnceLock;

static TOKEN: OnceLock<Option<String>> = OnceLock::new();

pub fn init_from_env() {
    let t = std::env::var("GEULOS_BRIDGE_TOKEN").ok().filter(|s| !s.is_empty());
    let _ = TOKEN.set(t);
}

pub fn verify(received: &str) -> bool {
    match TOKEN.get().and_then(|o| o.as_deref()) {
        Some(expected) => {
            let a = expected.as_bytes();
            let b = received.as_bytes();
            if a.len() != b.len() {
                return false;
            }
            let mut diff: u8 = 0;
            for (x, y) in a.iter().zip(b.iter()) {
                diff |= x ^ y;
            }
            diff == 0
        }
        None => true,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn constant_time_compare_basic() {
        fn ct_eq(a: &[u8], b: &[u8]) -> bool {
            if a.len() != b.len() {
                return false;
            }
            let mut diff: u8 = 0;
            for (x, y) in a.iter().zip(b.iter()) {
                diff |= x ^ y;
            }
            diff == 0
        }
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }
}
