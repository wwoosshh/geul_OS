# ADR-035 — 편집/저장 + 권한 다이얼로그 (M9)

- **상태:** Accepted
- **결정일:** 2026-05-22
- **부모 spec:** `docs/specs/2026-05-22-geulos-m9-edit-save-permission.md`

## Context

M8까지 Window는 read-only viewer. 편집/저장이 없어 텍스트 OS로 자라기 위한 *기초 write 메서드*가 부재. AI bridge가 write할 때 사용자 동의 흐름이 없으면 자동화가 위험.

## Decision

1. `File@1`에 `save(content)` 메서드 추가. desktop-shell이 핸들러 — `std::fs::write` + UTF-8 1MB cap.
2. `Window@1`에 `edit_mode: bool`, `dirty: bool` 상태 + `toggle_edit`/`save_to_file`/`close_confirm` 메서드.
3. `Dialog@1` 신규 builtin — props `(title, message, kind, actions)` + state `result`. modal (z 최상위 + 다른 입력 block).
4. desktop-shell에 `permission` 모듈: `judge(actor, op) -> Allow | ConfirmRequired`. v1 표는 `(local-user, Save)=Allow`, `(ai, Save)=Confirm`. M10에서 create/delete/rename 추가.
5. AI write 흐름: Dialog mount + `tokio::sync::oneshot`으로 응답 대기.

## Alternatives 검토

- **Inline edit (Window 항상 편집 가능)** — viewer/editor 구분 없음. UX 단순하지만 실수 우려. 거부.
- **별도 `Editor@1` 타입** — Window 둘로 분리. 코드 중복 + Explorer 메뉴 분리. 거부.
- **세션 grant 권한** — 첫 AI write OK면 세션 동안 자유. v1 단순화 위해 매 작업 confirm. v2 재검토.
- **server-side ACL 확장** — set_state ACL은 이미 wildcard pass (T8.19). 권한 정책을 server 측에 두면 두 곳에 흩어짐 — desktop-shell 단일 모듈로 격리.

## Consequences

- compositor 입력 처리에 *모드 분기* 등장 (Cli / Window viewer / Window editor / Dialog modal)
- desktop-shell이 비동기 응답 대기 패턴 도입 (PendingSave map + oneshot)
- M10 같은 권한 프레임워크 위에서 create/delete/rename 빠르게 추가
- v2: atomic write (temp+rename), undo/redo, multi-byte cursor 정확도

## Trade-offs

- v1은 utf-8 텍스트만 (binary edit_mode 비활성)
- 1MB 초과 파일 부분 편집 X (전체 save만, 미만 한도)
- 동시 AI write 큐잉 X — 두 번째는 즉시 reject

## 참고
- ADR-026/027/033 (M8 read-only 멀티 윈도우 + 뷰어 + 스크롤)
- T8.19 (set_state ACL wildcard pass)
- KI-011 (tombstone — Dialog destroy 패턴 일치)
