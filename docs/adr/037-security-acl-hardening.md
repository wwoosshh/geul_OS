# ADR-037 — 보안 ACL 강화 (wildcard 제거 + AllowIfGrantedDir)

- **상태:** Accepted
- **결정일:** 2026-05-23
- **부모 spec:** `docs/specs/2026-05-23-geulos-m11-security-acl.md`
- **부모 plan:** `docs/plans/2026-05-23-geulos-m11-security-acl.md`
- **해소 KI:** KI-001 (M3부터 wildcard ACL), KI-016 (M8 set_state wildcard)

## Context

M9/M10 마감 시점에도 desktop-shell 객체 거의 전부에 `add_wildcard_acl`이
박혀있어 외부 client가 임의 객체 invoke + Dialog.respond 우회 가능. M9/M10
spec에 ACL 교체 task가 포함되지 않아 이월된 부채. M11 단일 목표로 해소.

## Decision

1. **ACL 표현:** 객체별 inline `Vec<AclEntry>` *유지*. typed helper 5개로
   분화 — `add_ui_object_acl/add_fs_object_acl/add_dialog_acl/add_filesystem_acl/
   add_container_acl`. 타입별 policy table 도입은 v2로 미룸 (불필요한 복잡도).

2. **AI invoke path-aware:** 새 `AclEffect::AllowIfGrantedDir` + 객체의
   `props.path`를 runtime에 `GrantContext.is_granted(actor, path)`로 조회.
   AI는 Filesystem@1 (항상) + granted_dirs 안의 Folder/File (조건부) 만 통과.

3. **set_state ACL 일관화:** server의 set_state 핸들러가 별도 wildcard 검사
   하던 임시 로직을 제거하고 invoke와 동일한 `Object::is_allowed(actor, AclOp,
   grants)` 사용. `MethodPattern::SetState` variant로 op 구분.

4. **GrantStore wire 동기화:** desktop-shell의 Dialog 응답으로 grant 추가/철회
   시 `GrantUpdate` wire 메시지로 server-host의 GrantStore에 반영. server는
   sender의 actor가 `app:desktop-shell:*` 일 때만 수락 — 외부 client가 자기에게
   grant를 주는 우회 차단.

5. **Dialog 영구 차단:** `add_dialog_acl`이 *system:compositor의 respond
   invoke만* 허용. AI/외부 app의 respond 호출은 PermissionDenied. 이로써 AI
   동의 우회 영구 차단 — KI-001 해소의 가장 큰 가치.

## 대안

- (A) wildcard 유지하고 intercept만 강화: ACL 명목적 교체로 끝나 보안 모델
  명료성 X. 기각.
- (B) 타입별 policy table (중앙 dispatch): 깔끔하나 invoke/set_state 경로
  변경 큼. v2 후보.
- (C) desktop-shell이 AI invoke를 proxy로 re-invoke: server 변경 없음.
  invoke 이중 round-trip + 응답 source 혼란. 기각.

## Consequences

**Positive:**
- KI-001/016 해소. 외부 client의 Dialog 우회 / 임의 invoke 차단.
- AI 동작 경계가 *명확* — Filesystem@1 + granted dir만.
- set_state ACL 평가 경로 통일 — 미래 권한 모델 확장 base.
- scan.rs (dead code)의 잔여 wildcard도 함께 정리 (T15 grep guard로 발견).

**Negative:**
- AllowIfGrantedDir의 동적 평가가 매 invoke마다 path lookup. HashSet O(1)
  + Path prefix 비교라 미미하나 측정값 없음. v2에 prof 검토.
- ActorPattern enum에 5 variant (Exact/Wildcard/SystemCompositor/AiSession/
  App)로 늘어남. Wire 직렬화 형식이 enum tag 의존이라 *동시 client/server
  업그레이드* 필요 (현재는 launcher가 일괄 배포라 무관).

**Neutral:**
- echo-app도 wildcard 제거 — 외부 client press는 SystemCompositor/AiSession/
  App enumeration으로 통과. 다음 외부 앱 추가 시 helper 갱신 필요.
- AI가 Cli@1.append_line/submit_input/clear invoke 시 PermissionDenied 응답.
  system_prompt에는 method 노출되어 있으나 *AI 자동 호출 코드는 없음*. 후속
  task로 system_prompt에 "Cli@1 invoke 차단됨, report_done 사용" 가이드 추가
  검토.

## 측정

- `cargo test --workspace` 전 통과 (T1-T13 회귀 + T7 e2e + T8 helper unit).
- `scripts/check-no-wildcard-acl.sh` — src/ 안 ActorPattern::Wildcard 0건.
- 수동 acceptance 시나리오 12개 — `docs/manual-tests/m11-acceptance.md`
  (사용자 후속 실행 + 결과 기록).
