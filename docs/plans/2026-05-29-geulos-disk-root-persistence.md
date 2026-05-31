> **Status:** completed (2026-05-29)
> **Note:** 디스크 루트 영속화 정식 마감 — Stage 1 geulos-bootstrap (initramfs) + switch_root → Stage 2 geulos-init (disk root). /root 영속.

# 디스크 루트 영속화 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** VM의 런타임 루트(`/`)를 휘발성 initrd 램디스크에서 영속 virtio-blk 디스크로 옮긴다 (`switch_root`). 재부팅·재빌드 후에도 `/root` 사용자 파일이 유지된다.

**Architecture:** 부팅을 2단계로 분리한다. Stage 1(`geulos-bootstrap`, initramfs `/init`)이 virtio_blk+ext4 모듈을 적재하고, `/dev/vda`가 비었으면 포맷하고, 디스크를 마운트하고, initramfs의 `/payload/*` 시스템 파일을 디스크로 동기화(`/root`·`/home` 보존)한 뒤 `switch_root`로 디스크 루트로 넘어간다. Stage 2(`geulos-init`, 디스크 `/sbin/init`)는 기존 init 로직(모듈·네트워크·spawn)을 거의 그대로 수행한다.

**Tech Stack:** Rust (edition 2021, `x86_64-unknown-linux-musl` via `cargo zigbuild`), `nix` 0.29 (mount/fs/process), Alpine LTS 커널 + apk 추출 모듈, busybox-static(mke2fs), QEMU virtio-blk, PowerShell 빌드/부팅 스크립트.

---

## 사전 환경 (모든 부팅/빌드 단계 공통)

새 셸마다 PATH 세팅 (zig + cargo + qemu):

```powershell
$env:PATH = [System.Environment]::GetEnvironmentVariable("Path","Machine") + ";" + `
            [System.Environment]::GetEnvironmentVariable("Path","User") + ";" + `
            "$env:USERPROFILE\.cargo\bin;C:\Program Files\qemu"
```

**핵심 주의:** 이미지 재빌드 전 반드시 실행 중인 QEMU 종료(initrd/디스크 파일 잠금 → 빌드 실패):

```powershell
Get-Process qemu-system-x86_64 -ErrorAction SilentlyContinue | Stop-Process -Force
```

**빌드/부팅/로그 기본 명령:**

```powershell
& .\boot\build.ps1 -Release                 # 크로스컴파일 + initrd + 디스크 이미지
& .\boot\qemu\launch.ps1 -Graphics          # 그래픽 부팅 (직렬로그 → boot/serial.log)
Get-Content boot/serial.log -Tail 60        # 부팅 로그 확인
```

**검증 철학(중요):** VM/부팅 코드는 호스트 단위테스트로 검증 불가하다. 그래서 이 계획은 두 종류의 검증을 쓴다.
1. **순수 로직(슈퍼블록 매직 판정, 동기화 보존 규칙, /proc/mounts 파싱)** → 호스트 `cargo test`로 진짜 TDD.
2. **통합(모듈 적재·포맷·마운트·switch_root·부팅)** → `cargo zigbuild` 컴파일 통과 + VM 부팅 + `boot/serial.log`의 정확한 기대 로그 라인 확인 + (필요 시) 사용자 시각 확인.

Linux 전용 코드는 Windows 로컬 빌드에서 cfg로 스킵되므로, **push/완료 전 반드시 musl 타겟으로 실제 컴파일**해야 한다.

---

## 파일 구조 (생성/수정 맵)

**신규 크레이트 `crates/geulos-bootstrap/`** (Stage 1):
| 파일 | 책임 |
|---|---|
| `Cargo.toml` | 크레이트 정의, nix(linux) 의존 |
| `src/superblock.rs` | ext 슈퍼블록 매직 판정 (**순수, 호스트 테스트**) |
| `src/syncplan.rs` | 동기화 시 보존/복사 경로 분류 (**순수, 호스트 테스트**) |
| `src/modload.rs` | `finit_module` + 지정 모듈 적재 (cfg linux) |
| `src/disk.rs` | `/dev/vda` 대기·probe·포맷·마운트 (cfg linux) |
| `src/sync.rs` | `/payload`→디스크 복사 실행 (cfg linux, M1) |
| `src/switchroot.rs` | switch_root 시퀀스 (cfg linux) |
| `src/main.rs` | Stage 1 오케스트레이션 + 폴백 + stage2-stub 분기 |

**수정:**
| 파일 | 변경 |
|---|---|
| `Cargo.toml` (워크스페이스 루트) | members에 `crates/geulos-bootstrap` 추가 |
| `geulos-init/src/mount.rs` | `is_mounted` 가드 (idempotent) — **순수 파서 호스트 테스트** |
| `geulos-init/src/modules.rs` | LOAD_ORDER에 `virtio_blk`·ext4 의존 추가 |
| `boot/modules/fetch.ps1` | 모듈 목록에 디스크/FS 모듈 추가 |
| `boot/tools/fetch-busybox.ps1` (신규) | busybox-static apk 추출 |
| `boot/build.ps1` | bootstrap 컴파일·`/payload` 스테이징·busybox 포함·디스크 이미지 생성 |
| `boot/qemu/launch.ps1` | virtio-blk drive 추가 (양 분기) |

---

## M0 — 포맷 스파이크 (최대 리스크 먼저 못박기)

목표: "포맷 → 마운트 → switch_root → 디스크 루트에서 stub init이 한 줄 찍기"를 직렬 로그로 증명. busybox `mke2fs`가 VM에서 동작하는지 결정.

### Task 1: `geulos-bootstrap` 크레이트 골격 + 순수 모듈

**Files:**
- Create: `crates/geulos-bootstrap/Cargo.toml`
- Create: `crates/geulos-bootstrap/src/main.rs`
- Create: `crates/geulos-bootstrap/src/superblock.rs`
- Create: `crates/geulos-bootstrap/src/syncplan.rs`
- Modify: `Cargo.toml` (워크스페이스 루트 members)

- [ ] **Step 1: 워크스페이스 멤버 추가**

`Cargo.toml`의 `members` 배열에 한 줄 추가 (`crates/geulos-launcher` 다음):

```toml
    "crates/geulos-launcher",
    "crates/geulos-bootstrap",
]
```

- [ ] **Step 2: 크레이트 Cargo.toml 작성**

Create `crates/geulos-bootstrap/Cargo.toml`:

```toml
[package]
name = "geulos-bootstrap"
version = "0.0.1"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "GeulOS stage-1 부트스트랩 — virtio-blk 디스크 포맷/마운트/동기화 후 switch_root (PID 1)"

[[bin]]
name = "geulos-bootstrap"
path = "src/main.rs"

# 순수 모듈(superblock, syncplan)은 모든 타겟에서 컴파일/테스트된다.
# 시스템콜 모듈(disk, modload, switchroot)은 main.rs에서 cfg(linux)로만 포함.
[target.'cfg(target_os = "linux")'.dependencies]
nix = { workspace = true, features = ["mount", "fs", "process"] }
```

- [ ] **Step 3: superblock.rs 실패 테스트 작성 (순수, TDD)**

Create `crates/geulos-bootstrap/src/superblock.rs`:

```rust
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
```

- [ ] **Step 4: 테스트 실패 확인**

main.rs가 아직 모듈을 선언하지 않아 컴파일 단위에 안 들어가므로, 먼저 Step 6의 main.rs를 만든 뒤 테스트한다. (이 Step에서는 코드만 작성.)

- [ ] **Step 5: syncplan.rs 작성 (순수, TDD)**

Create `crates/geulos-bootstrap/src/syncplan.rs`:

```rust
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
```

- [ ] **Step 6: main.rs 골격 (순수 모듈 선언 + 비-linux fallback)**

Create `crates/geulos-bootstrap/src/main.rs`:

```rust
//! GeulOS stage-1 부트스트랩 (initramfs /init = PID 1).
//!
//! 책임: virtio_blk+ext4 모듈 적재 → /dev/vda 포맷(빈 경우) → 마운트 →
//! 시스템 파일 동기화(M1) → switch_root로 디스크 루트 진입.
//! 모든 실패는 램디스크 폴백으로 degrade — PID 1은 절대 그냥 종료하지 않는다.

// 순수 모듈 — 모든 타겟에서 컴파일/테스트.
mod superblock;
mod syncplan;

#[cfg(target_os = "linux")]
fn main() {
    // M0/M1에서 구현 채움.
    println!("[bootstrap] geulos-bootstrap stage 1 (PID {})", std::process::id());
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("geulos-bootstrap only runs on Linux (initramfs PID 1 role).");
    eprintln!("Cross-compile: cargo zigbuild --target x86_64-unknown-linux-musl -p geulos-bootstrap");
    std::process::exit(1);
}
```

- [ ] **Step 7: 순수 테스트 실행 — 통과 확인**

Run: `cargo test -p geulos-bootstrap`
Expected: `superblock::tests`(4) + `syncplan::tests`(3) 전부 PASS. (호스트=Windows에서 실행됨, cfg(linux) 모듈 미포함이라 컴파일 OK.)

- [ ] **Step 8: 커밋**

```powershell
git add Cargo.toml crates/geulos-bootstrap
git commit -m "feat(bootstrap): geulos-bootstrap 크레이트 골격 + 슈퍼블록/동기화 순수 로직 (M0)"
```

---

### Task 2: busybox-static 추출 스크립트

**Files:**
- Create: `boot/tools/fetch-busybox.ps1`

- [ ] **Step 1: 추출 스크립트 작성**

Create `boot/tools/fetch-busybox.ps1`:

```powershell
# boot/tools/fetch-busybox.ps1 — Alpine busybox-static 추출
#
# Stage 1이 빈 디스크를 포맷할 때 쓰는 mke2fs를 제공한다. busybox-static은
# 완전 정적 바이너리(musl 로더 불필요)라 우리 정적 musl initramfs에 그대로 넣을 수 있다.
# 추출 결과: boot/tools/busybox  (build.ps1이 initrd /bin/busybox 로 복사)

param(
    [string]$AlpineVersion = "v3.21",
    [string]$PkgVersion = "",        # 빈 경우 자동 탐색
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$ToolsDir   = $PSScriptRoot
$CacheDir   = Join-Path $ToolsDir ".cache"
$OutPath    = Join-Path $ToolsDir "busybox"
$null = New-Item -ItemType Directory -Force -Path $CacheDir

if ((Test-Path $OutPath) -and -not $Force) {
    Write-Host "[busybox] using cached $OutPath ($((Get-Item $OutPath).Length) bytes)"
    return
}

if (-not $PkgVersion) {
    $pageUrl = "https://pkgs.alpinelinux.org/package/$AlpineVersion/main/x86_64/busybox-static"
    try {
        $page = Invoke-WebRequest -Uri $pageUrl -UseBasicParsing
        if ($page.Content -match 'busybox-static-([0-9][^<"]*?-r[0-9]+)') {
            $PkgVersion = $Matches[1]
        } elseif ($page.Content -match 'Version[^<]*<[^>]+>\s*([0-9][^<]*-r[0-9]+)') {
            $PkgVersion = $Matches[1]
        } else { throw "버전 파싱 실패: $pageUrl" }
        Write-Host "[busybox] detected version: $PkgVersion"
    } catch {
        throw "busybox-static 버전 탐색 실패: $_  (직접 -PkgVersion 지정)"
    }
}

$ApkUrl  = "https://dl-cdn.alpinelinux.org/alpine/$AlpineVersion/main/x86_64/busybox-static-$PkgVersion.apk"
$ApkPath = Join-Path $CacheDir "busybox-static-$PkgVersion.apk"
if (-not (Test-Path $ApkPath) -or $Force) {
    Write-Host "[busybox] downloading $ApkUrl ..."
    Invoke-WebRequest -Uri $ApkUrl -OutFile $ApkPath
}

$ExtractDir = Join-Path $CacheDir "extract-busybox-$PkgVersion"
if (Test-Path $ExtractDir) { Remove-Item -Recurse -Force $ExtractDir }
$null = New-Item -ItemType Directory -Force -Path $ExtractDir

# apk = concatenated tar.gz → --ignore-zeros 필수 (fetch.ps1과 동일)
& tar --ignore-zeros -xzf $ApkPath -C $ExtractDir 2>&1 | Out-Null

# busybox-static는 /bin/busybox.static 로 설치된다
$bbCandidate = Get-ChildItem -Path $ExtractDir -Recurse -ErrorAction SilentlyContinue |
               Where-Object { $_.Name -eq "busybox.static" -or $_.Name -eq "busybox" } |
               Select-Object -First 1
if (-not $bbCandidate) { throw "busybox 정적 바이너리를 apk에서 못 찾음 (트리: $ExtractDir)" }

Copy-Item $bbCandidate.FullName $OutPath
Write-Host "[busybox] extracted -> $OutPath ($((Get-Item $OutPath).Length) bytes)"
```

- [ ] **Step 2: 스크립트 실행 — busybox 확보**

Run: `& .\boot\tools\fetch-busybox.ps1`
Expected: `boot/tools/busybox` 파일 생성 (수백 KB~1MB대). 실패 시 `-PkgVersion <ver>` 수동 지정.

- [ ] **Step 3: 커밋** (바이너리는 .gitignore 권장 — 트래킹 제외)

```powershell
Add-Content -Path .gitignore -Value "boot/tools/busybox`nboot/tools/.cache/"
git add boot/tools/fetch-busybox.ps1 .gitignore
git commit -m "build(boot): busybox-static 추출 스크립트 (stage1 mke2fs용)"
```

---

### Task 3: 디스크/FS 커널 모듈 추출 추가

**Files:**
- Modify: `boot/modules/fetch.ps1:15-23`

- [ ] **Step 1: 모듈 목록에 디스크/FS 모듈 추가**

`boot/modules/fetch.ps1`의 `$ModuleNames` 기본값에 추가 (기존 항목 유지, 끝에 추가):

```powershell
    [string[]]$ModuleNames = @(
        "e1000",
        "virtio", "virtio_ring",
        "virtio_pci", "virtio_pci_modern_dev", "virtio_pci_legacy_dev",
        "virtio_dma_buf",
        "drm", "drm_kms_helper", "drm_shmem_helper",
        "virtio-gpu",
        "virtio_input", "evdev",
        "virtio_blk",
        "ext4", "jbd2", "mbcache", "crc16"
    ),
```

- [ ] **Step 2: fetch 재실행 — 새 모듈 확보**

Run: `& .\boot\modules\fetch.ps1`
Expected: `virtio_blk.ko` 추출. `ext4`/`jbd2`/`mbcache`/`crc16` 중 일부는 커널 built-in이면 `module 'X' not found in apk` 경고가 날 수 있음 — **정상**(아래 부팅에서 실제 필요 여부 판정). `.ko.zst` 경고가 나오면 위험항목(스펙 §위험) — 해당 모듈만 수동 처리 필요.

- [ ] **Step 3: 추출 결과 확인**

Run: `Get-ChildItem boot/modules/*/*.ko | Select-Object Name`
Expected: 최소 `virtio_blk.ko` 존재. (ext4 계열은 환경에 따라.)

- [ ] **Step 4: 커밋** (.ko는 트래킹 정책에 따름 — 기존에 트래킹 중이면 add)

```powershell
git add boot/modules/fetch.ps1
git commit -m "build(boot): virtio_blk + ext4 계열 커널 모듈 추출 추가"
```

---

### Task 4: Stage 1 시스템콜 모듈 (modload / disk / switchroot)

**Files:**
- Create: `crates/geulos-bootstrap/src/modload.rs`
- Create: `crates/geulos-bootstrap/src/disk.rs`
- Create: `crates/geulos-bootstrap/src/switchroot.rs`

> 이 모듈들은 시스템콜을 쓰므로 호스트 단위테스트 불가. 검증은 Task 5의 부팅으로. 단, `superblock`/`syncplan` 순수 로직을 호출하는 부분은 그 테스트가 간접 보증.

- [ ] **Step 1: modload.rs 작성**

Create `crates/geulos-bootstrap/src/modload.rs`:

```rust
//! Stage 1 전용 모듈 적재 — 디스크 접근에 필요한 모듈만 의존 순서로 적재.
//! (geulos-init/modules.rs는 "전체 적재" 정책이라 별개. 여기선 최소 부분집합만.)

use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use nix::libc;

/// `/lib/modules` 아래 첫 커널 버전 디렉터리.
pub fn find_kernel_dir() -> Option<PathBuf> {
    std::fs::read_dir("/lib/modules")
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir())
}

/// `finit_module(fd, "", 0)`. 이미 적재(EEXIST)면 성공 취급. 파일 없으면 Ok(스킵).
fn finit_module(path: &Path) -> Result<(), String> {
    if !path.exists() {
        println!("[bootstrap]   (skip, absent) {}", path.display());
        return Ok(());
    }
    let file = std::fs::File::open(path).map_err(|e| format!("open {}: {}", path.display(), e))?;
    let res = unsafe {
        libc::syscall(libc::SYS_finit_module, file.as_raw_fd() as libc::c_int, c"".as_ptr(), 0 as libc::c_int)
    };
    if res < 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EEXIST) {
            return Ok(());
        }
        return Err(format!("finit_module {}: {}", path.display(), err));
    }
    Ok(())
}

/// 지정 이름 목록을 순서대로 적재. 디스크 접근에 필요한 최소 세트.
/// virtio 전송 → virtio_blk → ext4 의존 → ext4.
pub fn load_disk_stack(kernel_dir: &Path) {
    const ORDER: &[&str] = &[
        "virtio.ko",
        "virtio_ring.ko",
        "virtio_pci_legacy_dev.ko",
        "virtio_pci_modern_dev.ko",
        "virtio_pci.ko",
        "virtio_blk.ko",
        "crc16.ko",
        "mbcache.ko",
        "jbd2.ko",
        "ext4.ko",
    ];
    for name in ORDER {
        let p = kernel_dir.join(name);
        match finit_module(&p) {
            Ok(()) => println!("[bootstrap]   loaded {}", name),
            Err(e) => eprintln!("[bootstrap]   {} load failed: {}", name, e),
        }
    }
}
```

- [ ] **Step 2: disk.rs 작성**

Create `crates/geulos-bootstrap/src/disk.rs`:

```rust
//! /dev/vda 대기 · 포맷 여부 probe · 포맷(busybox mke2fs) · 마운트.

use std::io::Read;
use std::path::Path;

use nix::mount::{mount, MsFlags};

use crate::superblock;

pub const DISK_DEV: &str = "/dev/vda";
pub const NEWROOT: &str = "/newroot";

/// /dev/vda 등장 대기 (virtio-blk PCI enum 지연 대비, 최대 ~3초).
pub fn wait_for_disk() -> bool {
    for attempt in 0..30 {
        if Path::new(DISK_DEV).exists() {
            if attempt > 0 {
                println!("[bootstrap] {} appeared (attempt {})", DISK_DEV, attempt);
            }
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    eprintln!("[bootstrap] {} did not appear", DISK_DEV);
    false
}

/// 디바이스 앞부분을 읽어 ext 매직이 있으면 true(=이미 포맷).
pub fn is_formatted() -> bool {
    let mut buf = vec![0u8; superblock::EXT_MAGIC_OFFSET + 2];
    match std::fs::File::open(DISK_DEV) {
        Ok(mut f) => match f.read_exact(&mut buf) {
            Ok(()) => superblock::has_ext_magic(&buf),
            Err(e) => {
                eprintln!("[bootstrap] read {} failed: {} — treat as blank", DISK_DEV, e);
                false
            }
        },
        Err(e) => {
            eprintln!("[bootstrap] open {} failed: {} — treat as blank", DISK_DEV, e);
            false
        }
    }
}

/// busybox mke2fs로 ext 파일시스템 생성. (busybox는 ext2를 만들지만 ext4 드라이버가 마운트함.)
pub fn format() -> Result<(), String> {
    println!("[bootstrap] formatting {} (busybox mke2fs) ...", DISK_DEV);
    let status = std::process::Command::new("/bin/busybox")
        .args(["mke2fs", "-F", DISK_DEV])
        .status()
        .map_err(|e| format!("spawn busybox mke2fs: {}", e))?;
    if !status.success() {
        return Err(format!("mke2fs exit: {:?}", status.code()));
    }
    println!("[bootstrap] format done");
    Ok(())
}

/// /dev/vda를 ext4 드라이버로 /newroot에 마운트.
pub fn mount_disk() -> Result<(), String> {
    std::fs::create_dir_all(NEWROOT).map_err(|e| format!("mkdir {}: {}", NEWROOT, e))?;
    mount(Some(DISK_DEV), NEWROOT, Some("ext4"), MsFlags::empty(), None::<&str>)
        .map_err(|e| format!("mount {} -> {}: {}", DISK_DEV, NEWROOT, e))?;
    println!("[bootstrap] mounted {} on {}", DISK_DEV, NEWROOT);
    Ok(())
}
```

- [ ] **Step 3: switchroot.rs 작성**

Create `crates/geulos-bootstrap/src/switchroot.rs`:

```rust
//! switch_root — initramfs(rootfs)에서 디스크 루트로 전환.
//! rootfs는 pivot_root 불가 → util-linux switch_root 알고리즘 사용.

use std::ffi::CString;

use nix::mount::{mount, MsFlags};
use nix::unistd::{chdir, chroot, execv};

use crate::disk::NEWROOT;

/// /proc, /sys, /dev를 newroot 하위로 mount --move.
pub fn move_virtual_filesystems() {
    for vfs in ["proc", "sys", "dev"] {
        let from = format!("/{}", vfs);
        let to = format!("{}/{}", NEWROOT, vfs);
        if let Err(e) = std::fs::create_dir_all(&to) {
            eprintln!("[bootstrap]   mkdir {}: {}", to, e);
            continue;
        }
        match mount(Some(from.as_str()), to.as_str(), None::<&str>, MsFlags::MS_MOVE, None::<&str>) {
            Ok(()) => println!("[bootstrap]   moved {} -> {}", from, to),
            Err(e) => eprintln!("[bootstrap]   move {} failed: {}", from, e),
        }
    }
}

/// newroot를 /로 만들고 `/sbin/init`을 PID 1으로 exec. 성공 시 반환하지 않음.
/// 실패 시 Err 반환 → 호출자가 폴백.
pub fn switch_root_to_disk(init_arg: &str) -> Result<(), String> {
    chdir(NEWROOT).map_err(|e| format!("chdir {}: {}", NEWROOT, e))?;
    mount(Some(NEWROOT), "/", None::<&str>, MsFlags::MS_MOVE, None::<&str>)
        .map_err(|e| format!("mount --move {} /: {}", NEWROOT, e))?;
    chroot(".").map_err(|e| format!("chroot .: {}", e))?;
    chdir("/").map_err(|e| format!("chdir /: {}", e))?;

    let init = CString::new("/sbin/init").unwrap();
    let arg = CString::new(init_arg).unwrap();
    // execv는 성공 시 반환하지 않음.
    execv(&init, &[init.clone(), arg]).map_err(|e| format!("execv /sbin/init: {}", e))?;
    Ok(())
}
```

- [ ] **Step 4: 크로스컴파일 확인 (아직 main이 미사용 → dead_code 경고 허용)**

Run: `cargo zigbuild --target x86_64-unknown-linux-musl -p geulos-bootstrap`
Expected: 컴파일 성공(경고 가능). 실패 시 nix feature/시그니처 수정.

- [ ] **Step 5: 커밋**

```powershell
git add crates/geulos-bootstrap/src/modload.rs crates/geulos-bootstrap/src/disk.rs crates/geulos-bootstrap/src/switchroot.rs
git commit -m "feat(bootstrap): 모듈적재/디스크/switch_root 시스템콜 모듈 (M0)"
```

---

### Task 5: M0 스파이크 — 오케스트레이션 + 빌드/부팅 배선 + 증명

**Files:**
- Modify: `crates/geulos-bootstrap/src/main.rs`
- Modify: `boot/build.ps1`
- Modify: `boot/qemu/launch.ps1`

- [ ] **Step 1: main.rs 오케스트레이션 (M0: 동기화 없이, stub stage2)**

Replace `crates/geulos-bootstrap/src/main.rs` linux `main()` + 모듈 선언:

```rust
//! GeulOS stage-1 부트스트랩 (initramfs /init = PID 1).

mod superblock;
mod syncplan;

#[cfg(target_os = "linux")]
mod modload;
#[cfg(target_os = "linux")]
mod disk;
#[cfg(target_os = "linux")]
mod switchroot;

#[cfg(target_os = "linux")]
mod mountvfs {
    use nix::mount::{mount, MsFlags};
    /// /proc·/sys·/dev 마운트 (stage 1 진입 직후).
    pub fn mount_essentials() {
        let m: &[(&str, &str, &str)] = &[
            ("proc", "/proc", "proc"),
            ("sysfs", "/sys", "sysfs"),
            ("devtmpfs", "/dev", "devtmpfs"),
        ];
        for (src, tgt, fstype) in m {
            let _ = std::fs::create_dir_all(tgt);
            match mount(Some(*src), *tgt, Some(*fstype), MsFlags::empty(), None::<&str>) {
                Ok(()) => println!("[bootstrap] mounted {} on {}", src, tgt),
                Err(e) => eprintln!("[bootstrap] mount {} failed: {}", tgt, e),
            }
        }
    }
}

/// 디스크 단계 실패 시 폴백: initramfs의 stage2(/payload/sbin/init)를 직접 exec.
/// (= 현재의 비영속 램디스크 동작.) PID 1 유지를 위해 exec.
#[cfg(target_os = "linux")]
fn fallback_ramdisk() -> ! {
    use std::ffi::CString;
    use nix::unistd::execv;
    eprintln!("[bootstrap] FALLBACK → ramdisk boot (/payload/sbin/init, 비영속)");
    let init = CString::new("/payload/sbin/init").unwrap();
    if let Err(e) = execv(&init, &[init.clone()]) {
        eprintln!("[bootstrap] fallback execv failed: {} — PID 1 sleep loop", e);
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

#[cfg(target_os = "linux")]
fn main() {
    // M0 stub stage-2: switch_root가 `/sbin/init stage2-stub`로 재진입하면 여기로.
    if std::env::args().nth(1).as_deref() == Some("stage2-stub") {
        println!("[stage2-stub] ===== GEULOS STAGE2 REACHED ON DISK ROOT =====");
        println!("[stage2-stub] cwd={:?} /sbin/init exists={}",
            std::env::current_dir(), std::path::Path::new("/sbin/init").exists());
        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    }

    println!();
    println!("=== GeulOS bootstrap (stage 1, PID {}) ===", std::process::id());

    mountvfs::mount_essentials();

    let kernel_dir = match modload::find_kernel_dir() {
        Some(d) => {
            println!("[bootstrap] modules dir: {}", d.display());
            d
        }
        None => {
            eprintln!("[bootstrap] no /lib/modules — fallback");
            fallback_ramdisk();
        }
    };
    modload::load_disk_stack(&kernel_dir);

    if !disk::wait_for_disk() {
        fallback_ramdisk();
    }

    if disk::is_formatted() {
        println!("[bootstrap] {} already formatted — skip mkfs", disk::DISK_DEV);
    } else {
        println!("[bootstrap] {} blank — formatting", disk::DISK_DEV);
        if let Err(e) = disk::format() {
            eprintln!("[bootstrap] format failed: {} — fallback", e);
            fallback_ramdisk();
        }
    }

    if let Err(e) = disk::mount_disk() {
        eprintln!("[bootstrap] mount failed: {} — fallback", e);
        fallback_ramdisk();
    }

    // M0: 동기화 생략. stub stage-2 = 부트스트랩 자기 자신을 디스크 /sbin/init로 복사.
    {
        let _ = std::fs::create_dir_all(format!("{}/sbin", disk::NEWROOT));
        let self_exe = std::env::current_exe().expect("current_exe");
        let dest = format!("{}/sbin/init", disk::NEWROOT);
        if let Err(e) = std::fs::copy(&self_exe, &dest) {
            eprintln!("[bootstrap] copy self -> {} failed: {} — fallback", dest, e);
            fallback_ramdisk();
        }
        // exec 비트 보장 (ext에선 보통 유지되나 명시).
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
        }
    }

    switchroot::move_virtual_filesystems();
    if let Err(e) = switchroot::switch_root_to_disk("stage2-stub") {
        eprintln!("[bootstrap] switch_root failed: {} — fallback", e);
        fallback_ramdisk();
    }
    // 도달 불가 (switch_root 성공 시 exec).
    unreachable!();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("geulos-bootstrap only runs on Linux (initramfs PID 1 role).");
    std::process::exit(1);
}
```

- [ ] **Step 2: 순수 테스트 재확인 (회귀 없음)**

Run: `cargo test -p geulos-bootstrap`
Expected: superblock(4) + syncplan(3) PASS.

- [ ] **Step 3: build.ps1 — bootstrap 빌드 + initramfs /payload + busybox + 디스크 이미지**

`boot/build.ps1` 변경 (4곳):

(a) Step 1 cross-compile에 bootstrap 추가 — `-p geulos-init ...` 줄들에 `-p geulos-bootstrap` 추가:

```powershell
        & cargo zigbuild --target x86_64-unknown-linux-musl --release `
            -p geulos-init -p geulos-server-host -p geulos-echo-app -p geulos-desktop-shell -p geulos-bootstrap
```
(디버그 분기 줄에도 동일하게 `-p geulos-bootstrap` 추가.)

(b) 바이너리 경로 변수 추가 (`$ShellBin` 다음):

```powershell
$BootstrapBin = Join-Path $BinDir "geulos-bootstrap"
```
그리고 존재 확인 `foreach` 배열에 `$BootstrapBin` 추가.

(c) Step 2 initramfs 조립 — stage 레이아웃을 `/payload` 구조로 변경. 기존 `Copy-Item $InitBin (Join-Path $StageDir "init")` 등을 다음으로 교체:

```powershell
    # /init = stage 1 부트스트랩
    Copy-Item $BootstrapBin (Join-Path $StageDir "init")

    # /newroot 마운트포인트
    $null = New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "newroot")

    # busybox (stage1 mke2fs) — /bin/busybox
    $BusyboxBin = Join-Path $BootDir "tools/busybox"
    if (-not (Test-Path $BusyboxBin)) {
        Write-Host "  busybox 없음 — fetch-busybox.ps1 실행"
        & (Join-Path $BootDir "tools/fetch-busybox.ps1")
    }
    Copy-Item $BusyboxBin (Join-Path $StageDir "bin/busybox")

    # /payload = switch_root 후 디스크로 동기화될 시스템 트리 (stage 2)
    $PayloadDir = Join-Path $StageDir "payload"
    $null = New-Item -ItemType Directory -Force -Path (Join-Path $PayloadDir "sbin")
    $null = New-Item -ItemType Directory -Force -Path (Join-Path $PayloadDir "bin")
    Copy-Item $InitBin   (Join-Path $PayloadDir "sbin/init")          # stage 2 = geulos-init
    Copy-Item $ServerBin (Join-Path $PayloadDir "bin/geulosd")
    Copy-Item $SkeletonBin (Join-Path $PayloadDir "bin/geulos-vm-compositor")
    Copy-Item $ShellBin  (Join-Path $PayloadDir "bin/geulos-desktop-shell")
```

(주의: 기존 `Copy-Item $EchoBin ...` 등 stage `/bin` 복사 라인은 제거. echo-app은 payload에 불필요.)

(d) 모듈 복사 블록(`foreach ($modVer ...)`) — initramfs `/lib/modules`(stage1용)와 `/payload/lib/modules`(stage2용) **양쪽에 복사**. 기존 `$stageModDir` 복사 직후 payload에도:

```powershell
    foreach ($ko in $koFiles) {
        Copy-Item $ko.FullName (Join-Path $stageModDir $ko.Name)
        $payloadModDir = Join-Path $StageDir "payload/lib/modules/$($modVer.Name)"
        $null = New-Item -ItemType Directory -Force -Path $payloadModDir
        Copy-Item $ko.FullName (Join-Path $payloadModDir $ko.Name)
    }
```

(e) Step 3 직후(커널 확인 뒤) 디스크 이미지 생성 블록 추가:

```powershell
# 디스크 이미지 (없을 때만 — 영속성 보존)
$DiskDir  = Join-Path $BootDir "disk"
$DiskPath = Join-Path $DiskDir "geulos-root.img"
$null = New-Item -ItemType Directory -Force -Path $DiskDir
if (-not (Test-Path $DiskPath)) {
    Write-Host "[disk] creating 2GiB sparse image: $DiskPath"
    $fs = [System.IO.File]::Create($DiskPath)
    try { $fs.SetLength(2GB) } finally { $fs.Close() }
    & fsutil sparse setflag $DiskPath 2>$null
} else {
    Write-Host "[disk] reuse existing $DiskPath ($([math]::Round((Get-Item $DiskPath).Length/1MB,1)) MB) — 영속 보존"
}
```

- [ ] **Step 4: launch.ps1 — virtio-blk drive 추가**

`boot/qemu/launch.ps1`: `$QemuArgs` 기본 배열(`-m` 다음, 또는 AccelArgs 뒤) 공통으로 디스크 추가. `$QemuArgs = @(...) + $AccelArgs` 뒤에 삽입:

```powershell
# 영속 루트 디스크 (virtio-blk). 양 분기 공통.
$DiskPath = Join-Path $WorkspaceRoot "boot/disk/geulos-root.img"
if (Test-Path $DiskPath) {
    $QemuArgs += @(
        "-drive", "file=$DiskPath,if=none,id=disk0,format=raw",
        "-device", "virtio-blk-pci,drive=disk0"
    )
    Write-Host "disk:      $DiskPath (virtio-blk /dev/vda)"
} else {
    Write-Host "disk:      (없음 — 램디스크 폴백 부팅)"
}
```

- [ ] **Step 5: 크로스컴파일 + 이미지 빌드**

```powershell
Get-Process qemu-system-x86_64 -ErrorAction SilentlyContinue | Stop-Process -Force
& .\boot\build.ps1 -Release
```
Expected: bootstrap 포함 전 크레이트 빌드 성공, initrd 조립, 디스크 이미지 생성 로그.

- [ ] **Step 6: 부팅 + 직렬 로그로 증명 (결정 게이트)**

```powershell
& .\boot\qemu\launch.ps1 -Graphics
# 몇 초 후 다른 셸에서:
Get-Content boot/serial.log -Tail 80
```
Expected (핵심 라인 순서대로):
```
[bootstrap] mounted proc on /proc
[bootstrap]   loaded virtio_blk.ko
[bootstrap]   loaded ext4.ko            (또는 "skip, absent" — built-in이면 mount 단계서 판정)
[bootstrap] /dev/vda blank — formatting
[bootstrap] format done
[bootstrap] mounted /dev/vda on /newroot
[bootstrap]   moved /proc -> /newroot/proc
[stage2-stub] ===== GEULOS STAGE2 REACHED ON DISK ROOT =====
```

**결정 게이트:**
- ✅ 위 stage2-stub 라인이 보이면 → busybox mke2fs 경로 성공. **M1로 진행.**
- ❌ `mount /dev/vda ... : No such device`(ext4 모듈 부재) → Task 3에서 누락 모듈 추가/`.ko.zst` 처리 후 재시도.
- ❌ `busybox: applet not found` / mke2fs 실패 → 스펙 §5 폴백: e2fsprogs+musl 번들, 또는 최후 FAT(`tools/mkdisk` 호스트 `fatfs`). 이 경우 별도 계획 보강.
- 부팅 종료: `Get-Process qemu-system-x86_64 | Stop-Process -Force`

- [ ] **Step 7: 커밋**

```powershell
git add crates/geulos-bootstrap/src/main.rs boot/build.ps1 boot/qemu/launch.ps1
git commit -m "feat(boot): M0 스파이크 — 포맷/마운트/switch_root 디스크 부팅 증명"
```

---

## M1 — 전체 부트스트랩 (동기화 + 실 stage-2)

### Task 6: stage-2 idempotent 마운트 (geulos-init)

**Files:**
- Modify: `geulos-init/src/mount.rs`
- Test: `geulos-init/src/mount.rs` (인라인 `#[cfg(test)]`)

- [ ] **Step 1: /proc/mounts 파싱 실패 테스트 작성 (순수, TDD)**

`geulos-init/src/mount.rs` 끝에 추가:

```rust
/// `/proc/mounts` 내용에서 target 마운트포인트가 이미 있으면 true. (순수 — 테스트 가능.)
/// 각 줄: "<src> <mountpoint> <fstype> <opts> ...". 두 번째 필드 비교.
pub fn mountpoint_present(proc_mounts: &str, target: &str) -> bool {
    proc_mounts.lines().any(|line| {
        line.split_whitespace().nth(1).map(|mp| mp == target).unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "proc /proc proc rw,nosuid 0 0\n\
                          sysfs /sys sysfs rw 0 0\n\
                          devtmpfs /dev devtmpfs rw 0 0\n";

    #[test]
    fn detects_existing_mountpoints() {
        assert!(mountpoint_present(SAMPLE, "/proc"));
        assert!(mountpoint_present(SAMPLE, "/sys"));
        assert!(mountpoint_present(SAMPLE, "/dev"));
    }

    #[test]
    fn absent_mountpoint_is_false() {
        assert!(!mountpoint_present(SAMPLE, "/newroot"));
        assert!(!mountpoint_present(SAMPLE, "/proc/extra"));
        assert!(!mountpoint_present("", "/proc"));
    }
}
```

- [ ] **Step 2: 테스트 실패→통과 확인**

Run: `cargo test -p geulos-init`
Expected: 새 테스트 2개 PASS. (mount.rs는 cfg(linux) 게이트 없는 순수 함수 → Windows에서 컴파일/실행됨. 만약 mount.rs 전체가 `use nix::mount`로 인해 비-linux 컴파일 실패하면, `mountpoint_present`+tests를 cfg 게이트 밖에 두도록 `use`를 함수 안으로 이동.)

- [ ] **Step 3: mount_essentials를 idempotent화**

`mount_essentials` 안 `for` 루프에서 mount 호출 전 가드 추가:

```rust
    let proc_mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    for (source, target, fstype, flags) in mounts {
        if let Err(e) = std::fs::create_dir_all(target) {
            errors.push(format!("mkdir {}: {}", target, e));
            continue;
        }
        if mountpoint_present(&proc_mounts, target) {
            println!("[init] {} already mounted — skip", target);
            continue;
        }
        match mount(Some(*source), *target, Some(*fstype), *flags, None::<&str>) {
            Ok(()) => println!("[init] mounted {} on {}", source, target),
            Err(e) => errors.push(format!("mount {} -> {}: {}", source, target, e)),
        }
    }
```

- [ ] **Step 4: 크로스컴파일 확인**

Run: `cargo zigbuild --target x86_64-unknown-linux-musl -p geulos-init`
Expected: 성공.

- [ ] **Step 5: 커밋**

```powershell
git add geulos-init/src/mount.rs
git commit -m "feat(init): stage-2 idempotent mount (switch_root 후 /proc 중복 마운트 회피)"
```

---

### Task 7: 동기화 실행 (sync.rs) + 실 stage-2 전환

**Files:**
- Create: `crates/geulos-bootstrap/src/sync.rs`
- Modify: `crates/geulos-bootstrap/src/main.rs`

- [ ] **Step 1: sync.rs 작성 (syncplan 순수 로직 사용)**

Create `crates/geulos-bootstrap/src/sync.rs`:

```rust
//! /payload 시스템 트리를 디스크 루트(/newroot)로 복사 (B 모델).
//! syncplan::should_preserve로 /root·/home은 건드리지 않음. .tmp+rename 원자성.

use std::path::Path;

use crate::disk::NEWROOT;
use crate::syncplan;

const PAYLOAD: &str = "/payload";

/// /payload/* → /newroot/* 재귀 복사. 보존 경로는 스킵.
pub fn sync_system_files() {
    println!("[bootstrap] syncing {} -> {} (preserve root/home)", PAYLOAD, NEWROOT);
    copy_dir_rec(Path::new(PAYLOAD), "");
    println!("[bootstrap] sync done");
}

/// `rel`은 PAYLOAD 기준 상대경로(디스크 루트 기준과 동일). 빈 문자열=루트.
fn copy_dir_rec(payload_root: &Path, rel: &str) {
    let src_dir = if rel.is_empty() { payload_root.to_path_buf() } else { payload_root.join(rel) };
    let entries = match std::fs::read_dir(&src_dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[bootstrap]   read_dir {}: {}", src_dir.display(), e);
            return;
        }
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let child_rel = if rel.is_empty() { name.to_string() } else { format!("{}/{}", rel, name) };

        if syncplan::should_preserve(&child_rel) {
            println!("[bootstrap]   preserve {}", child_rel);
            continue;
        }
        let dest = format!("{}/{}", NEWROOT, child_rel);
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_dir() {
            let _ = std::fs::create_dir_all(&dest);
            copy_dir_rec(payload_root, &child_rel);
        } else {
            copy_file_atomic(&entry.path(), &dest);
        }
    }
}

/// .tmp로 쓰고 rename (같은 FS 내 원자적). 실패는 로그 후 계속.
fn copy_file_atomic(src: &Path, dest: &str) {
    if let Some(parent) = Path::new(dest).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = format!("{}.tmp", dest);
    if let Err(e) = std::fs::copy(src, &tmp) {
        eprintln!("[bootstrap]   copy {} -> {}: {}", src.display(), tmp, e);
        return;
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    if let Err(e) = std::fs::rename(&tmp, dest) {
        eprintln!("[bootstrap]   rename {} -> {}: {}", tmp, dest, e);
    }
}
```

- [ ] **Step 2: main.rs — stub 제거, 동기화 + 실 init exec**

`main.rs`에서: (a) `mod sync;`(linux) 추가, (b) `stage2-stub` 분기 **삭제**, (c) "self 복사" 블록을 동기화 호출로 교체, (d) switch_root 인자를 빈 문자열로.

`#[cfg(target_os = "linux")] mod switchroot;` 다음에:
```rust
#[cfg(target_os = "linux")]
mod sync;
```

`main()` 상단 `stage2-stub` if 블록 전체 삭제.

"self 복사" 블록(`{ let _ = std::fs::create_dir_all(format!("{}/sbin"...)) ... }`)을 다음으로 교체:
```rust
    // 기본 디렉터리 보장 (사용자 데이터 영속 영역 포함)
    for d in ["proc", "sys", "dev", "root", "home", "bin", "sbin", "lib", "etc"] {
        let _ = std::fs::create_dir_all(format!("{}/{}", disk::NEWROOT, d));
    }
    sync::sync_system_files();
```

switch_root 호출 인자 변경:
```rust
    switchroot::move_virtual_filesystems();
    if let Err(e) = switchroot::switch_root_to_disk("") {
        eprintln!("[bootstrap] switch_root failed: {} — fallback", e);
        fallback_ramdisk();
    }
```

`switch_root_to_disk`가 빈 인자를 받으면 init에 빈 argv를 넘기지 않도록 switchroot.rs 조정:
```rust
    let init = CString::new("/sbin/init").unwrap();
    let args: Vec<CString> = if init_arg.is_empty() {
        vec![init.clone()]
    } else {
        vec![init.clone(), CString::new(init_arg).unwrap()]
    };
    execv(&init, &args).map_err(|e| format!("execv /sbin/init: {}", e))?;
```

- [ ] **Step 3: 순수 테스트 + 크로스컴파일**

Run: `cargo test -p geulos-bootstrap`
Expected: superblock(4)+syncplan(3) PASS.
Run: `cargo zigbuild --target x86_64-unknown-linux-musl -p geulos-bootstrap`
Expected: 성공.

- [ ] **Step 4: 빌드 + 부팅 — 실 데스크톱이 디스크 루트에서 뜨는지**

```powershell
Get-Process qemu-system-x86_64 -ErrorAction SilentlyContinue | Stop-Process -Force
& .\boot\build.ps1 -Release
& .\boot\qemu\launch.ps1 -Graphics
Get-Content boot/serial.log -Tail 80
```
Expected 로그:
```
[bootstrap] syncing /payload -> /newroot (preserve root/home)
[bootstrap] sync done
[bootstrap]   moved /proc -> /newroot/proc
=== GeulOS init (PID 1) ===           ← 실 stage-2(geulos-init) 진입
[init] /proc already mounted — skip
[init] spawning /bin/geulosd ...
[init] spawning /bin/geulos-desktop-shell ...
[init] spawning /bin/geulos-vm-compositor ...
```
**시각 확인(사용자):** QEMU 창에 평소 데스크톱(FileTree/Explorer/CLI)이 뜬다. FileTree는 디스크 루트(`/bin /sbin /lib /root ...`)를 보여준다.

- [ ] **Step 5: 커밋**

```powershell
git add crates/geulos-bootstrap/src/sync.rs crates/geulos-bootstrap/src/main.rs crates/geulos-bootstrap/src/switchroot.rs
git commit -m "feat(bootstrap): M1 — 시스템 파일 동기화 + 실 stage-2(geulos-init) switch_root"
```

---

### Task 8: stage-2 모듈 순서 보강 (geulos-init)

**Files:**
- Modify: `geulos-init/src/modules.rs:53-73`

- [ ] **Step 1: LOAD_ORDER에 디스크/FS 모듈 추가**

`LOAD_ORDER` 배열에 (virtio 코어 다음, 네트워크 앞 등 적절히) 추가. 이미 stage1이 적재해 EEXIST=Ok이지만 일관성/단독부팅 대비:

```rust
    // 디스크 + 파일시스템 (stage1이 이미 적재 — EEXIST 무해)
    "virtio_blk.ko",
    "crc16.ko",
    "mbcache.ko",
    "jbd2.ko",
    "ext4.ko",
    // 네트워크
    "e1000.ko",
    "virtio_net.ko",
```

- [ ] **Step 2: 크로스컴파일 확인**

Run: `cargo zigbuild --target x86_64-unknown-linux-musl -p geulos-init`
Expected: 성공.

- [ ] **Step 3: 커밋**

```powershell
git add geulos-init/src/modules.rs
git commit -m "feat(init): stage-2 LOAD_ORDER에 virtio_blk/ext4 추가 (일관성)"
```

---

## M2 — 영속성 수용 검증

### Task 9: 재부팅·재빌드 영속성 확인 (사용자-in-loop)

> 자동화 불가 — 사용자가 VM 안에서 파일을 만들고 재부팅 후 확인. 에이전트는 serial.log로 단계 보조.

- [ ] **Step 1: 첫 부팅 — /root에 파일 생성**

VM 부팅 후, 데스크톱 CLI에서 `/root`에 파일 생성(예: CLI로 파일 쓰기, 또는 ShellRunner). 생성 경로/이름 기록.
*만약 현재 CLI에 파일 생성 수단이 없으면:* stage-1 main.rs에 1회용 마커 생성을 임시 추가하는 대신, `geulos-init` stage-2 진입 시 `/root/.boot-count` 를 증가 기록하는 임시 코드로 대체 검증(검증 후 제거). 우선 CLI 경로를 시도.

- [ ] **Step 2: 재부팅 — 파일 유지 확인**

```powershell
Get-Process qemu-system-x86_64 -ErrorAction SilentlyContinue | Stop-Process -Force
& .\boot\qemu\launch.ps1 -Graphics
```
Expected: `boot/serial.log`에 `[bootstrap] /dev/vda already formatted — skip mkfs` (포맷 스킵=디스크 재사용). FileTree의 `/root`에 Step 1의 파일이 **그대로 존재**.

- [ ] **Step 3: 재빌드 후 영속 확인 (B 모델 검증)**

컴포지터 등 사소한 변경(예: 로그 한 줄) 후:
```powershell
Get-Process qemu-system-x86_64 -ErrorAction SilentlyContinue | Stop-Process -Force
& .\boot\build.ps1 -Release
& .\boot\qemu\launch.ps1 -Graphics
Get-Content boot/serial.log -Tail 40
```
Expected: 새 코드/로그가 반영(시스템 동기화됨) **AND** Step 1의 `/root` 파일 유지(보존됨). = 성공 기준 (4)·(5) 충족.

- [ ] **Step 4: 회귀 — 램디스크 폴백 부팅**

`launch.ps1`에서 디스크를 임시로 떼거나(이미지 rename) `-drive` 없이 부팅 → `[bootstrap] FALLBACK → ramdisk boot` 로그 + 데스크톱 정상 동작 확인(비영속). 헤드리스(`launch.ps1` -Graphics 없이)도 동작 확인.

- [ ] **Step 5: 임시 검증 코드 제거 + 커밋** (Step 1에서 임시 코드 썼을 경우만)

```powershell
git add -A
git commit -m "test(boot): 영속성 수용 검증 완료 (임시 검증 코드 제거)"
```

---

## M3 — 문서 (ADR)

### Task 10: ADR 작성

**Files:**
- Create: `docs/adr/NNN-disk-root-persistence.md` (다음 ADR 번호 확인 후)

- [ ] **Step 1: 다음 ADR 번호 확인**

Run: `Get-ChildItem docs/adr/*.md | Select-Object Name`
다음 순번 NNN 결정.

- [ ] **Step 2: ADR 작성**

`docs/adr/NNN-disk-root-persistence.md` 작성. 내용:
- **결정**: 런타임 루트를 virtio-blk 디스크에 두고 2단계 부팅(`geulos-bootstrap` → `switch_root` → 디스크 `/sbin/init`). 시스템 파일은 B 모델(매 부팅 동기화), `/root`·`/home` 영속. 파일시스템 ext(busybox mke2fs→ext2, ext4 드라이버 마운트).
- **맥락**: 램디스크 단일 부팅으로 재부팅 시 전부 소실 → "진짜 OS" 영속성 부재. 핸드오프의 "Phase E virtio-blk" 실행.
- **대안 기각**: A(1회 설치형, 개발 루프 마찰), C(overlayfs, 시맨틱 복잡), 사용자 파일만 마운트(루트 영속 아님).
- **결과**: 재부팅·재빌드 후 사용자 데이터 유지. 비-목표: 트리/세션 영속, 정식 ext4 저널, 멀티유저.
- (선택) zig/musl 빌드 채택 ADR 빚도 함께 기록.

- [ ] **Step 3: 커밋**

```powershell
git add docs/adr/NNN-disk-root-persistence.md
git commit -m "docs(adr): 디스크 루트 영속 + switch_root + B 동기화 채택"
```

---

## Self-Review (작성자 점검 완료)

**1. 스펙 커버리지:**
- 디스크 배선(launch/build) → Task 5. 모듈(fetch/modules.rs) → Task 3·8. 2단계 init+switch_root → Task 4·5·7. B 동기화(/root 보존) → Task 7(sync.rs)+syncplan(Task 1). 포맷 M0 스파이크 → Task 2·5. 폴백 → Task 5(main.rs `fallback_ramdisk`). idempotent mount → Task 6. 단위/수용/회귀 테스트 → Task 1·6(단위), 9(수용/회귀). ADR → Task 10. **누락 없음.**

**2. Placeholder 스캔:** 모든 코드 스텝에 실제 코드 포함. "적절한 에러처리" 류 없음. Task 9 Step 1의 "CLI 파일 생성 수단 부재 시" 분기는 조건부 대안을 구체 명시(임시 .boot-count). 통과.

**3. 타입/시그니처 일관성:** `has_ext_magic`/`EXT_MAGIC_OFFSET`(superblock) → disk.rs `is_formatted`에서 동일 사용. `should_preserve`(syncplan) → sync.rs `copy_dir_rec`에서 동일 사용. `NEWROOT`/`DISK_DEV`(disk) → switchroot.rs·main.rs·sync.rs에서 `disk::` 경유 일관. `find_kernel_dir`/`load_disk_stack`(modload) → main.rs 호출 일치. `mountpoint_present`(mount) → mount_essentials 호출 일치. `switch_root_to_disk(init_arg)` 시그니처 → main.rs 호출(Task5 "stage2-stub", Task7 "")과 Task7 Step2의 빈문자열 처리 일치. **일관.**

**알려진 리스크(스펙 §위험 반영):** busybox mke2fs 가용성(M0 게이트), ext4 모듈 `.ko.zst` 패키징, switch_root MS_MOVE 순서 — 전부 Task 5 Step 6 결정 게이트에서 조기 노출되도록 배치.
