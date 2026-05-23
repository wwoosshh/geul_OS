# M10 Phase 3 Acceptance — Filesystem@1 escape hatch

**Spec/Plan:** `docs/specs/2026-05-23-geulos-m10-object-native-filesystem.md` Phase 3.
**ADR:** `docs/adr/036-object-native-filesystem.md`.

## 사전 조건

- 3 프로세스 spawn (`cargo run -p geulos-launcher`) — server-host + desktop-shell + compositor
- ANTHROPIC_API_KEY 환경 변수 설정 (CLI `/ai start` 흐름용)
- desktop-shell 로그에 `[desktop-shell] cwd = ...` 1줄 확인 (시작 시 cwd 결정 확인)
- `list_objects_by_type("aios.builtin/Filesystem@1")` 호출 시 *정확히 1개* 객체 ID 반환

## 시나리오 K — Filesystem@1 escape hatch (cwd 밖 read/write)

1. `/ai start` → AI 채팅 진입
2. AI에게 "C:\\Users\\Public\\Desktop\\test.txt 내용 읽어줘" 같은 cwd 밖 path 요청.
   (사전에 그 경로에 짧은 텍스트 파일 1개 만들어두기 — 예: `테스트 본문`.)
3. AI가 `list_objects_by_type("aios.builtin/Filesystem@1")` → fs_id 1개 확인.
4. AI가 `invoke_method(target=<fs_id>, method="read_external", args={"path": "C:\\..."})` 호출.
5. desktop-shell 로그 `[desktop-shell] read_external OK (<N> bytes) → ...` 확인.
6. AI가 `get_object(<fs_id>)` → `state.last_read_path` / `state.last_read_content` 에 본문 확인.
7. AI 답변에 그 본문이 포함되어 있어야 함.

## 시나리오 — cwd 안 거부 (객체 흐름 유도)

8. AI에게 "D:\\GeulOS\\README.md 내용 읽어줘" (cwd 안 경로) 요청.
9. AI가 잘못 `read_external`을 호출하면 desktop-shell 로그 `[desktop-shell] read_external 거부 — ... 는 cwd 안. File@1.read() 사용 권장 ...` 확인.
10. (system_prompt에 안내된 대로) AI는 `Folder.list` → `File.read` 객체-네이티브 흐름으로 대체.
11. cwd 안 path를 `write_external`로 호출해도 동일하게 거부 + Folder@1.create_file/File@1.save 안내 로그.

## 시나리오 — cwd 밖 write Dialog

12. AI에게 cwd 밖 경로에 write 요청 (예: "C:\\Users\\Public\\Desktop\\new.txt 에 'hi' 써줘").
13. desktop-shell이 `Dialog@1` (kind="warn", title="AI 외부 경로 write 확인") mount.
14. compositor 화면에 modal로 표시 — [허용] / [거부].
15. [허용] 클릭 → desktop-shell 로그 `[desktop-shell] write_external 승인 → ...`. 디스크에 실제 파일 생성 (탐색기로 확인).
16. [거부] 클릭 → 로그 `[desktop-shell] AI 요청 거부됨 (action=거부)`. 디스크 변경 없음.
17. 같은 dir에 두 번째 `write_external` 호출 시 Dialog 또 뜸 (cwd 밖이라 dir grant 모델 적용 X — 항상 confirm).

## 통과 조건

- 시나리오 K 모든 단계 성공 (read 본문 정확).
- cwd 안 호출은 *거부* + 객체 흐름 안내 로그 — Filesystem@1로 cwd 안 우회 차단.
- cwd 밖 write는 *항상* Dialog (read는 자유).
- M7-M10 Phase 1/2 회귀 0 — 기존 Folder/File 메서드 + watcher 정상.

## 알려진 한계

- `Filesystem@1`은 state로 read 결과를 노출 — 큰 파일은 wire 부담. v2에 streaming/청크 검토.
- `granted_dirs` state는 *시각 표시*만 — Phase 3 v1은 cwd 밖 dir grant 모델 도입 X (위험도 높음). 매번 confirm.
- `granted_dirs` (cwd 안) 세션 영속 X — desktop-shell 재시작 시 reset. v2에 영속 저장 검토.
- glob/grep/run_command 미포함 — M11+ 검토 (Bash 보안 review 필수).

## 후속 작업

- v2: read도 root-level grant 모드 옵션, streaming 본문 전송, granted dir 영속.
- M11+: glob/grep/run_command builtin (보안 review 후).
