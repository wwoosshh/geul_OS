# Known Issues / 추적 중인 부채

GeulOS 진행 중 누적된 *알려진 한계, 임시 우회, 보안 부채*. 각 항목은 *언제 해소되어야 하는가*가 명시되어 있다.

이 파일은 정기적으로 정리되어야 한다. 누적되면 신호 — 새 마일스톤 시작 전에 검토.

## 마일스톤 종료 시점

- **M8 정식 마감 (2026-05-20):** T8.0~T8.20 + 6 회귀 fix 완료. T8.11 통합 acceptance
  통과 (commit `242f57f`). T8.12 final review + dead-code cleanup 통과 — `cargo
  test --all` 35 binary 모두 통과 / `clippy -D warnings` 클린 / `fmt --check` 클린.
  잔여 dead modules (workspace.rs / scan.rs / fs_ops.rs / invoke_handler::handle_canvas_set_file
  + handle_file_tree_select / layout::layout_tree_node)는 `#[allow(dead_code)]` + M9
  재활용 메모로 *보존 정책 일관* 유지. 보안 부채 KI-001 / KI-016는 M9 진입 시 일괄
  해소 예정.
- **M9 정식 마감 (2026-05-22):** 편집/저장 + 권한 Dialog 인프라 완료. PendingFs/PendingEntry
  enum 7 variants + judge_with_path(actor, op, dir, granted)로 AI write 동의 모델 도착.
  Window.save_to_file은 UI direct action으로 권한 면제, AI invoke는 GrantedDirs 캐시 +
  Dialog modal. ai-bridge system_prompt 한국어 unified (standalone + CLI chat). 회귀: AI
  Object.children stale (mount.rs auto-push) / 외부 rename 누락 (notify-rs Modify::Name 분기) /
  Dialog 매 호출 발생 (path 기반 grant) / Delete tombstone broadcast 누락 (extra state_sets).
  보안 부채 KI-001/016는 **여전히 미이행** — M9 spec에 ACL 교체 task가 포함되지 않아 이월.
- **M10 정식 마감 (2026-05-23):** Object-native filesystem CRUD 완료. Phase 1 객체 메서드
  (Folder.create_file/create_folder/delete/rename + File.read/delete/rename + Filesystem@1
  escape hatch) / Phase 2 file_watcher mpsc + 100ms polling tick / Phase 3 Filesystem@1
  AI 도구 (read_external/write_external). geulos-launcher 신설 — 단일 binary로 geulosd +
  desktop-shell + compositor 서브프로세스 + 로그 forwarding. main.rs 2882→1279줄 refactor
  (handlers/ 모듈 분리, submit_input + ai_session 결합부만 main 잔존). 회귀: 스크롤
  PixelDelta 먹통 (float accumulator) / navigate_to 빈 폴더 watcher 누락 / Explorer scroll_y
  잔존 (navigate에서 0 reset) / navigate active_folder race (local 동기 갱신, commit `ea39b61`).
  KI-001/016는 *이번 마감에서도 미이행* — 별 마일스톤(M11+ 보안 강화) 명시 필요.
- **M11 정식 마감 (2026-05-23):** KI-001 / KI-016 해소. desktop-shell의
  wildcard ACL 16곳 (+ scan.rs dead code 2곳)을 객체 타입별 typed helper로
  교체. AllowIfGrantedDir 새 AclEffect로 AI path-aware grant 도입. GrantUpdate
  wire 메시지로 desktop-shell ↔ server GrantStore 동기. set_state ACL이
  invoke와 동일 평가 경로로 통일. ADR-037 참조. 정기 manual acceptance는
  `docs/manual-tests/m11-acceptance.md` 시나리오 12개.
- **M11.1 정식 마감 (2026-05-26):** AI 비동기 흐름 + JSONL 대화 로그.
  desktop-shell submit_input의 AI dispatch를 tokio::spawn + mpsc channel +
  main select! arm으로 분리. chat_session을 Arc<tokio::sync::Mutex>로 wrap.
  즉시 echo + sentinel "(응답 대기 중...)" 표시 → 응답 도착 시 sentinel
  제거. AI 응답 대기 중 UI 멈춤 해소. 빈 응답도 명시 피드백 (code review I-1
  fix).
  ai-bridge ChatSession::audit를 JSONL event 형식으로 전환 (user_prompt/
  ai_text/tool_call/tool_result/tool_error/report_done/end_turn/send_done
  8 종류 + latency_ms 포함). CliChatSession::start/load가 자동으로
  ~/.geulos/logs/ai-chat/<session>-<ts>.jsonl 활성. ADR-038.
- **M11.1 후속 진단 세션 (2026-05-26):** 사용자가 첫 manual 사용 ("D:/GeulOS
  README.md 요약") 시도 → max_inner_turns 도달로 실패. JSONL audit가 *진단
  인프라*로 정확히 작동, 4개의 누적 버그 root cause를 *코드 한 줄까지* 짚어
  4건 연속 fix:
  1. `a50f11f` — `read_external` cwd-inside silent fail. 기존엔 `eprintln` +
     empty outcome으로 wire 응답 ok:true이나 state 갱신 0 → AI가 도구 동작
     여부만 알고 이유 추측. Fix: state.last_read_content에 명시 ERROR 메시지
     SetState broadcast.
  2. `ba84219` — M11 T8 `add_filesystem_acl`이 다른 4 helper와 달리
     `App("desktop-shell") + SetState` entry 누락 → 위 fix의 SetState wire가
     ACL에서 PermissionDenied로 *조용히* 거부. Fix: entry 추가.
  3. `2303b11` — `add_fs_object_acl`이 모든 AI method를 AllowIfGrantedDir로
     처리 → list/read 같은 read-only도 grant 없으면 거부 → AI catch-22
     (grant는 mutation Dialog로만 받는데 list 자체가 막힘). Fix: read-only
     (list/read)는 Allow, mutation만 AllowIfGrantedDir로 분리.
  4. `7eaafb6` — `list_objects_by_type`이 ID 배열만 반환 → AI가 path
     매칭을 위해 get_object를 N번 호출 → turn 폭주. Fix: 결과에 각 객체의
     `{id, type_uri, name, path}` summary inline. 한 호출로 path 매칭 가능.

  fix 후 test5 시나리오 — AI가 8 turn 안에 cwd 안 README.md 발견 →
  File.read → state.content (7871 bytes) 수신 → 한국어 요약 작성 →
  report_done 정상 종료. **M11.1 JSONL 진단의 실전 가치 증명**.
- **M11.2 fix + controller-as-tester PoC (2026-05-26):** 사용자 비전
  *"외부 client = AI = 사용자 = 동일 wire protocol"*의 직접 시연. controller가
  `ai-bridge/examples/auto_crud_demo.rs`로 두 wire connection (Role::Ai +
  Role::Compositor) 동기 실행, 6 stage 자동 검증. 첫 실행 즉시 root cause 발견:
  - **회귀 fix**: M11.1의 AllowIfGrantedDir이 AI mutation을 server-level에서
    즉시 차단 → desktop-shell handler 도달 X → Dialog mount 자체 안 됨 →
    catch-22 (grant는 Dialog로만 받는데 Dialog가 안 뜸). `add_fs_object_acl`을
    M9 원래 모델로 복원 (AiSession Wildcard Allow + handler가 path-aware
    Dialog 흐름). commit `a0e99ad`.
  - **검증 통과**: 실제 D:/에 sandbox Folder 생성 → 삭제 wire-level 자동
    실행, KI-001 차단 (AI Dialog.respond → PermissionDenied) wire-level 실측,
    ACL spec 일치 62 mounted 객체 검증.
  - **신규 KI**: 아래 KI-022/023/024.

- **M12 정식 마감 (2026-05-26):** ShellRunner@1 escape hatch 도입. AI/사용자가
  화이트리스트 binary (git/npm/yarn/pnpm/npx/cargo/rustc/docker/node/python/pip)를
  Dialog 동의 후 실행. tokio::process::Command (fork+execve, shell injection 무관)
  + 120초 timeout + 8 state SetState broadcast. PendingFs::ShellRun variant +
  dialog_methods 정식 arm. ADR-039. 검증: auto_react_project example로 npx
  create-vite → react 프로젝트 자동 생성 end-to-end (T7).
  후속: M13 typed Process Objects (GitRepo@1/NpmProject@1/CargoProject@1),
  M14 container 격리 환경.

### 신규 발견 (M11.2 진단 세션)

#### KI-022 — delete 후 server-side `destroyed` flag 미반영

- **언제 발견:** 2026-05-26 (auto_crud_demo cleanup 단계).
- **상황:** desktop-shell handler가 *실제 fs::remove_dir* 호출 + `[desktop-shell] AI delete_folder 승인` 로그 출력. 그러나 후속 `get_object`로 객체 확인 시 `destroyed: false`. 실제 디스크는 비어있음 (검증).
- **원인 가설:** delete handler가 *fs 동작은 수행*하나 *server.emit_destroyed* (KI-011 tombstone) 호출 누락. 또는 호출하나 `destroyed` flag SetState broadcast 안 됨.
- **영향:** AI/외부 client의 객체 tree view가 *fs 실제와 stale*. UI는 fs_watcher가 별도로 처리하나 *AI는 객체 tree만 봄* → 삭제된 줄 모르고 호출 시도 → NotFound 후속 처리.
- **언제 해소:** M11.2 후속 또는 M12.

#### KI-023 — Dialog.respond field name이 한국어/영문 mixed

- **언제 발견:** 2026-05-26.
- **상황:** Dialog 객체의 `actions` props는 한국어 (`["허용", "거부"]`). handler가 args에서 `action` 필드 읽음 (`args.get("action")`), 값도 한국어 매칭 (`if action == "허용"`). 외부 client/AI가 모르고 `choice` 필드 또는 영문 `allow` 보내면 *기본값 "거부"로 fallback*.
- **영향:** wire spec 모호. system_prompt에 명시 없음. 외부 검증 시 흔히 막힘.
- **언제 해소:** spec 문서화 + 영문/한국어 alias 처리 (e.g. action: "허용"|"거부"|"allow"|"deny" 모두 수락). 작은 fix.

#### KI-024 — 외부 client 인증 부재 (Role::Compositor 자유 발급)

- **언제 발견:** M11 spec 시점 명시 + M11.2 실측.
- **상황:** server-host의 `connection.rs`가 hello.role을 *auth 검증 없이* 그대로 ActorId로 매핑. `Role::Compositor`로 connect하면 *누구나* `system:compositor` 권한 받음. KI-001 wildcard 해소가 *action 자체는 통제*하나 *어떤 actor가 system:compositor를 자처할지*는 미통제.
- **영향:** production에서 *외부 누구나 사용자 동작 임의 시뮬 가능*. 시연/dev OK, production 절대 X.
- **사용자 비전과의 균형:** "외부 = AI = 사용자 동등"이라는 동형성 자체는 *유지*. 인증은 *누가 어느 role을 자처할 권리가 있는지* 검증하는 별 layer.
- **언제 해소:** M12+ 보안 마일스톤. 옵션: Unix socket + file perm / TCP+TLS+token / launcher가 발급한 시스템 token / OS-level capability.

#### KI-025 — PowerShell console에서 desktop-shell log 한국어 깨짐

- **언제 발견:** M11.1 진단 + M11.2 진단 반복.
- **상황:** `~/.geulos/logs/shell.log`는 UTF-8 (정상). PowerShell 5.1 `Get-Content` 기본 인코딩이 CP949 (한국어 Windows) 또는 UTF-16 → UTF-8 한국어가 *깨져 표시*. 파일 자체는 정상. WSL `tail`로는 정상 read.
- **영향:** 진단 UX. log 분석 시 *내가 PowerShell로 read 실패 → bash로 우회*.
- **언제 해소:** PowerShell read 시 `-Encoding UTF8` 명시 또는 `[System.IO.File]::ReadAllBytes` + 직접 decode. 단순 fix, manual-tests 문서에 가이드 추가.

---

## 🔴 보안 부채 (해소 필수)

### KI-001 — ✅ echo-app + desktop-shell wildcard ACL (해소됨)

- **언제 들어왔나:** M3-T7 (echo-app). M8에서 desktop-shell로 확장.
- **언제 해소:** 2026-05-23 (M11 정식 마감). 자세한 내역 ADR-037.
- **변경 요약:** 객체별 typed helper 5개 (add_ui_object/fs_object/dialog/
  filesystem/container_acl)로 wildcard 16곳 일괄 교체. Dialog.respond는
  system:compositor 단독 — 외부 우회 영구 차단. AI는 Filesystem@1 + granted
  dir 안 Folder/File만 통과 (AllowIfGrantedDir effect).
- **검증:** `scripts/check-no-wildcard-acl.{sh,ps1}`, `docs/manual-tests/
  m11-acceptance.md` (12 시나리오).

### KI-002 — 매니페스트 `permissions` 선언만 받고 실제 강제 안 함

- **언제 들어왔나:** M3 (앱 런타임).
- **왜:** ADR-012로 *명시 연기* (사용자 동의 UI는 M4 컴포지터 도착 후). M3에서는 매니페스트의 `permissions` 배열을 받기만 함.
- **무엇이 문제:** `fs.user.docs`, `clipboard.read` 같은 권한 카테고리가 *아무 의미 없음*. 앱이 선언만 하고 실제로는 어떤 권한도 받지 않은 채 동작.
- **언제 해소:** M5에 FS/clipboard API 도입 시 권한 카테고리와 *실제 강제* 연결.
- **검증 방법:** 권한 미선언 앱이 `fs.user.docs` API 호출 시 거부됨을 통합 테스트로.

### KI-016 — ✅ set_state ACL wildcard (해소됨)

- **언제 들어왔나:** M8 T8.19.
- **언제 해소:** 2026-05-23. KI-001과 함께 — set_state ACL 검사가 invoke와
  동일한 `Object::is_allowed(actor, AclOp::SetState(_), &grants)` 평가
  경로로 통일.

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

### KI-017 — Word-wrap 시 char width 휴리스틱 (T8.20)

- **언제 들어왔나:** M8 T8.20. compositor의 viewer 본문 word-wrap이 *14px/char* 가정.
- **상황:** 한글(double-width) + ASCII(single-width) 혼합 텍스트에서 14px/char는 *한글 기준 보수적* 으로 적절하나, ASCII-only 텍스트에서는 한 줄에 들어갈 수 있는 문자보다 *적게* wrap → 우측 여백이 항상 남음. 시각적 비효율은 있으나 *truncate / 깨짐은 없음* (안전).
- **왜:** fontdue로 per-char measure_text_width를 호출하면 정확하나 매 wrap 계산마다 O(N) glyph layout → 큰 파일에서 비용. M8에서는 휴리스틱으로 통과시키고 v2에 정확도 + 캐싱 도입 결정.
- **언제 해소:** v2 (M9 후속 또는 별 task). fontdue per-char measure_text_width + glyph-width 캐시 (HashMap<char, f32>).
- **진행 (2026-05-23 갱신):** M9 part 2에서 `measure_text_width` 정확 계산 도입 (Window 본문 wrap) —
  단 desktop-shell submit 흐름과 큰 트리에는 미적용. 잔여 휴리스틱 위치를 v2에서 정밀화.
- **검증 방법:** ASCII-only 파일이 컴포지터 창 너비를 가득 채우도록 wrap. 한글 mix-text도 화면 가로 한계에서 wrap. 둘 다 truncate 없음.

### KI-018 — SetState 송신 시 local mounted_objects 동기 갱신 정책 (M10 회귀로 노출)

- **언제 들어왔나:** 정책 자체는 M4부터 *암묵적*. M10 작업 중 명시화.
- **언제 발견:** 2026-05-23. Explorer navigate_to/up이 active_folder를 SetState로만 broadcast하고
  *local mounted_objects는 server StateSet 왕복까지 stale*. 사용자가 빠르게 연속 클릭하면
  다음 핸들러가 이전 active_folder를 읽어 잘못된 parent chain을 따라감 (test1 → / 한 번에
  드라이브 일람까지 튐). commit `ea39b61`로 navigate_to/up 양쪽에 동기 갱신 + scroll_y reset 적용.
- **무엇이 문제:** invoke handler가 *outcome.state_sets*를 wire에 push 하면서 local 상태도
  동시에 갱신하지 않으면, 같은 객체의 다른 state(특히 동일 호출이 *읽고 쓰는* state)에
  race window가 열린다. M10의 navigate가 첫 사례이지만 동형 패턴이 다른 핸들러에 잠재.
- **재발 방지 정책:** invoke handler가 *반환할 state_sets에 포함되는 모든 (target, key) 조합*에
  대해 *동일 함수 안에서* `mounted_objects.iter_mut().find().state.insert(...)`로 동기 갱신.
  특히 핸들러가 *direct로 그 state를 read-modify-write* 하는 경우 필수. read-only state
  (e.g. focus / z) 는 risk 낮으나 동일 정책 권장.
- **언제 해소:** 정책 자체는 적용됨 (navigate_to/up). 다른 invoke handler들을 별 PR에서
  audit — 후보: handle_open_file의 focused/z 갱신 (실제로 local 동기 갱신 이미 함, OK),
  handle_window_move/resize/focus/close (window_methods.rs — 확인 필요), CLI handler (특히
  awaiting 모드 SetState 전환).
- **검증 방법:** 회귀 테스트 추가 — `test_navigate_to_then_up_uses_synced_active_folder`
  (1) navigate_to(A) → state_sets에 active_folder=A 포함 *그리고* mounted_objects.active_folder=A,
  (2) 즉시 navigate_up 호출 → A.parent 기반으로 동작.

### KI-020 — AI in-flight 중 chat_session lock 직렬화

- **언제 들어왔나:** M11.1 (2026-05-26).
- **상황:** desktop-shell의 spawn task가 Arc<tokio::sync::Mutex<Option<
  CliChatSession>>> guard를 send().await 전체 동안 유지. 결과 main loop의
  다른 chat_session.lock().await 호출 (e.g. /ai exit, /ai start, /ai list의
  is_some() 체크)이 AI 응답 도착까지 block.
- **사용자 영향:** *AI 응답 대기 중 AI 명령 (/ai exit 등)이 즉시 반응 X.*
  UI 스크롤/클릭/창 동작 같은 *non-AI 동작*은 정상 (main loop의 다른 select!
  arm은 영향 없음).
- **언제 해소:** M12+ 후보. chat_session take/replace 패턴 또는 channel-based
  ownership pass로 lock scope 좁히기. v2 redesign 필요.
- **검증:** AI prompt 후 응답 도중 /ai exit 입력 → 응답 도착 후에야 exit 처리.

### KI-021 — main loop biased select! ai_response_rx starvation 가능성

- **언제 들어왔나:** M11.1 (2026-05-26).
- **상황:** tokio::select! biased 순서: stream.read → ai_response_rx →
  watcher_tick. stream.read가 *극히 빠르게 frame을 보내는 경우* (예: 사용자가
  스크롤 wheel 연속 굴림 + compositor가 매 frame SetState 발신) ai_response_rx
  의 recv()가 polling 안 됨 → AI 응답이 buffer에 대기.
- **실측 위험:** 낮음 — stream이 그렇게 hammered되는 시나리오가 흔치 않음 +
  buffer 크기 16으로 즉시 drop X.
- **언제 해소:** 측정 후 starvation 실제 발생 시 ai_response_rx를 stream 위로
  올림. M12+ 측정 task.

### KI-019 — M8 부터 보존 중인 dead modules (M10에서도 재활용 안 됨)

- **언제 들어왔나:** M8 T8.12 final cleanup에서 `#[allow(dead_code)]` + "M9 재활용 가능" 메모로 보존.
- **남은 모듈:** `apps/desktop-shell/src/scan.rs` (+ scan_test.rs), `apps/desktop-shell/src/workspace.rs`,
  `apps/desktop-shell/src/fs_ops.rs`, `compositor/src/invoke_handler::handle_canvas_set_file`,
  `compositor/src/invoke_handler::handle_file_tree_select`, `compositor/src/layout::layout_tree_node`.
- **M9/M10 결과:** scan(eager 전체 트리)은 *lazy_mount + fs_watcher*가 그 역할을 대체 — 재활용 가능성 낮음.
  workspace.rs / fs_ops.rs도 file_ops/folder_ops로 책임 분리되어 unused. canvas/file_tree_select는
  M7 prototype 잔재. layout_tree_node는 layout_explorer로 대체.
- **언제 해소:** M11 entry 시 정리 — git history로 복구 가능하므로 영구 제거 안전. 또는 *명시적
  재활용 계획*이 있는 v2 기능 task 시점에 다시 검토.
- **검증 방법:** `git grep -l '#\[allow(dead_code)\]'` 결과가 의도적 보존 항목만 남는지.

---

## 정기 검토 시점

- **M12 entry 시:** KI-002 (매니페스트 권한 강제) + KI-003 (query owner ai
  매칭) + KI-015 (session 파일 잔존 도구) + granted_dirs 디스크 영구화 +
  AI 감사 로그. M11.5 후보들.
  후속 항목 (M11.1 마감 추가):
  - AI JSONL log retention 정책 (파일 N개 보관 후 rotate)
  - AI 응답 streaming (Anthropic SSE)
- **M13 entry 시:** M13 typed Process Objects (GitRepo@1 / NpmProject@1 / CargoProject@1)
- **M14 entry 시:** M14 container 격리 환경 (Docker / VM)
- **6개월 (2026-11-23):** KI-014/017 v2 확인.
- **12개월 (2027-05-23):** 전체 회고.
