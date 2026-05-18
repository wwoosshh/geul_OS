# ADR-027 — M8 Read-only 강제 (write 메서드 부재)

**Status:** Accepted (2026-05-18)
**Supersedes part of:** ADR-021 (워크스페이스 단방향 격리 → M8에서 격리 *해제*, 대신 read-only로 보호)

## Context
ADR-021은 M7에서 워크스페이스를 `%USERPROFILE%\GeulOS\workspace`로 *격리*하고 그 안에서만 write를 허용했다. M8은 사용자 결정으로 **전체 파일시스템 접근**으로 확장 — Windows 모든 드라이브를 root로 mount하고 자유롭게 탐색.

격리 해제와 *동시에* write 활성화는 보안 후퇴가 너무 크다:
- AI가 임의 경로에 write 가능 (KI-001 wildcard ACL 잔존 상태에서)
- 실수로 시스템 파일 변조 위험
- 사용자 명시적 권한 grant UX(M9 권한 다이얼로그) 부재

옵션:
- **ACL Deny 추가** — 매니페스트 기반 deny 룰. KI-001 wildcard ACL이 살아있는 동안에는 의미 약함. ACL 시스템 자체가 미완성.
- **화이트리스트 경로** — 특정 경로만 write 허용. 사용자가 어떤 경로를 grant했는지 모호, UX 복잡.
- **팩토리에서 write 메서드 제거** — `std_types::file`/`folder`가 만드는 객체에 write/create/delete/rename 메서드 자체가 *없음*. invoke 시 자연스럽게 `MethodNotFound`. ACL 이전 단계에서 차단.

## Decision
M8 동안 `std_types::file` / `std_types::folder` 팩토리에서 다음 메서드를 *부재*시킨다:
- `File@1`: `write`, `delete`, `rename`
- `Folder@1`: `create_file`, `create_folder`, `delete`, `rename`

ACL deny가 아닌 *메서드 부재*가 핵심. invoke 측에서 어떤 actor든 동일하게 `MethodNotFound`를 받는다.

- **fs_ops 함수는 유지:** `apps/desktop-shell/src/fs_ops.rs`의 `atomic_write` / `create_empty_file` / `delete_file` 함수는 dead code로 보존 (`#[allow(dead_code)]` + M9 복귀 트리거 주석). M9에서 메서드 복원 시 호출 분기 재활성만 하면 됨.
- **솔로 dogfooding 가정:** M8은 사용자 본인 머신, 본인 AI. 멀티-유저/원격 invoke는 가정 없음. read-only로도 *탐색 + AI 분석* 시나리오 충분.
- **M9 복귀 트리거:** 권한 다이얼로그 마일스톤 도착 시 (1) 팩토리에 메서드 재추가 (2) fs_ops 호출 분기 재활성 (3) `#[allow(dead_code)]` 제거.

## Consequences
- M8 동안 *전체 FS 탐색은 가능하지만 변조 불가능* — 안전한 dogfooding 환경.
- KI-001(wildcard ACL)이 잔존해도 write는 차단됨 — 메서드가 없으니 ACL 분기 자체 도달 X.
- 사용자가 GeulOS 내부에서 파일을 만들거나 편집할 수 없음 — Windows 탐색기/외부 에디터 사용. *명시적 M8 한계*로 README/매뉴얼에 문서화.
- KI-002(워크스페이스 외부 fs 접근) 의미가 변함 — M8은 *읽기 한정*으로 외부 접근 정식 허용.
- M9에서 write 복귀할 때 권한 다이얼로그가 *없으면* AI가 임의 write 가능 — M9 마일스톤이 권한 UX와 메서드 복귀를 *함께* 묶는 이유.

## 참고
- 관련 ADR: ADR-021 (워크스페이스 단방향 — 본 ADR이 부분 supersede), ADR-009 (AI 기본 불신), ADR-006 (매니페스트 기반 권한)
- 관련 known-issues: KI-001 (wildcard ACL), KI-002 (워크스페이스 외부 fs 접근)
- 관련 spec: `docs/specs/2026-05-18-geulos-m8-multi-window-explorer.md` §3, §6
