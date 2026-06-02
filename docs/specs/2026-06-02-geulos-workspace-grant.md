# GeulOS 워크스페이스 사전 승인 (Workspace Grant) — 설계

> **Status:** designed (2026-06-02)

## 목표

사용자가 **AI가 자유롭게 작업할 디렉터리(워크스페이스)를 미리 지정**하면, 그 하위 전체에서 AI는 **어떤 fs 작업(생성/수정/이름변경/삭제)도 권한 프롬프트 없이** 수행한다. 워크스페이스 **밖**의 작업만 Dialog 확인을 요구한다. 지정은 재부팅 후에도 유지된다.

배경: 사용자가 AI로 React 프로젝트 작업 중, 파일마다·작업마다 Dialog 승인이 반복돼 흐름이 끊김. AI 접근 범위가 호스트 전체 파일이라 무권한은 위험 → "신뢰 영역" 모델로 절충.

## 확정된 결정 (브레인스토밍 2026-06-02)

1. **워크스페이스 내 완전 신뢰** — 생성/수정/이름변경 + **삭제까지** 모두 무프롬프트. (현재 AI Delete는 granted여도 항상 확인 → 워크스페이스 안에선 해제.)
2. **지정 방식 = CLI 명령 + Dialog 버튼 둘 다** — `/workspace add|list|remove <path>` + 첫 권한 Dialog에 "이 폴더 전체 신뢰" 버튼.
3. **영속화** — 지정한 워크스페이스는 `~/.geulos/workspaces.json`에 저장, desktop-shell 시작 시 로드.

## 핵심 발견 (현재 인프라)

권한 모델은 대부분 존재 (M10/M11, ADR-036/037):
- `permission::judge_with_path(actor, op, dir, granted)` — AI Save/Create/Rename은 granted dir에서 Allow.
- `GrantedDirs` 캐시 + `grant_dir`/`revoke_dir`(서버 `GrantUpdate` 동기).
- 서버 `is_granted`는 **prefix 매칭**(`path.starts_with(granted)`).

**버그(이 작업의 핵심 수정):** desktop-shell `GrantedDirs.contains`는 **정확 일치(HashSet)** — 서버의 prefix 매칭과 불일치. 그래서 `D:\proj\src` 승인 후 `D:\proj\src\components`는 *다시 프롬프트*. 워크스페이스 모델은 prefix 매칭이 전제.

## 아키텍처

### 권한 판정 흐름 (변경 후)

```
AI invoke fs mutation (target dir D)
  desktop-shell handler → judge_with_path(ai, op, D, granted)
    ├ granted.contains(D)?  ← prefix 매칭으로 변경: D == G 또는 D.starts_with(G)
    │   true  → Allow → 즉시 실행 (Dialog 없음)
    │   false → ConfirmRequired → Dialog mount
    │             ├ [이번만 허용] → 실행 + granted.insert(D) (세션 한정)
    │             ├ [이 폴더 전체 신뢰] → 실행 + granted.insert_persistent(D) (디스크 저장)
    │             └ [거부] → 취소
    └ op == Delete: 더 이상 항상-확인 아님 — 다른 op와 동일하게 granted면 Allow
```

### 영속화

```
~/.geulos/workspaces.json  =  ["D:\\react_project1", "C:\\AiOS", ...]
desktop-shell 시작
  → GrantedDirs::load_persisted() → 각 path를 inner+persistent에 insert
  → 각 path에 GrantUpdate(Add) wire 송신 (서버 GrantStore 동기)
```

## 컴포넌트 (변경 단위)

### C1. `GrantedDirs` prefix 매칭 + 영속 — `apps/desktop-shell/src/granted_dirs.rs`

```rust
pub struct GrantedDirs {
    inner: Mutex<HashSet<PathBuf>>,       // 활성 grant 전체 (세션 + 영속)
    persistent: Mutex<HashSet<PathBuf>>,  // 디스크에 저장되는 부분집합
}
impl GrantedDirs {
    /// prefix 매칭: dir 자신 또는 상위가 granted면 true (서버 is_granted와 일치).
    pub fn contains(&self, dir: &Path) -> bool {
        self.inner.lock().unwrap().iter().any(|g| dir == g.as_path() || dir.starts_with(g))
    }
    pub fn insert(&self, dir: PathBuf);             // 세션 grant (inner만)
    pub fn insert_persistent(&self, dir: PathBuf);  // inner+persistent + save_persisted()
    pub fn remove(&self, dir: &Path);               // inner+persistent + save_persisted()
    pub fn list_persistent(&self) -> Vec<PathBuf>;
    /// 시작 시 호출 — workspaces.json 읽어 inner+persistent 채움. 반환: 로드된 path들(GrantUpdate 송신용).
    pub fn load_persisted(&self) -> Vec<PathBuf>;
}
fn workspaces_path() -> PathBuf;  // ~/.geulos/workspaces.json
fn save_persisted(set: &HashSet<PathBuf>);  // best-effort, 실패는 log
```

- **테스트:** prefix 매칭(상위 granted → 하위 true, 형제 false), insert_persistent 후 list_persistent, save→load round-trip(tempdir).

### C2. 삭제 정책 완화 — `apps/desktop-shell/src/permission.rs`

`judge_with_path`에서 `if op == Op::Delete { return ConfirmRequired }` 제거 → Delete도 granted면 Allow:
```rust
pub fn judge_with_path(actor, op, dir, granted) -> Verdict {
    if actor == &ActorId::local_user() { return Allow; }
    if granted.contains(dir) { Allow } else { ConfirmRequired }
}
```
- **테스트:** `ai_delete_in_granted_dir_allowed`(신규), 기존 `ai_delete_always_confirm_path` → granted면 Allow로 수정.

### C3. `/workspace` CLI 명령 — `apps/desktop-shell` (CLI 명령 파서)

`/ai` 명령을 파싱하는 지점에 `/workspace` 추가:
- `/workspace add <path>` → 절대경로 검증 + `grant_dir`(영속) + 확인 메시지.
- `/workspace list` → persistent + 세션 grant 목록 출력.
- `/workspace remove <path>` → `revoke_dir` + 저장.
- **보안:** Cli.submit_input 경로(=compositor/사용자)에서만 도달 — AI는 self-grant 불가(권한 모델 핵심). 슬래시 명령은 AI tool 표면에 없음.

### C4. Dialog "이 폴더 전체 신뢰" 버튼 — core + desktop-shell + compositor

- `core/src/object/std_types.rs` `dialog()` — `actions`를 `["이번만 허용", "이 폴더 전체 신뢰", "거부"]` 3개로 (현재 `["허용","거부"]`).
- `apps/desktop-shell/src/handlers/dialog_methods.rs` — `respond(action)` 분기에 "이 폴더 전체 신뢰" 추가: pending op 실행 + `insert_persistent(dir)`. 기존 한/영 alias(KI-023) 함께 정리.
- **컴포지터 (양 백엔드)** — Dialog 3버튼 렌더 + hit_test 3분할. host `main.rs` + `bin/geulos-vm-compositor`. (현재 2버튼 → 3버튼 레이아웃.)

### C5. 시작 시 영속 grant 로드 — `apps/desktop-shell/src/main.rs`

desktop-shell 시작(서버 연결 직후)에서 `granted.load_persisted()` → 각 path에 `grant_dir` wire 송신(GrantUpdate Add)으로 서버 GrantStore 동기.

### C6. 시각 표시 (polish) — `Filesystem@1.granted_dirs` state

grant 변경 시 `Filesystem@1.granted_dirs` state 갱신(이미 표시용 state 존재, std_types.rs:512) → 컴포지터가 신뢰 폴더 표시. v1은 최소 — list만 정확히 반영.

## 보안 고려

- **AI는 워크스페이스를 self-지정 불가** — `/workspace`는 CLI(사용자) 전용, Dialog 버튼은 compositor(사용자 클릭) 전용. AI tool 표면(invoke_method 등)엔 grant 권한 없음.
- **삭제 완전 신뢰는 워크스페이스 범위 한정** — 밖에선 여전히 Dialog. 사용자가 명시 지정한 영역에 국한.
- **영속 파일 위치** `~/.geulos/workspaces.json` — VM 디스크(/dev/vda, 영속). 단일 사용자 dev 머신 전제(KI-024 외부 인증은 별도).
- KI-024(외부 client role 자처)와 독립 — 이 설계는 *granted dir 범위*만 다룸.

## 테스트 전략

- C1 prefix/영속 (단위, 핵심), C2 delete 정책 (단위), C3 `/workspace` 파싱(단위 또는 수동), C4 Dialog 3버튼(수동 — 컴포지터 렌더), C5 시작 로드(수동), 전체 VM end-to-end: `/workspace add D:\proj` → AI가 그 안에서 생성/수정/삭제 무프롬프트, 밖은 Dialog.

## 구현 단계

- **Phase 1 — 정책 핵심** (즉시 효과): C1(prefix+영속) + C2(delete) + C5(시작 로드). 이것만으로 "한 번 승인하면 하위 전체 무프롬프트" + 재부팅 유지 달성.
- **Phase 2 — 명시 지정**: C3(`/workspace` CLI 명령).
- **Phase 3 — Dialog UX**: C4(3버튼, 컴포지터 양 백엔드) + C6(시각 표시).

각 Phase 독립 빌드·검증. Phase 1만으로도 사용자 통증 대부분 해소.

## 후속 / 비목표

- 워크스페이스별 세분 권한(읽기 전용 등) — v1은 full-trust 단일 등급.
- AI에게 "워크스페이스 제안" 허용 — 보안상 v1 제외.
- 관련: ADR-036(object-native fs)/037(ACL), KI "granted_dirs 디스크 영속화"(이 작업으로 해소), 메모리 `feedback_ai_user_identical_command_surface`.
