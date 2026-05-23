# M9 Acceptance — 편집/저장 + 권한 다이얼로그 + AI write

**Spec:** `docs/specs/2026-05-22-geulos-m9-edit-save-permission.md`
**Plan:** `docs/plans/2026-05-22-geulos-m9-edit-save-permission.md`

## 사전 조건
- 3 프로세스 spawn 순서 (KI-004 회피): `server-host` → `desktop-shell` → `compositor`
- `ANTHROPIC_API_KEY` (시나리오 C/D — AI write)
- 쓰기 가능한 작은 텍스트 파일 (예: `D:\GeulOS\scratch.txt` 또는 임의 .txt/.md)

## 시나리오 A — 사용자 직접 편집/저장
1. FileTree → 작은 .txt 파일 클릭 → Window viewer 등장 (content 표시)
2. Window 본문 클릭 → cursor가 클릭 위치에 등장 (편집 가능 — Ctrl+E 같은 토글 불필요)
3. 키 입력 → content 즉시 갱신, title에 `* ` 접두 (dirty=true)
4. 화살표/Backspace/Enter 모두 동작. 한글 char도 안전 (jamo 분해 단위 X, char 단위).
5. **Ctrl+S** → `* ` 사라짐 (dirty=false), 외부 에디터(notepad)로 같은 파일 read → 변경 반영
6. 텍스트가 *항상 창 안*에 fit (fontdue measure 기반 wrap). 한글/ASCII mix도 OK.

## 시나리오 B — Dirty close
7. 다시 편집 후 Window [x] 클릭
8. v1: dirty이면 `eprintln`만 + close 무시 (v2에 3-버튼 Dialog 예정)
9. Ctrl+S 후 [x] → Window 정상 destroy

## 시나리오 C — AI 저장 허용
10. (먼저 *작은 .txt 파일을 Window로 열어두기* — 그래야 File@1이 lazy mount → AI가 발견 가능)
11. CLI에서 `/ai start` (새 세션)
12. AI에게 한국어로: **"열려있는 파일에 'hello from ai'라고 저장해줘"**
13. AI가 `list_objects_by_type("aios.builtin/Window@1")` → `get_object`로 file_id 추출 → `File.save(content)` invoke
14. **화면 중앙 Dialog 모달 등장**: title `"AI 저장 확인"`, message `"AI가 <path>를 저장하려고 합니다 — 허용?"`, 버튼 `[허용]` / `[거부]`
15. **[허용]** → Dialog 사라짐 + 외부 read → "hello from ai" 저장 확인 + desktop-shell 로그 `"AI save 승인"`

## 시나리오 D — AI 저장 거부
16. 다시 AI에게 동일 요청 → Dialog 등장
17. **[거부]** → Dialog 사라짐 + 파일 변경 없음 + 로그 `"AI save 거부됨"`

## 시나리오 E — Modal block
18. Dialog 떠있는 동안 FileTree 폴더 클릭 → 무동작 (modal hit-block)
19. CLI 키 입력 → 무동작
20. Dialog 응답 → 정상 복귀

## 시나리오 F — CLI 출력
21. AI 응답이 가로로 *자동 줄바꿈* (fontdue measure 기반 wrap). 글자가 패널 밖으로 안 잘림.
22. PageUp → 위로 5라인 스크롤, PageDown → 아래로. 또는 CLI 영역 위에서 마우스 휠.
23. 새 입력 commit 시 자동으로 bottom으로 reset.

## 통과 조건
- A~F 모든 단계 시각·동작 정확
- 회귀 0 — M7 (CLI, AI chat, IME, paste) / M8 (viewer, scroll, multi-window, icons, zebra, parent nav) / M8.5 (stride 28, case-insensitive)

## 알려진 한계 (v2 / M10+)
- AI는 *현재 mount된 File*만 접근 가능 — 임의 disk path는 불가 (M10 `Filesystem@1` 도구로 해결 예정).
- 사용자 직접 save_to_file은 permission 우회 (UI Ctrl+S 명시적 신뢰).
- 3-버튼 Dirty close Dialog 미구현 (v1은 reject + 안내).
- atomic write X (crash 시 원본 손상 가능). v2 temp+rename.
- 동시 다발 AI write 큐잉 X — 두 번째는 즉시 reject.
- undo/redo X. multi-byte cursor가 grapheme 단위 X.
- IME → Window edit_mode 라우팅 X (Cli만 IME). v2.
- Binary 파일 edit X (viewer 미지원이므로 fallback).

## 회귀 가드 (자동)
- `cargo test --workspace` — FAIL 0
- 핵심 신규 단위 테스트:
  - core: `file_has_save_method` / `window_has_dirty_state_and_save_methods` / `dialog_factory_sets_props_state_methods` (3)
  - desktop-shell: `permission` (8) + `file_write` (3) + `dialog_ops` (3) + `window_ops::toggle_edit_flips_value` (1)
  - compositor: `editor` (22 — UTF-8 cursor + wrap_by_pixel_width + byte_offset_from_pixel + line_and_byte_in_line) + `layout` 회귀 + `server_client::std_types_query_coverage_smoke` (Dialog@1 포함)
- `cargo fmt --check` / `cargo clippy --workspace --all-targets -- -D warnings` 클린

## 후속
- M10: `Filesystem@1` builtin (path-based read/write/list/glob/grep) + 파일 생성/삭제/rename + 권한 모델 확장 (프로젝트 root 안/밖 구분).
- v2 후속: atomic write, undo/redo, syntax highlight, multi-cursor, IME edit_mode, 3-버튼 dirty close.
