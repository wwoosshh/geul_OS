# VM 디스플레이 기초 골격 — VM 안 화면 출력 + 입력 최소 증명

**Date:** 2026-05-29
**Status:** Draft (사용자 review 대기)
**Parent:** ADR-007 / ADR-013 (M6 시점 재검토로 연기됐던 "virtio-gpu 컴포지터" 결정의 실행)

## 동기

GeulOS는 현재 **두 개의 분리된 세계**로 갈라져 있다:

1. **VM 세계** — `geulos-init`이 PID 1로 부팅(Alpine 6.12.89 + 우리 initrd). `/proc·/sys·/dev` 마운트 → e1000.ko 적재 → eth0 → `geulosd`(객체 서버) + `echo-app` spawn. **화면·입력·영속성 전부 없는 headless 서버.**
2. **호스트 세계** — `geulos-launcher`(`geulos.exe`)가 `geulosd` + `desktop-shell` + `compositor`를 **Windows 네이티브 프로세스**로 띄워 `127.0.0.1:5550`로 통신. M7~M13의 모든 사용자 화면(파일 트리·탐색기·메모장·CLI·콘솔창·AI 챗·아이콘·UI)이 여기 살지만, 이건 **Windows 앱이지 OS가 아니다.**

근본 원인은 ADR-013이다. M4에서 *개발 편의*를 위해 컴포지터를 `softbuffer + winit`(호스트 창)으로 잠정 구현하고 "M6 완료 시 virtio-gpu와 함께 재검토"라 명시했으나, 그 재검토가 한 번도 실행되지 않은 채 7개 마일스톤이 호스트 위에만 쌓였다. 결과적으로 *AI-네이티브 OS*를 표방하면서 사용자 화면은 단 한 번도 VM 안에서 동작한 적이 없다.

**비전과의 충돌**: 설계 §2.1은 "PID 1을 GeulOS 객체 서버가 점유하고, 부팅 후 사용자가 보는 환경은 100% GeulOS"라 규정한다. 현재 부팅하면 보이는 건 텍스트 콘솔뿐 — 정의상 "진짜 OS"의 핵심 경험을 아직 못 낸다.

이 문서는 그 분리를 끝내는 **첫 단추**를 정의한다: 컴포지터가 호스트 창이 아니라 **VM 게스트 안에서 화면을 그리고 입력을 받을 수 있다는 것**을, 가장 작은 형태로 먼저 증명한다.

## 핵심 발견 (설계 근거)

- **컴포지터 렌더 코어는 이미 백엔드 독립적이다.** `compositor/src/render.rs`의 `render_frame(tree, layout, buffer: &mut [u32], width, height, ...)`는 평범한 픽셀 배열을 채울 뿐, winit/softbuffer에 의존하지 않는다. `layout.rs`·`hit_test.rs`도 좌표만 다루는 순수 함수. 호스트에 묶인 것은 사실상 `compositor/src/main.rs`(창 생성 + softbuffer present + winit 입력) 하나다.
- **커널이 필요한 것을 전부 지원한다 (모듈 형태).** Alpine 6.12.89 config 확인:
  - 화면: `CONFIG_DRM_VIRTIO_GPU=m`, `CONFIG_DRM_FBDEV_EMULATION=y`(DRM이 `/dev/fb0` 제공), `CONFIG_FB=y`, `CONFIG_FRAMEBUFFER_CONSOLE=y`.
  - 입력: `CONFIG_VIRTIO_INPUT=m`, `CONFIG_INPUT_EVDEV=m`(`/dev/input/event*`).
- **모듈을 initrd에 넣는 메커니즘은 e1000.ko로 이미 검증됨.** 그대로 확장하면 됨.

## 결정된 선택 (브레인스토밍 2026-05-29)

| 항목 | 선택 | 이유 |
|---|---|---|
| 첫 산출물 범위 | **최소 증명 먼저** | 화면+입력 배관이 실제로 된다는 걸 검증한 토대 위에 진짜 컴포지터 이식. 한 번에 하나씩 디버깅. |
| 화면 장치 | **virtio-gpu** | 설계 §2.1 "virtio 한 세트만" 원칙에 맞음. ADR-013이 미룬 바로 그 경로. |
| 화면 인터페이스 | **`/dev/fb0`** (mmap 후 픽셀 배열 직접 write) | `render_frame` 출력과 동일한 평면 픽셀 배열. 아래 드라이버가 무엇이든 유저스페이스 코드 동일. DRM/KMS 직접 제어(더블버퍼링 등)는 v2. |
| 입력 장치 | **virtio-input** (virtio-keyboard + virtio-tablet) | virtio 일관성. 태블릿이 마우스를 **절대 좌표**로 줌 → 컴포지터 클릭 위치 계산에 최적. |
| 입력 인터페이스 | **evdev** (`/dev/input/event*`) | 표준 입력 읽기 경로. |
| 배관 코드 위치 | **compositor 크레이트 내부**, `cfg(target_os = "linux")` | 이 배관이 그대로 진짜 이식의 토대. 버리는 코드 없음. 기존 그리기 코드(`fill_rect` 등) 재사용. |

## 비-목표 (이번 범위 밖)

- 진짜 컴포지터 전체를 VM에서 띄우기 (서버 연결 + `render_frame` 풀 루프) — **다음 단계.** 이번엔 사각형 + 클릭 자국까지만.
- DRM/KMS 직접 제어, 더블 버퍼링/페이지 플립 — *v2*.
- 영속 저장소(virtio-blk) — 별개 트랙.
- 한글 IME, 클립보드 등 호스트 컴포지터의 고급 입력 — *이식 이후*.
- 호스트 `softbuffer+winit` 경로 제거 — **유지.** 개발 디버그용으로 계속 가치 있음. 이번 작업은 *추가*이지 교체가 아님.

## 성공 기준

1. QEMU 부팅 시 텍스트 콘솔이 아니라 **그래픽 창**이 뜬다.
2. 그 창에 배경색 + 사각형 + (가능하면 기존 폰트 코드로) "GeulOS VM" 글자가 보인다.
3. 마우스로 창 안을 클릭하면 **클릭한 자리에 표시(작은 사각형/점)가 찍힌다** → 절대 좌표 입력 읽힘 증명.
4. 키를 누르면 화면에 그 키가 표시된다 → 키보드 입력 읽힘 증명.
5. 직렬 콘솔에 모듈 적재 로그가 한 줄씩 보인다.

**합격 판정**: 사용자가 QEMU 창에서 (2)를 보고 (3)을 두 눈으로 확인. 시각 확인은 자동화 불가 — 사용자 눈이 최종 판정.

## Architecture

### 1. 부팅 이미지 모듈 세트 확장 (`boot/modules/fetch.ps1`, `boot/build.ps1`)

현재 e1000.ko만 추출·포함. 아래 모듈 세트를 추가한다 (modules.dep 의존 사슬 확인됨):

**화면 (virtio-gpu):**
```
virtio.ko, virtio_ring.ko                         (코어, 무의존)
virtio_pci.ko ← virtio_pci_legacy_dev.ko,
                virtio_pci_modern_dev.ko           (PCI 전송)
virtio_dma_buf.ko                                  (무의존)
drm.ko, drm_kms_helper.ko, drm_shmem_helper.ko     (DRM 스택)
virtio-gpu.ko ← 위 전부                            (드라이버)
```
**입력 (virtio-input):**
```
virtio_input.ko ← virtio.ko, virtio_ring.ko        (virtio_pci는 화면과 공유)
evdev.ko                                           (무의존)
```

`build.ps1`은 이미 `boot/modules/<ver>/*.ko`를 stage에 복사한다. `fetch.ps1`이 Alpine apk에서 위 모듈을 추출하도록 확장. (initrd가 현재 ~1.7MB → DRM 스택으로 몇 MB 증가 예상.)

### 2. init 모듈 적재 확장 (`geulos-init/src/modules.rs`)

현재 e1000.ko를 `finit_module`로 적재. 위 모듈을 **의존 순서대로** 적재하는 로직 추가 (코어 → PCI 전송 → DRM 스택/virtio_dma_buf → virtio-gpu → virtio_input → evdev). 각 적재를 한 줄 로그(`[init] loaded <name>`). `/dev`는 devtmpfs라 `/dev/fb0`·`/dev/dri/card0`·`/dev/input/event*` 노드는 커널이 자동 생성.

### 3. QEMU 실행 변경 (`boot/qemu/launch.ps1`)

- 디바이스 추가: `-device virtio-gpu-pci`, `-device virtio-keyboard-pci`, `-device virtio-tablet-pci`.
- `-nographic` 제거 → 그래픽 창(Windows QEMU 기본 창) + 직렬 콘솔은 `-serial`로 분리해 로그 유지.
- 기존 텍스트 전용 부팅(현 `launch.ps1`)은 **그대로 유지** — 새 GUI 부팅은 옵션 스위치(`-Graphics`) 또는 별 스크립트로 추가해 회귀 없이.

### 4. 컴포지터 Linux 배관 (`compositor/src/`, `cfg(target_os = "linux")`)

- **`framebuffer` 모듈**: `/dev/fb0` open → `FBIOGET_VSCREENINFO`/`FBIOGET_FSCREENINFO` ioctl로 해상도·픽셀 형식·stride 질의 → mmap → `present(buffer: &[u32])`가 픽셀 배열을 화면 메모리에 blit(픽셀 형식·stride 보정). 32비트(XRGB/ARGB) 가정, 다르면 로그.
- **`input_evdev` 모듈**: `/dev/input/event*` open → `read`로 `input_event` 구조 디코드 → 키 이벤트(`EV_KEY`), 절대 좌표 포인터(`EV_ABS` x/y) + 클릭(`EV_KEY BTN_*`)을 컴포지터 입력 의미로 변환.

두 모듈은 `nix` 크레이트(이미 워크스페이스 의존)의 ioctl/mmap/read로 구현. winit/softbuffer 미사용.

### 5. 증명 실행파일 (compositor 크레이트, Linux 전용 bin)

새 `[[bin]]`(예: `geulos-vm-skeleton`, `cfg(target_os="linux")`). 그리기 루프:
1. `framebuffer::open()` + `input_evdev::open()`.
2. 화면 클리어 + `fill_rect`(기존 render 코드)로 사각형 + 폰트 코드로 "GeulOS VM".
3. evdev 이벤트 읽기: 클릭 → 좌표에 표시 사각형 추가, 키 → 화면에 표시.
4. 변경 시 fb0에 다시 present.

build.ps1에 이 bin을 musl 타겟으로 빌드 + initrd `/bin`에 포함. **이 증명 단계에서 skeleton은 standalone로 동작한다** — 서버(geulosd) 연결이 필요 없다(화면을 직접 그릴 뿐). init은 skeleton을 spawn하며, geulosd/echo-app spawn은 유지해도 무방하나 증명에 필수는 아니다.

## 데이터 흐름

```
부팅 → init: 모듈 적재 → /dev/fb0 + /dev/input/event* 생성
     → init: geulos-vm-skeleton spawn
     → skeleton: mmap(fb0) + open(evdev)
     → loop { evdev 입력 읽기 → 상태 갱신 → fb0에 그리기 }
```

## 에러 처리

- 모듈 적재 실패 / `/dev/fb0`·evdev 부재 → 직렬 콘솔에 명확한 메시지 + 종료(직렬을 살려두는 이유).
- 모듈 적재 한 줄씩 로그(e1000 패턴).
- fb0 픽셀 형식이 예상과 다르면 감지·로그(v1은 32비트만 정식 지원).

## 테스트/검증

- **빌드**: `x86_64-unknown-linux-musl` 크로스 컴파일 통과 필수. Linux 전용 코드라 Windows 로컬 fmt/clippy가 스킵하므로, push 전 musl 타겟으로 실제 컴파일 검증.
- **부팅**: VM 띄워 직렬 로그로 모듈 적재 라인 확인 + QEMU 창 시각 확인.
- **시각 확인**: 사용자 눈(자동화 불가) — 사각형 보이고 클릭 자국 남으면 합격.

## 위험

- DRM 모듈 스택 의존성이 많아 initrd 증가 / 적재 순서 까다로울 수 있음. (drm_kms_helper 등이 추가 의존을 끌 수 있어 modules.dep 재귀 확인 필요.)
- Windows QEMU + virtio-gpu + WHPX 가속 조합의 그래픽 창이 처음 — 예상 못한 문제 가능. **최소 증명의 목적이 이걸 일찍 드러내는 것.**
- fb0 픽셀 형식/stride가 환경마다 달라 blit 보정 필요할 수 있음.

## 후속 산출물 (이 스펙 이후)

- **ADR**: virtio-gpu + /dev/fb0 + virtio-input 채택 (ADR-007/013의 연기 결정 종결). 구현 계획 단계에서 작성.
- **다음 단계 스펙**: 증명 성공 후 — 진짜 컴포지터를 VM에서 (서버 연결 + `render_frame` 풀 루프) 띄우는 이식.
