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
    # cargo zigbuild 사용 — desktop-shell의 ring(C 암호화 코드)을 musl로 컴파일하려면
    # zig가 C 컴파일러/링커 역할. zig가 PATH에 있어야 함(winget zig.zig).
    if ($Release) {
        & cargo zigbuild --target x86_64-unknown-linux-musl --release `
            -p geulos-init -p geulos-server-host -p geulos-echo-app -p geulos-desktop-shell -p geulos-bootstrap
    } else {
        & cargo zigbuild --target x86_64-unknown-linux-musl `
            -p geulos-init -p geulos-server-host -p geulos-echo-app -p geulos-desktop-shell -p geulos-bootstrap
    }
    if ($LASTEXITCODE -ne 0) { throw "cargo zigbuild failed" }

    # VM 컴포지터 bin (compositor 크레이트, --bin 지정 — winit bin은 빌드 안 됨)
    if ($Release) {
        & cargo zigbuild --target x86_64-unknown-linux-musl --release `
            -p geulos-compositor --bin geulos-vm-compositor
    } else {
        & cargo zigbuild --target x86_64-unknown-linux-musl `
            -p geulos-compositor --bin geulos-vm-compositor
    }
    if ($LASTEXITCODE -ne 0) { throw "geulos-vm-compositor cross-compile failed" }
} finally {
    Pop-Location
}

$BinDir = Join-Path $WorkspaceRoot "target/x86_64-unknown-linux-musl/$ProfileDir"
$InitBin = Join-Path $BinDir "geulos-init"
$ServerBin = Join-Path $BinDir "geulosd"
$EchoBin = Join-Path $BinDir "geulos-echo-app"
$SkeletonBin = Join-Path $BinDir "geulos-vm-compositor"
$ShellBin = Join-Path $BinDir "geulos-desktop-shell"
$BootstrapBin = Join-Path $BinDir "geulos-bootstrap"

foreach ($b in @($InitBin, $ServerBin, $EchoBin, $SkeletonBin, $ShellBin, $BootstrapBin)) {
    if (-not (Test-Path $b)) { throw "missing binary: $b" }
}
Write-Host "  built: geulos-bootstrap, geulos-init, geulosd, geulos-echo-app, geulos-vm-compositor, geulos-desktop-shell"

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
$null = New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "newroot")

# /init = stage 1 부트스트랩 (PID 1). 디스크 포맷/마운트/동기화 후 switch_root.
Copy-Item $BootstrapBin (Join-Path $StageDir "init")

# e2fsprogs overlay (stage1 ext4 포맷) — /sbin/mke2fs + /lib/*.so* + musl 로더
$E2fsOverlay = Join-Path $BootDir "tools/e2fs-overlay"
if (-not (Test-Path (Join-Path $E2fsOverlay "sbin/mke2fs"))) {
    Write-Host "  e2fs-overlay 없음 — fetch-e2fsprogs.ps1 실행"
    & (Join-Path $BootDir "tools/fetch-e2fsprogs.ps1")
}
$null = New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "sbin")
$null = New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "lib")
Copy-Item (Join-Path $E2fsOverlay "sbin/*") (Join-Path $StageDir "sbin") -Force
Copy-Item (Join-Path $E2fsOverlay "lib/*")  (Join-Path $StageDir "lib")  -Force

# /payload = switch_root 후 디스크로 동기화될 시스템 트리 (stage 2)
$PayloadDir = Join-Path $StageDir "payload"
$null = New-Item -ItemType Directory -Force -Path (Join-Path $PayloadDir "sbin")
$null = New-Item -ItemType Directory -Force -Path (Join-Path $PayloadDir "bin")
Copy-Item $InitBin     (Join-Path $PayloadDir "sbin/init")          # stage 2 = geulos-init
Copy-Item $ServerBin   (Join-Path $PayloadDir "bin/geulosd")
Copy-Item $SkeletonBin (Join-Path $PayloadDir "bin/geulos-vm-compositor")
Copy-Item $ShellBin    (Join-Path $PayloadDir "bin/geulos-desktop-shell")

# 커널 모듈 포함 (ADR-017). boot/modules/<kernel-version>/ 의 .ko 파일들을
# stage/lib/modules/<kernel-version>/ 로 복사. 모듈 디렉터리가 없으면 fetch.ps1 자동 호출.
$ModulesSrc = Join-Path $BootDir "modules"
$ModuleVersions = @()
if (Test-Path $ModulesSrc) {
    $ModuleVersions = Get-ChildItem $ModulesSrc -Directory -ErrorAction SilentlyContinue |
                      Where-Object { $_.Name -notmatch '^\.' } |
                      Where-Object { (Get-ChildItem $_.FullName -Filter "*.ko" -ErrorAction SilentlyContinue).Count -gt 0 }
}
if ($ModuleVersions.Count -eq 0) {
    Write-Host "  no modules in boot/modules/<ver>/ — running fetch.ps1 to populate..."
    & (Join-Path $ModulesSrc "fetch.ps1")
    if ($LASTEXITCODE -ne 0) { throw "fetch.ps1 failed" }
    $ModuleVersions = Get-ChildItem $ModulesSrc -Directory -ErrorAction SilentlyContinue |
                      Where-Object { $_.Name -notmatch '^\.' } |
                      Where-Object { (Get-ChildItem $_.FullName -Filter "*.ko" -ErrorAction SilentlyContinue).Count -gt 0 }
}
foreach ($modVer in $ModuleVersions) {
    $stageModDir = Join-Path $StageDir "lib/modules/$($modVer.Name)"
    $null = New-Item -ItemType Directory -Force -Path $stageModDir
    $payloadModDir = Join-Path $StageDir "payload/lib/modules/$($modVer.Name)"
    $null = New-Item -ItemType Directory -Force -Path $payloadModDir
    $koFiles = Get-ChildItem $modVer.FullName -Filter "*.ko" -File
    foreach ($ko in $koFiles) {
        Copy-Item $ko.FullName (Join-Path $stageModDir $ko.Name)
        Copy-Item $ko.FullName (Join-Path $payloadModDir $ko.Name)
        Write-Host "  module: $($modVer.Name)/$($ko.Name) ($([math]::Round($ko.Length / 1KB, 1)) KB)"
    }
}

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
# Step 3.5: 영속 루트 디스크 이미지 (없을 때만 — 영속성 보존)
# -------------------------------------------------------------------
$DiskDir  = Join-Path $BootDir "disk"
$DiskPath = Join-Path $DiskDir "geulos-root.img"
$null = New-Item -ItemType Directory -Force -Path $DiskDir
if (-not (Test-Path $DiskPath)) {
    Write-Host "[disk] creating 2GiB sparse image: $DiskPath"
    $fs = [System.IO.File]::Create($DiskPath)
    try { $fs.SetLength(2GB) } finally { $fs.Close() }
    & fsutil sparse setflag $DiskPath 2>$null
} else {
    Write-Host "[disk] reuse existing $DiskPath ($([math]::Round((Get-Item $DiskPath).Length / 1MB, 1)) MB) — 영속 보존"
}

# -------------------------------------------------------------------
# Step 4: Next steps
# -------------------------------------------------------------------
Write-Host ""
Write-Host "[4/4] Build complete. Boot with:"
Write-Host "  pwsh boot/qemu/launch.ps1"
Write-Host ""
