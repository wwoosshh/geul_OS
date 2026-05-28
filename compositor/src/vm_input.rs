//! VM 게스트 evdev(`/dev/input/event*`) 입력.
//!
//! 순수 파싱은 모든 타겟에서 테스트. 실제 open/poll/read는 cfg(linux) 게이트.

pub const EV_KEY: u16 = 0x01;
pub const EV_ABS: u16 = 0x03;
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
