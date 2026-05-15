# 글 OS 빌드 오케스트레이터
#
# 사용법:
#   .\build.ps1 check        # 환경 점검
#   .\build.ps1 build        # 빌드 (페이즈 0 W7+에 활성화)
#   .\build.ps1 run          # 빌드 + QEMU 실행
#   .\build.ps1 debug        # 빌드 + QEMU + GDB 대기
#   .\build.ps1 bochs        # 빌드 + Bochs 실행
#   .\build.ps1 test         # 빌드 + QEMU 자동 종료
#   .\build.ps1 clean        # build/ 정리
#
# 페이즈 0 W6~W7에서 실제 컴파일 단계 활성화.

[CmdletBinding()]
param(
    [Parameter(Position=0)]
    [ValidateSet('build', 'run', 'debug', 'bochs', 'test', 'clean', 'check')]
    [string]$Command = 'check'
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

# ===== 경로 =====
$Root      = $PSScriptRoot
$SrcDir    = Join-Path $Root 'src'
$BuildDir  = Join-Path $Root 'build'
$ToolsDir  = Join-Path $Root 'tools'

$KernelSrc = Join-Path $SrcDir 'kernel.gl'
$KernelElf = Join-Path $BuildDir 'kernel.elf'

# 글 본가 (별도 프로젝트)
$GeulRoot  = 'C:\workspace\geul'
$GeulExe   = Join-Path $GeulRoot 'dist\compiler.exe'

# 외부 도구
$QemuExe   = 'C:\Program Files\qemu\qemu-system-x86_64.exe'
$BochsExe  = 'C:\Program Files\Bochs-3.0\bochs.exe'
$GdbExe    = 'C:\msys64\mingw64\bin\gdb.exe'

# mingw64 DLL 경로를 PATH 앞에 두어야 gdb/gcc/objdump 등이 자기 지원 DLL 발견
$env:Path = "C:\msys64\mingw64\bin;$env:Path"

# ===== 유틸 =====
function Write-Step($msg) { Write-Host "[글OS] $msg" -ForegroundColor Cyan }
function Write-Warn($msg) { Write-Host "[글OS][!] $msg" -ForegroundColor Yellow }
function Write-Err($msg)  { Write-Host "[글OS][X] $msg" -ForegroundColor Red }

function Test-Tooling {
    $missing = @()
    if (-not (Test-Path $GeulExe))  { $missing += "글 컴파일러 ($GeulExe)" }
    if (-not (Test-Path $QemuExe))  { $missing += "QEMU ($QemuExe)" }
    if (-not (Test-Path $BochsExe)) { $missing += "Bochs ($BochsExe)" }
    if (-not (Test-Path $GdbExe))   { $missing += "GDB ($GdbExe)" }

    if ($missing.Count -gt 0) {
        Write-Err "도구 누락:"
        $missing | ForEach-Object { Write-Host "  - $_" }
        Write-Host ""
        Write-Host "docs/05-테스트환경.md §9 체크리스트 확인."
        return $false
    }
    return $true
}

# ===== 명령들 =====

function Invoke-Check {
    Write-Step "환경 점검"
    if (-not (Test-Tooling)) { exit 1 }

    Write-Host ""
    Write-Step "도구 버전"
    $qver = (& $QemuExe --version | Select-Object -First 1)
    Write-Host "  $qver"
    $gver = (& $GdbExe --version | Select-Object -First 1)
    Write-Host "  $gver"
    Write-Host "  Bochs: 3.0 (path: $BochsExe)"
    $sz = (Get-Item $GeulExe).Length
    Write-Host "  글컴파일러: $GeulExe ($sz bytes)"

    Write-Host ""
    Write-Step "프로젝트 상태"
    if (Test-Path $KernelSrc) {
        Write-Host "  src/kernel.gl 존재"
    } else {
        Write-Warn "  src/kernel.gl 없음 — 페이즈 0 W12에 작성"
    }
    Write-Host "  build/: $BuildDir"
    Write-Host "  tools/: $ToolsDir"
}

function Invoke-Clean {
    Write-Step "build/ 정리"
    if (Test-Path $BuildDir) {
        Get-ChildItem $BuildDir -Recurse -ErrorAction SilentlyContinue | Remove-Item -Force -Recurse
    }
    Write-Host "  완료"
}

function Invoke-Build {
    Write-Step "빌드"
    if (-not (Test-Tooling)) { exit 1 }

    if (-not (Test-Path $KernelSrc)) {
        Write-Warn "src/kernel.gl 없음. 페이즈 0 W12 작업 시작 전이라면 정상."
        Write-Warn "지금은 환경 골격만 확인 — 'check' 명령으로 검증 가능."
        return
    }

    if (-not (Test-Path $BuildDir)) {
        New-Item -ItemType Directory -Path $BuildDir | Out-Null
    }

    # TODO (페이즈 0 W6~W7): freestanding + Multiboot2 ELF 출력
    # & $GeulExe --freestanding --멀티부트2 -출력 $KernelElf $KernelSrc
    # 현재 글 컴파일러는 --freestanding 미지원. 페이즈 0 W4~W7 작업 완료 후 활성화.

    Write-Warn "컴파일러 OS 모드 미구현 — 페이즈 0 W4~W7 완료 후 활성화"
}

function Invoke-Run {
    Invoke-Build
    if (-not (Test-Path $KernelElf)) {
        Write-Err "$KernelElf 없음"
        exit 1
    }
    Write-Step "QEMU 실행"
    $args = @('-kernel', $KernelElf, '-serial', 'stdio', '-m', '256M', '-no-reboot', '-no-shutdown')
    & $QemuExe @args
}

function Invoke-Debug {
    Invoke-Build
    if (-not (Test-Path $KernelElf)) {
        Write-Err "$KernelElf 없음"
        exit 1
    }
    Write-Step "QEMU + GDB 대기 (포트 1234)"
    Write-Host "  다른 터미널: $GdbExe $KernelElf"
    Write-Host "  (gdb) target remote :1234"
    Write-Host "  (gdb) continue"
    Write-Host ""
    $args = @('-kernel', $KernelElf, '-serial', 'stdio', '-m', '256M', '-no-reboot', '-no-shutdown', '-s', '-S')
    & $QemuExe @args
}

function Invoke-Bochs {
    Invoke-Build
    if (-not (Test-Path $KernelElf)) {
        Write-Err "$KernelElf 없음"
        exit 1
    }
    $bochsrc = Join-Path $ToolsDir 'bochsrc.txt'
    if (-not (Test-Path $bochsrc)) {
        Write-Err "$bochsrc 없음 — 페이즈 1에서 작성"
        exit 1
    }
    Write-Step "Bochs 실행"
    & $BochsExe -q -f $bochsrc
}

function Invoke-Test {
    Invoke-Build
    if (-not (Test-Path $KernelElf)) {
        Write-Err "$KernelElf 없음"
        exit 1
    }
    Write-Step "자동 부팅 테스트 (isa-debug-exit)"
    $args = @('-kernel', $KernelElf, '-device', 'isa-debug-exit,iobase=0xf4,iosize=0x04', '-display', 'none', '-serial', 'stdio', '-no-reboot')
    & $QemuExe @args

    if ($LASTEXITCODE -eq 1) {
        Write-Host "[테스트] 통과" -ForegroundColor Green
        exit 0
    } else {
        Write-Err "[테스트] 실패 (QEMU exit $LASTEXITCODE)"
        exit 1
    }
}

# ===== 디스패치 =====
switch ($Command) {
    'check' { Invoke-Check }
    'clean' { Invoke-Clean }
    'build' { Invoke-Build }
    'run'   { Invoke-Run }
    'debug' { Invoke-Debug }
    'bochs' { Invoke-Bochs }
    'test'  { Invoke-Test }
}
