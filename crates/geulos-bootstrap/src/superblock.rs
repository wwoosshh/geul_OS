//! ext 계열 슈퍼블록 매직 판정 (순수 — 호스트에서 테스트 가능).
//!
//! ext2/3/4는 디스크 시작에서 1024바이트 떨어진 슈퍼블록을 갖고, 그 안에서
//! 오프셋 0x38(=56)에 16비트 LE 매직 `0xEF53`이 있다. 즉 디바이스 기준
//! 절대 오프셋 0x438(=1080)에 매직. 이 매직이 있으면 "이미 포맷된 ext FS".

/// 디바이스 절대 오프셋: ext 슈퍼블록 매직 위치.
pub const EXT_MAGIC_OFFSET: usize = 0x438;
/// ext 매직 (LE u16).
pub const EXT_MAGIC: u16 = 0xEF53;

/// `region`은 디바이스 시작부터의 바이트(최소 `EXT_MAGIC_OFFSET + 2`바이트 필요).
/// ext 매직이 보이면 true(=이미 포맷됨). 짧거나 매직이 다르면 false(=빈 디스크 취급).
pub fn has_ext_magic(region: &[u8]) -> bool {
    let off = EXT_MAGIC_OFFSET;
    if region.len() < off + 2 {
        return false;
    }
    let magic = u16::from_le_bytes([region[off], region[off + 1]]);
    magic == EXT_MAGIC
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_or_short_region_is_not_formatted() {
        assert!(!has_ext_magic(&[]));
        assert!(!has_ext_magic(&[0u8; 100]));
        assert!(!has_ext_magic(&[0u8; EXT_MAGIC_OFFSET])); // 매직 직전까지만
    }

    #[test]
    fn zeroed_disk_is_not_formatted() {
        let region = vec![0u8; EXT_MAGIC_OFFSET + 2];
        assert!(!has_ext_magic(&region));
    }

    #[test]
    fn ext_magic_present_is_formatted() {
        let mut region = vec![0u8; EXT_MAGIC_OFFSET + 2];
        region[EXT_MAGIC_OFFSET] = 0x53; // LE 하위
        region[EXT_MAGIC_OFFSET + 1] = 0xEF; // LE 상위
        assert!(has_ext_magic(&region));
    }

    #[test]
    fn wrong_magic_is_not_formatted() {
        let mut region = vec![0u8; EXT_MAGIC_OFFSET + 2];
        region[EXT_MAGIC_OFFSET] = 0x34;
        region[EXT_MAGIC_OFFSET + 1] = 0x12;
        assert!(!has_ext_magic(&region));
    }
}
