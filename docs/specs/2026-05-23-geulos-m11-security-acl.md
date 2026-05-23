# M11 — 보안 ACL 강화 (wildcard 제거 + actor allowlist + AI path-aware grant)

**Date:** 2026-05-23
**Status:** Draft (사용자 review 대기)
**Parent:** M9 (Dialog/granted_dirs 인프라) + M10 (Filesystem@1)
**해소 대상 KI:** KI-001 (echo-app / desktop-shell wildcard ACL), KI-016 (set_state wildcard)

## 동기

M9/M10 마감 시점에도 desktop-shell 객체 거의 전부에 `(actor=*, method=*, effect=Allow)` 한 줄이 박혀있다 (`add_wildcard_acl` 호출 16곳). 의미: *외부에서 TCP 연결한 누구든* 임의 객체의 *어떤 메서드든* 호출 가능. 구체 위협:

- **Dialog 우회**: AI가 write 동의를 요청하는 Dialog를 외부 client가 `respond({choice: "allow"})`로 사용자 몰래 응답 → AI write 권한 통과.
- **임의 invoke**: Window.close / File.delete / Folder.delete 등 외부에서 호출 가능.
- **임의 set_state**: scroll_y/focused/title 등 UI 상태 외부 조작.

KI-001/016은 *"M9 진입 시 해소"* 약속이었으나 M9 spec에 task 미포함으로 이월. M11에서 일괄 해소.

## 범위 (단일 목표)

**KI-001 / KI-016 해소만 집중.** 다른 보안 부채 (KI-002 매니페스트 권한 강제, KI-015 session 파일 잔존, KI-003 query owner 매칭, AI 행위 감사 로그)는 M11 범위 *외* — M11.5/M12 또는 별 task로 미룸.

## 권한 모델 (Actor × 권한 매트릭스)

Wire actor는 3종:
- `system:compositor` — compositor의 user-facing 입력 (click/scroll/key) 대표
- `app:<id>:<uuid>` — desktop-shell 등 앱 (현재 `app:desktop-shell:<uuid>` 1개)
- `ai:<uuid>` — ai-bridge 세션마다 새 UUID

`user:local`은 *객체 owner marker*일 뿐 wire 연결 없음 — 사용자 동작은 항상 `system:compositor`가 대표.

### 객체 타입별 ACL

| 객체 타입 | invoke | set_state |
|---|---|---|
| **Window@1** | `system:compositor` (move/resize/focus/close/close_confirm/save_to_file) | `system:compositor` (scroll_y/focused/z/x/y/w/h/title/content/content_too_large) + `app:desktop-shell` (content read 결과 broadcast) |
| **Explorer@1** | `system:compositor` (navigate_to/navigate_up/open_file) | `system:compositor` (scroll_y) + `app:desktop-shell` (active_folder/expanded) |
| **FileTree@1** | `system:compositor` (expand/collapse) | `system:compositor` (scroll_y) + `app:desktop-shell` (expanded/children) |
| **Cli@1** | `system:compositor` (submit_input/clear) + `app:desktop-shell` (append_line) | `system:compositor` (scroll_y) + `app:desktop-shell` (lines/mode/session_name/pending_action/awaiting_api_key) |
| **Folder@1** | `system:compositor` (list/create_file/create_folder/delete/rename) + `ai:<uuid>` *if path ∈ granted_dirs* | `app:desktop-shell` (child_count) |
| **File@1** | `system:compositor` (read/save/delete/rename) + `ai:<uuid>` *if path ∈ granted_dirs* | `app:desktop-shell` (size_bytes/preview/mime) |
| **Dialog@1** | `system:compositor` (respond) — *외부 우회 영구 차단* | `app:desktop-shell` (text/choices/result) |
| **Filesystem@1** | `system:compositor` + `ai:<uuid>` (read_external/write_external) | (없음) |
| **Desktop@1** | (없음 — 마운트 컨테이너) | `app:desktop-shell` (children list 갱신) |

### 핵심 격차 (wildcard 대비)

1. **Dialog.respond는 `system:compositor` 단독** — `ai:*` / `app:*` 모두 DENY. AI 동의 우회 불가능.
2. **AI는 Filesystem@1 + path-allowed Folder/File만** — Window/Explorer/Cli/Dialog 절대 invoke 불가.
3. **set_state는 발신자 좁힘** — UI 상태는 compositor만, 데이터 상태는 desktop-shell만.

## 새 AclEffect — `AllowIfGrantedDir`

AI invoke 통과 조건이 *동적 path 조회*이므로 정적 ACL로 표현 불가. 기존 `AclEntry` struct 유지하고 `AclEffect`에 새 variant 추가 — 변경 표면 최소:

```rust
// core/src/object/acl.rs (확장)
pub enum AclEffect {
    Allow,
    Deny,
    /// 객체의 props.path가 호출자(actor)의 granted_dirs에 포함될 때만 Allow.
    /// path prop이 없는 객체에 이 effect가 매칭되면 Deny와 동일 처리.
    AllowIfGrantedDir,
}
```

`AclEntry { actor, method, effect: AclEffect::AllowIfGrantedDir }` 한 행이 *AI가 path 조건부로 호출 가능*을 표현.

ACL 검사기 시그니처에 *grant context* 추가:

```rust
// core/src/server/acl_check.rs (가칭)
pub trait GrantContext {
    fn is_granted(&self, actor: &ActorId, path: &Path) -> bool;
}

pub fn check_invoke(
    obj: &Object,
    actor: &ActorId,
    method: &str,
    grants: &dyn GrantContext,
) -> AclDecision { ... }
```

server-host의 invoke/set_state 핸들러는 `GrantContext` 구현을 *connection 단위*로 주입. desktop-shell이 grant 추가 시 server-host로 알릴 채널 필요 — 새 wire 메시지 또는 server-host 내부 상태.

### Grant 주입 흐름

```
desktop-shell의 PendingFs Dialog → 사용자 Allow → granted_dirs.insert(path)
                                                ↓
                       grant_update wire 메시지: {actor: ai:<uuid>, path: D:/foo}
                                                ↓
                       server-host의 GrantContext 상태에 반영
                                                ↓
                       이후 ai:<uuid>의 Folder.create_file invoke가 AllowIfGrantedDir 통과
```

**wire 메시지**: `GrantUpdate { actor: String, path: String, op: Add | Remove }` — desktop-shell만 발신 권한 (ACL로 보호 — `app:desktop-shell`만).

## ActorPattern / MethodPattern 확장

기존 두 enum에 variant 추가 — Wildcard는 *유지하되 helper에서 더는 사용 X* (회귀 grep 가드용 target):

```rust
// core/src/object/acl.rs
pub enum ActorPattern {
    Exact(ActorId),
    Wildcard,                  // 유지, 신규 사용 금지
    SystemCompositor,          // 신규 — system:compositor 단독 매칭
    AiSession,                 // 신규 — ai:<uuid> 모두 매칭 (prefix "ai:")
    App(String),               // 신규 — app:<id>:* (특정 app id) 매칭
}

pub enum MethodPattern {
    Exact(String),
    Wildcard,                  // 유지, 신규 사용 금지
    OneOf(Vec<String>),        // 신규 — 여러 method 중 하나 매칭
    SetState,                  // 신규 — server의 SetState 메시지 한정
                               // (invoke method 이름과 별 dispatch)
}
```

## Helper 함수 분화 (`apps/desktop-shell/src/handlers/mod.rs`)

기존 `add_wildcard_acl(obj)` 1개 → 5개로 분화. 각 helper는 권한 매트릭스를 정확히 반영. *invoke와 set_state 두 가지를 분리해 명시*:

```rust
use geulos_core::{AclEntry, AclEffect, ActorPattern, MethodPattern, Object};

/// Window/Explorer/FileTree/Cli — compositor가 user 동작 대표 + desktop-shell set_state
pub fn add_ui_object_acl(obj: &mut Object) {
    obj.acl.push(AclEntry {
        actor: ActorPattern::SystemCompositor,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
    obj.acl.push(AclEntry {
        actor: ActorPattern::App("desktop-shell".to_string()),
        method: MethodPattern::SetState,
        effect: AclEffect::Allow,
    });
}

/// Folder/File — compositor + AI(path 조건부) + desktop-shell set_state
pub fn add_fs_object_acl(obj: &mut Object) {
    obj.acl.push(AclEntry {
        actor: ActorPattern::SystemCompositor,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
    obj.acl.push(AclEntry {
        actor: ActorPattern::AiSession,
        method: MethodPattern::Wildcard,
        effect: AclEffect::AllowIfGrantedDir,  // path ∈ granted_dirs일 때만
    });
    obj.acl.push(AclEntry {
        actor: ActorPattern::App("desktop-shell".to_string()),
        method: MethodPattern::SetState,
        effect: AclEffect::Allow,
    });
}

/// Dialog — compositor 단독 invoke(respond) + desktop-shell set_state (외부 우회 차단)
pub fn add_dialog_acl(obj: &mut Object) {
    obj.acl.push(AclEntry {
        actor: ActorPattern::SystemCompositor,
        method: MethodPattern::Exact("respond".to_string()),
        effect: AclEffect::Allow,
    });
    obj.acl.push(AclEntry {
        actor: ActorPattern::App("desktop-shell".to_string()),
        method: MethodPattern::SetState,
        effect: AclEffect::Allow,
    });
}

/// Filesystem@1 — compositor + AI (path 무관, 두 method만)
pub fn add_filesystem_acl(obj: &mut Object) {
    obj.acl.push(AclEntry {
        actor: ActorPattern::SystemCompositor,
        method: MethodPattern::Wildcard,
        effect: AclEffect::Allow,
    });
    obj.acl.push(AclEntry {
        actor: ActorPattern::AiSession,
        method: MethodPattern::OneOf(vec!["read_external".into(), "write_external".into()]),
        effect: AclEffect::Allow,
    });
}

/// Desktop / Cli 히스토리 같은 컨테이너 — desktop-shell set_state 단독
pub fn add_container_acl(obj: &mut Object) {
    obj.acl.push(AclEntry {
        actor: ActorPattern::App("desktop-shell".to_string()),
        method: MethodPattern::SetState,
        effect: AclEffect::Allow,
    });
}
```

## 구현 단계

### Stage 1 — core 확장 (기반)
- `ActorPattern` 신규 variants (SystemCompositor / AiSession / App)
- `AclEntry::AllowIfGrantedDir` variant
- `GrantContext` trait
- `check_invoke` / `check_set_state` 시그니처에 `&dyn GrantContext` 인자 추가
- 단위 테스트: 각 패턴 매칭 + AllowIfGrantedDir 통과/거부

### Stage 2 — server-host 통합
- connection 별 `Arc<Mutex<GrantContextImpl>>` 보유
- `GrantUpdate` wire 메시지 추가 (proto/src) + handle_grant_update
- invoke/set_state 핸들러가 grant 인자 전달
- ACL test 회귀: 기존 wildcard 테스트 → 명시 패턴 테스트로 갱신

### Stage 3 — desktop-shell 교체
- `add_wildcard_acl` 호출 16곳 → 객체 타입별 5개 helper 중 적절한 것으로 교체
- granted_dirs.insert / remove 시 `GrantUpdate` wire 메시지 송신
- 회귀: 기존 manual test (M9/M10 시나리오 전체) 재실행

### Stage 4 — echo-app 정리
- echo-app의 wildcard 제거 → 매니페스트 actor 자기 자신만 set_state 허용
- M3 acceptance test 갱신

### Stage 5 — 회귀 검증 + grep 가드
- `git grep -n 'ActorPattern::Wildcard\|MethodPattern::Wildcard'` 결과가 *허용된 위치*(test 코드 + acl.rs definition)만 남는지 CI grep 가드 추가
- 전체 manual test 시나리오 재실행 (M3 echo-app, M8 multi-window, M9 편집/저장, M10 CRUD + Filesystem@1)

## 회귀 검증 시나리오

| 시나리오 | 기대 |
|---|---|
| 사용자 클릭 → Explorer.navigate_to | 통과 (compositor) |
| 사용자 키 → Cli.submit_input | 통과 (compositor) |
| AI: Filesystem@1.read_external | 통과 |
| AI: Folder.create_file *(granted_dirs 안)* | 통과 |
| AI: Folder.create_file *(granted_dirs 밖)* | DENY → Dialog 트리거 (M9 흐름) |
| AI: Dialog.respond | DENY (외부 우회 영구 차단) ← *핵심* |
| AI: Window.close | DENY |
| AI: Explorer.navigate_to | DENY |
| 외부 geulosh로 invoke Dialog.respond | DENY |
| desktop-shell의 SetState (scroll_y) | 통과 (app:desktop-shell) |
| compositor의 SetState (focused) | 통과 (system:compositor) |
| 외부 geulosh로 set_state Window.title | DENY |

## M11 범위 외 (별 마일스톤)

- **KI-002**: 매니페스트 권한 카테고리 강제 — M11.5 또는 외부 앱 ecosystem 시점
- **KI-003**: `query owner ai:<uuid>` ActorId::from_str 교체 — small PR (M11 진행 중 또는 후)
- **KI-015**: session 파일 잔존 API key 정리 도구 — 작은 CLI util, M11 진행과 무관
- **granted_dirs 영구화**: 디스크 저장 — M11.5 또는 별 작업
- **AI 행위 감사 로그**: M11.5 또는 M12

## 비-목표

- 새 actor 종류 도입 (예: `service:*`) — M11 범위 외
- ACL 표현 모델 변경 (객체별 inline → 타입별 policy table) — 사용자 결정으로 *유지*
- 권한 grant UI 개선 (현재 Dialog 그대로)
- M9의 `permission::judge_with_path` 로직 변경 — 그대로 사용, ACL 검사 *전*에 desktop-shell이 호출

## 검증 통과 기준

1. `git grep -n 'ActorPattern::Wildcard\|MethodPattern::Wildcard' apps/ compositor/ ai-bridge/`가 0건 (test/acl.rs 외)
2. M9/M10 manual test 시나리오 전체 통과
3. 외부 `geulosh invoke <dialog_id> respond '...'` → 응답이 `PermissionDenied`
4. 외부 `geulosh invoke <window_id> close` → 응답이 `PermissionDenied`
5. AI invoke 시나리오 (Filesystem@1, granted dir Folder, ungranted dir Folder) 각각 기대대로
6. `cargo test --all` 전 binary 통과 + `clippy -D warnings` + `fmt --check`

## 위험 + 완화

- **회귀 위험**: ACL 교체 시 *invoke 거부로 UI 멈춤* 가능. 완화: stage 3 적용 *전*에 compositor의 모든 invoke를 grep해 매트릭스 점검. 누락된 패턴은 helper에 추가.
- **AllowIfGrantedDir 성능**: 매 invoke마다 grant 조회. 완화: granted_dirs는 HashSet (O(1)). 충분.
- **grant 동기화 race**: desktop-shell이 GrantUpdate 보내기 *전*에 server가 AI invoke 받음. 완화: granted_dirs.insert + Dialog 응답 송신이 *AI 측 응답보다 먼저*. M9 흐름상 이미 그렇게 됨.
