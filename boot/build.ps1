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

$ProfileDir = if ($Release) { "release" } else { "debug" }

Push-Location $WorkspaceRoot
try {
    if ($Release) {
        & cargo build --target x86_64-unknown-linux-musl --release `
            -p geulos-init -p geulos-server-host -p geulos-echo-app
    } else {
        & cargo build --target x86_64-unknown-linux-musl `
            -p geulos-init -p geulos-server-host -p geulos-echo-app
    }
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

# cpio 또는 Python (pure-Python fallback) 둘 중 하나 필요
$cpioCmd = Get-Command cpio -ErrorAction SilentlyContinue
$gzipCmd = Get-Command gzip -ErrorAction SilentlyContinue
$pyCmd = Get-Command python -ErrorAction SilentlyContinue

if ($cpioCmd -and $gzipCmd) {
    # 1순위: 정통 cpio + gzip
    Push-Location $StageDir
    try {
        $tmpList = New-TemporaryFile
        Get-ChildItem -Recurse -Force | ForEach-Object {
            $_.FullName.Substring($StageDir.Length + 1).Replace('\', '/')
        } | Out-File -Encoding ASCII -FilePath $tmpList.FullName

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
} elseif ($pyCmd) {
    # 2순위: 순수 Python cpio newc + gzip (cpio 미설치 환경)
    Write-Host "  (cpio not found — using pure-Python mkinitrd.py)"
    $mkinitrd = Join-Path $InitrdDir "mkinitrd.py"
    & python $mkinitrd $StageDir $InitrdPath
    if ($LASTEXITCODE -ne 0) { throw "mkinitrd.py failed" }
} else {
    Write-Warning ""
    Write-Warning "Neither cpio nor python found in PATH."
    Write-Warning "Install one of:"
    Write-Warning "  - Git for Windows (includes cpio/gzip)"
    Write-Warning "  - Python 3 (any recent)"
    Write-Warning "  - MSYS2 / WSL"
    Write-Warning ""
    Write-Warning "Staged files at: $StageDir"
    throw "no archive tool available"
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
