# M6 — VM 부팅 수동 검증

GeulOS가 *진짜 OS로* QEMU에서 부팅되는지 직접 확인하는 절차.

## 사전 준비 (1회만)

### 1. Linux 커널 이미지 다운로드

```powershell
mkdir -Force boot/kernel
Invoke-WebRequest -Uri "https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/netboot/vmlinuz-lts" `
                  -OutFile boot/kernel/vmlinuz
```

대안: Debian의 netboot/linux, 또는 직접 컴파일한 vmlinuz. 가급적 *작고 virtio 지원*이 들어간 것.

### 2. QEMU 설치

```powershell
choco install qemu        # Windows (Chocolatey)
```

또는 https://qemu.weilnetz.de/w64/ 에서 직접 다운로드 후 PATH 추가.

확인:

```powershell
qemu-system-x86_64 --version
```

### 3. cpio + gzip (initrd 조립용)

Git Bash 또는 MSYS2 또는 WSL2 중 하나. Git for Windows가 가장 간편 — 설치 시 *Git Bash*가 자동 포함됨.

확인:

```powershell
where.exe cpio
where.exe gzip
```

### 4. WHPX (Windows Hypervisor Platform) 활성화

Windows 기능 켜기/끄기 → *"Windows Hypervisor Platform"* 체크 → 재부팅.

이게 없으면 QEMU가 TCG 모드(소프트웨어 에뮬레이션)로 폴백 — 10배 이상 느림.

### 5. Rust musl 타겟 (이미 자동 설치됨)

`rust-toolchain.toml`에 명시되어 있어 `cargo` 호출 시 자동 설치. 수동 확인:

```powershell
rustup target list --installed | findstr musl
# expect: x86_64-unknown-linux-musl
```

## 빌드 + 부팅

```powershell
# 1. 크로스 컴파일 + initrd 조립
pwsh boot/build.ps1 -Release

# 2. QEMU 부팅
pwsh boot/qemu/launch.ps1
```

## 기대 콘솔 출력

```
=== Boot GeulOS in QEMU ===
kernel:    C:\AiOS\boot\kernel\vmlinuz
initrd:    C:\AiOS\boot\initrd\geulos.cpio.gz
memory:    512M
accel:     whpx
forward:   host :5550 → guest :5550

Console below. ai-bridge can connect via 127.0.0.1:5550
Press Ctrl+A then X to quit QEMU.

[Linux kernel boot messages…]
[부팅 시 kernel이 /init를 PID 1로 실행]

=== GeulOS init (PID 1) ===

[init] mounted proc on /proc
[init] mounted sysfs on /sys
[init] mounted devtmpfs on /dev
[init] network: using QEMU user-mode (auto-configured)
[init] spawning /bin/geulosd ...
[init] geulosd PID = 2
geulosd listening on 0.0.0.0:5550
[init] spawning /bin/geulos-echo-app ...
[init] echo-app PID = 3
echo-app connecting to 127.0.0.1:5550...
[echo-app] HelloAck: actor=app:echo:...
[echo-app] mounted: container=..., text=..., button=...
[echo-app] subscribed to button events

[init] entering main loop (server PID 2, echo PID Some(3))
[init] external ai-bridge can connect via host-forwarded TCP
```

## 외부에서 연결 검증 (다른 터미널, 호스트)

VM이 부팅된 상태에서 *별 PowerShell 창*:

```powershell
cd C:\AiOS
cargo run -p geulos-ai-bridge -- run --scenario ai-bridge/scenarios/01_explore.toml
```

→ ai-bridge가 `127.0.0.1:5550` (호스트 포트, QEMU가 guest의 :5550으로 포워딩)으로 접속.
→ guest 안의 server-host와 통신.
→ report_done에서 echo-app의 3개 객체 발견 보고.

이 동작이 *처음으로 GeulOS의 모든 층이 동시에 작동*하는 순간:
- ③ Linux 커널 (vmlinuz)
- ② GeulOS 코어 (geulos-init + server-host)
- ① 앱 (echo-app)
- 외부 AI (ai-bridge + Claude API)

## 종료

QEMU 콘솔에서 `Ctrl+A`, 그 다음 `X`. 또는 다른 터미널에서:

```powershell
# 윈도우 작업 관리자에서 qemu-system-x86_64.exe 강제 종료
```

## 부팅 시간 측정

QEMU 시작부터 `[init] entering main loop`까지 시간 측정. 목표:

- **WHPX 가속:** 15초 이내
- **TCG (가속 없음):** 1~2분

너무 오래 걸리면:
- 커널이 너무 큰 빌드(불필요 드라이버 다수)일 가능성 — Alpine vmlinuz-lts 권장
- `-m 256M`로 메모리 줄여 시도
- WHPX 가속 활성화 확인 (`-accel whpx`)

## 알려진 실패 모드

| 증상 | 원인 | 대응 |
|---|---|---|
| `cpio: command not found` | Git Bash / MSYS2 / WSL 미설치 | Git for Windows 설치 |
| `WHPX failed` | Windows 기능 미활성 또는 다른 가상화와 충돌 | Windows Hypervisor Platform 활성화 / Hyper-V 비활성화 |
| `Kernel panic: ... no init found` | initrd의 /init가 실행 불가 | build.ps1 다시 실행, 권한 확인 |
| `[init]` 출력 후 멈춤 | mount 실패 또는 spawn 실패 | 콘솔에 `console=ttyS0` 보장됐는지 확인 |
| 호스트에서 `connect refused` | forwarded 포트가 잘못됨 | `-hostfwd=tcp::5550-:5550` 확인 |
| `[echo-app] connecting...` 후 멈춤 | server-host가 아직 ready 안 됨 | 정상 — 1초 후 재시도. 그래도 안 되면 server-host 출력 확인 |

## 다음 단계 (M6 완료 후)

M7 dogfooding으로 진입. 메모장 등 *진짜 앱*을 작성하고 ai-bridge로 자율 조작.
