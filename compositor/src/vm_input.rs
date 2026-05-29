//! VM 게스트 evdev(`/dev/input/event*`) 입력.
//!
//! 순수 파싱은 모든 타겟에서 테스트. 실제 open/poll/read는 cfg(linux) 게이트.

pub const EV_KEY: u16 = 0x01;
pub const EV_REL: u16 = 0x02;
pub const EV_ABS: u16 = 0x03;
pub const REL_WHEEL: u16 = 0x08;
pub const ABS_X: u16 = 0x00;
pub const ABS_Y: u16 = 0x01;
pub const BTN_LEFT: u16 = 0x110;

/// virtio-tablet 절대좌표 logical 최대값(virtio-input 기본 0..32767).
/// 정확한 값은 EVIOCGABS로 읽을 수 있으나 v1 증명에선 상수 가정.
pub const TABLET_LOGICAL_MAX: i32 = 32767;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawEvent {
    pub type_: u16,
    pub code: u16,
    pub value: i32,
}

/// 24바이트 input_event(x86_64, 64-bit timeval)에서 type/code/value 추출.
/// 레이아웃: [0..16]=timeval(무시), [16..18]=type, [18..20]=code, [20..24]=value.
pub fn parse_event(bytes: &[u8]) -> Option<RawEvent> {
    if bytes.len() < 24 {
        return None;
    }
    let type_ = u16::from_ne_bytes([bytes[16], bytes[17]]);
    let code = u16::from_ne_bytes([bytes[18], bytes[19]]);
    let value = i32::from_ne_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Some(RawEvent { type_, code, value })
}

/// 절대좌표(0..logical_max)를 화면 픽셀(0..screen-1)로 스케일.
pub fn scale_abs(val: i32, logical_max: i32, screen: u32) -> i32 {
    if logical_max <= 0 || screen == 0 {
        return 0;
    }
    let v = val.clamp(0, logical_max) as i64;
    ((v * (screen as i64 - 1)) / logical_max as i64) as i32
}

// 키보드 evdev 키코드 (linux/input-event-codes.h).
pub const KEY_BACKSPACE: u16 = 14;
/// Tab — GeulOS 한/영 토글 키. 우Alt(한/영키)는 Windows IME가 시스템 레벨에서
/// 가로채 VM에 전달되지 않으므로(0회 도달 확인), Windows가 안 먹고 CLI에서도
/// 미사용인 Tab으로 한/영을 토글한다.
pub const KEY_TAB: u16 = 15;
pub const KEY_ENTER: u16 = 28;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_RIGHTSHIFT: u16 = 54;
/// 좌/우 Ctrl — SP4 단축키(Ctrl+A/C/V/X) modifier.
pub const KEY_LEFTCTRL: u16 = 29;
pub const KEY_RIGHTCTRL: u16 = 97;
/// 좌Alt (Left Alt) — 한/영 토글 기본 키 (어느 키보드에서나 확실히 전달됨).
pub const KEY_LEFTALT: u16 = 56;
/// 우Alt (AltGr / Right Alt) — 한/영 토글 대안 키.
pub const KEY_RIGHTALT: u16 = 100;
/// 한/영 전환 키 (Linux keycode 122 = KEY_HANGEUL / KEY_HANGUEL).
pub const KEY_HANGEUL: u16 = 122;

/// US QWERTY evdev 키코드 → 문자(shift 반영). 글자/숫자/기본 문장부호/스페이스만.
/// 한글 IME는 별도(미구현) — winit이 해주던 logical_key/text 변환의 VM판.
pub fn keycode_to_char(code: u16, shift: bool) -> Option<char> {
    let base = match code {
        2 => '1', 3 => '2', 4 => '3', 5 => '4', 6 => '5',
        7 => '6', 8 => '7', 9 => '8', 10 => '9', 11 => '0',
        12 => '-', 13 => '=',
        16 => 'q', 17 => 'w', 18 => 'e', 19 => 'r', 20 => 't',
        21 => 'y', 22 => 'u', 23 => 'i', 24 => 'o', 25 => 'p',
        26 => '[', 27 => ']',
        30 => 'a', 31 => 's', 32 => 'd', 33 => 'f', 34 => 'g',
        35 => 'h', 36 => 'j', 37 => 'k', 38 => 'l', 39 => ';', 40 => '\'',
        41 => '`', 43 => '\\',
        44 => 'z', 45 => 'x', 46 => 'c', 47 => 'v', 48 => 'b',
        49 => 'n', 50 => 'm', 51 => ',', 52 => '.', 53 => '/',
        57 => ' ',
        _ => return None,
    };
    if !shift {
        return Some(base);
    }
    let shifted = match base {
        'a'..='z' => base.to_ascii_uppercase(),
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        '-' => '_',
        '=' => '+',
        '[' => '{',
        ']' => '}',
        ';' => ':',
        '\'' => '"',
        '`' => '~',
        '\\' => '|',
        ',' => '<',
        '.' => '>',
        '/' => '?',
        other => other,
    };
    Some(shifted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extracts_type_code_value() {
        let mut buf = [0u8; 24];
        buf[16..18].copy_from_slice(&EV_ABS.to_ne_bytes());
        buf[18..20].copy_from_slice(&ABS_X.to_ne_bytes());
        buf[20..24].copy_from_slice(&1234i32.to_ne_bytes());
        assert_eq!(parse_event(&buf), Some(RawEvent { type_: EV_ABS, code: ABS_X, value: 1234 }));
    }

    #[test]
    fn parse_rejects_short_buffer() {
        assert_eq!(parse_event(&[0u8; 10]), None);
    }

    #[test]
    fn scale_midpoint_maps_to_center() {
        // 절반 입력 → 화면 가운데 부근.
        assert_eq!(scale_abs(16383, 32767, 800), 399);
    }

    #[test]
    fn keymap_basic_and_shift() {
        assert_eq!(keycode_to_char(30, false), Some('a'));
        assert_eq!(keycode_to_char(30, true), Some('A'));
        assert_eq!(keycode_to_char(2, false), Some('1'));
        assert_eq!(keycode_to_char(2, true), Some('!'));
        assert_eq!(keycode_to_char(57, false), Some(' '));
        assert_eq!(keycode_to_char(KEY_ENTER, false), None); // Enter는 문자 아님
    }
}

#[cfg(target_os = "linux")]
pub use sys::EvdevSet;

#[cfg(target_os = "linux")]
mod sys {
    use super::parse_event;
    use std::fs::{File, OpenOptions};
    use std::io::Read;
    use std::os::fd::AsRawFd;

    /// 열린 모든 /dev/input/event* 모음. poll로 읽을 게 있는 fd만 read.
    pub struct EvdevSet {
        files: Vec<File>,
    }

    impl EvdevSet {
        /// /dev/input/event* 전부 non-blocking으로 open.
        pub fn open_all() -> Result<EvdevSet, String> {
            let mut files = Vec::new();
            for entry in
                std::fs::read_dir("/dev/input").map_err(|e| format!("read /dev/input: {}", e))?
            {
                let path = match entry {
                    Ok(e) => e.path(),
                    Err(_) => continue,
                };
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.starts_with("event") {
                    continue;
                }
                match OpenOptions::new().read(true).open(&path) {
                    Ok(f) => {
                        let fd = f.as_raw_fd();
                        unsafe {
                            let flags = libc::fcntl(fd, libc::F_GETFL);
                            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                        }
                        println!("[vm_input] opened {}", path.display());
                        files.push(f);
                    }
                    Err(e) => eprintln!("[vm_input] open {} failed: {}", path.display(), e),
                }
            }
            if files.is_empty() {
                return Err("no /dev/input/event* devices".into());
            }
            Ok(EvdevSet { files })
        }

        /// timeout_ms 동안 대기, 준비된 fd에서 input_event를 모두 읽어 콜백.
        pub fn poll_events<F: FnMut(super::RawEvent)>(&mut self, timeout_ms: i32, mut cb: F) {
            let mut pfds: Vec<libc::pollfd> = self
                .files
                .iter()
                .map(|f| libc::pollfd { fd: f.as_raw_fd(), events: libc::POLLIN, revents: 0 })
                .collect();
            let n =
                unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, timeout_ms) };
            if n <= 0 {
                return;
            }
            for (i, pfd) in pfds.iter().enumerate() {
                if pfd.revents & libc::POLLIN == 0 {
                    continue;
                }
                let mut buf = [0u8; 24 * 64];
                loop {
                    match self.files[i].read(&mut buf) {
                        Ok(0) => break,
                        Ok(read) => {
                            let mut off = 0;
                            while off + 24 <= read {
                                if let Some(ev) = parse_event(&buf[off..off + 24]) {
                                    cb(ev);
                                }
                                off += 24;
                            }
                        }
                        Err(_) => break, // EAGAIN 등 — 다음 poll에서
                    }
                }
            }
        }
    }
}
