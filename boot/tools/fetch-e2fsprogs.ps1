# boot/tools/fetch-e2fsprogs.ps1 -- Alpine e2fsprogs + musl loader + dependent .so
#
# Stage 1 (geulos-bootstrap) needs mke2fs to format a blank virtio-blk disk as
# real ext4. busybox-static has no ext format applet (only mkfs.vfat), so we
# bundle Alpine e2fsprogs (dynamic musl binary), its shared libs, and the musl
# dynamic loader into the initramfs.
#
# Output: boot/tools/e2fs-overlay/  (rootfs overlay; build.ps1 copies into initramfs)
#   sbin/mke2fs , sbin/e2fsck
#   lib/ld-musl-x86_64.so.1   (musl loader = mke2fs ELF interpreter)
#   lib/*.so*                 (libext2fs, libcom_err, libe2p, libblkid, libuuid, ...)
#
# Uses APKINDEX to resolve package versions (CDN-only; pkgs HTML page may be blocked).

param(
    [string]$AlpineVersion = "v3.21",
    [string[]]$Packages = @(
        "e2fsprogs",        # mke2fs and friends
        "e2fsprogs-libs",   # libext2fs, libe2p
        "libcom_err",       # libcom_err
        "libblkid",         # libblkid
        "libeconf",         # libeconf.so.0 (needed by libblkid)
        "libuuid",          # libuuid
        "musl"              # ld-musl-x86_64.so.1 (dynamic loader)
    ),
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$ToolsDir   = $PSScriptRoot
$CacheDir   = Join-Path $ToolsDir ".cache"
$OverlayDir = Join-Path $ToolsDir "e2fs-overlay"
$MirrorBase = "https://dl-cdn.alpinelinux.org/alpine/$AlpineVersion/main/x86_64"
$null = New-Item -ItemType Directory -Force -Path $CacheDir

Write-Host ""
Write-Host "=== e2fsprogs + musl fetcher ($AlpineVersion main) ==="
Write-Host ""

# 1. Download + parse APKINDEX (P:name / V:version records, blank-line separated)
$IndexApk = Join-Path $CacheDir "APKINDEX.tar.gz"
if ((-not (Test-Path $IndexApk)) -or $Force) {
    Write-Host "[e2fs] downloading APKINDEX ..."
    Invoke-WebRequest -Uri "$MirrorBase/APKINDEX.tar.gz" -OutFile $IndexApk
}
$IndexDir = Join-Path $CacheDir "apkindex"
if (Test-Path $IndexDir) { Remove-Item -Recurse -Force $IndexDir }
$null = New-Item -ItemType Directory -Force -Path $IndexDir
& tar --ignore-zeros -xzf $IndexApk -C $IndexDir 2>&1 | Out-Null
$IndexFile = Join-Path $IndexDir "APKINDEX"
if (-not (Test-Path $IndexFile)) { throw "APKINDEX extract failed" }

$versions = @{}
$raw = Get-Content $IndexFile -Raw
foreach ($rec in ($raw -split "`n`n")) {
    $name = $null; $ver = $null
    foreach ($line in ($rec -split "`n")) {
        $t = $line.Trim()
        if ($t.StartsWith("P:")) { $name = $t.Substring(2) }
        elseif ($t.StartsWith("V:")) { $ver = $t.Substring(2) }
    }
    if ($name -and $ver) { $versions[$name] = $ver }
}
Write-Host "[e2fs] APKINDEX parsed ($($versions.Count) packages)"

# 2. Download + extract each package; flatten binaries to sbin/ and libs to lib/
if (Test-Path $OverlayDir) { Remove-Item -Recurse -Force $OverlayDir }
$null = New-Item -ItemType Directory -Force -Path $OverlayDir
$OverlaySbin = Join-Path $OverlayDir "sbin"
$OverlayLib  = Join-Path $OverlayDir "lib"
$null = New-Item -ItemType Directory -Force -Path $OverlaySbin
$null = New-Item -ItemType Directory -Force -Path $OverlayLib

foreach ($pkg in $Packages) {
    $ver = $versions[$pkg]
    if (-not $ver) { throw "package '$pkg' version not found in APKINDEX" }
    $apkPath = Join-Path $CacheDir "$pkg-$ver.apk"
    if ((-not (Test-Path $apkPath)) -or $Force) {
        Write-Host "[e2fs] downloading $pkg-$ver.apk ..."
        Invoke-WebRequest -Uri "$MirrorBase/$pkg-$ver.apk" -OutFile $apkPath
    }
    $exDir = Join-Path $CacheDir "ex-$pkg"
    if (Test-Path $exDir) { Remove-Item -Recurse -Force $exDir }
    $null = New-Item -ItemType Directory -Force -Path $exDir
    # apk symlinks (mkfs.ext4 -> mke2fs etc.) can't be created by Windows tar
    # (Invalid argument). We don't need them (we call mke2fs directly), so
    # tolerate per-entry errors and extract the real files only.
    $prevEAP = $ErrorActionPreference
    $ErrorActionPreference = 'SilentlyContinue'
    & tar --ignore-zeros -xzf $apkPath -C $exDir 2>$null
    $ErrorActionPreference = $prevEAP

    # binaries (sbin, usr/sbin, bin, usr/bin) -> overlay/sbin
    foreach ($bindir in @("sbin", "usr/sbin", "bin", "usr/bin")) {
        $src = Join-Path $exDir $bindir
        if (Test-Path $src) {
            $bins = Get-ChildItem $src -File -ErrorAction SilentlyContinue
            foreach ($b in $bins) { Copy-Item $b.FullName (Join-Path $OverlaySbin $b.Name) -Force }
        }
    }
    # shared libs (*.so*) anywhere -> overlay/lib, plus SONAME copy
    # (replaces the soname symlink libX.so.N -> libX.so.N.M.. that Windows tar drops)
    $libs = Get-ChildItem $exDir -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -like "*.so*" }
    foreach ($lib in $libs) {
        Copy-Item $lib.FullName (Join-Path $OverlayLib $lib.Name) -Force
        if ($lib.Name -match '^(.*\.so\.[0-9]+)\.') {
            Copy-Item $lib.FullName (Join-Path $OverlayLib $Matches[1]) -Force
        }
    }
    Write-Host "  + $pkg-$ver"
}

# 3. Summary
Write-Host ""
Write-Host "[e2fs] overlay: $OverlayDir"
Write-Host "[e2fs] sbin/:"
$sb = Get-ChildItem $OverlaySbin -ErrorAction SilentlyContinue
foreach ($f in $sb) { Write-Host "    $($f.Name)" }
Write-Host "[e2fs] lib/:"
$lb = Get-ChildItem $OverlayLib -ErrorAction SilentlyContinue
foreach ($f in $lb) { Write-Host "    $($f.Name)  ($($f.Length) bytes)" }

$loader = Join-Path $OverlayLib "ld-musl-x86_64.so.1"
if (Test-Path $loader) { Write-Host "[e2fs] OK: musl loader present" }
else { Write-Warning "[e2fs] musl loader (lib/ld-musl-x86_64.so.1) missing" }
$mke2fs = Join-Path $OverlaySbin "mke2fs"
if (Test-Path $mke2fs) { Write-Host "[e2fs] OK: sbin/mke2fs present" }
else { Write-Warning "[e2fs] sbin/mke2fs missing -- check path" }

# tar leaves a non-zero $LASTEXITCODE from the (ignored) symlink failures; exit clean.
exit 0
