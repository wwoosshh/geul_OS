# 아이콘 Acceptance (ADR-034)

**Spec:** `docs/specs/2026-05-20-geulos-icons.md`
**Plan:** `docs/plans/2026-05-22-geulos-icons.md`

## 사전 조건
- 3 프로세스 (server-host → desktop-shell → compositor, KI-004 회피 순서)
- ANTHROPIC_API_KEY 무관 (AI 비활성 OK)

## 시나리오 A — 폴더 아이콘 (FolderClosed/Open 전환)
1. 컴포지터 띄움 → 좌측 트리에 `[+] {folder-closed icon} C:\` 식 표시
2. `[+]` 좌측 영역 (~36px) 클릭 → expand → `[-] {folder-open icon} C:\` (closed → open 전환)
3. 다시 토글 클릭 → collapse → folder-closed 복귀
4. 우측 Explorer에서 폴더 항목 → folder-closed 아이콘 (Explorer에서는 expand 무관)

## 시나리오 B — 파일 타입별 아이콘 (우측 Explorer)
5. `D:\GeulOS\docs` navigate → 우측 list에 `.md` 파일 → **markdown 아이콘** (file-text 글리프)
6. `D:\GeulOS\src` 또는 `core/src` navigate → `.rs` 파일 → **code 아이콘**
7. `D:\GeulOS\Cargo.toml` 경로 → `.toml` 파일 → **config 아이콘** (settings 글리프)
8. `D:\GeulOS\compositor\icons` navigate → `.png` 파일 → **image 아이콘**
9. `.env` / `.gitignore` / `.editorconfig` → **dotfile 아이콘** (key-round 글리프)
10. `README` / `LICENSE` (확장자 없음) → **text 아이콘** (T8.19 guess_mime이 text/plain 반환)
11. `.zip` 파일 (있다면) → **archive 아이콘** (package 글리프)

## 시나리오 C — 미지원 / 기본값
12. `.xyz` 또는 알 수 없는 확장자 → **generic 아이콘** (file 글리프)
13. binary 파일을 Explorer에서 클릭 → Window 본문은 `[viewer 미지원]` 또는 `[텍스트 파일 아님]`이지만 *아이콘은 type에 맞게 표시* (예: .png → image 아이콘)
14. 아이콘 시각 구분 — folder closed vs open이 *명확히 다른 글리프*, 파일 카테고리 6종이 *서로 구분*

## 통과 조건
- A/B/C 모든 시나리오 *시각적으로* 정확한 아이콘 등장
- 아이콘 클릭 (또는 그 옆 텍스트 클릭) 시 *기존 동작* 회귀 X — 폴더 navigate, [+] 토글, File 클릭 → Window mount
- 좁은 폭 (FileTree 25%)에서 아이콘 + 텍스트가 *겹치지 않음* (텍스트 잘림은 OK, 가독성 유지)
- Window 본문 viewer/스크롤 / CLI / AI chat (M7~M8 전체) 기능 회귀 X
- 컴포지터 크래시 X (decode fallback이 작동하면 빈 사각형 — 그래도 크래시 X)

## 알려진 한계 (v2 또는 후속)
- Window title bar 아이콘 X (v2)
- 다크 모드 단일 셋 (Lucide 기본 light bg, 다크는 v2)
- 사용자 커스텀 아이콘 X (v2)
- 20x20 / 24x24 옵션 X (v1은 16x16 고정)
- 애니메이션 (folder open/close transition) X (v2)
- syntax-specific code 아이콘 X (모든 code → 같은 글리프, v2에 rs/py/js 분리 가능)

## 회귀 가드 (단위 테스트)
- compositor: icons 11 tests (10 라우팅 + 1 decode_all_icons_succeeds) — `cargo test -p geulos-compositor --lib icons`
- 기타 회귀 0 확인 — `cargo test --all` (~35 binary, FAILED 0)

## 후속
- T-icon.5: spec/quality review + push (T-icon.1~4 5 commits)
- M9: 권한 다이얼로그 + 편집·저장 (write 메서드 복귀)
- v2: 다크 모드, 사용자 커스텀, title bar 아이콘 등
