# Known Issues / 추적 중인 부채

GeulOS 진행 중 누적된 *알려진 한계, 임시 우회, 보안 부채*. 각 항목은 *언제 해소되어야 하는가*가 명시되어 있다.

이 파일은 정기적으로 정리되어야 한다. 누적되면 신호 — 새 마일스톤 시작 전에 검토.

---

## 🔴 보안 부채 (해소 필수)

### KI-001 — echo-app의 wildcard ACL

- **언제 들어왔나:** M3-T7 (echo-app 실구현). build_ui에서 button에 `ActorPattern::Wildcard + MethodPattern::Wildcard + AclEffect::Allow` 추가.
- **왜:** M3 acceptance(외부 클라이언트 → press → count 증가) 통과 위해. 사용자 동의 다이얼로그가 없는 M3 단계의 *임시 우회*.
- **무엇이 문제:** 앱이 *명시적으로 자기 보안을 약화*시키는 패턴. Android `exported=true` 실수와 동형. 신뢰 영역 T0~T5 (설계 §7.1)와 보안 불변식 S1~S6 (§7.8)에 직접 충돌.
- **언제 해소:** **M5 진입 전.** 사용자 동의 다이얼로그가 어디서 도입되든(M5 글 AI 드라이버 + 권한 ladder 또는 별도 M4.5), wildcard ACL을 *즉시 제거*하고 명시적 grant로 교체.
- **검증 방법:** acceptance 테스트가 *권한 다이얼로그 통과 후에만* 외부 invoke 성공하도록 변경. wildcard 검색 grep 통과 = 보안 회귀.

### KI-002 — 매니페스트 `permissions` 선언만 받고 실제 강제 안 함

- **언제 들어왔나:** M3 (앱 런타임).
- **왜:** ADR-012로 *명시 연기* (사용자 동의 UI는 M4 컴포지터 도착 후). M3에서는 매니페스트의 `permissions` 배열을 받기만 함.
- **무엇이 문제:** `fs.user.docs`, `clipboard.read` 같은 권한 카테고리가 *아무 의미 없음*. 앱이 선언만 하고 실제로는 어떤 권한도 받지 않은 채 동작.
- **언제 해소:** M5에 FS/clipboard API 도입 시 권한 카테고리와 *실제 강제* 연결.
- **검증 방법:** 권한 미선언 앱이 `fs.user.docs` API 호출 시 거부됨을 통합 테스트로.

---

## 🟡 기능 한계 (UX 영향 있으나 보안 무관)

### KI-003 — `query owner ai:<uuid>` 정확 매칭 불가

- **언제 들어왔나:** M2 (와이어 프로토콜).
- **상황:** geulosh의 `query owner <actor>` 및 server-host의 `handle_query`에서 `user:local`과 `system:compositor`만 정확 매칭. `ai:<uuid>`나 `app:<id>:<uuid>` 패턴은 fallback으로 `local_user()` 사용 → 항상 0건 반환.
- **왜:** `ActorId::from_str` 부재 시절의 임시 우회. M3에서 `ActorId::from_str`가 도입되었으나 *기존 코드가 이를 사용하도록 갱신되지 않음*.
- **언제 해소:** M5 plan 작성 시 또는 별 PR. `dispatch::handle_query`의 `parse_actor_for_query`를 `ActorId::from_str` 호출로 교체.
- **검증:** `query owner ai:<known-uuid>`로 정확히 그 액터 소유 객체만 반환되는지.

### KI-004 — ✅ 컴포지터가 동적 mount된 객체를 못 봄 (해소됨)

- **언제 들어왔나:** M4 (컴포지터).
- **언제 발견:** M7 T7 이후 부채로 인식, M8 T8.10까지 진행 후 *폴더 expand 후 자식 안 보임* 증상으로 사용자 시각 확인.
- **언제 해소:** 2026-05-18 (M8 회귀 fix #1).
- **변경 내용 (type-level subscribe 도입):**
  - `core::server::subscribe::SubscriptionTarget` enum 신설 — `ById(ObjectId)` / `ByType(TypeUri)`.
  - `ObjectServer::subscribe_by_type` public API 추가 (기존 `subscribe`는 그대로 → ID-based 호환).
  - `SubscriptionManager::deliver`가 `ev_type_uri: Option<&TypeUri>` 인자를 받아 ByType 매칭. 호출자(mount/invoke/set_state/emit_destroyed)는 emit 직전 type_uri를 캐싱.
  - wire 프로토콜: `SubscribeMsg.target`이 `"type:<uri>"` prefix면 type-level. ID-based UUID는 기존대로 (server-host/connection.rs::handle Subscribe 분기).
  - server-host actor에 `SubscribeByType` Command + handle 추가.
  - 컴포지터(`compositor/src/server_client.rs`):
    - startup 시 STD_TYPES 각각에 `format!("type:{}", t)` 으로 type-level Lifecycle 구독 추가.
    - `handle_server_frame`에 `Lifecycle::Created` 분기 — Get으로 본문 fetch + 그 ID에 ID-based subscribe (Invoke/StateSet/Lifecycle) 추가 등록.
- **검증:**
  - 신규 회귀 테스트 `core/tests/server_subscribe_test.rs::subscribe_by_type_receives_created_for_future_mounts` 등 4건 통과.
  - 컴포지터 시작 후 desktop-shell이 lazy_mount로 새 Folder/File을 만들면 자동으로 트리에 반영 (폴더 expand 후 자식 노드 표시).
- **남은 부채:** Get/Subscribe 응답 대기 중 server-host의 push task(100ms 간격)가 다른 이벤트 프레임을 그 사이에 push할 수 있는 *이론적* race window 존재. → **KI-013으로 분리 + 2026-05-18 해소.**

### KI-005 — ✅ `include_initial: true` 미구현 (의도된 효과로 해소)

- **언제 들어왔나:** M2.
- **상황:** Subscribe 메시지의 `include_initial` 플래그가 *받지만 무시됨*. mount 시점에 발행된 Lifecycle::Created 이벤트는 *항상* 후속 구독자에게 전달 안 됨.
- **언제 해소:** 2026-05-18 (KI-004과 동시).
- **해소 방식:** 플래그 자체는 여전히 *무시*되지만, *원래 의도된 효과* (구독 후 이전 상태도 받는 것)가 두 메커니즘으로 분할되어 제공:
  - *컴포지터 startup 시점까지의* 객체: STD_TYPES Query → Get (기존 path).
  - *그 후 mount되는* 객체: type-level Subscribe → Created 도착 시 Get (KI-004 해소).
- **남은 부채:** 진정한 `include_initial=true` 단일 와이어 메시지로의 통합은 v2 (M9+). 현재는 client가 두 단계로 명시적으로 수행해야 함 — 컴포지터 외 클라이언트는 자체 처리 필요.

### KI-006 — geulosh `--connect` 모드의 명령 제한

- **언제 들어왔나:** M2-T8 (geulosh remote 모드).
- **상황:** remote 모드(--connect)에서 mount/invoke/ls 세 명령만 동작. subscribe/get/events/tree/drain은 in-process 모드 전용.
- **언제 해소:** 향후 PR — RemoteShell이 더 많은 명령 지원하도록 확장.
- **검증:** remote 모드에서 m1_smoke.gsh 시나리오 통과.

---

## 🟢 정보용 (즉시 행동 불필요)

### KI-007 — fontdue로 GPU 가속 없음

- **언제:** M4 (ADR-013로 의도적).
- **상황:** softbuffer + fontdue = CPU 픽셀 그리기. 큰 트리에서 느림.
- **언제 해소:** M6 또는 별 PR로 wgpu로 백엔드 교체 검토 (ADR-013 §"중립" 참고).

### KI-008 — Single-thread tokio runtime in compositor

- **언제:** M4-T8.
- **상황:** compositor의 두 thread 모두 `Builder::new_current_thread()`. 많은 동시 연결 시 비효율.
- **언제 해소:** 다중 클라이언트 시나리오 (M5+) 도입 시 multi_thread runtime으로.

### KI-009 — 컴포지터 폰트 파일 외부 의존성

- **언제:** M4-T6.
- **상황:** `compositor/fonts/font.ttf`는 .gitignored. 사용자가 직접 시스템 폰트(arial.ttf 등) 복사 필요. CI는 DejaVu 폰트로 fallback.
- **언제 해소:** OFL 라이선스 폰트(예: JetBrains Mono, Noto Sans KR Regular) 임베드 결정.

### KI-010 — ✅ echo-app 60초 idle 자동 종료 (해소됨)

- **언제 들어왔나:** M3-T7.
- **언제 발견:** 2026-05-17 ai-probe 시나리오 03 (multi_press) 실행 중.
- **증상:** 5번 press 모두 wire 응답은 `ok: true`인데 Text 객체는 `"count: 1"`에 박혀 변하지 않음. 사용자/AI가 *유령 상호작용*을 경험.
- **원인:** echo-app의 메인 이벤트 루프가 `tokio::time::timeout(60s, ...)`로 감싸여 있어 60초 동안 입력 없으면 자동 종료. 종료 후에도 *서버 측 객체는 그대로 남아 있어* (KI-011 참고) press 호출은 성공하지만 reactor가 없는 상태.
- **해소:** echo-app/src/main.rs의 read 루프에서 `tokio::time::timeout` wrapper 제거. 연결이 끊기거나 read 에러 시까지 계속 실행.
- **검증:** ai-probe 시나리오 03 재실행 → Text가 `"count: 1"`, `"count: 2"`, ..., `"count: 6"`으로 진행되어야 함 (이전 press 합산).

### KI-011 — ✅ `emit_destroyed`가 객체를 *실제로 제거*하지 않음 (해소됨)

- **언제 들어왔나:** M3-T6 (앱 라이프사이클).
- **언제 발견:** KI-010 진단 중 부수적으로 발견.
- **언제 해소:** 2026-05-17. KI-011 (b) tombstone 방식 채택.
- **변경 내용:**
  - `Object`에 `destroyed: bool` 필드 추가 (#[serde(default)]로 기존 JSON 호환).
  - `ObjectServer::emit_destroyed`가 객체 플래그를 `true`로 세팅 + Destroyed 이벤트 발행.
  - `query()`가 destroyed 객체 제외 (ByType/ByOwner/ChildrenOf 모두).
  - `roots()` 반환 타입이 `&[ObjectId]` → `Vec<ObjectId>`로 변경, destroyed 제외.
  - `invoke()`·`set_state()`가 tombstone에 대해 NotFound 반환.
  - `get()`은 그대로 — 호출자가 `destroyed` 플래그로 시각화 결정 가능.
- **검증:** `core/tests/tombstone_test.rs` 6개 회귀 테스트. ai-probe 시나리오 03 재실행 시 유령 객체가 더 이상 안 나타나야 함.

---

## 절차 부채

### KI-P1 — Subagent 디스패치가 자동 push까지 수행

- **언제:** M4 진행 중 발견.
- **상황:** subagent들이 controller의 명시적 지시 없이도 *자기 판단으로 git push까지* 수행. M4 진행 중 CI 실패 cascade와 모바일 알림 폭주의 원인.
- **언제 해소:** 향후 모든 subagent 디스패치 프롬프트에 *"NEVER push. Only commit. Controller will batch push at end."* 명시. 또는 controller가 *commit + push 단계는 직접* 처리.
- **검증:** subagent 보고에 push 행위 없음.

### KI-P2 — 의존성 핀 추가 시 사후 추적 부족

- **언제:** M1.5/M2/M3에서 3번 누적 후 발견.
- **규칙:** 향후 *cargo update --precise X* 또는 hand-rolled 우회를 1번 도입할 때마다 즉시 본 파일에 *왜 핀했는지 + 핀 해제 트리거*를 기록. 2개 누적되면 즉시 toolchain/dep 점검.

### KI-012 — ✅ Alpine 커널의 모듈식 NIC 설계 vs 우리 최소 initrd (해소됨)

- **언제 들어왔나:** M6 acceptance 작업 중 (2026-05-17~18).
- **상황:** Alpine `vmlinuz-virt`/`vmlinuz-lts` 둘 다 모든 NIC 드라이버(virtio_net, e1000, ...)를 *모듈*로 빌드. 우리 initrd엔 모듈 0개 → PCI에 NIC 디바이스가 보여도 바인딩할 드라이버가 없음 → `eth0`/`enp0s3` 인터페이스 미생성 → 외부 ai-bridge 접속 불가. ADR-005("AI는 모든 배치 토폴로지 지원")와 충돌.
- **해소:** M6.5 마일스톤. ADR-017 결정 후:
  - `boot/modules/fetch.ps1` — Alpine `linux-lts-X.Y.Z-rN.apk` 다운로드 + `e1000.ko` 추출
  - `boot/build.ps1` — initrd staging에 `/lib/modules/<kernel>/` 포함
  - `geulos-init/src/modules.rs` — `finit_module(2)` syscall 직접 호출로 .ko 적재
  - main 흐름: mount → **modules** → network → spawn
- **검증:** 2026-05-18 부팅 콘솔에 `e1000 0000:00:03.0 eth0: ... Link is Up 1000 Mbps`, `[init] eth0 UP (10.0.2.15/24)` 출력. 호스트의 ai-bridge가 `127.0.0.1:5550`(forwarded) → VM의 echo-app 3개 객체 발견 + report_done 성공 (3 turns / 15.1s / claude-sonnet-4-6).
- **향후 영향:** Phase D(virtio-gpu/input)와 Phase E(virtio-blk 영속성)도 같은 메커니즘 재사용 가능. `LOAD_ORDER` 상수에 모듈 이름 추가만으로 확장.

### KI-014 — ✅ CLI 한글 입력 무반응 (해소됨)

- **언제 들어왔나:** M7 T7.5 (ASCII v1). `compositor/src/main.rs::key_event_to_action`가
  `KeyboardInput.text`의 multi-char 케이스를 무조건 무시 + winit IME 채널 미활성화.
- **언제 해소:** 2026-05-18 (M7 T7.6, ADR-029).
- **무엇이 문제였나:** 한글 자판을 눌러도 화면에 아무 글자도 나타나지 않아 한국 사용자의
  도그푸딩이 *즉시 차단*. AI에 한국어 prompt를 보낼 방법 부재.
- **해소 방식:**
  - `compositor/src/main.rs::App::resumed`에서 `Window::set_ime_allowed(true)` 호출.
  - `WindowEvent::Ime(Ime::Preedit / Commit / Enabled / Disabled)` 핸들러 추가 — Windows TSF가
    winit를 통해 emit. `KeyboardFocus::Cli`일 때만 cli_state에 반영, Window/None focus에서는
    완전 무시.
  - `keyboard::CliLocalState`에 `preedit_text: String` 필드 + `handle_ime_preedit/commit`
    메서드 추가.
  - `render::render_cli`에서 preedit를 `input_buffer` 끝에 회색(`#888888`)으로 시각화.
  - T7.5의 `// TODO(T7.6): IME pre-edit 다중 문자 처리` 주석 마커 제거 (이제 IME 채널이 cover).
- **남은 부채:**
  - **Preedit cursor 위치 미반영 (v2 개선):** preedit가 cursor 위치와 무관하게 `input_buffer`
    *끝*에 그려진다 — 사용자가 cursor를 중간으로 옮긴 채 IME 입력 시 preedit가 끝에 표시되어
    혼란. v2에서 cursor 위치에 preedit 삽입 + cursor 자체를 preedit 내부 byte offset으로 이동.
  - **비-Windows 플랫폼:** winit IME는 Wayland(IBus/Fcitx 의존)·macOS(InputMethod)에서
    환경에 따라 동작 차이. 후속 마일스톤에서 검증. Fallback은 clipboard paste
    (Ctrl+V — T7.10에서 `arboard` crate로 일차 구현, 모든 mode에서 작동. M9에서 OS-level
    클립보드 권한 매니페스트로 정식화 예정).

### KI-015 — T7.9 awaiting 모드 잔존 key가 *과거* chat history에 남아있을 가능성

- **언제 들어왔나:** M7 T7.9 (ADR-032 awaiting_api_key 모드).
- **언제 발견 / 부분 해소:** 2026-05-18 (M7 T7.10). 사용자 보고: "가장 오래된 대화
  내용 출력해줘" → AI가 lines에 있던 key 발견 후 윤리 거부.
- **무엇이 문제였나:** T7.9 awaiting 분기에서 사용자가 입력한 API key 본문이
  `handle_cli_outcome`의 `input_echo`로 `Cli.state.lines`에 push됨 → AI tool
  `get_object(cli)`가 lines를 fetch할 경우 key가 그대로 노출.
- **T7.10 fix:** awaiting 모드의 *모든* `handle_cli_outcome` 호출처(cancel/검증
  실패/성공)에서 `input_echo = ""` 명시. 검증 결과 메시지(`[저장됨 ~/.geulos/api_key]`
  / `[검증 실패: ...]` / `(취소 — 셸 모드로 복귀)`)는 `output_lines`로 정상 표시.
  결과: 향후 awaiting 입력은 *어디에도 lines에 등장 안 함*.
- **남은 부채 (사용자 조치 필요):**
  - T7.10 fix *전*에 사용자가 awaiting 모드로 key를 입력했다면 그 시점 *동일 세션의
    chat history* (`~/.geulos/sessions/<name>.json`)에 *AI tool이 fetch한 lines 응답*
    형태로 key가 들어가 있을 수 있다. 본 fix는 history를 *수정하지 않는다* (idempotent
    code-only fix). 권장: `/ai start <새이름>` 로 새 세션 시작 또는 영향받은 세션 파일
    수동 삭제.
  - 영향 평가: 단순 *입력 직후* AI에게 prompt를 보내지 않았다면 lines는 그 process
    종료로 휘발 — history 영향 없음. AI에게 `get_object(cli)`를 명시 요청한 경우만
    history에 남음.
- **검증:** awaiting 진입 → 가짜 key 입력 → /ai exit → `Cli.state.lines`에 key
  바이트가 없는지 확인 (T7.10 부정 회귀 테스트는 v2에서 별 통합 테스트로 추가 검토).

### KI-013 — ✅ compositor handle_server_frame의 Get/Event interleave race (해소됨)

- **언제 들어왔나:** M4 (컴포지터). KI-004 fix (2026-05-18, commit `2f25e73`)로 Created 분기에 동기 Get+Subscribe round-trip이 들어오며 *명시화*. 그러나 race window 자체는 M4 시점부터 존재.
- **언제 발견:** 2026-05-18. KI-004 해소 후 *폴더 expand 시 자식 안 보임* 증상이 사용자 시각 검증에서 재현 (UX fix commit `83d5097`는 정상이나 자식 자체가 안 보이니 효과 X).
- **언제 해소:** 2026-05-18 (M8 회귀 fix #3).
- **무엇이 문제였나:**
  - `handle_server_frame::Lifecycle::Created` 분기가 Get을 보낸 직후 `read_typed::<GetResult>`로 *그 자리에서 stream.read* → server-host의 push task(100ms 간격)가 큐된 *모든 이벤트를 한꺼번에 drain*해 EventMsg를 연속 push.
  - 결과: Get 송신 직후 다음 frame이 EventMsg(kind="Event")면 GetResult deserialize 실패 → 그 객체가 *영영 트리에 안 들어옴*. 폴더에 자식 N개 mount 시 첫 1개만 처리되더라도 후속 N-1개가 모두 lost.
  - dyn Subscribe ack 대기에서도 동형 race.
- **변경 내용 (fire-and-forget + GetResult/GetError 분기):**
  - `compositor/src/server_client.rs`:
    - Created 분기는 Get *송신만* 수행, 응답 대기 X. `pending_gets: HashMap<String, ObjectId>`에 request_id → target_id 저장 후 즉시 return.
    - `handle_server_frame`에 `GetResult` 분기 신설 — request_id로 pending_gets lookup → Object deserialize → ObjectUpserted send + dyn ID-subscribe 송신 (ack 대기 X).
    - `GetError` 분기 — pending entry cleanup (메모리 누수 방지).
    - 알 수 없는 kind는 `_ => {}`로 silent drop (SubscribeAck/MountAck 등 모두 안전).
    - `Event` 분기는 `handle_event_frame` 별 함수로 추출.
    - `handle_server_frame` 시그니처에서 `accum`/`buf` 인자 제거 (내부 stream.read 안 함). 대신 `pending_gets` 추가.
- **검증:**
  - cargo build/test/fmt/clippy 모두 클린 (workspace 전체).
  - 모든 frame이 select! loop의 stream.read만으로 순차 도착 → handle_server_frame은 frame 하나만 처리 후 return. interleave 가능성 *구조적으로 차단*.
- **남은 부채:** 없음. 향후 만일 server-host가 wire 응답 우선순위/순서 보장을 추가 정밀화하더라도 client side는 이 fire-and-forget pattern으로 robust.

---

## 정기 검토 시점

- **3개월 (2026-08-17):** M4 후속 정리. KI-001/004/005 우선순위.
- **6개월 (2026-11-17):** M5 작업 중. KI-001/002 해소 확인.
- **12개월 (2027-05-17):** 전체 회고. 미해소 항목 정리.
