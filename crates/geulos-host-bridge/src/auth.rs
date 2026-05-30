//! per-launch 토큰 인증. launch.ps1이 GEULOS_BRIDGE_TOKEN env로 전달 → main이 startup
//! 시 1회 로드. **토큰 미설정이면 기본 거부(fail-closed)** — 운영에선 반드시 토큰 설정.
//! 개발 환경에서만 `GEULOS_BRIDGE_INSECURE_NO_AUTH=1` 명시 opt-in으로 우회 허용.

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
        // 기본 거부 (fail-closed). 개발용 명시 opt-in만 통과.
        None => std::env::var("GEULOS_BRIDGE_INSECURE_NO_AUTH").as_deref() == Ok("1"),
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
