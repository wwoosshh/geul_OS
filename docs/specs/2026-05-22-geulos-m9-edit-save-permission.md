# M9 — 편집/저장 + 권한 다이얼로그 인프라

**Date:** 2026-05-22
**Status:** Approved (사용자 승인 완료, 구현 대기)
**Milestone:** M9 (M10 = 생성/삭제/rename으로 분리)

## 목적

M8까지의 read-only 멀티 윈도우 탐색기 위에 *편집 + 저장*을 추가하고, 향후 CRUD
전체에서 재사용할 **권한 다이얼로그 인프라**를 동시에 구축한다. 사용자 직접 작업과
AI bridge 작업을 actor + 위험도 두 축으로 구분하는 권한 모델을 v1으로 정착시킨다.

## 범위

**포함:**
- 기존 텍스트 파일을 Window 본문에서 편집 → Ctrl+S로 저장
- 저장 안 한 변경(dirty) 시각 표시 + close 시 확인
- `Dialog@1` 신규 builtin type (modal confirm/warn)
- `permission` 모듈 — `actor + op → Allow | ConfirmRequired` 판정
- AI bridge가 보낸 write invoke 시 사용자 확인 흐름

**제외 (M10 이후):**
- 새 파일/폴더 생성
- 파일/폴더 삭제
- rename
- undo/redo, syntax highlight, multi-cursor, multi-byte cursor 정확도 (v2)
- 큰 파일(1MB 초과) 부분 편집 (v2)
- Binary 파일 편집 (viewer 미지원 → editor도 미지원)
- 동시 다발 AI write 큐잉 (v1은 첫 Dialog 미해결 시 두 번째 write를 즉시 거부)

## 아키텍처

```
사용자 키 입력 (edit_mode + Ctrl+S)
  ↓
compositor::editor → invoke Window/File.save
  ↓
desktop-shell::invoke_handler
  ↓ permission.judge(actor, op) == Allow
file_write::save(path, content)
  ↓ SetState dirty=false
compositor 다음 frame에 "*" 사라짐

────────────────────────────────────

AI bridge invoke File.save
  ↓
desktop-shell::invoke_handler
  ↓ permission.judge(actor, op) == ConfirmRequired
mount Dialog@1 + AI 호출 *pending*
  ↓
compositor: 모달 Dialog 그리기 (다른 입력 block)
  ↓ 사용자 클릭 → invoke Dialog.respond
desktop-shell: pending 정리 + (승인이면) file_write::save + AI invoke 응답
  ↓
Dialog destroy
```

## 새 타입 / 상태 / 메서드

### `File@1` 추가
- 메서드 `save(content: String)` — UTF-8 content를 디스크에 write. 1MB cap.

### `Window@1` 추가
- state `edit_mode: bool` (기본 false)
- state `dirty: bool` (기본 false)
- 메서드 `toggle_edit()` — viewer↔editor 전환
- 메서드 `save_to_file()` — content를 file에 commit (편집 commit + Ctrl+S 진입점)
- 기존 `close()`는 dirty면 Dialog로 confirm 후 진행

### `Dialog@1` (신규)
- props
  - `title: String`
  - `message: String`
  - `kind: String` — `"confirm"` | `"warn"` (color/icon 분기)
  - `actions: [String]` — 버튼 라벨 배열 (예: `["확인", "취소"]`)
- state
  - `result: Option<String>` — 사용자 클릭 결과 (null → pending, 그 외 → actions 중 하나)
- 메서드 `respond(action: String)` — 컴포지터가 사용자 클릭을 invoke로 전달

### `permission` 모듈 (desktop-shell, 신규)
```rust
pub enum Op { Save, Create, Delete, Rename }
pub enum Verdict { Allow, ConfirmRequired }
pub fn judge(actor: &ActorId, op: Op) -> Verdict;
```
v1 정책 표:

| Actor          | Save | Create | Delete    | Rename |
|----------------|------|--------|-----------|--------|
| local-user     | Allow | Allow | Confirm   | Allow  |
| ai (그 외 actor) | Confirm | Confirm | Confirm | Confirm |

M9에서는 Save만 호출 — 표 전체는 M10까지 활용. enum/매치는 미리 확장 가능하게.

## 데이터 흐름 — 사용자 직접 저장

1. 사용자가 Window 더블클릭 → `toggle_edit` invoke → `edit_mode=true`
2. compositor가 키 입력을 editor 상태로 누적 (content 변경 + dirty=true 즉시 SetState)
3. 사용자 Ctrl+S → `save_to_file` invoke
4. desktop-shell: `permission::judge(local-user, Save) == Allow`
5. `file_write::save(file.path, window.content)` → `std::fs::write`
6. 성공 → SetState `file.dirty=false`, `window.dirty=false`
7. 다음 frame: title의 `*` 사라짐, "저장됨" 안내(선택)

## 데이터 흐름 — AI 저장

1. ai-bridge가 `File.save(content)` invoke 발송
2. desktop-shell: `permission::judge(ai, Save) == ConfirmRequired`
3. Desktop 자식으로 `Dialog@1` mount
   - title `"AI 저장 확인"`, message `"AI가 <path>를 저장하려고 합니다 — 허용?"`,
     actions `["허용", "거부"]`
4. AI invoke 응답은 *대기* (pending map에 `(dialog_id → 원래 save args)` 저장)
5. compositor: Dialog rect를 *마지막 z*로 push + 다른 hit_test는 Dialog rect 밖이면 무시 (modal)
6. 사용자 [허용] 클릭 → `Dialog.respond("허용")` invoke
7. desktop-shell: pending 조회 → `permission::judge` 다시 안 함 (이미 통과) → `file_write::save` 수행
8. 결과로 원래 AI invoke 응답 + Dialog destroy
9. [거부]면 file_write skip + AI invoke 에러 응답 + Dialog destroy

## 권한 거부/에러 처리

- fs error (권한, 잠금, 디스크 가득) → invoke 응답에 `{ok: false, error: "..."}`. CLI에 안내.
- AI invoke 응답이 에러면 AI 측에서 다시 시도/사용자에 보고.
- 사용자가 [거부]했을 때도 동일한 invoke 에러 — `{ok: false, error: "권한 거부"}`.
- 1MB cap 초과 content는 save 거부 — `{ok: false, error: "1MB 초과 (M9 미지원)"}`.

## Dirty close 흐름

1. dirty=true Window의 [x] 클릭
2. compositor가 `close` invoke 전에 직접 Dialog mount 요청?
   → **단순화**: compositor는 평소처럼 `close` invoke만 보냄. desktop-shell이 *Window.dirty
   확인 후 Dialog 띄움* → 사용자 응답에 따라 분기.
3. desktop-shell: window.dirty 검사 → false면 즉시 close (기존 동작), true면 Dialog mount
   - title `"저장 안 함"`, message `"저장하지 않은 변경이 있습니다 — 어떻게 할까요?"`,
     actions `["저장 후 닫기", "그냥 닫기", "취소"]`
4. 사용자 응답에 따라 save→close / close / nothing

## 파일 구성

**신규**
- `apps/desktop-shell/src/file_write.rs` — write 핸들러 (`save(path, content) -> Result<(), Error>`, 1MB cap, atomic write 검토)
- `apps/desktop-shell/src/permission.rs` — `Op` enum, `Verdict` enum, `judge` 함수
- `apps/desktop-shell/src/dialog_ops.rs` — Dialog mount/respond/destroy 핸들러 + pending action 매핑
- `compositor/src/editor.rs` — editor cursor 위치, key→content 변경, dirty 검출 로직
- `docs/adr/035-edit-save-permission.md` — 결정 기록

**수정**
- `core/src/object/std_types.rs` — File.save, Window edit_mode/dirty/toggle_edit/save_to_file, Dialog@1 factory
- `apps/desktop-shell/src/main.rs` — save/toggle_edit/respond invoke 분기, dirty close 흐름
- `apps/desktop-shell/src/window_ops.rs` — dirty 추적, close 분기 변경
- `compositor/src/render.rs` — edit_mode 시 cursor 그리기, dirty면 title에 `*`, Dialog@1 모달 렌더
- `compositor/src/main.rs` — edit_mode 키 입력 처리(editor.rs 호출), Dialog 클릭 → respond
- `compositor/src/keyboard.rs` — Ctrl+S, Esc 등 단축키 라우팅

## 테스트

**단위 (cargo test)**
- `file_write::save` 성공/실패 (없는 디렉터리, read-only 파일, 1MB 초과)
- `permission::judge` 표 8칸 모두 (실제 M9는 2칸 사용이지만 enum 정의 검증)
- `dialog_ops` — mount/respond/destroy 시나리오, pending 매핑
- `editor` — char 삽입/삭제, dirty toggle, scroll 무관 cursor pos
- `std_types::dialog` factory — props/state/methods 등록 검증
- `Window::toggle_edit` — edit_mode toggle, save_to_file이 dirty=false 만듦

**수동 (acceptance 문서)**
- 시나리오 A: 사용자 직접 편집/저장 (텍스트 입력 → Ctrl+S → 디스크 확인)
- 시나리오 B: dirty close 확인 (변경 후 [x] → 3-buttons dialog → 각 선택지 동작)
- 시나리오 C: AI 저장 confirm (AI에 저장 요청 → Dialog 등장 → [허용] vs [거부])
- 시나리오 D: 권한 거부 path (read-only 파일 저장 시도 → 에러 안내)
- 시나리오 E: 회귀 — viewer 모드, scroll, multi-window, FileTree expand, CLI, AI chat 모두 OK

## 알려진 한계 / 후속

- v1은 utf-8 텍스트 파일만. Binary는 edit_mode 비활성화.
- 큰 파일 부분 편집 X (1MB cap).
- 동시 다발 AI write 큐잉 X — 첫 Dialog 미해결 시 두 번째 write 즉시 reject.
- undo/redo X.
- multi-byte char cursor가 grapheme 단위가 아닌 char 단위 (한글 jamo 분해 자모 분리 X).
- save가 atomic 아니면 crash 시 원본 손상 가능 — v2에 temp+rename 검토.
- M10: 생성/삭제/rename + 권한 표 6칸 활용.
- M11+: undo/redo, syntax highlight, find/replace, multi-cursor.

## 후속 결정 (구현 plan 단계)

- **atomic write 채택 여부** (temp file + rename) — v1은 직접 write로 시작 (단순).
- **editor cursor를 server state로 둘지** (지금 input_buffer는 컴포지터 local) — local 권장.
- **Dialog z-order**가 Window보다 항상 위 (modal이므로 yes).
- **Dialog open 시 다른 입력 block** — 다른 Window/CLI 클릭/키 무시. modal flag로 명시.
- **Pending invoke 메커니즘** — AI invoke 응답 대기 보관. `HashMap<DialogId, PendingSave>`
  (PendingSave = { file_id, content, response_chan }). v1은 Tokio oneshot channel로 plan에서
  최종 결정.
- **server-side ACL 무관** — T8.19에서 set_state는 wildcard 통과. 권한 정책은
  *desktop-shell 내부* `permission` 모듈만 — server는 broker 역할 유지.
