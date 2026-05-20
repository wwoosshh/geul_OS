# M8 part 2 Acceptance — 메모장 viewer + 공통 스크롤

> **상위 통합:** M8 마일스톤 전체 acceptance는 `m8-acceptance.md` 참고. 본 문서는
> part 2 (메모장 viewer + 스크롤, T8.13~T8.20) 단독 검증 절차.

**Spec/Plan:** `docs/specs/2026-05-20-geulos-m8-notepad-viewer-scroll.md` + `docs/plans/2026-05-20-geulos-m8-notepad-viewer-scroll.md`

**범위:** T8.13~T8.17 통합 검증 — Window 본문 viewer + FileTree/Explorer/Window 세 영역 스크롤.

## 사전 조건

- ANTHROPIC_API_KEY 불필요 (이 마일스톤은 AI 무관)
- 3 cmd 또는 controller spawn으로 server-host, desktop-shell, compositor 띄움
- 시작 순서: server → desktop-shell → compositor (KI-004 회피)
- Windows: 클립보드/마우스 휠 정상 작동 환경

## 시나리오 A — Window 본문 viewer (T8.14 + T8.15)

| 단계 | 동작 | 기대 |
|---|---|---|
| A-1 | 컴포지터 띄움 + Explorer에서 임의 `.md` 파일 클릭 | Window 등장. *전체 내용*이 들어있어야 함 (이전 v1: preview 512바이트만이었음) |
| A-2 | Window 본문 영역에 마우스 휠 위/아래 | 텍스트 스크롤. 각 휠 notch = 3 라인 |
| A-3 | Window 본문 클릭 (focus 전환) → PageUp | 10 라인 위로 점프 |
| A-4 | PageDown | 10 라인 아래로 점프 |
| A-5 | 긴 한 줄 가진 파일 (예: minified JSON, Cargo.lock의 dependencies 라인) 열기 | 줄 끝이 `…`로 truncate |
| A-6 | `.rs`/`.py`/`.json`/`.toml` 등 다른 텍스트 파일 열기 | 정상 표시 (mime이 `text/*`면 cover) |

## 시나리오 B — 1MB 초과 파일

| 단계 | 동작 | 기대 |
|---|---|---|
| B-1 | `D:\GeulOS\Cargo.lock` 같은 1MB 근접 파일 열기 (없으면 임의 큰 텍스트 만들기) | 첫 1MB만 표시 |
| B-2 | Window 본문 끝까지 스크롤 | `[파일이 1MB 초과 — 일부만 표시]` 안내 등장 |

## 시나리오 C — 비-텍스트 파일

| 단계 | 동작 | 기대 |
|---|---|---|
| C-1 | `.png`/`.jpg`/binary 클릭 | Window 본문에 `[viewer 미지원: image/png]` 또는 비슷 |
| C-2 | mime이 `application/octet-stream`인 파일 | 동일 안내 |
| C-3 | 권한 거부 또는 누락 경로 (가능하면) | `[읽기 실패: ...]` 안내. 컴포지터 크래시 X |
| C-4 | UTF-8 invalid 파일 (예: `Cargo.lock`은 UTF-8이지만, 첫 N바이트만 UTF-8 invalid한 binary가 text mime로 잘못 추정된 경우) | `[텍스트 파일 아님 — UTF-8 디코딩 실패]` |

## 시나리오 D — FileTree 스크롤 (좌측, T8.16)

| 단계 | 동작 | 기대 |
|---|---|---|
| D-1 | 좌측에서 큰 폴더 expand (수십 개 자식, 예: `C:\Windows`) | 자식들 트리 펼침, 화면 넘침 발생 |
| D-2 | 좌측 트리 영역에서 휠 위/아래 | 자식 행이 스크롤됨 (28px/라인 stride, 한 notch = 3 라인) |
| D-3 | 스크롤로 가려져있던 자식들 다 볼 수 있는지 | 깊은 트리 끝까지 도달 가능 |

## 시나리오 E — Explorer 스크롤 (우측)

| 단계 | 동작 | 기대 |
|---|---|---|
| E-1 | Explorer가 큰 폴더 (예: `C:\Windows\System32`) navigate | 자식 list가 화면 넘침 |
| E-2 | 우측 Explorer 영역에서 휠 | list 행 스크롤 (24px/라인 stride) |
| E-3 | 스크롤 위치 — 시각적으로 *행이 잘리지 않음* (한 줄씩 깔끔히) | OK |

## 통과 조건

- A-1~A-6, B-1~B-2, C-1~C-3, D-1~D-3, E-1~E-3 모두 기대대로 작동
- 컴포지터 크래시 X (모든 에러는 안내 메시지로)
- CLI/Window drag/resize 등 기존 M8 part 1 기능 회귀 X
- AI chat (T7.7~T7.10)도 회귀 X — 단 이 task는 AI 비활성으로 검증

## 알려진 한계 (v2 또는 후속)

- 9px/char 휴리스틱 (max_chars_per_line) — 한국어/멀티바이트에서 truncate 위치 부정확. v2에 measure_text_width per char.
- PageUp/Down은 *Window focused*일 때만. FileTree/Explorer 키보드 스크롤은 마우스 휠만 (v2).
- 수평 스크롤 미지원 — 긴 줄은 truncate만. v2.
- md 진짜 markdown 렌더 X (plain text). v2에 pulldown-cmark.
- 이미지/PDF viewer X. v2.
- 검색 (Ctrl+F) X. v2.
- 파일/폴더 아이콘 X. 별 task.
- 매 휠 notch마다 SetState 송신 — 매우 빠른 스크롤 시 race / wire 부담 가능. v2에 rate limit.

## 회귀 가드

- 단위 테스트: T8.13 (4 std_types), T8.14 (5 file_read), T8.16 (2 layout). 총 11 신규 + 기존 회귀 X.
- 시각 검증: 이 문서 시나리오 5종.

## 후속 단계

- M8 T8.11 (acceptance 통합 문서) — M8 part 1 + part 2 합쳐서 정식 마일스톤 acceptance
- M8 T8.12 (final review) — 전체 dead code cleanup + 문서 정리
- M9 — 권한 다이얼로그 + 편집·저장 (write 메서드 복귀)
- 별 task: 파일/폴더 아이콘 (M8 part 3 또는 M9 묶음)
