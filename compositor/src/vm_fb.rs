//! VM 게스트 framebuffer(`/dev/fb0`) 출력.
//!
//! 순수 로직(픽셀 형식 변환)은 모든 타겟에서 컴파일·테스트. 실제 mmap/ioctl은
//! `cfg(target_os = "linux")`로 게이트.

/// fb_var_screeninfo의 red/green/blue bitfield offset에서 결정된 픽셀 형식.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelFormat {
    pub bits_per_pixel: u32,
    pub red_offset: u32,
    pub green_offset: u32,
    pub blue_offset: u32,
}

/// 컴포지터 색(0xAARRGGBB)을 fb 픽셀 워드로 변환 (32bpp 가정).
/// 알파는 버린다 — fb는 보통 X 비트.
pub fn argb_to_fb_pixel(argb: u32, fmt: &PixelFormat) -> u32 {
    let r = (argb >> 16) & 0xFF;
    let g = (argb >> 8) & 0xFF;
    let b = argb & 0xFF;
    (r << fmt.red_offset) | (g << fmt.green_offset) | (b << fmt.blue_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xrgb_format_is_identity_on_rgb() {
        // XRGB8888: R@16 G@8 B@0 → 입력 RGB가 그대로.
        let fmt = PixelFormat { bits_per_pixel: 32, red_offset: 16, green_offset: 8, blue_offset: 0 };
        assert_eq!(argb_to_fb_pixel(0xFF_12_34_56, &fmt), 0x12_34_56);
    }

    #[test]
    fn bgr_format_swaps_red_blue() {
        // BGRX 류: R@0 B@16 → R/B 위치 교환.
        let fmt = PixelFormat { bits_per_pixel: 32, red_offset: 0, green_offset: 8, blue_offset: 16 };
        assert_eq!(argb_to_fb_pixel(0xFF_12_34_56, &fmt), 0x56_34_12);
    }
}

#[cfg(target_os = "linux")]
pub use sys::Framebuffer;

#[cfg(target_os = "linux")]
mod sys {
    use super::{argb_to_fb_pixel, PixelFormat};
    use std::fs::OpenOptions;
    use std::os::fd::AsRawFd;

    // linux/fb.h ioctl 요청 번호. musl은 ioctl request가 c_int, glibc는 c_ulong이라
    // 호출 시 `as _`로 코어스.
    const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;
    const FBIOGET_FSCREENINFO: libc::c_ulong = 0x4602;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct FbBitfield {
        offset: u32,
        length: u32,
        msb_right: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct FbVarScreeninfo {
        xres: u32,
        yres: u32,
        xres_virtual: u32,
        yres_virtual: u32,
        xoffset: u32,
        yoffset: u32,
        bits_per_pixel: u32,
        grayscale: u32,
        red: FbBitfield,
        green: FbBitfield,
        blue: FbBitfield,
        transp: FbBitfield,
        nonstd: u32,
        activate: u32,
        height: u32,
        width: u32,
        accel_flags: u32,
        pixclock: u32,
        left_margin: u32,
        right_margin: u32,
        upper_margin: u32,
        lower_margin: u32,
        hsync_len: u32,
        vsync_len: u32,
        sync: u32,
        vmode: u32,
        rotate: u32,
        colorspace: u32,
        reserved: [u32; 4],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct FbFixScreeninfo {
        id: [u8; 16],
        smem_start: libc::c_ulong,
        smem_len: u32,
        type_: u32,
        type_aux: u32,
        visual: u32,
        xpanstep: u16,
        ypanstep: u16,
        ywrapstep: u16,
        line_length: u32,
        mmio_start: libc::c_ulong,
        mmio_len: u32,
        accel: u32,
        capabilities: u16,
        reserved: [u16; 2],
    }

    pub struct Framebuffer {
        ptr: *mut u8,
        map_len: usize,
        pub xres: usize,
        pub yres: usize,
        stride_bytes: usize, // line_length
        bpp_bytes: usize,    // bits_per_pixel / 8
        fmt: PixelFormat,
        _file: std::fs::File,
    }

    impl Framebuffer {
        /// `/dev/fb0` 열고 해상도/형식 질의 후 mmap.
        pub fn open() -> Result<Framebuffer, String> {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/fb0")
                .map_err(|e| format!("open /dev/fb0: {}", e))?;
            let fd = file.as_raw_fd();

            let mut var: FbVarScreeninfo = unsafe { std::mem::zeroed() };
            let mut fix: FbFixScreeninfo = unsafe { std::mem::zeroed() };
            let r1 = unsafe { libc::ioctl(fd, FBIOGET_VSCREENINFO as _, &mut var as *mut _) };
            if r1 < 0 {
                return Err(format!("FBIOGET_VSCREENINFO: {}", std::io::Error::last_os_error()));
            }
            let r2 = unsafe { libc::ioctl(fd, FBIOGET_FSCREENINFO as _, &mut fix as *mut _) };
            if r2 < 0 {
                return Err(format!("FBIOGET_FSCREENINFO: {}", std::io::Error::last_os_error()));
            }

            let map_len = fix.smem_len as usize;
            let ptr = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    map_len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    fd,
                    0,
                )
            };
            if ptr == libc::MAP_FAILED {
                return Err(format!("mmap fb0: {}", std::io::Error::last_os_error()));
            }

            Ok(Framebuffer {
                ptr: ptr as *mut u8,
                map_len,
                xres: var.xres as usize,
                yres: var.yres as usize,
                stride_bytes: fix.line_length as usize,
                bpp_bytes: (var.bits_per_pixel / 8) as usize,
                fmt: PixelFormat {
                    bits_per_pixel: var.bits_per_pixel,
                    red_offset: var.red.offset,
                    green_offset: var.green.offset,
                    blue_offset: var.blue.offset,
                },
                _file: file,
            })
        }

        pub fn format(&self) -> PixelFormat {
            self.fmt
        }

        /// 컴포지터 픽셀 배열(`&[u32]`, 0xAARRGGBB, width=xres 가정)을 화면에 blit.
        pub fn present(&mut self, buffer: &[u32]) {
            if self.bpp_bytes != 4 {
                eprintln!("[vm_fb] 미지원 bpp={} (32만 지원) — skip", self.bpp_bytes * 8);
                return;
            }
            for y in 0..self.yres {
                let row_off = y * self.stride_bytes;
                for x in 0..self.xres {
                    let argb = buffer[y * self.xres + x];
                    let px = argb_to_fb_pixel(argb, &self.fmt);
                    let byte = row_off + x * 4;
                    if byte + 4 <= self.map_len {
                        unsafe {
                            *(self.ptr.add(byte) as *mut u32) = px;
                        }
                    }
                }
            }
        }
    }

    impl Drop for Framebuffer {
        fn drop(&mut self) {
            unsafe {
                libc::munmap(self.ptr as *mut libc::c_void, self.map_len);
            }
        }
    }
}
