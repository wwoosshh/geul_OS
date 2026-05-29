# boot/qemu/launch.ps1 — QEMU로 GeulOS VM 부팅 (Windows host, WHPX 가속)
#
# 사전:
# - boot/kernel/vmlinuz 준비됨 (Alpine vmlinuz-lts 권장)
# - boot/initrd/geulos.cpio.gz 빌드됨 (pwsh boot/build.ps1)
# - QEMU 설치됨 (choco install qemu)
# - WHPX 활성화 (Windows 기능: "Windows Hypervisor Platform")

param(
    [string]$Kernel = "boot/kernel/vmlinuz",
    [string]$Initrd = "boot/initrd/geulos.cpio.gz",
    [int]$ForwardPort = 5550,
    [int]$Memory = 512,
    [switch]$NoAccel,  # 가속 없이 TCG (느림, 디버깅용)
    [switch]$Graphics  # virtio-gpu 그래픽 창 + virtio 입력 (VM 디스플레이 골격용)
)

$ErrorActionPreference = "Stop"

# 사전 점검
$WorkspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$KernelPath = Join-Path $WorkspaceRoot $Kernel
$InitrdPath = Join-Path $WorkspaceRoot $Initrd

if (-not (Test-Path $KernelPath)) {
    Write-Error "kernel not found: $KernelPath"
    Write-Host ""
    Write-Host "Download a kernel image, e.g.:"
    Write-Host "  Alpine LTS: https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/netboot/vmlinuz-lts"
    Write-Host "  Save as: $KernelPath"
    exit 1
}
if (-not (Test-Path $InitrdPath)) {
    Write-Error "initrd not found: $InitrdPath"
    Write-Host ""
    Write-Host "Build it first:"
    Write-Host "  pwsh boot/build.ps1 -Release"
    exit 1
}

$qemu = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue
if (-not $qemu) {
    Write-Error "qemu-system-x86_64 not found in PATH"
    Write-Host "Install: choco install qemu  (or download from qemu.org)"
    exit 1
}

$AccelArgs = if ($NoAccel) {
    @("-accel", "tcg")
} else {
    @("-accel", "whpx")
}

Write-Host ""
Write-Host "=== Boot GeulOS in QEMU ==="
Write-Host "kernel:    $KernelPath"
Write-Host "initrd:    $InitrdPath"
Write-Host "memory:    ${Memory}M"
Write-Host "accel:     $(if ($NoAccel) { 'tcg (slow)' } else { 'whpx' })"
Write-Host "forward:   host :$ForwardPort  →  guest :5550"
Write-Host ""
Write-Host "Console below. ai-bridge can connect via 127.0.0.1:$ForwardPort"
Write-Host "Press Ctrl+A then X to quit QEMU."
Write-Host ""

$QemuArgs = @(
    "-kernel", $KernelPath,
    "-initrd", $InitrdPath,
    "-m", "${Memory}M"
) + $AccelArgs

# 영속 루트 디스크 (virtio-blk → 게스트 /dev/vda). 양 분기(-Graphics/headless) 공통.
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

# NIC: e1000 사용. Alpine virt 커널은 virtio_net을 모듈로만 빌드하는데 우리 initrd에
# (네트워크) 모듈이 없어 바인딩 실패함. e1000 드라이버는 거의 모든 커널에 built-in이라
# 호환성 안전. 두 모드 모두 host :$ForwardPort → guest :5550 포워딩 유지.
if ($Graphics) {
    # VM 디스플레이 골격: virtio-gpu를 유일한 디스플레이로(-vga none) + virtio 입력.
    # 직렬 콘솔은 파일로 빼서 그래픽 창과 동시에 로그 확인.
    $SerialLog = Join-Path $WorkspaceRoot "boot/serial.log"
    $QemuArgs += @(
        # video=: virtio-gpu DRM 커넥터에 1280x800 모드 강제 (기본 640x480 → 흐림 해소).
        "-append", "console=ttyS0 video=1280x800",
        "-serial", "file:$SerialLog",
        # zoom-to-fit=off: 프레임버퍼를 창 크기에 맞춰 스케일링(보간 흐림)하지 않고 1:1 표시.
        "-display", "gtk,zoom-to-fit=off",
        "-vga", "none",
        "-device", "virtio-gpu-pci",
        "-device", "virtio-keyboard-pci",
        "-device", "virtio-tablet-pci",
        "-netdev", "user,id=net0,hostfwd=tcp::${ForwardPort}-:5550",
        "-device", "e1000,netdev=net0"
    )
    Write-Host "graphics:  virtio-gpu 창 (-vga none) + 직렬 로그 -> $SerialLog"
} else {
    # 기존 텍스트 전용 부팅 (headless 서버 + ai-bridge).
    $QemuArgs += @(
        "-nographic",
        "-append", "console=ttyS0",
        "-netdev", "user,id=net0,hostfwd=tcp::${ForwardPort}-:5550",
        "-device", "e1000,netdev=net0"
    )
}

# QEMU 창을 DPI-aware로 — Windows 디스플레이 배율(125%/150% 등)이 창 비트맵을
# 보간 스케일링해 흐려지는 것을 막고 게스트 프레임버퍼를 물리 픽셀 1:1로 표시.
if ($Graphics) {
    $env:__COMPAT_LAYER = "HighDpiAware"
    # GTK(GDK)가 200% 모니터에서 창을 2배 보간 스케일링하는 것을 끔 → 게스트 fb 1:1 표시.
    $env:GDK_SCALE = "1"
    $env:GDK_DPI_SCALE = "1"
}

# 호스트 브리지 기동 — VM이 10.0.2.2:5560으로 도달해 호스트 C:/D: 읽기 탐색.
# release 우선, 없으면 debug. 바이너리 없으면 호스트 드라이브 비활성(VM 루트만, graceful).
$bridgeProc = $null
$BridgeExe = Join-Path $WorkspaceRoot "target/release/geulos-host-bridge.exe"
if (-not (Test-Path $BridgeExe)) {
    $BridgeExe = Join-Path $WorkspaceRoot "target/debug/geulos-host-bridge.exe"
}
if (Test-Path $BridgeExe) {
    $bridgeProc = Start-Process $BridgeExe -PassThru -WindowStyle Hidden
    Write-Host "host-bridge: 기동 (PID $($bridgeProc.Id), 127.0.0.1:5560)"
} else {
    Write-Host "host-bridge: 바이너리 없음 — 호스트 드라이브 비활성 (pwsh boot/build.ps1로 빌드)"
}

try {
    & qemu-system-x86_64 @QemuArgs
} finally {
    # QEMU 종료 후 브리지 정리.
    if ($bridgeProc -and -not $bridgeProc.HasExited) {
        Stop-Process -Id $bridgeProc.Id -Force -ErrorAction SilentlyContinue
        Write-Host "host-bridge: 정리됨"
    }
}
