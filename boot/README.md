# boot/ — GeulOS VM 부팅 자원 (M6)

Linux 커널 + initrd로 GeulOS를 *진짜 OS*로 부팅.

## 구조

```
boot/
├── README.md           ← 이 파일
├── kernel/             ← gitignored, 사용자가 vmlinuz 다운로드해 둠
│   └── vmlinuz
├── initrd/             ← build.ps1이 조립
│   ├── build.ps1       (Task 2)
│   └── geulos.cpio.gz  (산출물, gitignored)
└── qemu/
    ├── launch.ps1      (Task 7 — Windows WHPX 가속)
    └── launch.sh       (Task 7 — Linux KVM 가속)
```

## 부팅 절차 (Task 7 완료 후)

```powershell
# 1. 크로스 컴파일 + initrd 조립
pwsh boot/build.ps1 -Release

# 2. QEMU로 부팅
pwsh boot/qemu/launch.ps1
```

자세한 검증은 `docs/manual-tests/m6-boot.md` (Task 8).

## 의존

- Rust 1.95 + `x86_64-unknown-linux-musl` 타겟
- QEMU 7.0+ (Windows: choco install qemu)
- Linux 커널 이미지 (Alpine vmlinuz-lts 권장)
- cpio + gzip (Git Bash 또는 WSL2)
