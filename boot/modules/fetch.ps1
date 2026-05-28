# boot/modules/fetch.ps1 — Alpine 커널 모듈 추출 헬퍼
#
# Alpine `linux-lts-X.Y.Z-rN.apk`를 다운로드해서:
#   1. 매칭되는 vmlinuz-lts를 boot/kernel/vmlinuz로 갱신 (버전 어긋남 방지)
#   2. 요청된 모듈을 .ko (decompressed) 형식으로 boot/modules/<kernel>/에 저장
#
# apk는 *concatenated gzip stream* (signature + .PKGINFO + data) 구조라서
# Windows 기본 tar.exe(bsdtar)에 --ignore-zeros 플래그가 필요하다.
#
# ADR-017 참고.

param(
    [string]$AlpineVersion = "v3.21",
    [string]$LinuxLtsVersion = "",                   # 빈 경우 자동 탐색
    [string[]]$ModuleNames = @(
        "e1000",
        "virtio", "virtio_ring",
        "virtio_pci", "virtio_pci_modern_dev", "virtio_pci_legacy_dev",
        "virtio_dma_buf",
        "drm", "drm_kms_helper", "drm_shmem_helper",
        "virtio-gpu",
        "virtio_input", "evdev"
    ),                                                # 추출할 모듈 (확장자 없이)
    [switch]$Force                                    # 캐시 무시
)

$ErrorActionPreference = "Stop"
$BootDir       = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$ModulesRoot   = $PSScriptRoot
$KernelDir     = Join-Path $BootDir "kernel"
$KernelPath    = Join-Path $KernelDir "vmlinuz"
$CacheDir      = Join-Path $ModulesRoot ".cache"

Write-Host ""
Write-Host "=== Alpine kernel module fetcher ==="
Write-Host ""

# ----------------------------------------------------------------------
# 1. 최신 linux-lts 버전 탐색 (필요 시)
# ----------------------------------------------------------------------
if (-not $LinuxLtsVersion) {
    Write-Host "[fetch] resolving latest linux-lts version ..."
    $pageUrl = "https://pkgs.alpinelinux.org/package/$AlpineVersion/main/x86_64/linux-lts"
    try {
        $page = Invoke-WebRequest -Uri $pageUrl -UseBasicParsing
        if ($page.Content -match 'Version[^<]*<[^>]+>\s*([0-9]+\.[0-9]+\.[0-9]+-r[0-9]+)') {
            $LinuxLtsVersion = $Matches[1]
        } elseif ($page.Content -match 'linux-lts-([0-9]+\.[0-9]+\.[0-9]+-r[0-9]+)') {
            $LinuxLtsVersion = $Matches[1]
        } else {
            throw "could not parse version from $pageUrl"
        }
        Write-Host "  detected: $LinuxLtsVersion"
    } catch {
        Write-Warning "version detection failed: $_"
        Write-Warning "fall back to a known-good version. Pass -LinuxLtsVersion to override."
        $LinuxLtsVersion = "6.12.89-r0"
    }
}

# ----------------------------------------------------------------------
# 2. apk 다운로드
# ----------------------------------------------------------------------
$ApkUrl  = "https://dl-cdn.alpinelinux.org/alpine/$AlpineVersion/main/x86_64/linux-lts-$LinuxLtsVersion.apk"
$null    = New-Item -ItemType Directory -Force -Path $CacheDir
$ApkPath = Join-Path $CacheDir "linux-lts-$LinuxLtsVersion.apk"

if ((Test-Path $ApkPath) -and -not $Force) {
    Write-Host "[fetch] using cached apk: $ApkPath ($([math]::Round((Get-Item $ApkPath).Length / 1MB, 1)) MB)"
} else {
    Write-Host "[fetch] downloading $ApkUrl ..."
    try {
        Invoke-WebRequest -Uri $ApkUrl -OutFile $ApkPath
        Write-Host "  saved: $ApkPath ($([math]::Round((Get-Item $ApkPath).Length / 1MB, 1)) MB)"
    } catch {
        throw "apk download failed: $_  (URL: $ApkUrl)"
    }
}

# ----------------------------------------------------------------------
# 3. apk 추출 (concatenated tar.gz — --ignore-zeros 필수)
# ----------------------------------------------------------------------
$ExtractDir = Join-Path $CacheDir "extract-$LinuxLtsVersion"
if (Test-Path $ExtractDir) { Remove-Item -Recurse -Force $ExtractDir }
$null = New-Item -ItemType Directory -Force -Path $ExtractDir

Write-Host "[fetch] extracting apk to .cache/extract-$LinuxLtsVersion/ ..."

$tarCmd = Get-Command tar -ErrorAction SilentlyContinue
if (-not $tarCmd) {
    throw "tar.exe not found. Windows 10/11 should have it at C:\Windows\System32\tar.exe"
}

# bsdtar는 --ignore-zeros로 concatenated stream 처리. -z(gzip), -x(extract), -f(file)
$tarOutput = & tar --ignore-zeros -xzf $ApkPath -C $ExtractDir 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Warning "tar reported: $tarOutput"
    Write-Warning "exit code $LASTEXITCODE — proceeding to check if expected files exist anyway"
}

# ----------------------------------------------------------------------
# 4. vmlinuz-lts 추출 → boot/kernel/vmlinuz 갱신
# ----------------------------------------------------------------------
$VmlinuzInApk = Join-Path $ExtractDir "boot\vmlinuz-lts"
if (-not (Test-Path $VmlinuzInApk)) {
    # 경로 변형도 확인 (apk마다 약간 다름)
    $VmlinuzInApk = Get-ChildItem -Path $ExtractDir -Recurse -Filter "vmlinuz-lts" -ErrorAction SilentlyContinue |
                    Select-Object -First 1 -ExpandProperty FullName
}
if ($VmlinuzInApk) {
    $null = New-Item -ItemType Directory -Force -Path $KernelDir
    if (Test-Path $KernelPath) {
        $BakPath = "$KernelPath.bak-$(Get-Date -Format yyyyMMdd-HHmmss)"
        Move-Item $KernelPath $BakPath
        Write-Host "  backed up old kernel: $BakPath"
    }
    Copy-Item $VmlinuzInApk $KernelPath
    $kSize = (Get-Item $KernelPath).Length
    Write-Host "  updated $KernelPath ($([math]::Round($kSize / 1MB, 1)) MB)"
} else {
    Write-Warning "vmlinuz-lts not found in apk — leaving boot/kernel/vmlinuz untouched"
    Write-Warning "extracted file tree (boot dir):"
    if (Test-Path (Join-Path $ExtractDir "boot")) {
        Get-ChildItem (Join-Path $ExtractDir "boot") -Recurse -File | ForEach-Object {
            Write-Warning "  $($_.FullName)"
        }
    }
}

# ----------------------------------------------------------------------
# 5. 커널 버전 디렉터리 탐지
# ----------------------------------------------------------------------
$ModulesInApk = Join-Path $ExtractDir "lib\modules"
if (-not (Test-Path $ModulesInApk)) {
    throw "no /lib/modules in apk — apk extraction likely incomplete. Check tar output above."
}
$KernelVersion = (Get-ChildItem $ModulesInApk -Directory | Select-Object -First 1).Name
if (-not $KernelVersion) {
    throw "no kernel version subdir under $ModulesInApk"
}
Write-Host "[fetch] apk kernel modules version: $KernelVersion"

# ----------------------------------------------------------------------
# 6. 요청된 모듈 추출 + decompress
# ----------------------------------------------------------------------
$TargetDir = Join-Path $ModulesRoot $KernelVersion
$null = New-Item -ItemType Directory -Force -Path $TargetDir

Add-Type -AssemblyName System.IO.Compression

foreach ($modName in $ModuleNames) {
    # 모듈은 보통 .ko.gz로 패키징됨. 정확히 매칭하기 위해 "$modName.ko*" 패턴
    $candidates = Get-ChildItem -Path $ModulesInApk -Recurse -ErrorAction SilentlyContinue |
                  Where-Object { $_.Name -eq "$modName.ko" -or $_.Name -eq "$modName.ko.gz" -or $_.Name -eq "$modName.ko.zst" }
    $found = $candidates | Select-Object -First 1
    if (-not $found) {
        Write-Warning "module '$modName' not found in apk"
        continue
    }

    $destKo = Join-Path $TargetDir "$modName.ko"
    if ($found.Name.EndsWith(".ko.gz")) {
        Write-Host "  decompress: $($found.Name) -> $modName.ko"
        $inStream  = [System.IO.File]::OpenRead($found.FullName)
        $outStream = [System.IO.File]::Create($destKo)
        $gz        = New-Object System.IO.Compression.GZipStream($inStream, [System.IO.Compression.CompressionMode]::Decompress)
        try { $gz.CopyTo($outStream) } finally { $gz.Close(); $outStream.Close(); $inStream.Close() }
    } elseif ($found.Name.EndsWith(".ko.zst")) {
        Write-Warning "  $($found.Name) uses zstd — not yet supported. Need .ko.gz or .ko."
        continue
    } else {
        Copy-Item $found.FullName $destKo
        Write-Host "  copy: $($found.Name) -> $modName.ko"
    }
    Write-Host "    size: $((Get-Item $destKo).Length) bytes"
}

# ----------------------------------------------------------------------
# 7. 요약
# ----------------------------------------------------------------------
Write-Host ""
Write-Host "[fetch] done."
Write-Host "  kernel:    $KernelPath"
Write-Host "  modules:   $TargetDir"
Write-Host "  to use:    pwsh boot/build.ps1 -Release"
Write-Host ""
