> **Status:** superseded (2026-05-18)
> **Note:** M7 방향이 메모장에서 *데스크톱 셸 (FileTree + Canvas)*로 전환 — 같은 날 작성된 `2026-05-18-geulos-m7-desktop-shell.md`가 실 구현 plan. 메모장 자체는 M8 Window viewer + M9 editor로 흡수.

# GeulOS M7 — 도그푸딩: 메모장 + AI 시나리오 + 보안 부채 해소

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. **NEVER push** — controller batches push at end.

**Goal:** 사용자가 *매일 켜는* 첫 GeulOS 시스템 — 호스트 모드 컴포지터 위에서 메모장 앱이 실행되고, 사용자가 텍스트를 타이핑·저장·재열기 할 수 있으며, Claude가 자율로 메모를 생성·요약·검색·정리. KI-001 wildcard ACL 보안 부채를 해소하고 *사용자 동의 다이얼로그*를 첫 도입.

**Why this milestone:** M6.5까지 *기술적 인프라*가 완성됐지만 (4층 통신, VM 부팅) 사용자가 *실제로 매일 쓸* 시스템은 없었음. echo-app은 데모이지 도구가 아님. M7이 *첫 진짜 도구*. 본 마일스톤이 끝나면 사용자 본인이 "오늘 메모는 GeulOS에서 쓰자"가 자연스러워져야 한다.

**Scope choice — 호스트 모드 우선:** VM 내 GUI(Phase D)는 8~12주 추가 작업이 필요한 *독립* 마일스톤. M7은 *호스트 모드 컴포지터* (M4 acceptance에서 이미 검증됨)에서 진행. Phase D 도착 시 같은 notepad-app이 VM 안으로 자동 포팅 — 앱은 OS 표면과 *분리*됐기 때문 (와이어 프로토콜이 같음).

**Architecture:**
```
사용자 키보드/마우스 ──┐
                       ▼
                  ┌────────────┐         ┌──────────────┐
                  │ compositor │ ──TCP──▶│ geulosd      │
                  │ (winit +   │◀── ──── │ (server-host)│
                  │  softbuffer│         │              │
                  │  + TextArea)         └─────┬────────┘
                  └────────────┘                │
                                                │ ACL gate
                                                ▼
                                          ┌─────────────┐
                  Claude API ──ai-bridge──▶│ notepad-app │
                                          │ (별 process)│
                                          │             │
                                          │ FS layer ───┼─▶ %APPDATA%/GeulOS/memos/*.txt
                                          └─────────────┘
                                                ▲
                                                │ 권한 다이얼로그 응답
                                          ┌─────┴───────┐
                                          │ user        │
                                          │ via         │
                                          │ compositor  │
                                          └─────────────┘
```

**Tech Stack:**
- 신규 객체 타입: `aios.std/Memo@1`, `aios.std/TextArea@1`, `aios.std/MemoList@1`
- 컴포지터 키보드 입력 (winit `WindowEvent::KeyboardInput`)
- 호스트 파일시스템 영속성 (`std::fs`)
- 권한 다이얼로그 신규 객체 + ACL 동적 추가

**Selection criteria (완료 조건):**
- `cargo build --workspace --all-targets` 그린, 경고 0
- `cargo test --workspace` 그린 (M0~M6 회귀 + 신규 notepad 테스트)
- 호스트에서 server-host + notepad-app + compositor + ai-bridge 4-터미널 시연:
  - 컴포지터 창에 메모 목록 + 활성 메모 TextArea 표시
  - 사용자가 키보드로 타이핑 → TextArea 즉시 갱신
  - "저장" 버튼 → %APPDATA%/GeulOS/memos/<title>.txt에 평문 저장
  - 재실행 시 폴더 스캔으로 기존 메모 자동 mount
  - AI 시나리오 4건(04~07) 모두 acceptance 통과
- KI-001 wildcard ACL grep 결과 0건 (echo-app도 정리됨)
- 권한 다이얼로그가 *AI의 첫 invoke 시* 사용자에게 표시되고 yes/no/always 선택 가능
- CI 그린

**Scope estimate:** 설계 §9.2의 *4주+* 추정. 실제로는 **5~7주**가 현실적 — 컴포지터 키보드 입력(처음 도입)과 권한 다이얼로그(처음 도입)가 새로운 영역이라 *디버깅 윈도우* 필요.

---

## ADR 시드

- **ADR-018 — 권한 동의 다이얼로그 디자인.** *사용자가 그 자리에서 yes/no/always* 결정. 결과는 동적 ACL 항목으로 객체에 추가. AI 토큰엔 *발급 시점에 박힌 범위*만 신뢰 — 다이얼로그 결과로 *세션 범위만* 확장 가능.
- **ADR-019 — 메모 파일 포맷.** 평문 UTF-8. 메타데이터(생성·수정 시각, 태그)는 TOML front-matter. 사용자가 GeulOS 밖에서도 *grep·텍스트 에디터*로 접근 가능. 이게 "AI-네이티브이되 사용자 친화적" 균형.

---

## 파일 구조 (사전 매핑)

```
apps/notepad-app/                       # 신규 크레이트
├── Cargo.toml
├── src/
│   ├── lib.rs                          # 메모 트리 구성, 메서드 로직
│   ├── main.rs                         # 진입점, server-host 연결, 이벤트 루프
│   ├── fs.rs                           # 메모 파일 IO
│   └── manifest.toml                   # 매니페스트 (TOML)
└── tests/
    ├── memo_logic_test.rs              # 단위 테스트
    └── fs_test.rs                      # IO 테스트

core/src/std_types.rs                   # 수정
                                        # + memo(), text_area(), memo_list()

compositor/src/
├── layout.rs                           # 수정: TextArea 케이스 추가
├── render.rs                           # 수정: TextArea 렌더 + 커서
├── hit_test.rs                         # 수정: TextArea 클릭 처리
├── text_input.rs                       # 신규: 키보드 → invoke 변환
├── main.rs                             # 수정: WindowEvent::KeyboardInput 처리
└── permission_dialog.rs                # 신규: 권한 동의 다이얼로그

core/src/permission.rs                  # 수정: dynamic ACL grant API

apps/echo-app/src/lib.rs                # 수정: KI-001 wildcard ACL 제거

ai-bridge/scenarios/
├── 04_write_memo.toml                  # 신규
├── 05_summarize_memo.toml              # 신규
├── 06_search_memos.toml                # 신규
└── 07_organize_memos.toml              # 신규

docs/
├── adr/018-permission-dialog.md        # 신규
├── adr/019-memo-file-format.md         # 신규
├── plans/2026-05-18-geulos-m7-notepad.md  # 이 문서
└── manual-tests/m7-acceptance.md       # 신규
```

---

## Task T1: 표준 타입 확장 — Memo, TextArea, MemoList

**Files:**
- Modify: `core/src/std_types.rs`
- Modify: `core/tests/std_types_test.rs` (or 신규)
- Optional: `core/src/lib.rs` re-export

타입 정의:

| 타입 URI | 역할 | props | state | methods |
|---|---|---|---|---|
| `aios.std/Memo@1` | 메모 한 건 | `title: String` | `body: String`, `created_at: i64`, `updated_at: i64`, `tags: [String]` | `insert_text(at: usize, text: String)`, `delete_range(from: usize, to: usize)`, `set_title(title: String)`, `set_tags(tags: [String])`, `save()` |
| `aios.std/TextArea@1` | 편집 가능한 텍스트 위젯 | `bound_memo: ObjectId` | `cursor_pos: usize`, `selection: Option<(usize, usize)>`, `focused: bool` | (compositor가 직접 다룸 — 메서드 없음) |
| `aios.std/MemoList@1` | 메모 목록 컨테이너 | — | `active_memo: Option<ObjectId>` | `create_memo(title: String)`, `delete_memo(id: ObjectId)`, `set_active(id: ObjectId)` |

- [ ] **Step 1: std_types.rs에 builder 함수 3개 추가**
- [ ] **Step 2: 각 타입의 method 시그니처가 wire 프로토콜로 정확 전송되는 round-trip 테스트**
- [ ] **Step 3: 커밋**

---

## Task T2: notepad-app 크레이트 스캐폴드 + manifest

**Files:**
- Create: `apps/notepad-app/Cargo.toml`
- Create: `apps/notepad-app/src/main.rs` (스켈레톤)
- Create: `apps/notepad-app/src/manifest.toml`

Cargo.toml: tokio, serde, geulos-proto 등 echo-app 패턴 따라.

manifest.toml — 매니페스트가 *권한을 명시*:
```toml
[app]
id = "aios.std/notepad"
version = "0.1.0"
title = "메모장"
description = "AI와 함께 쓰는 메모장"

[permissions]
# FS 권한 — 메모 폴더만
fs = [
    { path = "%APPDATA%/GeulOS/memos", access = "read-write" }
]
# 이 앱이 게시하는 객체 타입들
publishes = ["aios.std/Memo@1", "aios.std/MemoList@1", "aios.std/TextArea@1"]
```

매니페스트의 `permissions.fs`가 *서버 측에서 실제로 강제*됨 (KI-002 해소 시작). M7 범위에선 *path prefix 검사*만 — 더 정교한 capability 시스템은 후속.

- [ ] **Step 1: Cargo.toml + 스켈레톤 main.rs**
- [ ] **Step 2: manifest.toml**
- [ ] **Step 3: `cargo build -p geulos-notepad-app` 그린**
- [ ] **Step 4: 커밋**

---

## Task T3: 메모 데이터 모델 + 메서드 핸들러

**Files:**
- Create: `apps/notepad-app/src/lib.rs`
- Create: `apps/notepad-app/tests/memo_logic_test.rs`

핵심 로직:
- `Memo::insert_text(at, text)`: body 문자열에 삽입. 경계 검사. updated_at 갱신.
- `Memo::delete_range(from, to)`: 범위 삭제. 경계 검사.
- `MemoList::create_memo(title)`: 새 Memo 객체 생성 + 리스트에 추가 + active로 설정.

테스트 시나리오:
- insert_text가 한글·이모지 같은 멀티바이트 안전 (byte index 아닌 char index? — *byte index 사용* 결정. UTF-8 byte 경계만 검증)
- delete_range가 잘못된 범위(from > to, to > len) 거부
- create_memo가 unique title 자동 부여 (title 충돌 시 (1), (2) 접미사)

- [ ] **Step 1: lib.rs 로직**
- [ ] **Step 2: 단위 테스트**
- [ ] **Step 3: 커밋**

---

## Task T4: 파일시스템 영속성

**Files:**
- Create: `apps/notepad-app/src/fs.rs`
- Modify: `apps/notepad-app/tests/fs_test.rs`

저장 형식 (ADR-019):
```
%APPDATA%/GeulOS/memos/
├── <title>.md         # 본문 (UTF-8 평문 + Markdown)
└── <title>.toml       # 메타 (created_at, updated_at, tags)
```

대안 검토 (ADR-019에서 결정):
- 단일 파일 (front-matter): 사용자가 다른 도구로 열 때 *완전한 문서*. 그러나 GeulOS가 메타만 갱신해도 본문 파일이 변경된 것처럼 보임.
- 분리 (.md + .toml): GeulOS 객체 모델과 자연스러운 1:1. 사용자가 .md만 봐도 OK.

**결정: 분리.** 동기화는 *.md와 .toml이 같은 stem*으로 묶임.

부팅 시: `notepad-app` 시작 시점에 폴더 스캔 → 각 .md 발견 → Memo 객체 생성 + .toml 메타 로드.

저장 시: `Memo::save()` 호출 → updated_at 갱신 + 두 파일 atomic write (tempfile + rename).

- [ ] **Step 1: fs.rs — load_all + save**
- [ ] **Step 2: 테스트 (tmpdir + 실제 IO)**
- [ ] **Step 3: lib.rs와 통합 — notepad-app main.rs가 시작 시 load_all 호출**
- [ ] **Step 4: 커밋**

---

## Task T5: 컴포지터 TextArea 위젯 (렌더링 + 커서)

**Files:**
- Modify: `compositor/src/layout.rs`
- Modify: `compositor/src/render.rs`
- Modify: `compositor/src/hit_test.rs`

레이아웃:
- TextArea의 높이 = 컨테이너 내 *나머지 공간* (vstack의 마지막 자식이면 fill)
- 또는 명시적 props.height

렌더링:
- 배경 (밝은 회색)
- 텍스트 내용 (현재 fontdue를 그대로 사용, multi-line)
- 커서 (cursor_pos 위치에 깜빡이는 세로줄 1px)
- 선택 영역 (selection 있으면 반투명 강조)

힙트테스트:
- TextArea 클릭 → 좌표를 char index로 역변환 → cursor_pos 갱신
- 드래그 → selection 갱신 (M7 v2로 deferred — v1은 클릭만)

- [ ] **Step 1: 레이아웃 (TextArea fill 처리)**
- [ ] **Step 2: 렌더 (텍스트 + 커서)**
- [ ] **Step 3: 힙트테스트 (클릭 → cursor)**
- [ ] **Step 4: 커밋**

---

## Task T6: 컴포지터 키보드 입력

**Files:**
- Create: `compositor/src/text_input.rs`
- Modify: `compositor/src/main.rs`

winit의 `WindowEvent::KeyboardInput`을 캡처 → 활성 TextArea의 bound_memo에 `invoke_method("insert_text", {at: cursor, text: ...})` 또는 `delete_range`.

처리 키:
- 일반 문자 (printable Unicode) → insert_text
- Backspace → delete_range(cursor-1, cursor)
- Delete → delete_range(cursor, cursor+1)
- Arrow keys → cursor 이동 (TextArea state 갱신, server invoke 없음 — 로컬)
- Enter → insert_text("\n")
- Ctrl+S → 활성 Memo의 save() invoke
- Tab, Esc 등은 무시 (v2 deferred)

IME (한글 입력) — M7 v1에선 *영문만 지원* 솔직히 인정. 한글 IME는 Phase E 작업으로 따로 추적 (KI-013으로 등록).

- [ ] **Step 1: text_input.rs 작성**
- [ ] **Step 2: main.rs WindowEvent 디스패치 추가**
- [ ] **Step 3: 커밋**

---

## Task T7: 권한 다이얼로그 — KI-001 해소의 핵심

**Files:**
- Create: `compositor/src/permission_dialog.rs`
- Create: `docs/adr/018-permission-dialog.md`
- Modify: `core/src/permission.rs`
- Modify: `apps/echo-app/src/lib.rs` (wildcard ACL 제거)

흐름:
1. AI가 `invoke(memo_id, save)` 호출
2. 서버의 권한 게이트가 ACL 조회 — Allow가 없으면 **PermissionPending 상태로 큐잉**
3. 컴포지터에 `PermissionRequest` 이벤트 전달 (구독자) — payload에 actor·target·method·payload
4. 컴포지터가 dialog 객체 mount (모달, 화면 중앙):
   ```
   AI가 메모 "회의록"의 save를 호출하려 합니다.
   허용하시겠습니까?
   [ 이번만 ]  [ 이 세션 ]  [ 영구 ]  [ 거부 ]
   ```
5. 사용자 클릭 → 컴포지터가 `permission_response` invoke → 서버가 ACL 갱신 + 큐잉된 invoke 재실행
6. AI에게 응답 도달 — 사용자 입장에선 *몇 초의 지연*만 인식

KI-001 동시 해소:
- echo-app의 wildcard ACL 제거
- 대신 `aios.builtin/permission-broker` 객체가 *AI에게 보이는 권한 API*
- AI는 자기 권한 변경은 못 함 (ADR-009 S2 불변식 보존)

도전:
- 다이얼로그 객체 자체가 ACL을 *어떻게* 갖는가? — 사용자만 invoke 가능. user actor pattern을 ACL에 박음
- 다이얼로그를 보지 않는 환경(headless test)에서는? — 환경 변수 `GEULOS_AUTO_PERMIT=allow` 또는 `deny`로 비대화형 모드

- [ ] **Step 1: ADR-018 작성**
- [ ] **Step 2: core/src/permission.rs — PermissionPending 상태 + 동적 grant API**
- [ ] **Step 3: compositor/src/permission_dialog.rs — 다이얼로그 렌더 + 응답 처리**
- [ ] **Step 4: echo-app의 wildcard ACL 제거 — m3_smoke.gsh가 깨질 가능성, m3_smoke를 명시적 권한 부여로 갱신**
- [ ] **Step 5: 통합 테스트 — wildcard 없이도 외부 invoke 가능 (사용자 동의 후)**
- [ ] **Step 6: 커밋**

---

## Task T8: AI 시나리오 (L5)

**Files:**
- Create: `ai-bridge/scenarios/04_write_memo.toml`
- Create: `ai-bridge/scenarios/05_summarize_memo.toml`
- Create: `ai-bridge/scenarios/06_search_memos.toml`
- Create: `ai-bridge/scenarios/07_organize_memos.toml`

각 시나리오는 *권한 다이얼로그 통과* 후 작동. AI 어댑터에 `--auto-permit` 옵션 추가 (테스트용).

### 04_write_memo — *가장 단순한 첫 시연*
```
goal = "Create a new memo titled 'AI 첫 메모'. Then write the body:
'GeulOS에서 Claude가 처음으로 메모를 작성한 순간. 4층 아키텍처가 살아 있다.'
Save the memo. Use create_memo + insert_text + save.
Report the memo's final ObjectId via report_done."
budget.max_turns = 12
budget.max_wall_secs = 90
```

### 05_summarize_memo
```
goal = "Find all memos on the system. For each, fetch its body. Then create a
new memo titled '<오늘> 요약' containing 1-2 sentence summaries of each.
Save it. Don't modify the original memos."
budget.max_turns = 20
```

### 06_search_memos
```
goal = "Find all memos containing the word '회의' in their body or title.
Report the list via report_done — title + first 50 chars of body."
budget.max_turns = 10
```

### 07_organize_memos
```
goal = "Inspect all memos. For each, suggest 1-3 tags (Korean). Set them via
set_tags. Don't modify body. Report the final tag assignments."
budget.max_turns = 24
```

- [ ] **Step 1: 4개 시나리오 TOML 작성**
- [ ] **Step 2: ai-bridge에 `--auto-permit` 옵션 추가 (옵션 — 헤드리스 테스트용)**
- [ ] **Step 3: 각 시나리오 수동 실행해 통과 확인**
- [ ] **Step 4: 커밋**

---

## Task T9: M7 acceptance + 도그푸딩 시작

**Files:**
- Create: `docs/manual-tests/m7-acceptance.md`
- Modify: `README.md` (M7 acceptance 결과)

### acceptance 시나리오 (수동 검증)

1. **3 터미널 시작**: server-host, notepad-app, compositor
2. **컴포지터 창**: 메모장 UI — 좌측 메모 목록 + 우측 활성 메모 TextArea
3. **메모 작성**: 사용자가 "새 메모" 버튼 클릭 → 제목 입력 → TextArea로 본문 작성
4. **저장**: Ctrl+S 또는 "저장" 버튼 → %APPDATA%/GeulOS/memos/<제목>.md, .toml 생성 확인
5. **재시작**: 모든 프로세스 종료 → 다시 시작 → 메모가 자동 로드돼 보임
6. **AI 시연 (4번째 터미널)**: `cargo run -p geulos-ai-bridge -- run --scenario ai-bridge/scenarios/04_write_memo.toml --auto-permit`
   - Claude가 메모 생성 + 본문 작성 + 저장
   - 컴포지터 창에 *실시간* 반영 (구독 + StateSet)
7. **권한 다이얼로그**: `--auto-permit` 없이 실행 → 다이얼로그 뜸 → 사용자가 "이 세션" 클릭 → 통과

### 사용자 도그푸딩 약속

M7 마무리 시점부터 **2주간** 사용자 본인이 GeulOS 메모장으로:
- 일일 작업 계획 메모
- 회의 노트
- 코드 스니펫
- AI 보조 정리

매주 회고 메모 1건 작성 (메타 메모). 무엇이 좋고 어디가 거슬리는지 기록 → M8 우선순위 결정에 활용.

- [ ] **Step 1: m7-acceptance.md 작성**
- [ ] **Step 2: 사용자 수동 검증**
- [ ] **Step 3: 결과 README 반영**
- [ ] **Step 4: 커밋 + push (controller 일괄)**

---

## 자체 점검

**스펙 커버리지:**
- 설계 §9.2 M7 산출물: 메모장 + AI 시나리오 모두 포함 ✓
- 설계 §2.2 시나리오 B (일상 사용자) — M7이 *첫 진입점* ✓
- KI-001 (wildcard ACL) 해소 ✓
- KI-002 (매니페스트 권한 강제) — *부분 해소* (fs path prefix 만; full capability는 후속) ⚠️

**위험과 완충:**

| 위험 | 완충 |
|---|---|
| 컴포지터 키보드 입력 처음 도입 — winit API 미지조 | M7 v1은 *영문만*, IME는 KI-013으로 추적 |
| 권한 다이얼로그 디자인 함정 (피로감) | "이 세션" 기본 선택 + auto-permit 환경 변수 |
| 메모 동시 편집 충돌 (사용자 + AI 동시) | 단일 라이터 이벤트 루프가 *자동 직렬화* (ADR-003 결과) |
| 파일시스템 atomic write — Windows의 rename 제약 | std::fs::rename + 실패 시 정리 + 단위 테스트 |
| 4~6주 추정의 *낙관성* | 디버깅 윈도우 1~2주 여유 명시. M7.5는 *연기 옵션* |

**플레이스홀더 스캔:** TBD 없음. IME·다중 사용자·암호화 메모 등은 명시적으로 *out of scope*.

**알려진 한계 (M7 범위 밖):**
- 한글 IME (KI-013 신규)
- 메모 암호화
- 다중 사용자 (single-user OS 가정)
- 메모 그래프/링크 (위키 스타일 — 후속 마일스톤)
- 첨부 파일 / 이미지
- VM 내 실행 (Phase D 의존)

**Phase D와의 인터페이스:**
- M7 산출물 (notepad-app, TextArea 위젯, 권한 다이얼로그)은 *모두 OS 표면에 독립*
- Phase D에서 컴포지터를 Linux 백엔드(virtio-gpu + virtio-input)로 포팅하면 *같은 notepad-app이 VM 안에서 자동 동작*
- 이게 M7을 *호스트 모드에서 먼저*하는 정당화 — 작업이 낭비 안 됨
