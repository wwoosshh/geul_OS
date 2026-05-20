# M8 Acceptance — 전체 파일시스템 + 멀티-윈도우 탐색기 통합

**마일스톤 spec/plan:**
- `docs/specs/2026-05-18-geulos-m8-multi-window-explorer.md` (part 1)
- `docs/plans/2026-05-18-geulos-m8-multi-window-explorer.md` (part 1)
- `docs/specs/2026-05-20-geulos-m8-notepad-viewer-scroll.md` (part 2)
- `docs/plans/2026-05-20-geulos-m8-notepad-viewer-scroll.md` (part 2)

**범위:** T8.0~T8.10 (part 1) + T8.13~T8.20 (part 2) + 6 회귀 fix (KI-004 type-subscribe,
KI-013 Get/Event race, tree_model orphan, Lifecycle parse, set_state ACL wildcard,
word-wrap 한글).

**T8.18 별도 문서:** `m8-notepad-acceptance.md` (part 2 5 시나리오) — 이 문서가
*마일스톤 통합* acceptance이며 그것을 포함한다. 시각 검증 시 둘 다 수행할 필요 없음;
본 문서의 시나리오 D가 part 2 핵심을 cover.

## 사전 조건

- Windows 11, `ANTHROPIC_API_KEY`는 시나리오 G(AI 도그푸딩)에서만 필요
- 빌드:
  ```powershell
  cargo build -p geulos-server-host -p geulos-desktop-shell -p geulos-compositor
  ```
- 3 cmd 또는 controller spawn으로 띄우는 순서:
  **server-host → desktop-shell → compositor** (KI-004 회피)
- 시각적 검증 위주 + 디스크는 read-only (ADR-027 — write 메서드 부재)
- 한국어 IME 활성화 (시나리오 E-2)
- 폰트: `compositor/fonts/font.ttf` + 한글 글리프 fallback (Noto Sans KR Regular,
  commit `255e06e`로 임베드)

## 시나리오 A — 드라이브 자동 mount + 탐색 (T8.3 / T8.5)

| 단계 | 동작 | 기대 |
|---|---|---|
| A-1 | 3 프로세스 띄움 | 컴포지터 창에 좌측 FileTree에 `[+] C:\`, `[+] D:\` 등 *모든 사용 가능* 드라이브 |
| A-2 | `[+] C:\` 좌측 약 36px (=ExpandToggle hit area) 클릭 | 좌측 트리에 `C:\` 직계 자식 등장 (lazy mount — 첫 click 시점에 fs scan) |
| A-3 | `C:\` 폴더 텍스트 클릭 (Body hit area) | 우측 Explorer에 `C:\` 내용 list — *폴더 먼저 + 이름순* |
| A-4 | 다른 드라이브 (D:\ 등) expand | 동일 동작. 드라이브 간 독립 |

## 시나리오 B — Explorer 탐색 + Window 다중 (T8.6 / T8.7)

| 단계 | 동작 | 기대 |
|---|---|---|
| B-1 | Explorer에서 폴더 클릭 | 그 폴더로 navigate, 우측 list 갱신 (active_folder 변경) |
| B-2 | Explorer에서 *파일* 클릭 (예: README.md) | 새 Window 등장 — title bar + 본문 + [x] 닫기 + 우하단 resize handle |
| B-3 | 다른 파일 클릭 | 두 번째 Window가 cascade(+30, +30) 위치에 등장, *새 윈도우가 focused* |
| B-4 | 같은 파일 다시 클릭 | 새 윈도우 생성 X; *기존* Window focus + z-order 최상위 (T8.7 dedup) |

## 시나리오 C — Window 조작 (T8.9 / T8.10)

| 단계 | 동작 | 기대 |
|---|---|---|
| C-1 | Window title bar drag | 위치 이동 (drag end 시점에 한 번 set_state — 중간 시각 피드백 X, v1 trade-off) |
| C-2 | Window 우하 코너 drag | 크기 변경 (resize 종료 시 set_state) |
| C-3 | Window 본문 클릭 | focus 전환 (다른 윈도우 unfocus, title bar 색 변화) |
| C-4 | [x] 빨간 닫기 버튼 | 윈도우 사라짐 (emit_destroyed → tombstone), 다른 윈도우 자동 focus |

## 시나리오 D — Viewer + 스크롤 (T8.13 ~ T8.20)

| 단계 | 동작 | 기대 |
|---|---|---|
| D-1 | `.md` 또는 `.rs` 파일 열기 | Window 본문에 *전체 내용* (1MB cap) + 한글 자동 줄바꿈 (T8.20 word-wrap) |
| D-2 | Window 본문 마우스 휠 | 3 lines/notch 스크롤 |
| D-3 | Window focused + PageUp/PageDown | 10 lines/page 점프 |
| D-4 | 1MB 초과 파일 (예: 큰 log) | 첫 1MB만 + `[파일이 1MB 초과 — 일부만 표시]` 안내 |
| D-5 | `.png`/binary 파일 클릭 | `[viewer 미지원: image/png]` 형태 안내 (크래시 X) |
| D-6 | `.env`/`.gitignore`/README (확장자 없음/특수) | text/plain 인식, 정상 viewer |
| D-7 | 끝까지 스크롤 후 더 휠 | scroll_y가 누적 안 됨 — 즉시 다시 위로 스크롤 가능 (T8.20 clamp) |
| D-8 | 좌측 FileTree에서 큰 폴더 expand + 휠 (예: `C:\Windows`) | 좌측 트리 행이 스크롤 (28px stride, 3 lines/notch) |
| D-9 | 우측 Explorer에서 큰 폴더 navigate + 휠 (예: `System32`) | 우측 list 스크롤 (24px stride) |

## 시나리오 E — CLI (M7 보조, T7.5 / T7.6 / T7.7 / T7.8 / T7.9 / T7.10)

> M8 회귀 가드 — CLI 기능이 M8 코드 변경에 영향받지 않았음을 시각 확인.

| 단계 | 동작 | 기대 |
|---|---|---|
| E-1 | 하단 CLI 클릭 → `help` Enter | 명령 목록 (한글 안내 포함) |
| E-2 | `echo 안녕하세요` Enter | 한글 IME 작동 (preedit 회색 → commit) + echo 출력 |
| E-3 | `/ai start` Enter | (API key 있으면) AI 모드 진입, 없으면 `[API key 입력]` awaiting 모드 |
| E-4 | (E-3에서 awaiting 진입 시) API key 입력 후 Enter | 검증 → `~/.geulos/api_key` 저장 → AI 모드 자동 진입 |
| E-5 | 한국어 prompt 입력 → AI 응답 | history 누적 (`~/.geulos/ai-sessions/<name>.json`) |
| E-6 | `/exit` Enter | 일반 셸 모드 복귀 |
| E-7 | `/ai list` → `/ai load <name>` | 이전 세션 불러오기, 이전 history 화면 복원 |

## 시나리오 F — Read-only 검증 (ADR-027)

> 외부 client (예: `geulosh --connect 127.0.0.1:5550`)가 필요한 선택 시나리오.

| 단계 | 동작 | 기대 |
|---|---|---|
| F-1 | 외부 client로 `invoke <file_id> write '{"content":"x"}'` | `unknown_method` 에러 (write 메서드 부재; M9에 복귀) |
| F-2 | `invoke <folder_id> create_file '{"name":"x"}'` | 동일 에러 |
| F-3 | 디스크 파일 변경 X 확인 (Explorer 새로고침 / 외부 stat) | 무변경 |

## 시나리오 G — AI 도그푸딩 (옵션, `ANTHROPIC_API_KEY` 필요)

> ADR-009(객체 모델 가시성)의 *체감* 시연. M8이 *AI에 무엇을 보여주는지* 확인.

| 단계 | 동작 | 기대 |
|---|---|---|
| G-1 | `/ai start` 후 `현재 데스크톱에 어떤 Window가 열려있나요?` | AI가 `query type aios.builtin/Window@1` tool 호출 → 응답에 윈도우 일람 (title + 본문 일부) |
| G-2 | `현재 우측 Explorer가 어떤 폴더를 보고 있나요?` | AI가 `query type Explorer + get Explorer` → active_folder의 path 추출 → 자연어 답변 |
| G-3 | (위 두 시나리오가 AI에게 *객체 모델 가시성*을 증명 — ADR-009 시연) | 사용자 체감으로 확인 |

## 통과 조건

- A-1~A-4, B-1~B-4, C-1~C-4, D-1~D-9, E-1~E-7 모두 작동
- F-1~F-3은 외부 client 필요 — 선택. 통과 시 read-only 보장 보강
- G-1~G-3은 API key 필요 — 선택. 통과 시 AI dogfood 통과
- 컴포지터 크래시 X (모든 에러는 안내 메시지로 흡수)
- 회귀 0 — M7 T7.5/T7.6/T7.7/T7.8/T7.9/T7.10이 *동시 작동* (시나리오 E)
- AI session 영속 확인 (`~/.geulos/ai-sessions/`)

## 알려진 한계 (M9 또는 후속)

`docs/known-issues.md`의 M8 종료 시점 상태와 동기화:
- KI-001 wildcard ACL — M8 동안 FileTree/Explorer/Folder/File/Window에도 확장 (부채 확대). M9에 통합 정리.
- KI-002 manifest permissions 미강제 — M9.
- KI-004 type-level subscribe — ✅ 해소 (2026-05-18, `2f25e73`).
- KI-013 compositor Get/Event race — ✅ 해소 (2026-05-18, `98e5edf` fire-and-forget).
- KI-014 한글 IME preedit cursor 위치 — 부분 해소; v2.
- KI-015 chat history 잔존 key — v2 (사용자 수동 조치 권장).
- KI-016 신규 — set_state ACL wildcard 통과 (T8.19). M9에 wildcard 전체 제거 + 매니페스트 권한.
- KI-017 신규 — scroll word-wrap 정확도 (14px char 휴리스틱). v2에 fontdue per-char measure_text_width.

기능 한계:
- **편집·저장 X** — M9 권한 다이얼로그 + write 메서드 복귀
- **파일·폴더 아이콘 X** — 별 task (M8 후속)
- **이미지/PDF viewer X** — v2
- **검색 (Ctrl+F) X** — v2
- **syntax highlighting X** — v2
- **markdown 진짜 렌더 (h1/bold/list) X** — v2 (pulldown-cmark)
- **수평 스크롤 X** — wrap만 (v2)
- **drag 중 시각 피드백 X** — drop 시 한 번 set_state (v1 trade-off)
- **AI blocking await** — UX 약점, v2 비동기 spawn
- **9px → 14px char width 휴리스틱** — 한글/wide-char 정확도 약함, v2 fontdue 정확 측정

## 회귀 가드 (단위 + 통합 테스트)

- **core:** std_types 25+ tests (Window/Explorer 팩토리, Folder/File read-only,
  scroll_y/content state, ACL wildcard)
- **compositor:** layout 16 + render 5 + tree_model 5 + server_client 1
  (std_types_query_coverage_smoke)
- **desktop-shell:** drives 2 + lazy_mount 3 + file_read 5 + window_ops 5 +
  cli_handler 22 + invoke_handler 4 + 그 외
- **ai-bridge:** api_key 6 + chat_session 3 + chat_persist 4 + adapter unit

총 약 120 단위/통합 테스트. `cargo test --all` 결과 0 failed.

```powershell
cargo test --all
```

## 후속 단계

- **M8 T8.12** — final review + dead code cleanup (workspace.rs/scan.rs/fs_ops.rs 등
  M8 중 사용되지 않게 된 path 정리)
- **별 task — 파일·폴더 아이콘** (UX 보강, brainstorm 필요)
- **M9** — 권한 다이얼로그 + 편집·저장 (write 메서드 복귀, wildcard ACL 전체 제거,
  KI-001/KI-002/KI-016 일괄 해소)
