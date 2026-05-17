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
    [switch]$NoAccel  # 가속 없이 TCG (느림, 디버깅용)
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
) + $AccelArgs + @(
    "-nographic",
    "-append", "console=ttyS0",
    "-netdev", "user,id=net0,hostfwd=tcp::${ForwardPort}-:5550",
    # NIC: e1000 사용. Alpine virt 커널은 virtio_net을 모듈로만 빌드하는데
    # 우리 initrd에 모듈이 없어 바인딩 실패함. e1000 드라이버는 거의 모든
    # 커널에 built-in이라 호환성 안전. 향후 virtio_net 모듈을 initrd에
    # 포함하거나 custom 커널 사용 시 virtio-net-pci로 복귀 검토.
    "-device", "e1000,netdev=net0"
)

& qemu-system-x86_64 @QemuArgs
