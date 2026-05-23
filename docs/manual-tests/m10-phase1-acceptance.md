# M10 Phase 1 Acceptance — Folder/File 객체 메서드 + Dialog grant

**Spec:** `docs/specs/2026-05-23-geulos-m10-object-native-filesystem.md`
**Plan:** `docs/plans/2026-05-23-geulos-m10-object-native-filesystem.md`
**ADR:** `docs/adr/036-object-native-filesystem.md`

## 사전 조건
- 3 프로세스 spawn 순서: server-host → desktop-shell → compositor
- ANTHROPIC_API_KEY (AI write 시나리오)
- 쓰기 가능한 작은 디렉터리 — 예: `D:\GeulOS\scratch\` (미리 비워두면 깔끔)

## 시나리오 H — AI 파일 생성 + 디렉터리 grant
1. compositor에서 FileTree → `D:\GeulOS\scratch\` 폴더로 navigate (Explorer에서 확인)
2. CLI에서 `/ai start` (또는 기존 세션 load) → 새 prompt 적용됨
3. AI에게: **"D:\GeulOS\scratch 안에 hello.txt 만들어줘"**
4. AI가 `list_objects_by_type("aios.std/Folder@1")` → `get_object`로 path 확인 → `invoke_method(target=<folder_id>, method="create_file", args={"name": "hello.txt"})`
5. **Dialog 모달** 등장: title "AI 파일 생성 확인", message "AI가 D:\GeulOS\scratch 안에 'hello.txt'를 생성하려고 합니다 — 허용?", 버튼 [허용] / [거부]
6. **[허용]** → Dialog 사라짐 + 디스크에 `hello.txt` (빈 파일) 생성 + FileTree·Explorer에서 새 File 객체 등장
7. (이어서) AI에게: **"같은 폴더에 world.txt 만들어줘"**
8. AI가 같은 Folder.create_file 호출 → **Dialog 없이** 즉시 생성 (grant 적용)
9. (다른 폴더 시도) AI에게: "D:\GeulOS\docs 폴더에도 readme_extra.md 만들어줘"
10. **다시 Dialog 등장** (다른 dir이라 별도 grant 필요)

## 시나리오 I — AI 파일 삭제 (항상 confirm)
11. AI에게: "hello.txt 지워줘"
12. **Dialog** 등장 (grant 무관) → [허용] → 파일 삭제 + 객체 destroyed=true
13. (다시) AI에게: "world.txt도 지워줘"
14. **Dialog 또 등장** (delete는 grant 따르지 않음)

## 시나리오 L — AI 이름변경
15. (이전 세션의 grant가 남아 있으므로) 새 파일 생성 후 AI에게: "그 파일을 final.txt로 이름변경"
16. **Dialog 없이** (rename은 dir grant 따라감, 같은 dir에 이미 grant)
17. 다른 dir 파일을 rename 시도 → **Dialog 등장**

## 시나리오 M — 사용자 직접 (UI 우회)
18. compositor에서 *사용자가* Window를 열어 Ctrl+S → permission 우회로 즉시 저장 (M9 유지)
19. 사용자 직접 액션은 *모두 UI 자체가 confirm* 역할이므로 Dialog 없음

## 통과 조건
- H/I/L/M 모두 시각·동작 정확
- delete는 *grant 무관* 항상 Dialog
- create/rename은 *grant 한 번 후* 같은 dir 자유
- 다른 dir 작업은 다시 Dialog
- M7/M8/M9 회귀 0

## 알려진 한계 (Phase 2/3 이전 — 의도된 v1 한계)
- **cwd 자동 mount 없음** (Phase 2) — AI는 *사용자가 expand한 dir의 children*만 보임. 큰 프로젝트는 사용자가 미리 폴더 클릭 필요.
- **외부 파일 변경 감지 없음** (Phase 2) — 외부 에디터로 파일 수정 시 객체 갱신 안 됨. desktop-shell 재시작해야 보임.
- **cwd 밖 path 접근 불가** (Phase 3) — `Filesystem@1.read_external` 미구현. AI는 mount된 객체에만 접근.
- **delete 후 tombstone broadcast 누락** — destroyed=true는 local만 갱신, 다른 actor의 tree에는 갱신 안 됨. M10 후속 fix 예정.
- **Save 분기 path-blind** — File.save (M9)는 dir grant 무관 항상 Dialog. 같은 dir에 create_file을 grant했어도 save는 별도 confirm 필요. M10 후속 fix 예정.
- **granted_dirs 세션 영속 X** — desktop-shell 재실행 시 reset.

## 회귀 가드 (자동)
- `cargo test --workspace` — FAIL 0
- 핵심 신규 테스트:
  - core: `folder_has_fs_methods` / `file_has_fs_methods` (2)
  - desktop-shell: `granted_dirs` (3) + `permission` (15) + `folder_ops` (6) + `file_ops` (4) + `dialog_ops` (3) + 기존
- `cargo fmt --check` / `cargo clippy --workspace --all-targets -- -D warnings` 클린

## 후속 (Phase 2 별도 plan)
- T10 notify-rs 7.x dep
- T11 fs_watcher.rs (notify-rs spawn + mpsc + 이벤트 → invoke 변환 + echo 무시)
- T12 main cwd auto-mount + watcher spawn
- T13 외부 이벤트 → 객체 state SetState (last_change_actor="external") + Window content reload
- T14 `/root <path>` CLI 명령 (옵션)
- T15 acceptance G/J

## 후속 (Phase 3 별도 plan)
- T16 std_types::filesystem() factory + 메서드
- T17 desktop-shell main Filesystem@1 mount + read_external/write_external 핸들러
- T18 acceptance K + AI prompt 갱신
