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
