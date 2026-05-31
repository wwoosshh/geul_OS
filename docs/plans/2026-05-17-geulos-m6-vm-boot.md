> **Status:** completed (2026-05-17)
> **Note:** M6 VM 부팅 정식 마감 — Alpine + initrd + PID 1 geulos-init 안착. 후속 M6.5 모듈 로더로 외부 NW 확장.

# GeulOS M6 — VM 부팅 통합 실행 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. **NEVER push** — controller batches push at end.

**Goal:** GeulOS가 *진짜 OS로* 부팅된다. Linux 커널 위에서 PID 1을 GeulOS init이 점유, 필수 시스템 서비스(mount, network)를 우리가 직접 셋업, server-host가 자동 실행, echo-app도 spawn. 외부 호스트의 ai-bridge가 forwarded TCP 포트로 접속해 조작.

**Why this is the hardest milestone so far:**

지금까지의 M0~M5는 *호스트 OS 위에서 Rust 프로세스로* 동작. M6는 *처음으로* GeulOS가 *완전한 OS 부팅 시퀀스*를 책임짐. 다음 영역이 모두 새로 들어옴:

- Linux 커널 이미지 (vmlinuz) 준비
- initrd / initramfs 조립
- 크로스 컴파일 (Windows host → Linux x86_64 musl)
- PID 1 책임 — /proc, /sys, /dev mount, hostname 설정, 시그널 처리
- 네트워크 bring-up (virtio-net + DHCP 또는 static IP)
- QEMU 커맨드라인 (WHPX 가속, 포트 포워딩)
- *모든 것이 한 번에 작동해야* 첫 부팅 성공

**Honest scope estimate:** 설계 §9.4의 *4주* 추정은 *낙관적*. 실제로는 **5~7주**가 현실적. 첫 부팅 디버깅에 1~2주 추가 가능.

---

## ADR 시드

- **ADR-016 — Init 전략 결정.** GeulOS가 PID 1을 *직접 점유*하되, *최소한의 init 책임만* 수행 (mount, network, server-host spawn). systemd / BusyBox init 미사용. 단, *우리 init이 BusyBox init이 보장하는 기본*(mount/network)만 묶은 *얇은 레이어*임을 명시. 외부 분석 §위험 D 응답.

---

## 파일 구조 (사전 매핑)

```
boot/                              # 신규 디렉터리 — M6 전용
├── README.md                      # 부팅 절차 안내
├── kernel/                        # Linux 커널 (gitignored, 다운로드)
│   └── vmlinuz                    # ~10MB 압축 커널
├── initrd/                        # initrd 조립
│   ├── build.ps1                  # PowerShell 빌드 스크립트
│   └── manifest.txt               # initrd에 포함될 파일 목록
├── qemu/
│   ├── launch.ps1                 # WHPX 가속 QEMU 실행 (Windows)
│   └── launch.sh                  # KVM 가속 QEMU 실행 (Linux/Mac)
└── README.md

geulos-init/                       # 신규 크레이트 — PID 1 책임자
├── Cargo.toml                     # nix 의존 (Linux syscall)
└── src/
    ├── main.rs                    # PID 1 진입점
    ├── mount.rs                   # /proc, /sys, /dev mount
    ├── network.rs                 # virtio-net DHCP 또는 static
    ├── spawn.rs                   # server-host + echo-app spawn
    └── signal.rs                  # SIGCHLD, reaping 등

# 크로스 컴파일 산출물 (gitignored)
target/x86_64-unknown-linux-musl/release/
├── geulos-init
├── geulosd
├── geulos-echo-app
```

---

## Task 1: ADR-016 — Init 전략 결정 + 크레이트 스캐폴드

**Files:**
- Create: `docs/adr/016-init-strategy.md`
- Modify: 루트 `Cargo.toml` (`geulos-init` 멤버 추가)
- Create: `geulos-init/Cargo.toml`
- Create: `geulos-init/src/main.rs` (placeholder)
- Create: `geulos-init/src/{mount,network,spawn,signal}.rs` (모듈 스텁)
- Create: `boot/README.md` (디렉터리 마커)

- [ ] **Step 1: ADR-016 작성**

`docs/adr/016-init-strategy.md`:

```markdown
# ADR-016: GeulOS가 PID 1을 직접 점유하되 *최소 init 책임만* 수행

- **상태:** Accepted
- **일자:** 2026-05-17
- **결정자:** wwoosshh

## 맥락

설계 §2.1 "PID 1을 GeulOS 객체 서버가 점유한다"는 약속을 어떻게 *실현*할지가 M6의 첫 결정. 외부 분석 메모 §위험 D는 *완전 대체*의 위험을 지적:

> "/sbin/init를 GeulOS 코어로 *완전히* 대체하면 udev, dbus, mount, network 같은 *기본 시스템 서비스*가 모두 사라짐. 'ssh도 안 되는 OS'가 됨."

선택지:
1. **순수 PID 1** — GeulOS server-host를 PID 1으로. 다른 어떤 init도 안 함.
2. **GeulOS init layer** — 작은 PID 1 wrapper (`geulos-init`)가 mount/network 책임. 그 위에 server-host + 앱을 spawn. *우리가 "BusyBox init이 하는 일"의 최소 집합을 직접 작성*.
3. **systemd 위 service** — systemd가 PID 1, GeulOS는 그 위 service. *"PID 1 점유" 약속 불일치*.
4. **BusyBox init coexist** — BusyBox init이 PID 1, GeulOS 자동 시작.

## 결정

**선택지 2: GeulOS init layer.** 새 크레이트 `geulos-init`가 PID 1.

책임:
1. /proc, /sys, /dev (devtmpfs) mount
2. hostname 설정
3. virtio-net 인터페이스 up + DHCP (또는 static IP)
4. `geulosd` (server-host) spawn
5. `geulos-echo-app` spawn
6. SIGCHLD 수신, 좀비 reaping
7. 모든 자식이 죽으면 시스템 shutdown

*하지 않는* 것:
- udev (정적 device set만 가정)
- dbus (필요 없음)
- 로그인 매니저 (없음)
- cron (없음)

## 결과

### 긍정적

- *"PID 1 = GeulOS"* 약속 유지 — 외부 분석가가 정확히 약속한 형태
- 부담스럽지 않은 코드 — 추정 ~500줄 Rust (mount + network + spawn + signal)
- 모든 시스템 서비스가 *우리 통제 하*에 있어 *AI 가시성* 보장 (장기 비전)
- BusyBox/systemd 의존 0 — 깔끔한 단일 OS 아이덴티티

### 부정적

- ssh 등 표준 Linux 도구 없음 — *디버깅이 어렵*. 콘솔(virtio-console) 또는 wire 프로토콜로만 진단.
- 부팅 실패 시 *처음에 매우 어려울 가능성*. 단계별 작은 검증 필수.

### 중립

- 후속에 udev 통합 (M8+) 또는 ssh 추가 (M7+)는 가능. 지금은 *최소 부팅*만 책임.

## 대안 검토

- **선택지 1 (순수 PID 1):** 너무 빈약 — mount조차 없으면 server-host가 /proc 못 읽음.
- **선택지 3 (systemd):** "PID 1 = GeulOS" 약속 불일치. 후속 베어메탈 가는 길에 systemd 의존이 짐.
- **선택지 4 (BusyBox coexist):** 깔끔하지만 GeulOS 정체성 약화. M6 단순화에는 좋지만 *우리 약속의 핵심*과 어긋남.

## 참고

- 설계 §2.1 (Linux 위 PID 1 점유)
- 외부 분석 메모 §위험 D (M6 위험)
- ADR-001 (Linux 커널 채용)
```

- [ ] **Step 2: 루트 `Cargo.toml` 멤버 추가**

```toml
members = [
    # ... 기존 ...
    "geulos-init",
]
```

`[workspace.dependencies]`에 추가:

```toml
nix = { version = "0.29", features = ["mount", "process", "signal", "fs"] }
```

- [ ] **Step 3: `geulos-init/Cargo.toml` 생성**

```toml
[package]
name = "geulos-init"
version = "0.0.1"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "GeulOS PID 1 init layer — mount /proc/sys/dev, bring up network, spawn server-host"

[[bin]]
name = "geulos-init"
path = "src/main.rs"

# Linux x86_64 musl 전용. 다른 타겟에선 빌드 안 됨.
[target.'cfg(target_os = "linux")'.dependencies]
nix = { workspace = true }
```

- [ ] **Step 4: `geulos-init/src/main.rs` placeholder**

```rust
//! GeulOS init — Linux PID 1 책임자.
//!
//! 본격 구현은 후속 Task에서.
//! 빌드는 Linux 타겟에서만 동작 (cross-compile required).

#[cfg(target_os = "linux")]
fn main() {
    println!("geulos-init (M6 T1 scaffold — Task 3 이후 본격 구현)");
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("geulos-init only builds for Linux target. Use --target x86_64-unknown-linux-musl");
    std::process::exit(1);
}
```

- [ ] **Step 5: 모듈 스텁**

`geulos-init/src/{mount,network,spawn,signal}.rs`:

```rust
//! Task N에서 구현
```

- [ ] **Step 6: `boot/README.md` 생성**

```markdown
# boot/ — GeulOS VM 부팅 자원

M6 마일스톤. Linux 커널 + initrd로 GeulOS 부팅.

## 구조

- `kernel/vmlinuz` — Linux 커널 (gitignored, build.ps1이 다운로드)
- `initrd/` — initrd 조립 스크립트 + 매니페스트
- `qemu/` — QEMU 실행 스크립트 (WHPX/KVM)

## 빠른 시작

자세한 절차는 Task 7 완료 후 보강.
```

- [ ] **Step 7: 빌드 + 커밋 (no push)**

```bash
# 호스트(Windows)에서는 geulos-init이 빌드 안 됨 — main.rs cfg가 처리.
# 다른 크레이트 빌드 확인:
cargo build --workspace --exclude geulos-init
cargo clippy --workspace --exclude geulos-init -- -D warnings
git add -A
git commit -m "build(geulos-init): M6 T1 — ADR-016 + 크레이트 스캐폴드"
```

---

## Task 2: 크로스 컴파일 환경 + 빌드 스크립트

**Files:**
- Create: `boot/build.ps1`
- Create: `rust-toolchain.toml` 보강 (target 추가)

- [ ] **Step 1: rust-toolchain.toml 갱신**

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.95"
components = ["rustfmt", "clippy"]
targets = ["x86_64-unknown-linux-musl"]
```

다음에 `rustup`이 자동으로 musl 타겟 설치.

- [ ] **Step 2: `boot/build.ps1` 생성**

```powershell
# boot/build.ps1 — GeulOS VM 부팅 이미지 빌드 (Windows host)
#
# 1. Cross-compile init/server-host/echo-app for x86_64-unknown-linux-musl
# 2. Assemble initrd from compiled binaries
# 3. Download Linux kernel if not present
# 4. Print QEMU command to run it

param(
    [switch]$Release,
    [switch]$NoKernel
)

$ErrorActionPreference = "Stop"
$WorkspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$BootDir = $PSScriptRoot
$KernelPath = Join-Path $BootDir "kernel/vmlinuz"
$InitrdPath = Join-Path $BootDir "initrd/geulos.cpio.gz"

Write-Host "[1/4] Cross-compile (target: x86_64-unknown-linux-musl)..."
$ProfileArg = if ($Release) { "--release" } else { "" }
$ProfileDir = if ($Release) { "release" } else { "debug" }

Push-Location $WorkspaceRoot
try {
    & cargo build --target x86_64-unknown-linux-musl $ProfileArg `
        -p geulos-init -p geulos-server-host -p geulos-echo-app
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
} finally {
    Pop-Location
}

$BinDir = Join-Path $WorkspaceRoot "target/x86_64-unknown-linux-musl/$ProfileDir"
$InitBin = Join-Path $BinDir "geulos-init"
$ServerBin = Join-Path $BinDir "geulosd"
$EchoBin = Join-Path $BinDir "geulos-echo-app"

if (-not (Test-Path $InitBin)) { throw "missing $InitBin" }
if (-not (Test-Path $ServerBin)) { throw "missing $ServerBin" }
if (-not (Test-Path $EchoBin)) { throw "missing $EchoBin" }

Write-Host "[2/4] Assemble initrd..."
$Stage = New-Item -ItemType Directory -Force -Path (Join-Path $BootDir "initrd/stage")
$Bin = New-Item -ItemType Directory -Force -Path (Join-Path $Stage.FullName "bin")
$Proc = New-Item -ItemType Directory -Force -Path (Join-Path $Stage.FullName "proc")
$Sys = New-Item -ItemType Directory -Force -Path (Join-Path $Stage.FullName "sys")
$Dev = New-Item -ItemType Directory -Force -Path (Join-Path $Stage.FullName "dev")

# /init 심볼릭 링크 (Linux는 /init를 PID 1로 실행)
Copy-Item $InitBin (Join-Path $Stage.FullName "init")
Copy-Item $ServerBin (Join-Path $Bin "geulosd")
Copy-Item $EchoBin (Join-Path $Bin "geulos-echo-app")

# WSL2 또는 Git Bash가 있으면 cpio + gzip 사용
$cpioCmd = Get-Command cpio -ErrorAction SilentlyContinue
if ($cpioCmd) {
    Push-Location $Stage.FullName
    try {
        $files = Get-ChildItem -Recurse -File | ForEach-Object {
            $_.FullName.Substring($Stage.FullName.Length + 1).Replace('\', '/')
        }
        $files | Out-File -Encoding ASCII -FilePath (Join-Path $env:TEMP "cpio_files.txt")
        & cpio -o -H newc < (Join-Path $env:TEMP "cpio_files.txt") | & gzip > $InitrdPath
    } finally {
        Pop-Location
    }
} else {
    Write-Warning "cpio not found — install Git Bash or use WSL"
    Write-Warning "stage at: $($Stage.FullName)"
    throw "cannot assemble initrd"
}

Write-Host "[3/4] Check kernel..."
if (-not $NoKernel -and -not (Test-Path $KernelPath)) {
    Write-Host "Kernel not present at $KernelPath"
    Write-Host "Please download a Linux kernel image manually:"
    Write-Host "  - Alpine: https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/netboot/vmlinuz-lts"
    Write-Host "  - Place at: $KernelPath"
    throw "kernel missing"
}

Write-Host "[4/4] Build complete. Boot with:"
Write-Host "  pwsh boot/qemu/launch.ps1"
```

- [ ] **Step 3: 커밋**

```bash
git add -A
git commit -m "build(boot): M6 T2 — cross-compile + initrd 빌드 스크립트"
```

---

## Task 3: PID 1 mount 책임 — /proc, /sys, /dev

**Files:**
- Modify: `geulos-init/src/main.rs`
- Modify: `geulos-init/src/mount.rs`

- [ ] **Step 1: `geulos-init/src/mount.rs` 구현**

```rust
//! /proc, /sys, /dev 마운트.

#[cfg(target_os = "linux")]
use nix::mount::{mount, MsFlags};

#[cfg(target_os = "linux")]
pub fn mount_essentials() -> Result<(), String> {
    let mounts: &[(&str, &str, &str, MsFlags)] = &[
        ("proc", "/proc", "proc", MsFlags::empty()),
        ("sysfs", "/sys", "sysfs", MsFlags::empty()),
        ("devtmpfs", "/dev", "devtmpfs", MsFlags::empty()),
    ];

    for (source, target, fstype, flags) in mounts {
        // 디렉터리가 없으면 생성
        std::fs::create_dir_all(target).map_err(|e| {
            format!("mkdir {}: {}", target, e)
        })?;
        mount(Some(*source), *target, Some(*fstype), *flags, None::<&str>)
            .map_err(|e| format!("mount {} -> {}: {}", source, target, e))?;
        println!("[init] mounted {} on {}", source, target);
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn mount_essentials() -> Result<(), String> {
    Err("mount only on Linux".to_string())
}
```

- [ ] **Step 2: `main.rs` 갱신 — mount 호출**

```rust
#[cfg(target_os = "linux")]
fn main() {
    println!("[init] GeulOS init starting (PID {})", std::process::id());
    if let Err(e) = geulos_init::mount::mount_essentials() {
        eprintln!("[init] mount failed: {}", e);
    }
    // 다음 Task에서 network + spawn
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}
```

mod 선언이 main.rs에 있어야 하므로 `lib.rs` 추가 또는 main.rs에 `mod mount;` 추가. 후자 선택:

```rust
mod mount;
mod network;
mod spawn;
mod signal;

#[cfg(target_os = "linux")]
fn main() {
    // ...
}
```

(다른 모듈은 다음 Task에서 본격.)

- [ ] **Step 3: 빌드 확인 (Linux 타겟)**

Windows에서:
```bash
cargo build --target x86_64-unknown-linux-musl -p geulos-init
```

성공해야 함.

- [ ] **Step 4: 커밋**

---

## Task 4: PID 1 network 책임 — virtio-net DHCP

**Files:**
- Modify: `geulos-init/src/network.rs`
- Modify: `geulos-init/Cargo.toml` (DHCP 관련 deps)

- [ ] **Step 1: 가장 단순한 경로 — `ip` 명령 사용**

DHCP 클라이언트는 *작성 안 함*. 대신 *initrd에 BusyBox 또는 udhcpc 바이너리 포함* OR *static IP 설정*.

가장 빠른 첫 부팅: **static IP**. virtio-net이 QEMU에서 `192.168.x.x` 또는 link-local을 자동 부여하는 게 아니므로 우리가 설정.

`network.rs`:

```rust
#[cfg(target_os = "linux")]
use std::process::Command;

#[cfg(target_os = "linux")]
pub fn bring_up_loopback_and_eth0() -> Result<(), String> {
    // `ip` 명령은 initrd에 포함 안 됨. 직접 ioctl 사용해야 정통이지만,
    // 일단 PoC로 `busybox ip` 또는 socket+ioctl 사용.
    //
    // 본 Task는 *최소* 구현: ifconfig 대신 직접 SIOCSIFFLAGS ioctl.
    // 자세한 ifup 로직은 후속 PR로.
    //
    // 임시: 그냥 print만 하고 통과
    eprintln!("[init] network setup TODO — using QEMU user-mode network, no config needed");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn bring_up_loopback_and_eth0() -> Result<(), String> {
    Ok(())
}
```

**중요 결정:** QEMU의 *user-mode networking* (`-netdev user`)을 사용하면 *guest가 자동으로 10.0.2.x를 받음*. DHCP 없이도 호스트의 `localhost:5550`이 guest의 `0.0.0.0:5550`에 *포트 포워딩*됨. 이게 *지금 시점에 가장 단순한 경로*.

따라서 *Task 4의 첫 구현은 거의 no-op*. QEMU 설정으로 처리.

- [ ] **Step 2: 커밋**

---

## Task 5: spawn — server-host + echo-app 자식 프로세스

**Files:**
- Modify: `geulos-init/src/spawn.rs`

- [ ] **Step 1: 구현**

```rust
#[cfg(target_os = "linux")]
use std::process::{Child, Command};

#[cfg(target_os = "linux")]
pub struct SpawnedProcesses {
    pub server: Child,
    pub echo_app: Option<Child>,
}

#[cfg(target_os = "linux")]
pub fn spawn_all() -> Result<SpawnedProcesses, String> {
    println!("[init] spawning geulosd...");
    let server = Command::new("/bin/geulosd")
        .arg("0.0.0.0:5550")
        .spawn()
        .map_err(|e| format!("spawn geulosd: {}", e))?;
    println!("[init] geulosd PID = {}", server.id());

    // server-host가 listening 준비될 시간 부여
    std::thread::sleep(std::time::Duration::from_secs(1));

    println!("[init] spawning geulos-echo-app...");
    let echo_app = Command::new("/bin/geulos-echo-app")
        .arg("127.0.0.1:5550")
        .spawn()
        .ok(); // 실패해도 진행

    if let Some(ref e) = echo_app {
        println!("[init] echo-app PID = {}", e.id());
    } else {
        println!("[init] echo-app spawn failed (continuing)");
    }

    Ok(SpawnedProcesses { server, echo_app })
}
```

- [ ] **Step 2: 커밋**

---

## Task 6: signal 처리 + main 통합

**Files:**
- Modify: `geulos-init/src/signal.rs`
- Modify: `geulos-init/src/main.rs`

- [ ] **Step 1: `signal.rs` — 좀비 reaping**

```rust
#[cfg(target_os = "linux")]
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
#[cfg(target_os = "linux")]
use nix::unistd::Pid;

#[cfg(target_os = "linux")]
pub fn reap_zombies() -> usize {
    let mut reaped = 0;
    loop {
        match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => break,
            Ok(WaitStatus::Exited(pid, code)) => {
                eprintln!("[init] reaped PID {} (exit {})", pid, code);
                reaped += 1;
            }
            Ok(_) => reaped += 1,
            Err(_) => break,
        }
    }
    reaped
}
```

- [ ] **Step 2: `main.rs` 본격**

```rust
mod mount;
mod network;
mod signal;
mod spawn;

#[cfg(target_os = "linux")]
fn main() {
    println!("\n=== GeulOS init (PID {}) ===\n", std::process::id());

    // 1) Mount essentials
    if let Err(e) = mount::mount_essentials() {
        eprintln!("[init] mount failed: {}", e);
    }

    // 2) Network (user-mode QEMU networking — auto)
    if let Err(e) = network::bring_up_loopback_and_eth0() {
        eprintln!("[init] network failed: {}", e);
    }

    // 3) Spawn services
    let processes = match spawn::spawn_all() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[init] spawn failed: {}", e);
            // PID 1이 죽으면 kernel panic — 최소한 살아있게
            loop { std::thread::sleep(std::time::Duration::from_secs(60)); }
        }
    };

    let server_pid = processes.server.id();

    // 4) Main loop — reap zombies, monitor children
    println!("[init] entering main loop");
    loop {
        signal::reap_zombies();

        // server가 죽으면 시스템 사실상 중단됨 — 그래도 init은 살아있어야 함
        // (kernel이 PID 1 죽으면 panic)
        std::thread::sleep(std::time::Duration::from_secs(1));

        // TODO: server-host 재시작 정책
        let _ = server_pid;
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("geulos-init only builds for Linux. Use:");
    eprintln!("  cargo build --target x86_64-unknown-linux-musl -p geulos-init");
    std::process::exit(1);
}
```

- [ ] **Step 3: 커밋**

---

## Task 7: QEMU 실행 스크립트

**Files:**
- Create: `boot/qemu/launch.ps1`
- Create: `boot/qemu/launch.sh`

- [ ] **Step 1: `boot/qemu/launch.ps1` (Windows host, WHPX 가속)**

```powershell
# boot/qemu/launch.ps1 — QEMU로 GeulOS VM 부팅 (WHPX 가속)
param(
    [string]$Kernel = "boot/kernel/vmlinuz",
    [string]$Initrd = "boot/initrd/geulos.cpio.gz",
    [int]$ForwardPort = 5550,
    [int]$Memory = 512
)

$ErrorActionPreference = "Stop"

# 사전 점검
if (-not (Test-Path $Kernel)) { throw "kernel not found: $Kernel" }
if (-not (Test-Path $Initrd)) { throw "initrd not found: $Initrd" }

$qemu = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue
if (-not $qemu) {
    throw "qemu-system-x86_64 not found. Install via: choco install qemu (or msys2)"
}

Write-Host "=== Boot GeulOS in QEMU ==="
Write-Host "kernel:    $Kernel"
Write-Host "initrd:    $Initrd"
Write-Host "memory:    ${Memory}M"
Write-Host "forward:   host :$ForwardPort → guest :5550"
Write-Host ""
Write-Host "Console output below. ai-bridge can connect via 127.0.0.1:$ForwardPort"
Write-Host "Press Ctrl+A then X to quit QEMU."
Write-Host ""

& qemu-system-x86_64 `
    -kernel $Kernel `
    -initrd $Initrd `
    -m "${Memory}M" `
    -accel whpx `
    -nographic `
    -append "console=ttyS0 quiet" `
    -netdev "user,id=net0,hostfwd=tcp::${ForwardPort}-:5550" `
    -device virtio-net-pci,netdev=net0
```

- [ ] **Step 2: `boot/qemu/launch.sh` (Linux/Mac host, KVM 가속)**

```bash
#!/usr/bin/env bash
set -euo pipefail

KERNEL="${KERNEL:-boot/kernel/vmlinuz}"
INITRD="${INITRD:-boot/initrd/geulos.cpio.gz}"
FORWARD_PORT="${FORWARD_PORT:-5550}"
MEMORY="${MEMORY:-512}"

if [ ! -f "$KERNEL" ]; then echo "kernel missing: $KERNEL" >&2; exit 1; fi
if [ ! -f "$INITRD" ]; then echo "initrd missing: $INITRD" >&2; exit 1; fi

ACCEL="kvm"
if ! [ -e /dev/kvm ]; then
    ACCEL="tcg"
    echo "[warn] KVM not available, using TCG (slow)"
fi

exec qemu-system-x86_64 \
    -kernel "$KERNEL" \
    -initrd "$INITRD" \
    -m "${MEMORY}M" \
    -accel "$ACCEL" \
    -nographic \
    -append "console=ttyS0 quiet" \
    -netdev "user,id=net0,hostfwd=tcp::${FORWARD_PORT}-:5550" \
    -device virtio-net-pci,netdev=net0
```

- [ ] **Step 3: 커밋**

---

## Task 8: 부팅 시간 측정 + 메모

**Files:**
- Create: `docs/manual-tests/m6-boot.md`

- [ ] **Step 1: 수동 부팅 안내**

`docs/manual-tests/m6-boot.md`:

```markdown
# M6 — 수동 VM 부팅 검증

## 사전 준비

1. **Linux 커널** 다운로드 (`boot/kernel/vmlinuz`):
   - Alpine: https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/netboot/vmlinuz-lts
   - 또는 Debian: https://deb.debian.org/debian/dists/stable/main/installer-amd64/current/images/netboot/debian-installer/amd64/linux
2. **QEMU** 설치 (Windows: `choco install qemu`, Linux: `apt install qemu-system-x86`)
3. **musl 타겟** 설치: `rustup target add x86_64-unknown-linux-musl`

## 빌드 + 부팅

```powershell
# Windows
pwsh boot/build.ps1 -Release
pwsh boot/qemu/launch.ps1
```

```bash
# Linux/Mac
./boot/build.sh --release   # (Task 9에서 추가)
./boot/qemu/launch.sh
```

## 기대 출력

```
=== GeulOS init (PID 1) ===
[init] mounted proc on /proc
[init] mounted sysfs on /sys
[init] mounted devtmpfs on /dev
[init] network setup TODO ...
[init] spawning geulosd...
[init] geulosd PID = 2
geulosd listening on 0.0.0.0:5550
[init] spawning geulos-echo-app...
[init] echo-app PID = 3
echo-app connecting to 127.0.0.1:5550...
[echo-app] HelloAck: actor=app:echo:...
[echo-app] mounted: container=..., text=..., button=...
[echo-app] subscribed to button events
[init] entering main loop
```

## 외부 연결 확인 (다른 터미널, 호스트)

```powershell
cd C:\AiOS
cargo run -p geulos-ai-bridge -- run --scenario ai-bridge/scenarios/01_explore.toml
```

→ VM 안의 server-host와 통신해야 함. report_done에서 echo-app의 3개 객체 발견 보고.

## 종료

QEMU 콘솔에서 `Ctrl+A` → `X`.

## 부팅 시간 측정

cold boot 시작부터 "entering main loop"까지 측정. 목표: **15초 이내**.
```

- [ ] **Step 2: 커밋**

---

## Task 9: M6 acceptance 자동 통합 테스트 (옵션)

**Files:**
- Create: `boot/tests/boot_smoke.ps1` (선택)

- [ ] **Step 1: smoke 스크립트**

이 task는 *고도 환경 의존*이라 CI에서 돌리기 어렵고 *사람 수동 검증이 1차*. 본 task는 *수동 단계만 자동화*:

```powershell
# boot/tests/boot_smoke.ps1
# 1) build
& "$PSScriptRoot/../build.ps1" -Release
# 2) QEMU 백그라운드 시작
$qemu = Start-Process -PassThru -NoNewWindow `
    -FilePath pwsh -ArgumentList "$PSScriptRoot/../qemu/launch.ps1"
Start-Sleep -Seconds 10
# 3) ai-bridge로 객체 발견
& cargo run -p geulos-ai-bridge -- run `
    --scenario ai-bridge/scenarios/01_explore.toml `
    --server 127.0.0.1:5550
$result = $LASTEXITCODE
# 4) cleanup
Stop-Process -Id $qemu.Id -Force
exit $result
```

- [ ] **Step 2: 커밋**

---

## Task 10: 최종 + 푸시

- [ ] **Step 1: 전체 검증 (호스트만 — VM 부팅은 사용자 수동)**

```bash
cargo build --workspace --exclude geulos-init
cargo build --target x86_64-unknown-linux-musl -p geulos-init -p geulos-server-host -p geulos-echo-app
cargo test --workspace --exclude geulos-init
cargo clippy --workspace --exclude geulos-init -- -D warnings
cargo fmt --all -- --check
```

- [ ] **Step 2: 단일 push**

- [ ] **Step 3: M6 완료 선언**

다음이 모두 사실:
- `geulos-init` 크레이트 빌드됨 (Linux 타겟)
- `boot/build.ps1`로 initrd 조립 가능
- `boot/qemu/launch.ps1`로 QEMU 부팅 시작
- 사용자가 *수동으로* 시도해 console 출력 확인
- ai-bridge가 forwarded TCP로 객체 발견

---

## 자체 점검

**스펙 커버리지:**
- 설계 §9.2 M6 산출물:
  - 커스텀 initrd → Task 2 + 9
  - PID 1 점유 → Task 1 (ADR-016) + Tasks 3-6
  - virtio-console/net → Task 7 QEMU 인자
  - QEMU 부팅 스크립트 → Task 7
  - 부팅 시간 측정 → Task 8

**스코프 인정 — 한계:**
- **자동화된 e2e 테스트 없음** — VM 환경이 너무 다양 (WHPX vs KVM vs Docker)
- **DHCP 구현 안 함** — QEMU user-mode 네트워킹에 의존
- **udev / 동적 device 관리 없음** — initrd의 정적 device set만
- **ssh / 디버깅 도구 없음** — virtio-console + wire protocol로만
- **첫 부팅 실패 시 디버깅 곤란** — 추가 1-2주 가능

**플레이스홀더:** TBD 없음. 단순함 위해 *고의로 미구현*인 부분(DHCP, udev)은 ADR-016 또는 본 문서에 명시.

**알려진 한계 (M6 범위 밖, M7+로):**
- 베어메탈 부팅 (VM only)
- 디스크 파일시스템 (RAM disk만)
- 영속성 (재부팅 시 모든 객체 사라짐)
- 다중 사용자
- 키보드/마우스 (M4 컴포지터와 통합 안 됨 — virtio-input + virtio-gpu 필요)
