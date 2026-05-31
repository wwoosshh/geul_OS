> **Status:** adopted (2026-05-31)
> **Note:** M8 part 2 정식 채택, ADR-033. 1MB cap 텍스트 viewer + 3영역 스크롤 구현 완료, 후속 마일스톤에서 그대로 유지.

# GeulOS M8 part 2 Spec — 메모장 viewer + 공통 스크롤

**Date:** 2026-05-20
**Status:** Design 승인됨, plan 작성 대기
**Author:** wwoosshh + Claude controller session
**Parent:** M8 (전체 파일시스템 + 멀티-윈도우 탐색기) — `docs/specs/2026-05-18-geulos-m8-multi-window-explorer.md`

---

## 1. 한 줄 요약

M8 v1 viewer가 *파일의 첫 512바이트만* 보여주는 한계 해소 + 좌측 트리/우측 Explorer/Window 본문 *세 영역 모두 스크롤* 지원. 읽기 전용 (M9 권한 다이얼로그 + 편집은 별 마일스톤).

---

## 2. Motivation (사용자 결정 — 2026-05-20)

사용자 보고:
> "스크롤기능이 없어서 아래로 잘린 내용을 볼수없다거나 어떤게 파일이고 어떤게 폴더인지 아이콘이미지가 없어서 보기불편한점, 단순한 txt외에 다른 md파일이나 기타 파일을 열수가 없다는점이야"

해석:
1. **viewer 부족** — 메모장 같은 텍스트 viewer 필요. txt/md/기타 텍스트 파일 모두.
2. **스크롤 부족** — 좌측 트리에서 큰 폴더 자식이 화면 넘침, 우측 Explorer에서도 동일, Window 본문도 *512바이트만*이라 의미 없음.
3. **아이콘 부족** — 별 후속 task (이 spec 범위 밖)

이 spec은 1+2 cover. 3은 별 task.

---

## 3. Scope

### In scope (M8 part 2 = T8.13~T8.18)
- Window 본문 *type-aware 텍스트 viewer* — UTF-8 text 파일을 *전체* (1MB cap) 표시 + 스크롤
- FileTree (좌측 25%) 행 스크롤 — 깊은 트리에서 화면 넘침 해소
- Explorer (우측 75%) 행 스크롤 — 큰 폴더 자식 일람
- 마우스 휠 + PageUp/Down 키 입력
- md 파일 *plain text*로 표시 (진짜 markdown 렌더는 v2)

### Out of scope (M9+ 또는 별 task)
- **편집 / 저장** — Folder/File write 메서드 복귀 필요. M9 권한 다이얼로그 마일스톤.
- **md 렌더** (h1/bold/list 시각화) — pulldown-cmark 등 v2
- **이미지 viewer** (.png/.jpg) — 별 image crate, v2
- **syntax highlighting** (.rs/.py/.json 색칠) — v2
- **검색** (Ctrl+F) — v2
- **파일/폴더 아이콘** — 별 task (사용자 명시한 추가 UX)
- **수평 스크롤** — v1은 *세로만*. 긴 줄은 wrap 또는 truncate (디자인 결정 §5.4).

---

## 4. 객체 모델 변경

### 4.1 `aios.builtin/Window@1` — state 확장
```
state:
  x, y, w, h, z, focused  (M8 part 1 그대로)
  scroll_y: i32           ← 신규 (px 단위 또는 라인 단위 — §5.3 결정)
  content: String         ← 신규 (Window mount 시점에 desktop-shell이 채움, 1MB cap)
  content_too_large: bool ← 신규 (1MB 초과 시 true → 첫 1MB만 보여줌)
```

### 4.2 `aios.builtin/FileTree@1` — state 확장
```
state:
  expanded, selected  (기존)
  scroll_y: i32       ← 신규
```

### 4.3 `aios.builtin/Explorer@1` — state 확장
```
state:
  active_folder, view_mode  (기존)
  scroll_y: i32             ← 신규
```

**Cli@1는 변경 없음** — CLI는 *line cap*(1000)이 있어 스크롤 불필요. 단 후속 task에서 CLI 스크롤 필요 시 같은 패턴.

---

## 5. 디자인 결정

### 5.1 Content fetch — Window mount 시점에 *full read*

desktop-shell의 `open_file` handler (T8.7):
- 현재: File 객체 lookup → Window mount (file_id만)
- 신규: 추가로 `lookup_file_path(file_id)` → `std::fs::read_to_string(path)` → 1MB cap → Window.state.content 채움 + content_too_large set
- 실패 (binary, 권한 거부, 1MB 초과): content = "" + content_too_large = true (1MB 초과만), 그 외 에러 메시지

UTF-8 검증: `read_to_string`이 자동 (invalid → Err). 그 경우 content = "[텍스트 파일 아님 — viewer 미지원]".

mime 사전 필터:
- `text/*` → 시도
- 그 외 (`application/octet-stream`, `image/*`, etc) → 즉시 "[viewer 미지원]" 메시지, read 안 함

### 5.2 Scroll 단위 — 라인 단위

`scroll_y`는 *라인 인덱스* (i32). px 아님. 이유:
- 텍스트 viewer는 *고정 라인 높이* (20px 권장)
- 트리/list도 *고정 행 높이* (24px)
- 라인 단위가 *predictable* — *반 라인만 보임* 같은 시각 약점 없음
- 마우스 휠 1 notch = 3 라인 (Windows 표준 휠 동작)

### 5.3 클립 렌더링

- 영역의 rect(x,y,w,h) 안에 *내용 그리기 전*에 *scroll_y* 만큼 *위로 offset*
- 가시 라인 = `[scroll_y, scroll_y + visible_lines)`
- 가시 영역 밖 라인은 *그림 skip* (fill_rect 호출 안 함)
- max scroll_y = `total_lines - visible_lines` (양수 clamp)
- scroll_y < 0 또는 > max → 자동 clamp

### 5.4 줄바꿈 — Wrap or truncate?

긴 줄 (>가시 폭) 처리:
- **추천: truncate** — `…` 표시. 단순. 가로 스크롤 없는 v1엔 자연.
- 대안: 자동 wrap (다음 줄에 이어). UI 복잡 (라인 수 변동).

추천: truncate. 사용자가 *수평 스크롤* 요청하면 v2.

### 5.5 마우스 휠 입력

`compositor/src/main.rs`에 `WindowEvent::MouseWheel { delta, .. }` 핸들러 신규:
- delta.y > 0 → 위로 스크롤 (scroll_y 감소)
- delta.y < 0 → 아래로 스크롤 (scroll_y 증가)
- 1 notch = 3 라인 (Windows 표준)
- hit_test로 *어느 영역인지* 판정:
  - Window 본문 hit → 그 Window의 scroll_y SetState invoke
  - FileTree rect hit → FileTree.scroll_y
  - Explorer rect hit → Explorer.scroll_y
- desktop-shell이 invoke 받아 SetState broadcast → 컴포지터 다음 redraw

### 5.6 PageUp/Down — focused window일 때만 (v1)

- `KeyboardFocus::Window(id)` + PageUp/Down → 그 Window의 scroll_y ±= visible_lines
- FileTree/Explorer 스크롤은 *마우스 휠 only* (v1). 키보드 스크롤은 *focus 모델* 확장 필요 — v2.

---

## 6. 컴포지터 변경

### 6.1 `compositor/src/render.rs`
- `render_window`: 기존 preview 분기 *제거*. 대신:
  - obj.state.content + content_too_large 받음
  - 빈 content → "(읽을 수 있는 텍스트 없음)" 또는 사전 안내 메시지
  - content 있으면 *라인 분리* (`.lines()`) + scroll_y offset 적용 + 가시 라인만 그림 + truncate (긴 줄 `…`)
- `render_file_tree` 또는 `layout_tree_node_folders_only`: scroll_y 보고 *가시 행만 layout/render*. clip.
- `render_explorer_list`: 동일 패턴.

### 6.2 `compositor/src/main.rs`
- `WindowEvent::MouseWheel` 핸들러 신규.
- hit_test로 영역 판정 (Window vs FileTree vs Explorer vs Cli vs 빈영역).
- `keyboard_focus == Window(id)` + PageUp/Down → 같은 invoke.

### 6.3 `compositor/src/layout.rs`
- FileTree/Explorer layout이 *내부 scroll_y* 반영해 자식 rect의 y 조정.

---

## 7. desktop-shell 변경

### 7.1 `open_file` handler 갱신 (T8.7)
- Window mount 직전에 `read_file_to_window(file_path, mime) -> (String, bool)` 호출:
  - mime이 `text/*` 아니면 → ("[viewer 미지원: " + mime + "]", false)
  - read_to_string 성공 + 1MB 이하 → (content, false)
  - 성공 + 1MB 초과 → (content[..1MB], true)
  - Err (UTF-8 invalid 등) → ("[텍스트 파일 아님]", false)
- Window.state.content + content_too_large set 후 mount.

### 7.2 신규 invoke handler — `set_scroll`
- Cli/Explorer/FileTree/Window 모두 `set_scroll(scroll_y: i32)` invoke. desktop-shell이 그 객체의 state.scroll_y SetState로 broadcast.
- 또는 *Window는 자기 메서드 추가* (scroll_y는 *상태*, *메서드 없이* 직접 SetState도 가능). 단순화: SetState 메시지를 컴포지터가 *직접* 보냄 (invoke 대신). 단, server-host SetState ACL이 wildcard라 OK.
- **추천**: 컴포지터가 *직접 SetState* 보냄 — desktop-shell이 invoke 안 받음. 더 단순. (move/resize와 다르게 *순수 시각 상태*라 server-side 의미 없음. 그러나 broadcast로 AI가 봄 — 가시성 OK.)

대안: invoke를 통해 desktop-shell이 받고 SetState — *통합 흐름*. AI가 invoke 패턴 일관. 코드 약간 더.

**최종 결정: 컴포지터가 직접 SetState** (v1 단순). v2에 invoke 통합 검토.

---

## 8. Core 변경

### 8.1 `core/src/object/std_types.rs::window` (확장)
```rust
obj.set_state("scroll_y", json!(0));
obj.set_state("content", json!(""));
obj.set_state("content_too_large", json!(false));
```

### 8.2 `core/src/object/std_types.rs::file_tree` (확장)
```rust
obj.set_state("scroll_y", json!(0));
```

### 8.3 `core/src/object/std_types.rs::explorer` (확장)
```rust
obj.set_state("scroll_y", json!(0));
```

### 8.4 라운드트립 테스트
- 신규 state 필드 모두 라운드트립 + default 값 검증.

---

## 9. ADR 시드

- **ADR-033 — M8 part 2: 메모장 viewer + 공통 스크롤.** Window 내장 type-aware 렌더 결정 (별 객체 X). 1MB content cap. 라인 단위 scroll_y. 마우스 휠 + PageUp/Down. M9 편집 trigger.

---

## 10. Sub-task 분해 (예상 1.5~2주)

| Task | 주제 | 추정 |
|---|---|---|
| T8.13 | ADR-033 + core std_types (Window/FileTree/Explorer scroll_y + Window content) | 1일 |
| T8.14 | desktop-shell: open_file에 read_file_to_window + 1MB cap + mime 필터 | 1.5일 |
| T8.15 | compositor render: render_window text + truncate + clip | 2일 |
| T8.16 | compositor render: FileTree + Explorer scroll_y clip | 1.5일 |
| T8.17 | compositor input: 마우스 휠 + PageUp/Down 핸들러 + SetState 송신 | 1.5일 |
| T8.18 | Acceptance (사용자 시연: 큰 폴더 + 큰 텍스트 파일) + spec/quality review | 1일 |

---

## 11. 자체 점검

### 스펙 커버리지 (사용자 요청 매핑)
- "txt외에 다른 md파일이나 기타 파일을 열수가 없다" → §5.1 content fetch + §6.1 render_window text 분기 ✅
- "스크롤기능이 없어서 아래로 잘린 내용을 볼수없다" — Window 본문 → §5.3/§5.5/§6.1 ✅
- "좌측패널의 파일구조에서도 스크롤" — FileTree → §6.1 + §5.5 ✅
- "세 영역 모두" — Explorer 추가 → §6.1 + §5.5 ✅

### Tradeoff 위험
| 위험 | 완충 |
|---|---|
| 큰 텍스트 파일 (>1MB)을 *전체* 보내면 wire 대역폭 + JSON 직렬화 부담 | 1MB cap. 초과 시 first 1MB + 안내 메시지. |
| File content가 Window state로 *broadcast* — AI가 read 가능 (의도) | M8 read-only 일관 (ADR-027) — file *content*가 read 대상. 의도. 단 *민감 파일* (.env, *_secret 등) 사용자가 열면 AI가 봄 — T7.10 KI-015와 같은 클래스 부채. M9 권한 모델에서 통합. |
| FileTree 깊은 트리 (만 단위 자식 누적) — render 클립 안에도 *전체 layout 계산 비용* O(N) | M8 v1엔 N < 수천 가정. v2에 *virtual scroll* (가시 영역만 layout). |
| 휠 delta 처리 — winit 0.30 MouseScrollDelta::LineDelta vs PixelDelta 두 케이스 모두 처리 | 둘 다 라인으로 정규화 (PixelDelta는 16px = 1 라인). |
| `scroll_y` 값이 *음수* 또는 *max 초과* | clamp [0, max]. SetState 시 정규화. |

### 의도된 design 갭
- truncate (`…`) — 사용자가 *긴 줄 전체 보기* 필요 시 v2 가로 스크롤.
- PageUp/Down는 Window만 — FileTree/Explorer 키보드 스크롤은 v2.
- *수평 스크롤* 전무.
- *Find / search* 없음.

---

## 12. 다음 단계

1. Spec 사용자 review
2. `writing-plans` 스킬로 plan 작성 (T8.13~T8.18 세분)
3. subagent-driven-development로 task별 implementer
4. T8.18에 사용자 시각 검증 — 큰 폴더 (시스템 폴더 expand) + 큰 텍스트 파일 (Cargo.lock 등)
5. M8 T8.11 (acceptance 통합 문서) + T8.12 (final review)로 M8 part 1+2 정식 종료
