# ADR-028 — 드라이브 자동 mount + 폴더 Lazy expand (M8)

**Status:** Accepted (2026-05-18)
**Supersedes part of:** ADR-021 (단일 워크스페이스 root → M8 multi-root)

## Context
ADR-021은 단일 root path (`%USERPROFILE%\GeulOS\workspace`)를 가정했다. M8 전체 FS 접근은 multi-root (각 드라이브 letter) + 동적 트리(폴더 expand 시 확장) 모델을 요구한다. 사용자 발언: "윈도우의 다른 파일에 접근을 할수가 없는상태인거야" — "내 PC 스타일"의 첫 인상.

마운트 전략 옵션:

- **시작 시 전체 재귀 mount** — 모든 드라이브의 모든 디렉터리를 재귀 스캔하여 Folder/File 객체 생성. *기각*: 메모리 폭주 (Windows 일반 사용자 머신 수십만~수백만 객체), 시작 시간 분 단위.
- **사용자가 CLI로 명시 `mount <path>`** — 안전·명시적. *기각*: 데스크톱 첫 인상이 빈 화면. 사용자가 "내 PC 스타일"을 선호 — 시작하자마자 드라이브들이 보여야 함.
- **시작 시 드라이브 letter만 mount + 폴더 expand 시 직계 자식 lazy mount** — 첫 인상은 "내 PC", 깊이는 사용자가 클릭한 만큼만 메모리.

## Decision
- **드라이브 열거:** desktop-shell 시작 시 Windows API `GetLogicalDrives` (winapi crate) 호출. 비트마스크에서 letter 추출 (`C:\`, `D:\`, ...) → 각각 `Folder@1` mount (children=[]).
- **Lazy expand:** 폴더 expand 이벤트 시점에 `read_dir`로 *직계 자식*만 스캔, Folder/File 객체로 mount. 한 번만 — 이미 children이 비어있지 않으면 re-fetch X.
- **Collapse:** children unmount하지 않음 (M8 v1 — 메모리 정리는 v2).
- **비-Windows fallback:** `[target.'cfg(not(windows))']`에서 단일 `/` root 하나만 mount. Linux/macOS 본격 지원(/Volumes, /mnt 등)은 후속.
- **권한 거부 폴더:** `read_dir` 결과가 `Err(PermissionDenied)` 등인 경우 *빈 폴더로 silent* — `System Volume Information`, `$Recycle.Bin`, 사용자 권한 외 경로 등. M8 trade-off: 에러를 UI에 표시하면 복잡(어디에·언제·dismiss) → M9의 토스트/다이얼로그 시스템과 함께 정식 처리.
- **winapi dependency:** `apps/desktop-shell/Cargo.toml`에 `[target.'cfg(windows)'.dependencies] winapi = { version = "...", features = ["fileapi"] }`.

## Consequences
- 첫 인상이 "내 PC" — 사용자 기대 UX 충족.
- 메모리 사용량은 *사용자가 클릭한 만큼*만 비례 — 안전.
- collapse 후 re-expand 시 *기존 children 그대로 사용* (re-fetch X) → 외부 fs 변경은 반영 안 됨. M9의 양방향 동기화/FS watcher 마일스톤과 함께 refresh 정책 정립.
- 권한 거부 폴더가 *빈 폴더로 보임* — 사용자가 "왜 비어있지?" 혼란 가능. M8 명시적 한계로 README/매뉴얼에 문서화. M9 UX 마일스톤이 정식 처리.
- 비-Windows에서는 `/`만 root — 개발/테스트는 가능하나 본격 사용 X. Linux/macOS 정식 지원은 별도 마일스톤.
- 드라이브가 *런타임 중 추가/제거*되는 케이스 (USB 삽입/제거)는 M8 미지원. 재시작 필요. M9+에서 처리.

## 참고
- 관련 ADR: ADR-021 (워크스페이스 단방향 — 본 ADR이 단일 root 가정 부분 supersede), ADR-027 (M8 read-only — 본 mount는 read-only로 보호됨)
- 관련 spec: `docs/specs/2026-05-18-geulos-m8-multi-window-explorer.md` §3, §5
- 외부: Windows API `GetLogicalDrives` (kernel32.dll)
