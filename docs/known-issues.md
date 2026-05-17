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

### KI-004 — 컴포지터가 동적 mount된 객체를 못 봄

- **언제 들어왔나:** M4 (컴포지터).
- **상황:** 컴포지터는 시작 시점에 표준 타입 Query → Get → Subscribe 1회 수행. 그 *후에* mount된 객체는 컴포지터 트리에 안 들어옴.
- **결과:** 사용자가 컴포지터 띄운 *후*에 echo-app을 시작하면 echo-app UI가 안 보임. *권장 시작 순서:* server → echo-app → compositor.
- **언제 해소:** "all-objects subscribe" 와이어 메시지 또는 *Mount/Lifecycle Created 이벤트의 전역 구독* 메커니즘 도입. M5 또는 별 PR.
- **검증:** 컴포지터 시작 후 새 앱 띄우면 *자동으로* UI 반영.

### KI-005 — `include_initial: true` 미구현

- **언제 들어왔나:** M2.
- **상황:** Subscribe 메시지의 `include_initial` 플래그가 *받지만 무시됨*. mount 시점에 발행된 Lifecycle::Created 이벤트는 *항상* 후속 구독자에게 전달 안 됨.
- **언제 해소:** 컴포지터의 KI-004 해소와 함께 처리 가능. include_initial=true 시 구독 시점에 *과거 이벤트 재생*.
- **검증:** Subscribe 후 즉시 drain → 과거 Created 이벤트가 나오는지.

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

### KI-011 — `emit_destroyed`가 객체를 *실제로 제거*하지 않음

- **언제 들어왔나:** M3-T6 (앱 라이프사이클).
- **언제 발견:** KI-010 진단 중 부수적으로 발견.
- **상황:** `ObjectServer::emit_destroyed(actor, id)` 가 Lifecycle::Destroyed *이벤트만 발행*하고 `self.objects` HashMap에서 객체를 제거하지는 않음. 따라서 actor 연결이 끊어진 *후에도* 객체들이 query/get으로 조회 가능.
- **결과:** *유령 객체* 시나리오. 죽은 앱의 UI가 "보이지만 아무도 응답 안 하는" 상태로 영원히 남음. 컴포지터에서 사용자에게 가장 혼란스러운 UX.
- **양면적 의도:** 설계 §5.2 "이벤트 로그가 원천 진실, 객체 트리는 유도 상태" 원칙과 *일관*. 하지만 *실용적으로* 부적합 — 사용자는 "앱이 죽으면 UI도 사라진다"를 기대.
- **결정 필요:** 다음 중 하나:
  - **(a) Destroyed 이벤트 발행 + 객체 즉시 제거** — 사용자 기대와 일치. 단, 이벤트 로그 재생 시 *제거된 객체에 대한* 이벤트가 의미를 잃음.
  - **(b) Destroyed 이벤트 발행 + 객체는 *tombstone* 상태로 마킹** — query/get에서 보이되 "dead" 플래그. 컴포지터가 회색 처리.
  - **(c) 그대로 유지** — 이벤트 로그 충실. 사용자 UX는 *상위 계층(컴포지터)*에서 dead actor의 객체를 숨김 처리.
- **권고:** **(b) tombstone**. 데이터 영속성 + UX 명확성 양립. 단, 구현은 M4.5 또는 M5 동반.
- **연결:** KI-004 (컴포지터 동적 객체 미감지)와 같이 처리 — actor lifecycle이 객체 라이프사이클로 *제대로* 반영되게.

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

---

## 정기 검토 시점

- **3개월 (2026-08-17):** M4 후속 정리. KI-001/004/005 우선순위.
- **6개월 (2026-11-17):** M5 작업 중. KI-001/002 해소 확인.
- **12개월 (2027-05-17):** 전체 회고. 미해소 항목 정리.
