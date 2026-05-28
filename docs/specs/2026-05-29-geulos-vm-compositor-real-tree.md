# VM 컴포지터 A — 실제 서버 트리 렌더 + 클릭 왕복

**Date:** 2026-05-29
**Status:** Draft (사용자 review 대기)
**Parent:** [[2026-05-29-geulos-vm-display-skeleton]] (fb0+evdev 배관 증명 완료) 후속

## 동기

VM 디스플레이 기초 골격으로 **화면 출력(`/dev/fb0`, virtio-gpu) + 입력(evdev, virtio-input)** 배관이 VM 게스트 안에서 동작함을 증명했다. 그러나 골격은 사각형을 직접 그리는 standalone 데모일 뿐, **진짜 컴포지터 기계**(서버 연결 + `render_frame` + 입력 라우팅)는 아직 VM에서 돌지 않는다.

전체 이식은 한 번에 하기엔 큰 작업이라 3 조각으로 분해한다:

- **A (본 스펙)**: VM 컴포지터가 geulosd에 접속 → 지금 VM에 있는 실제 객체(echo-app의 Container/Text/Button)를 `render_frame`으로 fb0에 그림 → 클릭하면 `hit_test`→`dispatch_click`→Invoke로 버튼이 실제로 눌림. **컴포지터 기계 전체(서버연결+렌더+입력왕복)가 VM에서 돈다는 걸 증명.**
- **B (후속)**: `desktop-shell`을 musl로 크로스 컴파일 + 부팅 포함 → 진짜 데스크톱(파일트리·탐색기·CLI 등)이 VM에 등장.
- **C (후속)**: 키보드/CLI/메모장 편집/스크롤/한글 IME를 winit→evdev로 완전 포팅.

A는 가장 위험이 적은 첫 조각이다 — desktop-shell 크로스 컴파일 위험 없이, 컴포지터 핵심 기계가 VM에서 왕복 동작함을 증명하면 B·C는 그 위에 쌓기만 하면 된다.

## 핵심 발견 (설계 근거)

- **server_client는 winit에 거의 안 묶여 있다.** `compositor/src/server_client.rs`의 winit 결합은 `proxy: Arc<EventLoopProxy<UserEvent>>` 파라미터 하나뿐 — `UserEvent::Redraw`/`Quit` 신호 전송 용도. 그 외 480줄(접속/query/get/subscribe/race-fix 루프)은 winit 독립.
- **render_frame / layout / hit_test는 순수 함수.** 이미 lib에 있고 `&mut [u32]` 버퍼·좌표만 다룸. `dispatch_click`(+ `find_file_tree`/`find_explorer`)만 `main.rs`에 free fn으로 있어 lib로 옮기면 공용 가능.
- **layout의 echo-app fallback.** `compositor/src/layout.rs::layout`은 Desktop 루트가 없으면 "기존 echo-app 호환" 경로로 그린다. 즉 echo-app 트리(Desktop 루트 없음)도 정상 렌더된다.
- **VM 현황.** init은 `geulosd` + `echo-app` + (현재) `geulos-vm-skeleton`을 spawn. echo-app은 Container/Text("count: N")/Button("+1", method `press`)를 mount. desktop-shell은 VM에 없다(조각 B 대상).

## 결정 (브레인스토밍 2026-05-29)

| 항목 | 선택 | 이유 |
|---|---|---|
| server_client winit 분리 | **mpsc 알림 채널** — `proxy` → `mpsc::UnboundedSender<UserEvent>` | 기존 채널 방식과 일관, 변경 최소, race-fix 로직 480줄을 호스트·VM 공유(DRY). 제네릭 trait·복제 대비 단순. |
| VM 컴포지터 위치 | **현 skeleton bin을 교체** | `/dev/fb0`는 단일 소유자 — skeleton과 VM 컴포지터가 동시에 fb0에 그릴 수 없음. skeleton은 역할을 다했으므로 교체. |
| A의 입력 범위 | **좌클릭만** (`hit_test`→`dispatch_click`→Invoke) | 키보드/스크롤/IME는 조각 C. 클릭 왕복만으로 입력 경로 증명 충분. |
| 렌더 타이밍 | **매 프레임 always-redraw** (~60fps) | echo-app 3객체라 비용 무시. event-driven 최적화는 후속. |

## 비-목표 (이번 범위 밖)

- desktop-shell을 VM에 올리기 — **조각 B**.
- 키보드 입력·CLI·메모장 편집·스크롤·드래그·한글 IME — **조각 C**.
- 더블 버퍼링/페이지 플립, 부분 렌더 최적화 — 후속.
- skeleton bin 보존 — 교체(필요 시 git 히스토리에 남음).

## 성공 기준

1. VM 부팅(`launch.ps1 -Graphics`) → QEMU 창에 echo-app UI가 **`render_frame`으로** 그려짐: 컨테이너 박스 + "count: 0" 텍스트 + "+1" 버튼.
2. 버튼을 클릭하면 **count가 증가**해 화면에 반영("count: 1", "count: 2", …).
3. 직렬 로그에 컴포지터 접속·트리 수신·클릭 Invoke 로그.
4. **호스트 컴포지터 무회귀**: `cargo run -p geulos-launcher`로 호스트 데스크톱이 기존과 동일하게 동작.

**합격 판정**: 사용자가 QEMU 창에서 (1)을 보고 (2)를 확인(클릭 시 숫자 증가). 시각 확인은 사용자 눈.

## Architecture

### 1. server_client winit 분리 (`compositor/src/server_client.rs`)

- `run_server_client`의 시그니처: `proxy: Arc<EventLoopProxy<UserEvent>>` → `notify: tokio::sync::mpsc::UnboundedSender<UserEvent>`.
- 본문의 `proxy.send_event(UserEvent::Redraw)` → `let _ = notify.send(UserEvent::Redraw);` (4곳), `UserEvent::Quit`도 동일.
- `use winit::event_loop::EventLoopProxy;` 제거. `UserEvent` enum은 server_client에 유지(또는 messages로 이동 — 구현 재량).
- `compositor/src/lib.rs`: `#[cfg(not(target_os = "linux"))] pub mod server_client;` → cfg 게이트 제거(`pub mod server_client;`) — 모든 타겟 노출.

### 2. dispatch_click을 lib로 이동 (`compositor/src/dispatch.rs` 신규)

- `main.rs`의 `dispatch_click(tree, target, obj, role) -> Vec<UiAction>` + 헬퍼 `find_file_tree`, `find_explorer`를 신규 `pub` 모듈 `dispatch`로 이동. `lib.rs`에 `pub mod dispatch;` 추가.
- `main.rs`는 `use geulos_compositor::dispatch::dispatch_click;`로 사용(기존 동작 동일).

### 3. 호스트 main.rs 적응 (`compositor/src/main.rs`)

- `proxy`로 직접 `run_server_client`에 넘기던 것을: mpsc 채널 `(notify_tx, mut notify_rx)` 생성 → `run_server_client(.., notify_tx)`. 별 스레드/태스크에서 `while let Some(ev) = notify_rx.recv().await { proxy.send_event(ev) }` forwarder.
- `dispatch_click` 정의 제거(이제 lib에서 import).
- 그 외 동작 변경 없음.

### 4. VM 컴포지터 bin (`compositor/src/bin/geulos-vm-skeleton.rs` 교체 → `geulos-vm-compositor.rs` 신규, skeleton 삭제)

`cfg(not(linux))` stub main + `cfg(linux)` 본체:
- `Arc<Mutex<TreeModel>>` 공유.
- tokio 스레드 A: `run_server_client("127.0.0.1:5550", event_tx, ui_rx, notify_tx)`.
- tokio 스레드 B(또는 동일 런타임): `while let Some(ev) = event_rx.recv() { tree.upsert/remove/set_state }`. `notify_rx`의 `Quit` 수신 시 종료 플래그(`AtomicBool`).
- 메인 루프(동기): `vm_input` poll → 좌클릭 시 `hit_test(&tree, &layout, x, y)` → `dispatch_click` → `ui_tx`; 매 프레임 `layout(&tree, w, h)` + `render_frame(&tree, &layout, &mut canvas, w, h, &cli_state_default, None)` → `fb.present`. 종료 플래그 set이면 break.
- `render_frame`은 `cli_state: &CliLocalState`와 `editor: Option<&EditorState>`를 받음 — A에선 `CliLocalState::default()` + `None` 전달(CLI/editor 없음).

### 5. init / build.ps1 (`geulos-init/src/spawn.rs`, `boot/build.ps1`)

- `spawn.rs`: `/bin/geulos-vm-skeleton` → `/bin/geulos-vm-compositor`. 인자로 서버 주소 `127.0.0.1:5550` 전달(또는 기본값).
- `build.ps1`: `--bin geulos-vm-skeleton` → `--bin geulos-vm-compositor`, stage 복사 경로 동일 갱신.

## 데이터 흐름

```
geulosd ──TCP── server_client(tokio) ── event_tx ──> tree thread ──> Arc<Mutex<TreeModel>>
                       ^                                                      |
                       | ui_tx (Invoke)                          메인 루프 layout+render_frame
                       |                                                      v
메인 루프 evdev 좌클릭 ─ hit_test ─ dispatch_click ──────────────────────> /dev/fb0
                                                                              ↑ present
클릭 → Invoke(press) → geulosd → echo-app count++ → StateSet broadcast → server_client
   → tree.set_state → 다음 프레임에 "count: N" 갱신 렌더
```

## 에러 처리

- 서버 접속 실패 → 직렬 로그(`[vm-compositor] connect 실패`) 후 종료(exit code). init이 살아있어 시스템은 유지.
- `Disconnected`(서버 종료) → 메인 루프 종료.
- fb/evdev 실패 → 기존 skeleton과 동일(명확한 메시지 + exit).

## 테스트/검증

- **server_client 리팩터 무회귀**: 호스트 `cargo build -p geulos-compositor` + `cargo test --workspace` 통과(특히 `std_types_query_coverage_smoke`). 가능하면 호스트 컴포지터 수동 실행으로 동작 확인.
- **VM 컴포지터**: musl 크로스 컴파일 통과. 부팅 시각 확인 — echo-app UI 렌더 + 버튼 클릭 시 count 증가. 직렬 로그로 접속·Invoke 확인.
- 순수 로직 변경 거의 없음(이동 위주)이라 신규 단위 테스트 최소. 기존 테스트 유지가 핵심.

## 위험

- `render_frame`이 fontdue 폰트로 텍스트를 그림 — 그 폰트가 musl 빌드에 컴파일타임 embed되는지 확인 필요(이미 image/fontdue가 musl로 빌드됨 → 가능성 높음). 폰트 미포함 시 텍스트 안 보임 → "count" 숫자 변화 확인이 어려움(버튼 색/위치로 보조 가능).
- echo-app fallback layout이 fb 해상도(1280x800)에서 적절히 배치되는지 — 너무 작거나 한쪽에 몰릴 수 있음(시각 확인에서 드러남, v1 허용).
- 매 프레임 전체 렌더 — echo-app은 무시 가능. 조각 B(큰 트리)에서 재검토.

## 후속 (이 스펙 이후)

- **조각 B**: desktop-shell을 VM에 (musl 크로스 컴파일 — notify/reqwest 이슈 해결 + spawn).
- **조각 C**: 키보드/CLI/editor/스크롤/IME 입력 완전 포팅.
