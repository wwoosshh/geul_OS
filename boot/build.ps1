# boot/build.ps1 — GeulOS VM 부팅 이미지 빌드 (Windows host)
#
# 1. Cross-compile init/server-host/echo-app for x86_64-unknown-linux-musl
# 2. Assemble initrd cpio.gz from compiled binaries
# 3. Check Linux kernel presence
# 4. Print next-step QEMU command
#
# Requires:
# - Rust 1.95 + x86_64-unknown-linux-musl target
# - cpio + gzip (Git Bash, MSYS2, or WSL)
# - QEMU (eventual, for boot)

param(
    [switch]$Release,
    [switch]$SkipKernelCheck
)

$ErrorActionPreference = "Stop"
$WorkspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$BootDir = $PSScriptRoot
$KernelPath = Join-Path $BootDir "kernel/vmlinuz"
$InitrdDir = Join-Path $BootDir "initrd"
$InitrdPath = Join-Path $InitrdDir "geulos.cpio.gz"
$StageDir = Join-Path $InitrdDir "stage"

Write-Host ""
Write-Host "=== GeulOS boot image builder ==="
Write-Host "workspace: $WorkspaceRoot"
Write-Host ""

# -------------------------------------------------------------------
# Step 1: Cross-compile
# -------------------------------------------------------------------
Write-Host "[1/4] Cross-compile (target: x86_64-unknown-linux-musl)..."

$ProfileArg = if ($Release) { @("--release") } else { @() }
$ProfileDir = if ($Release) { "release" } else { "debug" }

Push-Location $WorkspaceRoot
try {
    & cargo build --target x86_64-unknown-linux-musl @ProfileArg `
        -p geulos-init -p geulos-server-host -p geulos-echo-app
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
} finally {
    Pop-Location
}

$BinDir = Join-Path $WorkspaceRoot "target/x86_64-unknown-linux-musl/$ProfileDir"
$InitBin = Join-Path $BinDir "geulos-init"
$ServerBin = Join-Path $BinDir "geulosd"
$EchoBin = Join-Path $BinDir "geulos-echo-app"

foreach ($b in @($InitBin, $ServerBin, $EchoBin)) {
    if (-not (Test-Path $b)) { throw "missing binary: $b" }
}
Write-Host "  built: geulos-init, geulosd, geulos-echo-app"

# -------------------------------------------------------------------
# Step 2: Assemble initrd
# -------------------------------------------------------------------
Write-Host "[2/4] Assemble initrd..."

# Clean stage
if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
$null = New-Item -ItemType Directory -Force -Path $StageDir
$null = New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "bin")
$null = New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "proc")
$null = New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "sys")
$null = New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "dev")

# Linux는 initramfs/initrd의 /init를 PID 1으로 실행
Copy-Item $InitBin (Join-Path $StageDir "init")
Copy-Item $ServerBin (Join-Path $StageDir "bin/geulosd")
Copy-Item $EchoBin (Join-Path $StageDir "bin/geulos-echo-app")

# cpio + gzip 필요 (Git Bash / MSYS2 / WSL)
$cpioCmd = Get-Command cpio -ErrorAction SilentlyContinue
$gzipCmd = Get-Command gzip -ErrorAction SilentlyContinue

if (-not $cpioCmd -or -not $gzipCmd) {
    Write-Warning ""
    Write-Warning "cpio / gzip not found in PATH."
    Write-Warning "Install one of:"
    Write-Warning "  - Git for Windows (includes cpio/gzip in Git Bash)"
    Write-Warning "  - MSYS2 (pacman -S cpio gzip)"
    Write-Warning "  - WSL2"
    Write-Warning ""
    Write-Warning "Staged files at: $StageDir"
    Write-Warning "Build initrd manually with:"
    Write-Warning "  cd $StageDir"
    Write-Warning "  find . | cpio -o -H newc | gzip > $InitrdPath"
    throw "cpio/gzip missing"
}

# stage 안에서 cpio newc + gzip
Push-Location $StageDir
try {
    # Windows에서 find가 다를 수 있으므로 PowerShell로 파일 목록 생성
    $files = Get-ChildItem -Recurse -Force | ForEach-Object {
        $rel = $_.FullName.Substring($StageDir.Length + 1).Replace('\', '/')
        if (-not $_.PSIsContainer) { $rel } else { "$rel" }
    }

    $tmpList = New-TemporaryFile
    $files | Out-File -Encoding ASCII -FilePath $tmpList.FullName

    # cmd 경유로 파이프 — PowerShell 파이프는 바이너리 손상 위험
    $cmd = "cpio -o -H newc < `"$($tmpList.FullName)`" | gzip > `"$InitrdPath`""
    & bash -c $cmd
    if ($LASTEXITCODE -ne 0) {
        Remove-Item $tmpList.FullName -ErrorAction SilentlyContinue
        throw "cpio/gzip pipeline failed"
    }
    Remove-Item $tmpList.FullName -ErrorAction SilentlyContinue
} finally {
    Pop-Location
}

$initrdSize = (Get-Item $InitrdPath).Length
Write-Host "  initrd: $InitrdPath ($([math]::Round($initrdSize / 1KB, 1)) KB)"

# -------------------------------------------------------------------
# Step 3: Check kernel
# -------------------------------------------------------------------
Write-Host "[3/4] Check Linux kernel..."

if (-not $SkipKernelCheck -and -not (Test-Path $KernelPath)) {
    Write-Host ""
    Write-Warning "Kernel not found at $KernelPath"
    Write-Host ""
    Write-Host "Download one manually, e.g.:"
    Write-Host "  Alpine LTS: https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/netboot/vmlinuz-lts"
    Write-Host "  Save as:    $KernelPath"
    Write-Host ""
    throw "kernel missing — use -SkipKernelCheck to skip this check"
}

if (Test-Path $KernelPath) {
    $kernelSize = (Get-Item $KernelPath).Length
    Write-Host "  kernel: $KernelPath ($([math]::Round($kernelSize / 1MB, 1)) MB)"
} else {
    Write-Host "  kernel check skipped"
}

# -------------------------------------------------------------------
# Step 4: Next steps
# -------------------------------------------------------------------
Write-Host ""
Write-Host "[4/4] Build complete. Boot with:"
Write-Host "  pwsh boot/qemu/launch.ps1"
Write-Host ""
