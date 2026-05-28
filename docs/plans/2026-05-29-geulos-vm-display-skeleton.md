# VM 디스플레이 기초 골격 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** VM 게스트 안에서 화면(`/dev/fb0`, virtio-gpu)에 사각형을 그리고 마우스 클릭/키 입력(`/dev/input/event*`, virtio-input)을 읽는다는 것을 standalone 실행파일로 증명한다.

**Architecture:** 호스트 컴포지터의 winit/softbuffer 의존을 `cfg(not(target_os="linux"))`로 분리해 컴포지터 크레이트가 musl로 크로스 컴파일되게 한 뒤, Linux 전용 framebuffer/evdev 입출력 모듈을 추가한다. 증명 실행파일은 기존 `fill_rect` 그리기 코드를 재사용하고, 부팅 이미지에 virtio-gpu/virtio-input 커널 모듈을 포함시켜 init이 적재한다.

**Tech Stack:** Rust (musl 크로스 컴파일), libc (ioctl/mmap), Linux framebuffer ABI(`/dev/fb0`) + evdev ABI(`/dev/input/event*`), QEMU(virtio-gpu-pci/virtio-keyboard-pci/virtio-tablet-pci), PowerShell 빌드 스크립트.

---

## File Structure

| 파일 | 책임 | 변경 |
|---|---|---|
| `boot/modules/fetch.ps1` | Alpine apk에서 .ko 추출 | Modify — 모듈 목록에 화면/입력 모듈 추가 |
| `geulos-init/src/modules.rs` | finit_module 적재 순서 | Modify — LOAD_ORDER에 DRM/virtio-gpu/virtio-input/evdev 추가 |
| `compositor/Cargo.toml` | 의존성/바이너리 | Modify — 호스트 deps 타겟 분리 + libc(linux) + 새 bin |
| `compositor/src/lib.rs` | 모듈 선언 | Modify — server_client는 non-linux, vm_fb/vm_input 추가 |
| `compositor/src/render.rs` | 그리기 헬퍼 | Modify — `fill_rect` pub화 (1줄) |
| `compositor/src/vm_fb.rs` | `/dev/fb0` 출력 | Create |
| `compositor/src/vm_input.rs` | `/dev/input/event*` 입력 | Create |
| `compositor/src/bin/geulos-vm-skeleton.rs` | 증명 실행파일 | Create |
| `boot/build.ps1` | 크로스 컴파일 + initrd 조립 | Modify — skeleton bin 빌드 + initrd 포함 |
| `geulos-init/src/spawn.rs` | 자식 spawn | Modify — skeleton spawn 추가 |
| `boot/qemu/launch.ps1` | QEMU 부팅 | Modify — `-Graphics` 스위치 + virtio 디바이스 |

순수 로직(픽셀 형식 변환, 이벤트 파싱)은 `cfg` 게이트 없이 항상 컴파일되어 호스트에서 단위 테스트한다. 실제 syscall(mmap/ioctl/open)만 `cfg(target_os="linux")`로 게이트한다.

---

## Task 1: 부팅 이미지에 화면/입력 커널 모듈 포함

**Files:**
- Modify: `boot/modules/fetch.ps1:15` (기본 `$ModuleNames`)

의존 사슬 (modules.dep 확인됨): virtio-gpu ← virtio_dma_buf, drm_shmem_helper, drm_kms_helper, drm, virtio, virtio_ring. drm_shmem_helper ← drm_kms_helper, drm. drm_kms_helper ← drm. virtio_pci ← virtio_pci_legacy_dev, virtio_pci_modern_dev, virtio, virtio_ring. virtio_input ← virtio, virtio_ring. evdev ← (없음).

- [ ] **Step 1: 모듈 목록 확장**

`boot/modules/fetch.ps1`의 `param` 블록에서 기본값 교체:

```powershell
    [string[]]$ModuleNames = @(
        "e1000",
        "virtio", "virtio_ring",
        "virtio_pci", "virtio_pci_modern_dev", "virtio_pci_legacy_dev",
        "virtio_dma_buf",
        "drm", "drm_kms_helper", "drm_shmem_helper",
        "virtio-gpu",
        "virtio_input", "evdev"
    ),
```

(주의: `virtio-gpu`는 하이픈. fetch.ps1은 `"$modName.ko.gz"`로 매칭하므로 `virtio-gpu.ko.gz`를 찾는다.)

- [ ] **Step 2: 모듈 추출 실행**

Run:
```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;C:\Program Files\qemu;$env:PATH"
pwsh boot/modules/fetch.ps1 -Force
```
Expected: 콘솔에 `decompress: virtio-gpu.ko.gz -> virtio-gpu.ko` 등 각 모듈 추출 로그. 경고 없이 13개 모듈 처리.

- [ ] **Step 3: 추출 결과 확인**

Run:
```powershell
Get-ChildItem boot/modules/6.12.89-0-lts/*.ko | Select-Object Name, @{n='KB';e={[math]::Round($_.Length/1KB,1)}}
```
Expected: e1000.ko, virtio.ko, virtio_ring.ko, virtio_pci.ko, virtio_pci_modern_dev.ko, virtio_pci_legacy_dev.ko, virtio_dma_buf.ko, drm.ko, drm_kms_helper.ko, drm_shmem_helper.ko, virtio-gpu.ko, virtio_input.ko, evdev.ko — 13개 모두 존재. (`virtio-gpu.ko` 또는 `virtio_input.ko`이 누락이면 apk 내 경로/이름 확인.)

- [ ] **Step 4: 커밋**

```powershell
git add boot/modules/fetch.ps1
git commit -m "build(boot): fetch.ps1 — 화면(virtio-gpu)+입력(virtio-input) 커널 모듈 추출 추가"
```

---

## Task 2: init 모듈 적재 순서 확장

**Files:**
- Modify: `geulos-init/src/modules.rs:53-61` (`LOAD_ORDER`)

`load_all()`은 디렉터리의 모든 `.ko`를 적재하되 `LOAD_ORDER`의 항목을 *먼저, 그 순서대로* 적재한다. 의존 모듈이 먼저 와야 finit_module이 성공한다.

- [ ] **Step 1: LOAD_ORDER 교체**

`geulos-init/src/modules.rs`의 `const LOAD_ORDER` 전체 교체:

```rust
const LOAD_ORDER: &[&str] = &[
    // virtio 코어 + PCI 전송 (의존 그래프 최하단)
    "virtio.ko",
    "virtio_ring.ko",
    "virtio_pci_legacy_dev.ko",
    "virtio_pci_modern_dev.ko",
    "virtio_pci.ko",
    // DRM 스택 (drm → kms_helper → shmem_helper)
    "virtio_dma_buf.ko",
    "drm.ko",
    "drm_kms_helper.ko",
    "drm_shmem_helper.ko",
    // 디스플레이 드라이버
    "virtio-gpu.ko",
    // 입력
    "virtio_input.ko",
    "evdev.ko",
    // 네트워크
    "e1000.ko",
    "virtio_net.ko",
];
```

- [ ] **Step 2: musl 크로스 컴파일 검증**

Linux 전용 코드라 Windows 로컬 검사로는 안 잡힌다 (메모리 교훈).

Run:
```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo build --target x86_64-unknown-linux-musl -p geulos-init
```
Expected: `Finished` (에러 없음).

- [ ] **Step 3: 커밋**

```powershell
git add geulos-init/src/modules.rs
git commit -m "feat(init): LOAD_ORDER에 DRM/virtio-gpu/virtio-input/evdev 의존 순서 추가"
```

---

## Task 3: 컴포지터 호스트 의존을 타겟별로 분리

**Files:**
- Modify: `compositor/Cargo.toml:18-37` ([dependencies])
- Modify: `compositor/src/lib.rs:10` (`pub mod server_client;`)

호스트 전용 크레이트(winit/softbuffer/arboard)는 musl에서 링크 불가. `cfg(not(target_os="linux"))`로 분리하면 VM 빌드 시 컴파일되지 않는다. `server_client`는 winit을 쓰므로 함께 non-linux로 게이트. `geulos-compositor`(winit) 바이너리는 `--bin geulos-vm-skeleton` 빌드 시 컴파일되지 않으므로 그대로 둔다.

- [ ] **Step 1: Cargo.toml 의존성 분리**

`compositor/Cargo.toml`의 `[dependencies]`에서 `winit`, `softbuffer`, `arboard` 세 줄 제거 후, 파일 끝에 타겟별 블록 추가:

```toml
[dependencies]
geulos-core = { path = "../core" }
geulos-proto = { path = "../proto" }
tokio = { workspace = true }
fontdue = { workspace = true }
serde = { workspace = true }
serde_json = "1.0"
uuid = { workspace = true }
chrono = { workspace = true }
image = { version = "0.25", default-features = false, features = ["png"] }

# 호스트 개발 모드 전용 (winit 창). VM(Linux)에서는 fb/evdev 경로를 쓰므로 불필요.
[target.'cfg(not(target_os = "linux"))'.dependencies]
winit = { workspace = true }
softbuffer = { workspace = true }
arboard = "3"

# VM(Linux) 전용 — /dev/fb0 mmap + evdev ioctl/read.
[target.'cfg(target_os = "linux")'.dependencies]
libc = "0.2"
```

(`[[bin]] geulos-compositor`, `[lib]` 기존 블록은 유지. `geulos-vm-skeleton` bin 선언은 파일을 만드는 Task 7에서 추가한다 — 지금 선언하면 파일 부재로 중간 빌드가 깨진다.)

- [ ] **Step 2: lib.rs에서 server_client만 게이트**

`compositor/src/lib.rs`의 `pub mod server_client;` 줄을 교체:

```rust
#[cfg(not(target_os = "linux"))]
pub mod server_client;
```

(vm_fb/vm_input 선언은 각 파일을 만드는 Task 5·6에서 추가한다. 지금 선언하면 파일 부재로 빌드가 깨진다.)

- [ ] **Step 3: 호스트 + musl 빌드 무회귀 확인**

Run:
```powershell
cargo build -p geulos-compositor
cargo build --target x86_64-unknown-linux-musl -p geulos-compositor --lib
```
Expected: 둘 다 `Finished`. 호스트는 winit 경로 그대로, musl은 server_client/winit 제외된 순수 lib만 컴파일. (musl `--lib`이 처음으로 의존성 분리를 검증하는 지점 — 링크 에러 시 Cargo.toml 타겟 분리 재확인.)

- [ ] **Step 4: 커밋**

```powershell
git add compositor/Cargo.toml compositor/src/lib.rs
git commit -m "refactor(compositor): 호스트 의존(winit/softbuffer/arboard) 타겟 분리 + libc(linux)"
```

---

## Task 4: render.rs `fill_rect` 공개

**Files:**
- Modify: `compositor/src/render.rs:794`

- [ ] **Step 1: pub 추가**

`compositor/src/render.rs:794`:
```rust
pub fn fill_rect(buffer: &mut [u32], w: usize, h: usize, r: &Rect, color: u32) {
```
(`fn` → `pub fn`만 변경.)

- [ ] **Step 2: 호스트 빌드 확인**

Run:
```powershell
cargo build -p geulos-compositor
```
Expected: `Finished`. (이 시점 lib는 server_client(호스트)+pure 모듈, vm 모듈/skeleton bin 미선언 — 정상 컴파일.)

- [ ] **Step 3: 커밋**

```powershell
git add compositor/src/render.rs
git commit -m "refactor(compositor): fill_rect pub화 — VM skeleton 재사용"
```

---

## Task 5: framebuffer 출력 모듈 (vm_fb.rs)

**Files:**
- Modify: `compositor/src/lib.rs` (`pub mod vm_fb;` 선언 추가)
- Create: `compositor/src/vm_fb.rs`
- Test: 같은 파일 `#[cfg(test)] mod tests`

- [ ] **Step 1a: lib.rs에 모듈 선언**

`compositor/src/lib.rs`의 server_client 게이트 다음에 추가:

```rust
pub mod vm_fb;
```

(cfg 게이트 없음 — 순수 로직은 모든 타겟에서 컴파일, syscall만 모듈 내부에서 게이트.)

- [ ] **Step 1: 순수 로직 + 단위 테스트 작성**

`compositor/src/vm_fb.rs` 생성:

```rust
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
```

- [ ] **Step 2: 단위 테스트 통과 확인**

Run:
```powershell
cargo test -p geulos-compositor vm_fb
```
Expected: `test result: ok. 2 passed` (Step 1a에서 lib.rs에 선언했으므로 바로 통과 — 순수 함수라 TDD 빨강 단계 생략 무방).

- [ ] **Step 3: Linux syscall 부분 작성**

`vm_fb.rs` 끝에 추가:

```rust
#[cfg(target_os = "linux")]
pub use sys::Framebuffer;

#[cfg(target_os = "linux")]
mod sys {
    use super::{argb_to_fb_pixel, PixelFormat};
    use std::fs::OpenOptions;
    use std::os::fd::AsRawFd;

    const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;
    const FBIOGET_FSCREENINFO: libc::c_ulong = 0x4602;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct FbBitfield { offset: u32, length: u32, msb_right: u32 }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct FbVarScreeninfo {
        xres: u32, yres: u32, xres_virtual: u32, yres_virtual: u32,
        xoffset: u32, yoffset: u32, bits_per_pixel: u32, grayscale: u32,
        red: FbBitfield, green: FbBitfield, blue: FbBitfield, transp: FbBitfield,
        nonstd: u32, activate: u32, height: u32, width: u32, accel_flags: u32,
        pixclock: u32, left_margin: u32, right_margin: u32, upper_margin: u32,
        lower_margin: u32, hsync_len: u32, vsync_len: u32, sync: u32,
        vmode: u32, rotate: u32, colorspace: u32, reserved: [u32; 4],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct FbFixScreeninfo {
        id: [u8; 16], smem_start: libc::c_ulong, smem_len: u32,
        type_: u32, type_aux: u32, visual: u32,
        xpanstep: u16, ypanstep: u16, ywrapstep: u16,
        line_length: u32, mmio_start: libc::c_ulong, mmio_len: u32, accel: u32,
        capabilities: u16, reserved: [u16; 2],
    }

    pub struct Framebuffer {
        ptr: *mut u8,
        map_len: usize,
        pub xres: usize,
        pub yres: usize,
        stride_bytes: usize,   // line_length
        bpp_bytes: usize,      // bits_per_pixel / 8
        fmt: PixelFormat,
        _file: std::fs::File,
    }

    impl Framebuffer {
        /// `/dev/fb0` 열고 해상도/형식 질의 후 mmap.
        pub fn open() -> Result<Framebuffer, String> {
            let file = OpenOptions::new().read(true).write(true).open("/dev/fb0")
                .map_err(|e| format!("open /dev/fb0: {}", e))?;
            let fd = file.as_raw_fd();

            let mut var: FbVarScreeninfo = unsafe { std::mem::zeroed() };
            let mut fix: FbFixScreeninfo = unsafe { std::mem::zeroed() };
            let r1 = unsafe { libc::ioctl(fd, FBIOGET_VSCREENINFO, &mut var as *mut _) };
            if r1 < 0 { return Err(format!("FBIOGET_VSCREENINFO: {}", std::io::Error::last_os_error())); }
            let r2 = unsafe { libc::ioctl(fd, FBIOGET_FSCREENINFO, &mut fix as *mut _) };
            if r2 < 0 { return Err(format!("FBIOGET_FSCREENINFO: {}", std::io::Error::last_os_error())); }

            let map_len = fix.smem_len as usize;
            let ptr = unsafe {
                libc::mmap(std::ptr::null_mut(), map_len,
                           libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0)
            };
            if ptr == libc::MAP_FAILED { return Err(format!("mmap fb0: {}", std::io::Error::last_os_error())); }

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

        pub fn format(&self) -> PixelFormat { self.fmt }

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
            unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.map_len); }
        }
    }
}
```

- [ ] **Step 4: musl 크로스 컴파일 (skeleton bin은 Task 7에서 생성하므로 lib만)**

Run:
```powershell
cargo build --target x86_64-unknown-linux-musl -p geulos-compositor --lib
```
Expected: `Finished`. (winit/softbuffer가 타겟 분리됐는지 검증되는 첫 지점 — 링크 에러 시 Task 3 의존성 분리 재확인.)

- [ ] **Step 5: 호스트 단위 테스트 통과 확인 + 커밋**

Run:
```powershell
cargo test -p geulos-compositor vm_fb
```
Expected: `test result: ok. 2 passed`.

```powershell
git add compositor/src/lib.rs compositor/src/vm_fb.rs
git commit -m "feat(compositor): vm_fb — /dev/fb0 mmap + ARGB→fb 픽셀 변환 (Linux)"
```

---

## Task 6: evdev 입력 모듈 (vm_input.rs)

**Files:**
- Modify: `compositor/src/lib.rs` (`pub mod vm_input;` 선언 추가)
- Create: `compositor/src/vm_input.rs`
- Test: 같은 파일

- [ ] **Step 1a: lib.rs에 모듈 선언**

`compositor/src/lib.rs`의 `pub mod vm_fb;` 다음에 추가:

```rust
pub mod vm_input;
```

- [ ] **Step 1: 순수 로직 + 단위 테스트 작성**

`compositor/src/vm_input.rs` 생성:

```rust
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
pub struct RawEvent { pub type_: u16, pub code: u16, pub value: i32 }

/// 24바이트 input_event(x86_64, 64-bit timeval)에서 type/code/value 추출.
/// 레이아웃: [0..16]=timeval(무시), [16..18]=type, [18..20]=code, [20..24]=value.
pub fn parse_event(bytes: &[u8]) -> Option<RawEvent> {
    if bytes.len() < 24 { return None; }
    let type_ = u16::from_ne_bytes([bytes[16], bytes[17]]);
    let code = u16::from_ne_bytes([bytes[18], bytes[19]]);
    let value = i32::from_ne_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Some(RawEvent { type_, code, value })
}

/// 절대좌표(0..logical_max)를 화면 픽셀(0..screen-1)로 스케일.
pub fn scale_abs(val: i32, logical_max: i32, screen: u32) -> i32 {
    if logical_max <= 0 || screen == 0 { return 0; }
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
```

- [ ] **Step 2: 단위 테스트 통과 확인**

Run:
```powershell
cargo test -p geulos-compositor vm_input
```
Expected: `test result: ok. 3 passed`.

- [ ] **Step 3: Linux open/poll/read 부분 작성**

`vm_input.rs` 끝에 추가:

```rust
#[cfg(target_os = "linux")]
pub use sys::EvdevSet;

#[cfg(target_os = "linux")]
mod sys {
    use super::parse_event;
    use std::fs::{File, OpenOptions};
    use std::io::Read;
    use std::os::fd::AsRawFd;

    /// 열린 모든 /dev/input/event* 모음. poll로 읽을 게 있는 fd만 read.
    pub struct EvdevSet { files: Vec<File> }

    impl EvdevSet {
        /// /dev/input/event* 전부 non-blocking으로 open.
        pub fn open_all() -> Result<EvdevSet, String> {
            let mut files = Vec::new();
            for entry in std::fs::read_dir("/dev/input").map_err(|e| format!("read /dev/input: {}", e))? {
                let path = match entry { Ok(e) => e.path(), Err(_) => continue };
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.starts_with("event") { continue; }
                match OpenOptions::new().read(true).open(&path) {
                    Ok(f) => {
                        // non-blocking
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
            if files.is_empty() { return Err("no /dev/input/event* devices".into()); }
            Ok(EvdevSet { files })
        }

        /// timeout_ms 동안 대기, 준비된 fd에서 input_event를 모두 읽어 콜백.
        pub fn poll_events<F: FnMut(super::RawEvent)>(&mut self, timeout_ms: i32, mut cb: F) {
            let mut pfds: Vec<libc::pollfd> = self.files.iter()
                .map(|f| libc::pollfd { fd: f.as_raw_fd(), events: libc::POLLIN, revents: 0 })
                .collect();
            let n = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, timeout_ms) };
            if n <= 0 { return; }
            for (i, pfd) in pfds.iter().enumerate() {
                if pfd.revents & libc::POLLIN == 0 { continue; }
                let mut buf = [0u8; 24 * 64];
                loop {
                    match self.files[i].read(&mut buf) {
                        Ok(0) => break,
                        Ok(read) => {
                            let mut off = 0;
                            while off + 24 <= read {
                                if let Some(ev) = parse_event(&buf[off..off + 24]) { cb(ev); }
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
```

- [ ] **Step 4: musl 크로스 컴파일 (lib)**

Run:
```powershell
cargo build --target x86_64-unknown-linux-musl -p geulos-compositor --lib
```
Expected: `Finished`.

- [ ] **Step 5: 호스트 빌드 무회귀 + 커밋**

Run:
```powershell
cargo build -p geulos-compositor
cargo test -p geulos-compositor vm_
```
Expected: 빌드 `Finished`, 테스트 `ok. 5 passed` (vm_fb 2 + vm_input 3).

```powershell
git add compositor/src/lib.rs compositor/src/vm_input.rs
git commit -m "feat(compositor): vm_input(evdev) — input_event 파싱 + poll/read (Linux)"
```

---

## Task 7: 증명 실행파일 (geulos-vm-skeleton)

**Files:**
- Modify: `compositor/Cargo.toml` (`[[bin]]` 선언)
- Create: `compositor/src/bin/geulos-vm-skeleton.rs`

화면을 그리고 입력을 받는 standalone 루프. 서버 연결 없음. 호스트(비-Linux)에서는 stub main만 컴파일.

- [ ] **Step 0a: Cargo.toml에 bin 선언**

`compositor/Cargo.toml`에 추가 (기존 `[[bin]] geulos-compositor` 블록 다음):

```toml
[[bin]]
name = "geulos-vm-skeleton"
path = "src/bin/geulos-vm-skeleton.rs"
```

(이 bin 파일을 만드는 본 Task에서 함께 선언한다 — Task 3에서 미리 선언하면 파일 부재로 Task 4~6의 중간 `cargo build`가 깨지기 때문. 본 Task 내에서는 Step 1의 파일 생성과 함께 Step 2 빌드 전에 있으면 된다.)

- [ ] **Step 1: 실행파일 작성**

`compositor/src/bin/geulos-vm-skeleton.rs` 생성:

```rust
//! VM 디스플레이 기초 골격 — /dev/fb0에 사각형 + 클릭 자국 + 키 표시.
//! 화면·입력 배관이 VM 게스트 안에서 실제로 동작함을 증명한다.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("geulos-vm-skeleton은 VM(Linux) 전용입니다. 호스트 개발은 geulos-compositor를 쓰세요.");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    use geulos_compositor::layout::Rect;
    use geulos_compositor::render::fill_rect;
    use geulos_compositor::vm_fb::Framebuffer;
    use geulos_compositor::vm_input::{
        EvdevSet, ABS_X, ABS_Y, BTN_LEFT, EV_ABS, EV_KEY, TABLET_LOGICAL_MAX,
    };
    use geulos_compositor::vm_input::scale_abs;

    println!("[skeleton] starting — opening /dev/fb0");
    let mut fb = match Framebuffer::open() {
        Ok(fb) => fb,
        Err(e) => { eprintln!("[skeleton] framebuffer 실패: {}", e); std::process::exit(2); }
    };
    println!("[skeleton] fb {}x{} {:?}", fb.xres, fb.yres, fb.format());

    let mut input = match EvdevSet::open_all() {
        Ok(i) => i,
        Err(e) => { eprintln!("[skeleton] evdev 실패: {}", e); std::process::exit(3); }
    };

    let (w, h) = (fb.xres, fb.yres);
    let mut canvas = vec![0u32; w * h];

    // 상태
    let mut pointer = (w as i32 / 2, h as i32 / 2);
    let mut markers: Vec<(i32, i32)> = Vec::new();
    let mut key_count: u32 = 0;

    const BG: u32 = 0xFF_1E_1E_1E;       // 어두운 회색
    const TITLE: u32 = 0xFF_2D_8C_FF;    // 파랑 바
    const CENTER: u32 = 0xFF_4A_9E_FF;   // 밝은 파랑 사각형
    const MARKER: u32 = 0xFF_FF_55_55;   // 빨강 클릭 자국

    loop {
        // 입력 처리
        input.poll_events(16, |ev| {
            if ev.type_ == EV_ABS && ev.code == ABS_X {
                pointer.0 = scale_abs(ev.value, TABLET_LOGICAL_MAX, w as u32);
            } else if ev.type_ == EV_ABS && ev.code == ABS_Y {
                pointer.1 = scale_abs(ev.value, TABLET_LOGICAL_MAX, h as u32);
            } else if ev.type_ == EV_KEY && ev.code == BTN_LEFT && ev.value == 1 {
                markers.push(pointer);
                println!("[skeleton] click at ({}, {})", pointer.0, pointer.1);
            } else if ev.type_ == EV_KEY && ev.code != BTN_LEFT && ev.value == 1 {
                key_count = key_count.wrapping_add(1);
                println!("[skeleton] key code={} (count={})", ev.code, key_count);
            }
        });

        // 그리기
        fill_rect(&mut canvas, w, h, &Rect { x: 0, y: 0, w: w as i32, h: h as i32 }, BG);
        fill_rect(&mut canvas, w, h, &Rect { x: 0, y: 0, w: w as i32, h: 40 }, TITLE);
        fill_rect(&mut canvas, w, h,
            &Rect { x: w as i32 / 2 - 120, y: h as i32 / 2 - 60, w: 240, h: 120 }, CENTER);
        // 키 입력 표시기 — 우상단, 키 누를 때마다 색 변화
        let indicator = 0xFF_00_00_00 | ((key_count.wrapping_mul(40) & 0xFF) << 8) | 0x80;
        fill_rect(&mut canvas, w, h, &Rect { x: w as i32 - 60, y: 50, w: 40, h: 40 }, indicator);
        // 클릭 자국
        for &(mx, my) in &markers {
            fill_rect(&mut canvas, w, h, &Rect { x: mx - 5, y: my - 5, w: 10, h: 10 }, MARKER);
        }

        fb.present(&canvas);
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}
```

- [ ] **Step 2: musl 크로스 컴파일 (skeleton bin)**

Run:
```powershell
cargo build --target x86_64-unknown-linux-musl --release -p geulos-compositor --bin geulos-vm-skeleton
```
Expected: `Finished`. 산출물 `target/x86_64-unknown-linux-musl/release/geulos-vm-skeleton`.

- [ ] **Step 3: 호스트 빌드 무회귀 (stub main)**

Run:
```powershell
cargo build -p geulos-compositor --bin geulos-vm-skeleton
```
Expected: `Finished` (stub main 컴파일).

- [ ] **Step 4: 커밋**

```powershell
git add compositor/Cargo.toml compositor/src/bin/geulos-vm-skeleton.rs
git commit -m "feat(compositor): geulos-vm-skeleton — fb0 사각형 + 클릭 자국 + 키 표시 (VM 증명)"
```

---

## Task 8: build.ps1에 skeleton 빌드 + initrd 포함

**Files:**
- Modify: `boot/build.ps1:38-60` (cross-compile 단계 + 바이너리 stage)

- [ ] **Step 1: skeleton 크로스 컴파일 추가**

`boot/build.ps1`의 Step 1 `cargo build` 블록 바로 뒤(`Pop-Location` 전, 같은 `try` 안)에 추가:

```powershell
    # VM 디스플레이 골격 bin (compositor 크레이트, --bin 지정 — winit bin은 빌드 안 됨)
    if ($Release) {
        & cargo build --target x86_64-unknown-linux-musl --release `
            -p geulos-compositor --bin geulos-vm-skeleton
    } else {
        & cargo build --target x86_64-unknown-linux-musl `
            -p geulos-compositor --bin geulos-vm-skeleton
    }
    if ($LASTEXITCODE -ne 0) { throw "geulos-vm-skeleton cross-compile failed" }
```

- [ ] **Step 2: 바이너리 경로 + 존재 검증 추가**

`$EchoBin` 정의 다음 줄에 추가, 그리고 검증 루프에 포함:

```powershell
$SkeletonBin = Join-Path $BinDir "geulos-vm-skeleton"

foreach ($b in @($InitBin, $ServerBin, $EchoBin, $SkeletonBin)) {
    if (-not (Test-Path $b)) { throw "missing binary: $b" }
}
Write-Host "  built: geulos-init, geulosd, geulos-echo-app, geulos-vm-skeleton"
```

(기존 `foreach ($b in @($InitBin, $ServerBin, $EchoBin))` 와 그 다음 `Write-Host`를 위 내용으로 교체.)

- [ ] **Step 3: initrd stage에 복사 추가**

`Copy-Item $EchoBin (Join-Path $StageDir "bin/geulos-echo-app")` 다음 줄에 추가:

```powershell
Copy-Item $SkeletonBin (Join-Path $StageDir "bin/geulos-vm-skeleton")
```

- [ ] **Step 4: 전체 빌드 실행**

Run:
```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;C:\Program Files\qemu;$env:PATH"
pwsh boot/build.ps1 -Release
```
Expected: `built: geulos-init, geulosd, geulos-echo-app, geulos-vm-skeleton` + initrd 조립 성공 + 모듈 13개 복사 로그 (`module: 6.12.89-0-lts/virtio-gpu.ko ...` 포함).

- [ ] **Step 5: 커밋**

```powershell
git add boot/build.ps1
git commit -m "build(boot): geulos-vm-skeleton 크로스 컴파일 + initrd 포함"
```

---

## Task 9: init이 skeleton spawn

**Files:**
- Modify: `geulos-init/src/spawn.rs`
- Modify: `geulos-init/src/main.rs:58-62` (PID 로그)

증명 단계에서 skeleton은 standalone. geulosd/echo-app spawn은 유지하되, skeleton을 추가로 띄운다.

- [ ] **Step 1: SpawnedProcesses에 skeleton 필드 추가 + spawn**

`geulos-init/src/spawn.rs` 전체 교체:

```rust
//! server-host + echo-app + vm-skeleton 자식 프로세스 spawn.

use std::process::{Child, Command};

pub struct SpawnedProcesses {
    pub server: Child,
    pub echo_app: Option<Child>,
    pub skeleton: Option<Child>,
}

pub fn spawn_all() -> Result<SpawnedProcesses, String> {
    println!("[init] spawning /bin/geulosd ...");
    let server = Command::new("/bin/geulosd")
        .arg("0.0.0.0:5550")
        .spawn()
        .map_err(|e| format!("spawn geulosd: {}", e))?;
    println!("[init] geulosd PID = {}", server.id());

    std::thread::sleep(std::time::Duration::from_secs(1));

    println!("[init] spawning /bin/geulos-echo-app ...");
    let echo_app = match Command::new("/bin/geulos-echo-app").arg("127.0.0.1:5550").spawn() {
        Ok(child) => { println!("[init] echo-app PID = {}", child.id()); Some(child) }
        Err(e) => { eprintln!("[init] echo-app spawn failed: {} (continuing)", e); None }
    };

    println!("[init] spawning /bin/geulos-vm-skeleton ...");
    let skeleton = match Command::new("/bin/geulos-vm-skeleton").spawn() {
        Ok(child) => { println!("[init] vm-skeleton PID = {}", child.id()); Some(child) }
        Err(e) => { eprintln!("[init] vm-skeleton spawn failed: {} (continuing)", e); None }
    };

    Ok(SpawnedProcesses { server, echo_app, skeleton })
}
```

- [ ] **Step 2: main.rs PID 로그에 skeleton 반영**

`geulos-init/src/main.rs:58-62` 교체:

```rust
    let server_pid = processes.server.id();
    let echo_pid = processes.echo_app.as_ref().map(|c| c.id());
    let skeleton_pid = processes.skeleton.as_ref().map(|c| c.id());

    println!();
    println!("[init] entering main loop (server PID {}, echo PID {:?}, skeleton PID {:?})",
        server_pid, echo_pid, skeleton_pid);
```

- [ ] **Step 3: musl 빌드 검증**

Run:
```powershell
cargo build --target x86_64-unknown-linux-musl -p geulos-init
```
Expected: `Finished`.

- [ ] **Step 4: 커밋**

```powershell
git add geulos-init/src/spawn.rs geulos-init/src/main.rs
git commit -m "feat(init): geulos-vm-skeleton spawn 추가"
```

---

## Task 10: QEMU 그래픽 부팅 옵션

**Files:**
- Modify: `boot/qemu/launch.ps1`

기존 텍스트 전용 부팅은 유지하고, `-Graphics` 스위치로 그래픽 창 + virtio 디바이스 부팅 추가.

- [ ] **Step 1: param에 -Graphics 추가**

`boot/qemu/launch.ps1`의 `param` 블록에 추가:

```powershell
    [switch]$Graphics,  # virtio-gpu 그래픽 창 + virtio 입력 (VM 디스플레이 골격용)
```

- [ ] **Step 2: QEMU 인자 분기**

`$QemuArgs = @(...)` 정의를 교체:

```powershell
$QemuArgs = @(
    "-kernel", $KernelPath,
    "-initrd", $InitrdPath,
    "-m", "${Memory}M"
) + $AccelArgs

if ($Graphics) {
    # 그래픽 창 + 직렬은 별도 파일로 (창과 로그 동시 확인)
    $SerialLog = Join-Path $WorkspaceRoot "boot/serial.log"
    $QemuArgs += @(
        "-append", "console=ttyS0",
        "-serial", "file:$SerialLog",
        "-device", "virtio-gpu-pci",
        "-device", "virtio-keyboard-pci",
        "-device", "virtio-tablet-pci",
        "-netdev", "user,id=net0,hostfwd=tcp::${ForwardPort}-:5550",
        "-device", "e1000,netdev=net0"
    )
    Write-Host "graphics:  virtio-gpu 창 + 직렬 로그 → $SerialLog"
} else {
    $QemuArgs += @(
        "-nographic",
        "-append", "console=ttyS0",
        "-netdev", "user,id=net0,hostfwd=tcp::${ForwardPort}-:5550",
        "-device", "e1000,netdev=net0"
    )
}
```

(기존 `$QemuArgs = @(...)` 한 블록을 위 분기로 통째 대체. `& qemu-system-x86_64 @QemuArgs`는 유지.)

- [ ] **Step 3: 스크립트 파싱 검증 (부팅 없이)**

Run:
```powershell
powershell -NoProfile -Command "& { . { param([switch]$Graphics) } }"  # placeholder
Get-Command -Syntax (Resolve-Path boot/qemu/launch.ps1)
```
Expected: 구문 오류 없이 param에 `-Graphics`가 보임. (실제 부팅은 Task 11.)

- [ ] **Step 4: 커밋**

```powershell
git add boot/qemu/launch.ps1
git commit -m "build(boot): launch.ps1 -Graphics — virtio-gpu 창 + virtio 입력 부팅"
```

---

## Task 11: 통합 — VM 부팅 + 시각 확인

**Files:** 없음 (실행/관찰)

- [ ] **Step 1: 그래픽 모드로 부팅**

Run (별도 PowerShell 창에서 사용자가 직접 — 그래픽 창을 봐야 함):
```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;C:\Program Files\qemu;$env:PATH"
pwsh boot/qemu/launch.ps1 -Graphics
```
Expected: **QEMU 그래픽 창**이 뜨고, 잠시 후 어두운 배경 + 상단 파란 바 + 가운데 밝은 파란 사각형이 보인다.

- [ ] **Step 2: 마우스 클릭 확인**

창 안 임의 지점을 클릭.
Expected: 클릭한 자리에 **빨간 작은 사각형(자국)**이 찍힌다. 여러 번 클릭하면 자국이 누적된다.

- [ ] **Step 3: 키 입력 확인**

아무 키나 누른다.
Expected: 우상단 표시기 사각형의 **색이 변한다** (키 누를 때마다).

- [ ] **Step 4: 직렬 로그 확인**

Run (다른 창):
```powershell
Get-Content boot/serial.log -Tail 40
```
Expected 핵심 라인:
```
[init]   loaded virtio-gpu.ko
[init]   loaded virtio_input.ko
[init]   loaded evdev.ko
[init] spawning /bin/geulos-vm-skeleton ...
[skeleton] fb 1024x768 PixelFormat { ... }
[vm_input] opened /dev/input/event0
[skeleton] click at (x, y)
[skeleton] key code=...
```

- [ ] **Step 5: 합격 판정 + 종료**

사용자가 (1) 사각형이 보이고 (2) 클릭 자국이 남고 (3) 키로 표시기 색이 바뀌는 것을 확인하면 **합격**. QEMU 창을 닫거나:
```powershell
Get-Process qemu-system-x86_64 -ErrorAction SilentlyContinue | Stop-Process -Force
```

- [ ] **Step 6: 합격 시 — known-issues / 메모리 갱신은 controller가 별도 수행**

증명 성공 시: 다음 단계(진짜 컴포지터 이식) 스펙 작성으로 진행. 실패 시: 직렬 로그의 모듈 적재/fb open 에러를 systematic-debugging으로 추적 (흔한 실패: 모듈 의존 누락 → modules.dep 재귀 확인 / fb 픽셀 형식 불일치 → format() 로그 확인 후 argb_to_fb_pixel 분기).

---

## Self-Review

**Spec coverage:**
- 모듈 세트 확장 → Task 1 ✓
- init 적재 → Task 2 ✓
- QEMU 변경(virtio 디바이스 + 그래픽) → Task 10 ✓
- Linux 배관(framebuffer/evdev) → Task 5, 6 ✓
- 증명 실행파일(사각형+클릭+키) → Task 7 ✓
- 성공 기준(창+사각형+클릭 자국+키+직렬 로그) → Task 11 ✓
- 호스트 경로 유지(non-linux 분리) → Task 3, 10 ✓
- musl 빌드 검증(메모리 교훈) → Task 2/5/6/7/9 각 빌드 Step ✓

**알려진 단순화 (의도적, 비-목표와 일치):**
- 절대좌표 logical max를 상수(32767)로 가정 — EVIOCGABS 읽기는 v2.
- 32bpp만 지원 — 그 외 형식은 로그 후 skip.
- 텍스트("GeulOS VM") 렌더는 생략 — 키 표시기 사각형 색 변화로 대체(폰트 결합 회피). 스펙의 "(가능하면)" 조건과 일치.

**Type consistency:**
- `Rect { x, y, w, h }` (layout.rs, i32) — Task 7에서 동일 사용 ✓
- `fill_rect(buffer, w, h, &Rect, color)` (Task 4 pub화) — Task 7 호출 시그니처 일치 ✓
- `Framebuffer::open/present/format/xres/yres` (Task 5) — Task 7 사용 일치 ✓
- `EvdevSet::open_all/poll_events`, `RawEvent { type_, code, value }`, `parse_event`, `scale_abs`, 상수 EV_*/ABS_*/BTN_LEFT (Task 6) — Task 7 사용 일치 ✓
