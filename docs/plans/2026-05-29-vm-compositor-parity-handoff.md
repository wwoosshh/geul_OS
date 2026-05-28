# VM 컴포지터 파리티 — 진행 상황 & 다음 세션 핸드오프

**Date:** 2026-05-29
**목표:** VM 게스트가 호스트 컴포지터와 **동일하게 작동**하게 만들기 (사용자 지시: "vm이 컴포지터와 동일한 작동을 하기까지").

## 한 줄 요약

호스트 winit 창에서만 돌던 컴포지터를 **VM 게스트 안(virtio-gpu `/dev/fb0` + virtio-input evdev)에서 직접 동작**시키는 이식을 진행 중. 데스크톱 렌더·탐색·창 조작·CLI 키보드까지 동작. 스크롤/디스플레이 흐림/한글 IME가 남음.

## 큰 그림 (왜 이걸 하나)

GeulOS는 두 세계로 갈라져 있었음: ① VM(진짜 OS, PID 1 = init)이지만 화면 없는 headless 서버, ② 호스트 winit 창에 뜬 풍부한 데스크톱(컴포지터). 사용자 화면이 VM 안에서 단 한 번도 돈 적 없음(ADR-013 잠정 결정이 미실행). 이 이식이 그 분리를 끝낸다. 상세 스펙:
- `docs/specs/2026-05-29-geulos-vm-display-skeleton.md` (fb/evdev 배관 증명)
- `docs/specs/2026-05-29-geulos-vm-compositor-real-tree.md` (실제 트리 렌더 + 클릭)

## 완료 (2026-05-29 세션)

순서대로 다음이 **VM 안에서 실제 동작 + 사용자 시각 확인**됨:

1. **fb/evdev 배관 증명**(골격): virtio-gpu `/dev/fb0`(1280×800 32bpp XRGB) + virtio-input(evdev). 모듈 13종(virtio 코어→PCI→DRM 스택→virtio-gpu→virtio_input/evdev)을 initrd에 번들 + init이 의존순서 적재.
2. **server_client winit 분리**: `EventLoopProxy` → `mpsc::UnboundedSender<UserEvent>`. 호스트는 forwarder로 무회귀. lib cfg 게이트 해제로 호스트·VM 공유.
3. **`dispatch_click` lib 이동**: `compositor/src/dispatch.rs` (호스트·VM 공용).
4. **VM 컴포지터 bin** `compositor/src/bin/geulos-vm-compositor.rs`: geulosd 접속 → tree 공유 → `render_frame`으로 fb0 렌더 → evdev 입력 라우팅. (proof skeleton 교체.)
5. **desktop-shell을 VM에**: **zig/cargo-zigbuild로 musl 빌드**(ring 등 C 의존 해결 — winget `zig.zig` 설치). init이 spawn. → 진짜 데스크톱(FileTree/Explorer/Cli)이 VM 리눅스 루트 `/`를 보여줌.
6. **startup race 수정**: init이 desktop-shell 직후 곧바로 컴포지터를 spawn하던 것 → **3s 정착 지연 + echo-app 제거**(`geulos-init/src/spawn.rs`). 안 하면 Desktop/Explorer가 컴포지터 startup query에서 누락되어 echo-app fallback 렌더됨.
7. **마우스 커서**: VM엔 OS 커서 없음 → 컴포지터가 십자선 커서 직접 렌더.
8. **창 컨트롤**: 닫기(X)/타이틀바 드래그 이동/우하단 리사이즈 — main.rs와 동일 `window_geom` 상수. drag 상태 + release시 move/resize invoke. Explorer parent-nav도.
9. **CLI 키보드 입력**: evdev 키코드 → US QWERTY 키맵(`vm_input::keycode_to_char`, shift) → `CliLocalState` → Enter시 `Cli.submit_input`. 타이핑·명령 실행 확인.

## 남은 작업 (다음 세션, 우선순위 순)

### 1. C3 — 마우스 스크롤 (착수됨)
- `compositor/src/vm_input.rs`에 `EV_REL=0x02`, `REL_WHEEL=0x08` **상수 이미 추가됨**(핸들러 미작성).
- **할 일**: vm-compositor 이벤트 루프에 `EV_REL && REL_WHEEL` arm 추가. 1 notch = 3 lines. cursor 아래 hit_test → Window/ConsoleWindow는 자기 `scroll_y`, FileTree/Explorer는 자기 `scroll_y`, Folder/File hit은 부모 영역(좌 25% = FileTree, else Explorer)으로 매핑 → `UiAction::SetState { scroll_y }`. tm 잠금 범위 안에서 `(ObjectId, i64)` 계산 후 drop하고 send(borrow 주의).
- 참고: main.rs의 `MouseWheel` 핸들러 + `find_scroll_target` + `max_scroll_y_for`. v1은 `scroll_y.max(0)`만(상한 clamp 생략 — render가 시각 clamp). over-scroll 누적 거슬리면 `max_scroll_y_for` 포팅.
- CLI 출력 스크롤(cli_state.scroll_offset)도 휠로 — 선택.

### 2. C4 — 디스플레이 흐림 (미해결)
- 증상: 호스트 컴포지터 대비 화면이 흐림. `-display gtk,zoom-to-fit=off` 시도했으나 **효과 없음**.
- 추정 원인: GTK GL 필터 스케일링 또는 **호스트 Windows DPI 배율**로 QEMU 창이 비트맵 스케일됨.
- **다음 시도**: `-display sdl`(launch.ps1 graphics 분기에서 gtk→sdl 교체), 또는 `gtk,gl=off`, 또는 QEMU 창을 정확히 1280×800로 강제. 시각 확인은 사용자 필요(에이전트가 창을 못 봄).

### 3. 한글 IME (가장 어려움, 큰 작업)
- winit IME(Preedit/Commit)가 VM엔 없음. evdev raw 키만 있음.
- 한글 입력기(두벌식 오토마타: 초성/중성/종성 조합)를 직접 구현하거나 입력기 엔진 필요. **연구·설계 필요한 큰 항목** — 착수 전 사용자와 범위 합의 권장.

### 4. 나머지 입력 파리티 (필요 시)
- Window 본문 클릭 → 에디터 커서 위치 지정 + 텍스트 편집(Ctrl+S 저장). 현재 VM은 Window 본문 클릭 = focus만. main.rs `handle_window_edit_key`/`sync_editor_state` 포팅 필요(키보드가 현재 전부 CLI로 가는 것도 focus 기반 라우팅으로 바꿔야 함 — main.rs `KeyboardFocus`).
- Dialog 버튼 클릭(저장 confirm 등), ConsoleWindow 입력 — main.rs 분기 포팅.
- 키 autorepeat(evdev value==2) 미처리(현재 단발 press만).
- 클립보드 Ctrl+V (arboard) — VM엔 호스트 클립보드 없음, 별도.

### 5. B-docs (문서 빚)
- zig/musl 채택 ADR 미작성. virtio-gpu+/dev/fb0+virtio-input 채택은 skeleton 스펙에 기록됨(정식 ADR로 승격 권장).

## 핵심 기술 컨텍스트 (다음 세션이 알아야 할 것)

- **musl 빌드는 이제 `cargo zigbuild`** (cargo build 아님). `desktop-shell`이 `ai-bridge→reqwest→ring`(C 코드)에 의존 → musl C 컴파일러 필요 → **zig**(winget `zig.zig`, 0.16.0) + `cargo install cargo-zigbuild`. `boot/build.ps1`이 cargo zigbuild 사용. zig가 PATH에 있어야 함(winget Links 디렉터리, user PATH에 영구 추가됨 — 새 셸은 PATH 새로고침 필요).
- **컴포지터 호스트 의존 분리**: `winit`/`softbuffer`/`arboard`는 `cfg(not(target_os="linux"))`. VM(musl)은 순수 렌더 + `vm_fb`/`vm_input`만. `server_client`는 winit-free(공유).
- **렌더 재사용**: `render_frame`/`layout`/`hit_test`/`dispatch_click` 모두 백엔드 독립 — 호스트(winit)·VM(fb) 공유. VM은 `&CliLocalState` + `editor: None`으로 호출.
- **VM 컴포지터 구조**(`geulos-vm-compositor.rs`): tokio 스레드(server_client) + tree 갱신 스레드 + 메인 루프(evdev poll → 이벤트 모아 처리 → render_frame → 커서 → fb.present, 16ms). always-redraw.
- **spawn 순서**(`spawn.rs`): geulosd → 1s → desktop-shell → **3s(mount 정착)** → vm-compositor. echo-app 미spawn.
- **파일시스템**: VM은 리눅스 `/`(initrd 최소 구성: bin/dev/lib/proc/root/sys/init). C:/D: 없음(정상). 진짜 디스크는 virtio-blk(Phase E) 별도 트랙.
- 단순화: 절대좌표 max 상수 32767, 32bpp만, 키 autorepeat 없음.

## 빌드 / 부팅 / 검증 절차

```powershell
# PATH (zig + cargo + qemu)
$env:PATH = [System.Environment]::GetEnvironmentVariable("Path","Machine") + ";" + `
            [System.Environment]::GetEnvironmentVariable("Path","User") + ";" + `
            "$env:USERPROFILE\.cargo\bin;C:\Program Files\qemu"

# 1) 이미지 빌드 (cargo zigbuild + initrd 조립). zig 필수.
& .\boot\build.ps1 -Release

# 2) 그래픽 부팅 (virtio-gpu 창 + virtio 입력). 직렬 로그는 boot/serial.log.
& .\boot\qemu\launch.ps1 -Graphics

# 3) 직렬 로그 확인
Get-Content boot/serial.log -Tail 40

# 종료
Get-Process qemu-system-x86_64 -ErrorAction SilentlyContinue | Stop-Process -Force
```

**주의**: 재빌드 전 반드시 실행 중인 QEMU 종료(initrd 파일 잠금 → mkinitrd 실패).
**헤드리스(기존)**: `launch.ps1`(–Graphics 없이) = -nographic 텍스트 콘솔 + ai-bridge 테스트용. 그대로 유지됨.

## 파일 지도

| 파일 | 역할 |
|---|---|
| `compositor/src/bin/geulos-vm-compositor.rs` | VM 컴포지터 진입점 (입력 루프 + 렌더 + 커서). **C3/IME 등 추가 작업 위치.** |
| `compositor/src/vm_fb.rs` | `/dev/fb0` mmap + ARGB→fb 픽셀 변환 (순수부 + cfg(linux) syscall) |
| `compositor/src/vm_input.rs` | evdev 파싱 + 키맵 + 휠 상수 (순수부 단위테스트 有) |
| `compositor/src/dispatch.rs` | 클릭 dispatch (호스트·VM 공유) |
| `compositor/src/server_client.rs` | 서버 TCP 클라 (winit-free, 공유) |
| `compositor/src/main.rs` | 호스트 winit 컴포지터 (입력 로직 원본 — 포팅 참고처) |
| `geulos-init/src/spawn.rs` | 자식 spawn 순서 |
| `boot/build.ps1` | cargo zigbuild + initrd 조립 |
| `boot/qemu/launch.ps1` | QEMU 부팅 (-Graphics 분기) |

## 다음 세션 시작 추천

"이어서" 또는 "C3 진행"이라 하면 위 **C3(스크롤)**부터. 휠 상수는 이미 vm_input에 있음 → vm-compositor 루프에 wheel arm만 추가하면 됨. 그 다음 C4(디스플레이, sdl 시도) → 필요 시 에디터 입력/Dialog → 한글 IME(범위 합의 후).
