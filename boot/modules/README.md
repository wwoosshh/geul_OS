# boot/modules/ — Alpine 커널 모듈 캐시

`fetch.ps1`이 Alpine `linux-lts` apk를 다운로드해 우리가 필요한 .ko 파일들만 이 디렉터리 아래 `<kernel-version>/`에 추출 저장한다.

`boot/build.ps1`이 빌드 시 이 디렉터리를 참조해 initrd의 `/lib/modules/<kernel-version>/`에 복사하고, `geulos-init`이 부팅 시 `finit_module`로 적재한다.

설계 근거: [ADR-017](../../docs/adr/017-kernel-module-strategy.md)

## 직접 호출

```powershell
# 기본 (e1000 모듈만 추출, 자동 버전 탐색)
pwsh boot/modules/fetch.ps1

# 특정 버전 강제
pwsh boot/modules/fetch.ps1 -LinuxLtsVersion "6.12.89-r0"

# 추가 모듈도 함께 추출
pwsh boot/modules/fetch.ps1 -ModuleNames @("e1000", "virtio_net", "virtio_pci")

# 캐시 무시하고 다시 다운로드
pwsh boot/modules/fetch.ps1 -Force
```

## 산출물

```
boot/modules/
├── .cache/                          # apk 다운로드 캐시 (.gitignored)
│   ├── linux-lts-6.12.89-r0.apk
│   └── extract-6.12.89-r0/
│       ├── boot/vmlinuz-lts
│       └── lib/modules/.../
└── 6.12.89-0-lts/                   # 추출된 모듈 (.gitignored)
    └── e1000.ko                     # decompressed, finit_module 직접 적재 가능
```

`boot/kernel/vmlinuz`도 함께 fresh 다운로드되어 모듈과 *정확히 같은 버전* 보장.

## 향후 추가 모듈

| 마일스톤 | 모듈 |
|---|---|
| M6.5 (지금) | `e1000` (외부 NIC 통신) |
| Phase D | `virtio_gpu`, `virtio_input` (VM 내 GUI) |
| Phase E | `virtio_blk`, `9p`, `virtiofs` (영속 저장) |
