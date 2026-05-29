//! VM 디스플레이 — DRM/KMS 직접 제어 (dumb buffer + 프레임마다 DIRTYFB flush).
//!
//! fbdev(`/dev/fb0`)는 지연 플러시(deferred-I/O)로 화면 갱신을 ~10fps로 제한했다.
//! DRM dumb buffer에 직접 렌더하고 매 프레임 `dirty_framebuffer`(DIRTYFB)로 virtio-gpu에
//! 명시적 flush를 보내면 호스트가 즉시 갱신 → 60fps 근접. (drm 크레이트 = 순수 ioctl FFI,
//! 외부 libdrm 링크 없음 → musl 정적 빌드 가능.)

use std::fs::{File, OpenOptions};
use std::os::unix::io::{AsFd, BorrowedFd};

use drm::buffer::{Buffer, DrmFourcc};
use drm::control::{
    connector, dumbbuffer::DumbBuffer, framebuffer, ClipRect, Device as ControlDevice,
};
use drm::Device as BasicDevice;

/// `/dev/dri/card0` 래퍼 — drm 크레이트 트레이트 구현의 전제(AsFd).
struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}
impl BasicDevice for Card {}
impl ControlDevice for Card {}

pub struct DrmDisplay {
    card: Card,
    db: DumbBuffer,
    fb: framebuffer::Handle,
    pub xres: usize,
    pub yres: usize,
    pitch: usize,
    dirty_warned: bool,
}

impl DrmDisplay {
    /// card0 열고 연결된 커넥터/CRTC를 찾아 모드 설정 + dumb buffer 스캔아웃.
    pub fn open() -> Result<DrmDisplay, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/dri/card0")
            .map_err(|e| format!("open /dev/dri/card0: {}", e))?;
        let card = Card(file);

        let res = card.resource_handles().map_err(|e| format!("resource_handles: {}", e))?;

        // 연결된 커넥터 찾기.
        let con = res
            .connectors()
            .iter()
            .filter_map(|c| card.get_connector(*c, true).ok())
            .find(|i| i.state() == connector::State::Connected)
            .ok_or_else(|| "연결된 커넥터 없음".to_string())?;

        // 모드 선택: 1280x800 우선, 없으면 첫(선호) 모드.
        let modes = con.modes();
        if modes.is_empty() {
            return Err("커넥터에 모드 없음".into());
        }
        let mode = modes.iter().copied().find(|m| m.size() == (1280, 800)).unwrap_or(modes[0]);
        let (w, h) = mode.size();
        println!("[vm-drm] 커넥터 모드 {}개, 선택 {}x{}", modes.len(), w, h);

        // CRTC — virtio-gpu는 단일. 첫 CRTC 사용.
        let crtc = res.crtcs().first().copied().ok_or_else(|| "CRTC 없음".to_string())?;

        // dumb buffer (XRGB8888, 32bpp).
        let mut db = card
            .create_dumb_buffer((w as u32, h as u32), DrmFourcc::Xrgb8888, 32)
            .map_err(|e| format!("create_dumb_buffer: {}", e))?;
        let pitch = db.pitch() as usize;

        // 초기 클리어(검정).
        {
            let mut map = card.map_dumb_buffer(&mut db).map_err(|e| format!("map: {}", e))?;
            for b in map.as_mut() {
                *b = 0;
            }
        }

        let fb = card.add_framebuffer(&db, 24, 32).map_err(|e| format!("add_framebuffer: {}", e))?;

        card.set_crtc(crtc, Some(fb), (0, 0), &[con.handle()], Some(mode))
            .map_err(|e| format!("set_crtc: {}", e))?;

        println!("[vm-drm] DRM 디스플레이 {}x{} pitch={} 설정 완료", w, h, pitch);

        Ok(DrmDisplay {
            card,
            db,
            fb,
            xres: w as usize,
            yres: h as usize,
            pitch,
            dirty_warned: false,
        })
    }

    /// 컴포지터 캔버스(0xAARRGGBB u32)를 dumb buffer에 복사 + DIRTYFB flush.
    pub fn present(&mut self, buffer: &[u32]) {
        {
            let mut map = match self.card.map_dumb_buffer(&mut self.db) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("[vm-drm] map 실패: {}", e);
                    return;
                }
            };
            let dst = map.as_mut();
            let pitch = self.pitch;
            for y in 0..self.yres {
                let row = y * pitch;
                let src_row = y * self.xres;
                for x in 0..self.xres {
                    let argb = buffer[src_row + x];
                    let o = row + x * 4;
                    if o + 4 <= dst.len() {
                        // XRGB8888 little-endian: [B, G, R, X]
                        dst[o] = (argb & 0xFF) as u8;
                        dst[o + 1] = ((argb >> 8) & 0xFF) as u8;
                        dst[o + 2] = ((argb >> 16) & 0xFF) as u8;
                        dst[o + 3] = 0;
                    }
                }
            }
        }
        let clip = ClipRect::new(0, 0, self.xres as u16, self.yres as u16);
        if let Err(e) = self.card.dirty_framebuffer(self.fb, &[clip]) {
            if !self.dirty_warned {
                eprintln!("[vm-drm] dirty_framebuffer 미지원? {} — set_crtc 스캔아웃만 의존", e);
                self.dirty_warned = true;
            }
        }
    }
}
