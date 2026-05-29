# 디스크 루트 영속화 — initrd 램디스크에서 virtio-blk 디스크 루트로 (switch_root)

**Date:** 2026-05-29
**Status:** Draft (사용자 review 대기)
**Parent:** `2026-05-29-geulos-vm-compositor-real-tree.md` (VM 컴포지터 파리티의 다음 트랙). 핸드오프 문서가 "Phase E virtio-blk 별도 트랙"으로 미뤄둔 영속성 트랙의 실행.

## 동기

지난 세션들로 컴포지터를 VM 게스트 안으로 이식해, 데스크톱이 호스트 winit 창이 아니라 QEMU 게스트(virtio-gpu `/dev/fb0` + virtio-input) 안에서 직접 렌더된다. 즉 **화면은 이제 진짜 VM 안에 있다.**

그러나 그 아래 **OS의 알맹이는 비어 있다.** 현재 부팅 모델은 다음과 같다:

- 커널이 initrd(cpio.gz)를 램디스크로 풀고, 그 램디스크가 곧 `/`다.
- `geulos-init`이 PID 1로 `/proc·/sys·/dev` 마운트, 모듈 적재, 네트워크, `geulosd`/`desktop-shell`/`vm-compositor` spawn을 모두 수행한다.
- **모든 것이 휘발성 램디스크에 있다.** 영속 저장소가 없어 **재부팅하면 사용자가 만든 파일·상태가 전부 사라진다.**

"진짜 OS"라면 전원을 껐다 켜도 파일이 그대로 있어야 한다. 이 문서는 그 빈틈을 메우는 첫 sub-project를 정의한다: **진짜 OS 루트(`/`)를 virtio-blk 디스크에 올려, 일반 리눅스가 그러하듯 initrd는 부팅 부트스트랩으로만 쓰고 런타임 루트는 디스크에 영속**시킨다 (`switch_root`).

브레인스토밍에서 사용자는 영속 범위로 "사용자 파일만"이나 "파일+세션 상태"가 아니라 **"루트 전체를 디스크로"**를 선택했다. 가장 "진짜 OS"다운 구조이기 때문이다.

## 핵심 발견 (설계 근거)

- **부팅은 현재 램디스크 단일 단계다.** `geulos-init/src/main.rs`가 mount→modules→network→spawn→reap을 한 프로세스에서 한다. 디스크·영속성·`switch_root` 개념이 전혀 없다.
- **모듈은 Alpine apk에서 추출하는 검증된 메커니즘이 있다** (ADR-017). `boot/modules/fetch.ps1`의 `$ModuleNames` 목록에 추가 → `build.ps1`이 `stage/lib/modules/<ver>/`로 복사 → `modules.rs`의 `LOAD_ORDER`가 `finit_module`로 적재. 단 fetch는 `.ko.gz`만 풀고 `.ko.zst`는 미지원.
- **virtio-blk·파일시스템 모듈이 아직 없다.** 현재 추출된 13종은 네트워크(e1000)·GPU·입력뿐. `virtio_blk`와 ext4 드라이버가 없다.
- **spawn은 절대경로에 의존한다.** `spawn.rs`가 `/bin/geulosd`·`/bin/geulos-desktop-shell`·`/bin/geulos-vm-compositor`를 exec → **`switch_root` 후 이 경로들이 디스크 루트에 존재해야 한다** (B 모델 동기화가 보장).
- **desktop-shell은 VM에서 리눅스 `/`를 노출한다** (`drives.rs`의 비-Windows fallback `vec!["/"]`). 루트가 디스크가 되면 FileTree/Explorer가 자동으로 영속 디스크 내용을 보여준다 — 별도 작업 불필요.
- **우리 바이너리는 정적 musl이라 initramfs에 libc가 없다.** Alpine의 동적 링크 바이너리(e2fsprogs 등)를 그냥 넣으면 ld-musl 로더가 없어 못 돈다 → 포맷 도구 선택에 영향(아래 §Architecture 5).
- **geulosd 객체 트리는 순수 RAM이다** (`actor.rs`의 `ObjectServer::new()`, 디스크 직렬화 없음). 트리/세션 상태의 영속은 **이번 범위 밖**(비-목표).

## 결정된 선택 (브레인스토밍 2026-05-29)

| 항목 | 선택 | 이유 |
|---|---|---|
| 영속 범위 | **루트 전체를 디스크로** (`switch_root`) | 일반 리눅스 부팅 구조 = 가장 "진짜 OS"다움. initrd는 부트스트랩만. |
| 시스템 파일 제공·갱신 | **B: 매 부팅 동기화** | initramfs가 최신 시스템 파일을 매 부팅 디스크로 덮어쓰고, `/root`·`/home`은 보존. 재빌드→재부팅이면 새 코드 실행 + 사용자 데이터 유지. 솔로 개발 루프 최적. |
| 파일시스템 | **ext4 우선** (mount via ext4 드라이버) | 진짜 리눅스 루트 시맨틱(exec 비트·디렉터리). FAT은 최후 폴백. |
| 포맷 수단 | **M0 스파이크로 먼저 못박기** | "빈 디스크를 VM 안에서 어떻게 포맷하느냐"가 최대 미해결 변수. 후보 우선순위 + 폴백 준비(§Architecture 5). |
| Stage 1 위치 | **`geulos-bootstrap` 별도 크레이트** | 부트스트랩 책임을 명확히 분리. stage 2(`geulos-init`)는 디스크에서 도는 본체. |
| 사용자 홈 | **`/root` 영속** | 동기화가 건드리지 않는 영속 영역. 첫 영속성 수용 테스트의 대상. |

## 비-목표 (이번 범위 밖)

- **geulosd 객체 트리/세션 상태 영속** (열린 창·위치·CLI 히스토리) — 별도 후속 sub-project. 이번엔 *파일시스템 영속*만.
- **overlayfs 모델** — B(매 부팅 동기화)로 진행. overlay는 검토됐으나 시맨틱 복잡성으로 보류.
- **패키지 매니저 / 소프트웨어 배포** — 없음.
- **멀티유저 / Unix 퍼미션 / 보안 경계** — 단일 사용자 가정. ext4 퍼미션은 존재하나 정책은 미사용.
- **파티션 테이블** — 전체 블록 디바이스(`/dev/vda`)를 직접 포맷. GPT/MBR 없음.
- **베어메탈 / 다른 하이퍼바이저 부팅** — QEMU virtio-blk만.
- **호스트 winit 컴포지터 제거** — 유지(개발 디버그용). 이번 작업은 *추가*이지 교체가 아님.
- **A(1회 설치형)·B 전환 로직** — OS가 안정되면 후속. 지금은 B 고정.

## 성공 기준

1. QEMU가 virtio-blk 디스크를 붙인 채 그래픽으로 부팅하고, 직렬 로그에 **`switch_root` 성공 + 디스크 루트에서 stage 2 진입**이 보인다.
2. 데스크톱(FileTree/Explorer/CLI)이 평소처럼 뜨고, FileTree가 **디스크 루트**를 보여준다.
3. CLI(또는 파일 생성 경로)로 `/root`에 파일을 만든다.
4. **VM을 껐다가 다시 부팅하면 그 파일이 그대로 있다.** ← 핵심 합격 기준.
5. 코드를 재빌드(예: 컴포지터 변경)하고 재부팅하면 **새 코드가 돌면서도 (3)의 파일은 유지**된다 (B 모델 검증).
6. **회귀 없음**: `-drive` 없는 헤드리스 부팅은 램디스크 폴백으로 그대로 동작(ai-bridge 테스트 보존).

**합격 판정**: (4)·(5)는 사용자가 재부팅 후 파일 존재를 시각/CLI로 확인. 에이전트는 `boot/serial.log`로 단계 로그 확인.

## Architecture

### 1. 디스크 & QEMU 배선 (`boot/qemu/launch.ps1`, `boot/build.ps1`)

- **이미지**: `boot/disk/geulos-root.img`, raw, 2 GiB sparse. 게스트에서 `/dev/vda`.
- **`build.ps1`**: 이미지가 **없을 때만** 생성(0으로 채운 sparse raw). 이미 있으면 손대지 않음 → **재빌드해도 디스크 영속 보존**. (`fsutil sparse` 또는 길이만 설정한 빈 파일.)
- **`launch.ps1`**: `-Graphics`/헤드리스 두 분기 모두에 추가:
  ```
  -drive file=boot/disk/geulos-root.img,if=none,id=disk0,format=raw
  -device virtio-blk-pci,drive=disk0
  ```
  헤드리스 분기에도 붙이되, 디스크 없이도(누가 `-drive` 제거 시) 폴백 부팅하므로 필수는 아님.
- **재빌드 주의 유지**: 실행 중 QEMU 종료 후 빌드(이미지/initrd 파일 잠금 회피) — 기존 핸드오프 주의사항 그대로.

### 2. 커널 모듈 확장 (`boot/modules/fetch.ps1`, `geulos-init/src/modules.rs`)

- `fetch.ps1`의 `$ModuleNames`에 추가: `virtio_blk`, `ext4`, + ext4 의존(`jbd2`, `mbcache`, `crc16` 등 — `modules.dep`/`modinfo`로 확정). 일부가 커널 built-in이면 추출 불필요(빌드 시 확인).
- `modules.rs`의 `LOAD_ORDER`에 의존 순서로 삽입: virtio 코어(이미 있음) → `virtio_blk` → ext4 의존 → `ext4`. (모듈 적재는 stage 1이 직접 하므로 — §3 — `modules.rs`는 공유 로직으로 두고 stage 1/2가 각자 필요한 세트를 적재.)
- `.ko.zst` 미지원 한계: 대상 모듈이 zstd로 패키징돼 있으면 `fetch.ps1`에 zstd 해제 추가 필요(리스크 항목).

### 3. Stage 1 — `geulos-bootstrap` 크레이트 (initramfs `/init`)

새 워크스페이스 크레이트. `nix` 의존(mount/mkdir/chroot/exec/MS_MOVE). 알고리즘:

1. `/proc·/sys·/dev` 마운트.
2. `/lib/modules`에서 `virtio_blk`(+의존), ext4 드라이버(+의존) 적재.
3. `/dev/vda` 등장 대기 — sysfs 폴링(네트워크 `find_primary_iface`와 동일한 retry 패턴).
4. **`/dev/vda` 슈퍼블록 probe**: 오프셋 0x438에서 ext 매직 `0xEF53` 확인. 없거나 못 읽으면 "빈 디스크"로 간주 → 포맷(§5).
5. `/dev/vda`를 `/newroot`에 ext4로 마운트.
6. 기본 디렉터리 보장: `/newroot/{proc,sys,dev,root,bin,sbin,lib,etc}`.
7. **동기화(B 모델)**: initramfs의 `/payload/{sbin,bin,lib}` → `/newroot/...` 복사.
   - **덮어쓰기 대상**: `sbin/init`(stage 2), `bin/*`, `lib/modules/*`.
   - **보존(건드리지 않음)**: `/newroot/root`, `/newroot/home`.
   - **원자성**: 각 파일을 `name.tmp`로 쓰고 같은 FS 내 `rename` → 중간 크래시에도 잘린 바이너리 방지.
8. `/proc·/sys·/dev`를 `/newroot/...`로 `mount --move`(MS_MOVE) → stage 2가 재마운트할 필요 없음.
9. **`switch_root`**: `chdir(/newroot)` → `mount(MS_MOVE, ".", "/")` → `chroot(".")` → `chdir("/")` → `execv("/sbin/init")`. (initramfs는 rootfs라 `pivot_root` 불가 — `switch_root` 알고리즘 사용. 옵션: 구 initramfs 파일 삭제로 RAM 회수 — v1은 생략 가능.)

**initramfs 레이아웃** (`build.ps1`이 구성):
```
/init                         = geulos-bootstrap (stage 1)
/proc /sys /dev /newroot      = 빈 마운트포인트
/lib/modules/<ver>/*.ko       = 전체 모듈 (bootstrap이 blk/ext4 적재 + 디스크로 동기화)
/payload/sbin/init            = geulos-init (stage 2)
/payload/bin/{geulosd, geulos-desktop-shell, geulos-vm-compositor}
/payload/lib/modules/<ver>/*.ko
+ (포맷 도구, §5)
```

### 4. Stage 2 — `geulos-init` 축소 (디스크 `/sbin/init`)

기존 `geulos-init`을 거의 유지하되 stage 1과 책임이 겹치는 부분만 조정:

- `mount::mount_essentials`를 **idempotent화**: 각 마운트 전 이미 마운트됐는지 확인(`/proc/mounts` 또는 `statfs` fstype 비교)하고 이미 있으면 skip. stage 1이 `mount --move`로 넘긴 `/proc·/sys·/dev`를 중복 마운트하지 않기 위함.
- 모듈 적재: GPU/입력/네트워크 모듈을 (디스크의) `/lib/modules`에서 적재 — 기존 로직 그대로. virtio_blk/ext4는 stage 1이 이미 적재.
- 네트워크·spawn·reap 루프: **변경 없음.** `/bin/*` 경로는 동기화로 디스크에 존재.

`geulos-init`의 `main()`은 이제 "이미 디스크 루트에서 돈다"는 전제. 부트스트랩 책임(디스크 포맷/동기화/switch_root)은 전부 stage 1로 이동.

### 5. 포맷 수단 — M0 스파이크 (최대 리스크)

빈 `/dev/vda`를 VM 안에서 포맷하는 방법. **우선순위대로 시도, 첫 성공 채택:**

1. **`busybox-static`의 `mke2fs`** — Alpine `busybox-static` apk는 완전 정적(libc 불필요)이라 initramfs `/payload`가 아닌 initramfs `/bin`에 바로 넣고 stage 1이 `mke2fs -F /dev/vda` 실행. busybox `mke2fs`는 ext2를 만들지만 **ext4 드라이버가 ext2/3/4를 모두 마운트** → ext2 포맷 + ext4 마운트로 v1 충족(저널 없음). 가장 단순.
2. **풀 `e2fsprogs` `mke2fs` + musl 로더 번들** — 진짜 ext4(저널)가 필요하면 Alpine `e2fsprogs`(동적) + `/lib/ld-musl-x86_64.so.1` + 의존 lib을 initramfs에 포함. 더 무겁지만 정식 ext4.
3. **최후 폴백: 호스트 FAT32** — 호스트에서 순수 Rust `fatfs` 크레이트로 `geulos-root.img`를 미리 포맷(신규 `tools/mkdisk` 호스트 바이너리). 커널은 `vfat`로 마운트. 최소 rootfs엔 기능 충분(exec 됨, 심링크/퍼미션 미사용). ext4로의 이전은 후속. *진짜 OS다움은 떨어지므로 정말 1·2가 막힐 때만.*

스파이크 산출물: "포맷→마운트→사소한 payload로 `switch_root`→디스크에서 `/sbin/init`(테스트용 stub)이 한 줄 찍기"를 직렬 로그로 증명. 이게 서면 나머지는 표준 기법.

## 데이터 흐름

```
QEMU(-kernel + -initrd + -drive virtio-blk)
  └─ 커널: initrd → 램디스크(/), /init(=geulos-bootstrap) 실행 [PID 1]

[Stage 1: geulos-bootstrap]
  mount /proc /sys /dev
  → load virtio_blk + ext4 (+deps)
  → wait /dev/vda
  → probe 슈퍼블록 → (빈 디스크면) mkfs
  → mount /dev/vda /newroot
  → sync /payload/* → /newroot/*  (/root,/home 보존)
  → mount --move /proc,/sys,/dev → /newroot/...
  → switch_root /newroot  →  exec /sbin/init

[Stage 2: geulos-init  (디스크 / 에서)]
  mount_essentials (idempotent — skip)
  → load gpu/input/net modules
  → network up
  → spawn geulosd → desktop-shell → vm-compositor
  → reap loop

[런타임] 사용자가 /root에 파일 생성 → 디스크에 영속
[재부팅] Stage 1이 디스크에 이미 ext4 발견 → 포맷 skip → 시스템 파일만 재동기화 → /root 그대로
```

## 에러 처리 (PID 1은 절대 그냥 종료 금지 — 전부 degrade)

- **`/dev/vda` 없음**(예: `-drive` 없이 부팅) → 디스크 단계 전체 skip, initramfs에서 stage 2(`/payload/sbin/init`)를 직접 exec = **현재의 비영속 램디스크 동작**. 크게 로그. (헤드리스 ai-bridge 회귀 방지.)
- **포맷 실패 / 마운트 실패 / `switch_root` 실패** → 램디스크 폴백 부팅(위와 동일 경로). 절대 brick 안 됨.
- **동기화 부분 실패** → `.tmp`+`rename`으로 잘린 바이너리 방지. 개별 파일 실패는 로그 후 계속(치명적 파일=init/geulosd면 폴백).
- **모듈 적재 실패** → 한 줄씩 로그(기존 e1000 패턴). blk/ext4 적재 실패 시 폴백.
- 모든 분기에서 직렬 로그를 살려 진단 가능하게.

## 테스트 / 검증

- **단위(호스트 `cargo test`, VM 불필요)**:
  - 슈퍼블록 probe: 주어진 바이트 배열에서 ext 매직(0x438의 `0xEF53`) 판정.
  - 동기화 파일목록 계산: 복사 대상(sbin/bin/lib) vs 보존(root/home) 분류 순수 함수.
  - 레이아웃 경로 헬퍼.
- **빌드**: `x86_64-unknown-linux-musl` 크로스 컴파일(cargo zigbuild) 통과 — Linux 전용 코드라 Windows 로컬 검증이 스킵하므로 push 전 musl 타겟 실제 컴파일 필수.
- **수용(VM, 사용자 확인)**: 성공 기준 (1)~(5). 부팅→`/root`에 파일→재부팅→파일 존재. 코드 재빌드 후에도 파일 유지.
- **회귀**: `-drive` 제거(또는 헤드리스) 부팅이 램디스크 폴백으로 동작.

## 위험

- **포맷 수단(M0)**: 최대 미해결 변수. busybox `mke2fs` 적용 가부, ext2-포맷+ext4-마운트 호환, 정적 여부가 스파이크 전까지 불확실. FAT 폴백 준비로 완화.
- **`switch_root` 디테일**: MS_MOVE 순서, `/proc·/sys·/dev` 이동 타이밍, rootfs에서 `pivot_root` 불가 → `switch_root` 알고리즘 정확성. 표준이지만 직접 구현이라 검증 필요.
- **ext4 모듈 의존 사슬 / zstd 패키징**: `fetch.ps1`이 `.ko.gz`만 푼다. 대상 모듈이 `.ko.zst`면 해제 로직 추가 필요.
- **initramfs 크기 증가**: payload(시스템 파일 사본) + 모듈 + 포맷 도구로 커짐. 부팅 시 압축 해제·동기화 시간 약간 증가(수 MB, 무시할 수준 예상).
- **idempotent 마운트**: stage 2가 `/proc` 등을 중복 마운트하면 문제 → fstype 확인 가드 정확성 필요.

## 마일스톤 (리스크 우선)

- **M0 — 포맷 스파이크**: §5 우선순위로 포맷→마운트→`switch_root`→stub init 한 줄 로그 증명. 포맷 수단 확정.
- **M1 — 전체 부트스트랩**: `geulos-bootstrap` 완성(probe/포맷/마운트/동기화/switch_root) + `geulos-init` stage 2 축소 + virtio_blk/ext4 모듈 + `launch.ps1`/`build.ps1` 배선.
- **M2 — 영속성 수용**: `/root` 파일이 재부팅·재빌드 후 유지(성공 기준 4·5). FileTree가 디스크 루트 표시.
- **M3 — 문서**: 디스크 루트 / `switch_root` / B-동기화 모델 채택 ADR. (zig/musl ADR 빚도 함께 정리 권장.)

## 후속 산출물 (이 스펙 이후)

- **ADR**: 디스크 루트 영속 + `switch_root` + B-동기화 채택.
- **다음 sub-project 후보**: geulosd 객체 트리/세션 상태 영속(열린 창 복원), A(설치형) 전환, 진짜 ext4(저널)로 정착, `/home` 멀티 사용자.
