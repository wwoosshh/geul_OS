> **Status:** adopted (2026-05-31)
> **Note:** M10 정식 채택 (2026-05-23 마감), ADR-036. Phase 1~3(CRUD 메서드 + notify-rs watcher + Filesystem@1 escape hatch) 모두 구현, geulos-launcher 신설.

# M10 — 객체-네이티브 파일시스템 (cwd auto-mount + watcher + escape hatch)

**Date:** 2026-05-23
**Status:** Approved (사용자 승인 — Approach E 하이브리드)
**Parent:** M9 (편집/저장 + Dialog 인프라) 후속

## 동기

M9까지 AI는 *사용자가 명시적으로 연 File/Window*만 invoke 가능. 큰 프로젝트나 *진행 중인 작업 이어가기*에선 사용자가 모든 파일을 열어둘 수 없어 사용 불가.

기존 OS의 AI 도구 (Claude Code, Cursor, Aider 등)는 *path-based fs API*로 이를 해결하지만 — 그 결과 AI가 사용자가 보는 *살아있는 화면 상태*와 다른 정보를 봄. 사용자가 "지금 어떤 화면이야"를 매번 캡처/설명해야 함.

GeulOS 차별성은 *모든 게 객체화*되어 AI가 *현재 시스템 상태를 자동으로 본다*는 것 (README §"무엇이 다른가" 3·4). M10은 그 철학을 *파일시스템 차원*에서 실현한다:

> **사용자가 보는 GUI ≡ AI가 보는 object tree**

## 범위 (Approach E — 하이브리드)

**Phase 1 — 객체 메서드 (생성/삭제/rename)** [기존 객체-모델 확장]
- `Folder@1`에 `create_file(name)` / `create_folder(name)` / `delete(recursive)` / `rename(new_name)` 메서드 추가
- `File@1`에 `delete()` / `rename(new_name)` 메서드 추가
- M9의 `permission` + `Dialog@1` 인프라 재사용 — 디렉터리 단위 grant + 삭제 항상 confirm

**Phase 2 — cwd 자동 mount + file watcher** [GeulOS 철학 핵심]
- desktop-shell 시작 시 cwd (= 프로젝트 root) 결정. CLI `/root <path>`로 변경 가능 (옵션, 세션 한정).
- root 진입 시 root의 직계 children을 *자동 mount + subscribe*. 사용자가 FileTree expand하지 않아도 AI가 즉시 본다.
- `notify-rs` (또는 동등) file watcher가 cwd 재귀 감시 → 외부 변경 (다른 프로세스가 파일 수정/생성/삭제) 시 *자동으로* Folder/File 객체 갱신 + SetState broadcast → AI는 *subscribe만으로 변경을 받는다*.

**Phase 3 — cwd 밖 escape hatch** [실용성 안전망]
- 신규 `aios.builtin/Filesystem@1` singleton — 메서드 `read_external(path)` / `write_external(path, content)`만.
- cwd 안 호출은 *거부* + "해당 Folder/File 객체에 직접 invoke하세요" 안내.
- 매 호출 사용자 Dialog confirm (cwd 밖이라 항상 위험).

## 권한 모델

**디렉터리 단위 grant** (Claude Code acceptEdits 패턴):
- AI가 cwd 안 Folder의 첫 write/create/rename 호출 → Dialog `"AI가 <dir> 안에서 파일 작업을 시도합니다 — 이 디렉터리 허용?"` [허용]/[거부]
- [허용] 시 그 dir의 후속 write/create/rename은 confirm 없이 통과 (세션 한정, in-memory `granted_dirs: HashSet<PathBuf>`). 여기서 *세션 = desktop-shell process 한 번 실행*. AI 채팅 세션 (`/ai start ... /exit`)을 새로 시작해도 같은 desktop-shell process 동안은 grant 유지 — 한 작업 흐름 안에서 반복 confirm 막기 위함.
- `delete`는 항상 개별 confirm (위험)
- `read/list`는 cwd 안 자유 (사용자 부수효과 없음)
- cwd 밖 모든 작업 (read 포함) Dialog 확인 — `Filesystem@1.*_external`로만 도달 가능

권한 매트릭스 (M9 표 확장):

| Actor      | Read in | Write in (granted) | Write in (un-granted) | Delete | Read out | Write out |
|------------|---------|--------------------|-----------------------|--------|----------|-----------|
| local-user | Allow   | Allow              | Allow                 | Confirm| Allow    | Allow     |
| ai         | Allow   | Allow              | Confirm + grant       | Confirm| Confirm  | Confirm   |

## 새 타입 / 메서드

**Folder@1 추가:**
```
create_file(name: string) -> ObjectId
create_folder(name: string) -> ObjectId
delete(recursive: bool)
rename(new_name: string)
```

**File@1 추가:**
```
delete()
rename(new_name: string)
```

**`Filesystem@1` 신규 builtin (Phase 3):**
```
props: { root_path: string }   // 현재 cwd 표시용 (read-only props)
state: { granted_dirs: [string] }   // 시각 표시용 (실제 정책은 desktop-shell 내부)
methods:
  read_external(path: string) -> string
  write_external(path: string, content: string)
```

`granted_dirs`는 *시각 정보*만 — *실제 정책*은 desktop-shell의 in-memory `HashSet<PathBuf>`. SetState로 broadcast해 사용자가 어떤 dir이 grant되어 있는지 확인 가능.

## File Watcher 통합

`notify-rs` (cross-platform, Windows ReadDirectoryChangesW + Linux inotify + macOS FSEvents 추상화):
- desktop-shell 시작 시 cwd watcher spawn (Tokio task).
- 이벤트 → desktop-shell main의 mpsc 채널 → invoke handler가 받아서 SetState/Mount/Destroy 발송.
- last_change_actor: `"external"` (사용자도 AI도 아닌 외부 변경 — M5 노란 점과 다른 색 검토 — 회색?). v1은 `"external"`로 통일.

## 데이터 흐름 — AI write (cwd 안)

```
AI invoke Folder.create_file(name="foo.txt")
  → desktop-shell save 분기
  → permission::judge(actor=ai, op=CreateFile, dir=folder.path)
  → granted_dirs에 그 dir? Yes → Allow / No → ConfirmRequired
  → ConfirmRequired면 Dialog mount → 사용자 [허용] → granted_dirs.insert(dir)
  → fs::write(folder.path/name, "") + File@1 객체 mount + Folder.children에 추가
  → invoke 응답 OK
```

## 데이터 흐름 — 외부 변경 (file watcher)

```
사용자가 외부 에디터로 foo.txt 수정
  → notify-rs 이벤트 → desktop-shell mpsc
  → 해당 File@1 객체의 state.size/mtime/last_change_actor 갱신 + SetState broadcast
  → Window가 그 file_id를 가리키고 *editor_state가 active 아니면* content reload + SetState
  → AI가 subscribe 중이면 drain으로 그 변경 자동 인지 (사용자가 설명 X)
```

## 파일 구성

**신규**
- `apps/desktop-shell/src/folder_ops.rs` — create_file/create_folder/delete/rename 핸들러
- `apps/desktop-shell/src/file_ops.rs` — delete/rename 핸들러 (file_write.rs와 분리)
- `apps/desktop-shell/src/fs_watcher.rs` — notify-rs 통합 + mpsc + 이벤트 → invoke 변환
- `apps/desktop-shell/src/granted_dirs.rs` — HashSet 관리 + permission::judge 통합
- `core/src/object/std_types.rs` — Folder/File 메서드 + Filesystem@1 factory 추가
- `docs/adr/036-object-native-filesystem.md`
- `docs/manual-tests/m10-acceptance.md`

**수정**
- `apps/desktop-shell/src/main.rs` — Phase 1 (5 새 invoke 분기), Phase 2 (watcher spawn + 이벤트 처리), Phase 3 (Filesystem@1 mount + read_external/write_external)
- `apps/desktop-shell/src/permission.rs` — Op enum 확장 (Read/Write/CreateFile/CreateFolder/Delete/Rename × in/out), judge 시그니처에 (actor, op, path) — granted_dirs 참조
- `apps/desktop-shell/src/dialog_ops.rs` — PendingSave를 enum PendingFs로 (Save / CreateFile / CreateFolder / Delete / Rename / FsExternal)
- `apps/desktop-shell/Cargo.toml` — notify-rs dep
- `ai-bridge/src/system_prompt.md` — Folder/File 메서드 + Filesystem@1 가이드 + cwd 자동 mount 안내

server-side: 무변경. 모든 정책은 desktop-shell 내부.

## 알려진 한계 (v2/M11+)

- **granted_dirs 세션 영속 X** — 재실행 시 reset. v2에 `~/.geulos/granted.toml` 영속.
- **glob/grep/run_command 미포함** — M11+ 검토 (Bash는 보안 review 필수)
- **심볼릭 링크 따라가지 않음** (security)
- **atomic write 미적용** (M9와 동일 — crash 시 원본 손상 가능)
- **재귀 delete 안전장치 약함** — `recursive=true`면 진짜 재귀. v2에 *root-relative* depth 제한.
- **읽기 전용 grant 모드 없음** — read 안은 항상 자유, 밖은 항상 confirm. v2에 *프로젝트 외부 read 일괄 허용* grant 검토.
- **대량 파일 mount 부담** — cwd가 매우 크면 (예: node_modules 포함) mount 폭발. v2에 .gitignore 존중 + lazy expand 최적화.
- **다중 root 미지원** — 한 세션에 한 cwd. v2에 multi-root.
- **conflict resolution 없음** — AI write와 외부 write가 같은 파일에 동시 발생 시 마지막 승. v2에 mtime 기반 충돌 감지.

## 테스트

**단위**
- `folder_ops` (create_file/create_folder/delete/rename × 정상/에러 ~8)
- `file_ops` (delete/rename × 정상/에러 ~4)
- `granted_dirs` (insert/contains/grant 흐름 ~4)
- `permission::judge` 확장 표 (~12)
- `fs_watcher` 이벤트 → invoke 변환 mock (~4)
- `dialog_ops::PendingFs` enum 매핑 (~4)

**수동 (acceptance — 시나리오 G~M)**
- G: cwd 자동 mount 확인 (시작 직후 AI가 list로 모든 root 자식 인지)
- H: AI가 새 파일 생성 → Dialog → 허용 → 디스크 확인 + 같은 dir 후속 create는 confirm 없음
- I: AI가 파일 삭제 → 항상 Dialog (grant 무관)
- J: 외부 에디터로 파일 수정 → 자동 mount된 File 객체 state 갱신 + AI가 subscribe로 인지
- K: cwd 밖 path는 Folder/File 객체 없음 → AI가 `Filesystem@1.read_external` 사용 → Dialog → 허용/거부
- L: rename — granted dir 안은 자유, 밖은 confirm
- M: 회귀 — M7/M8/M9 모두 OK

## 후속 결정 (구현 plan 단계)

- notify-rs 버전 + features (recommended? default? — 빌드 부담 측정)
- watcher 이벤트 debounce — 1초 내 다수 변경 합치기 (큰 빌드 시 stutter 방지)
- `Filesystem@1` singleton의 mount ID 안정 — 매 실행 새 UUID vs 고정 UUID
- Folder.delete recursive=true 시 *모든 자식* destroyed=true 일괄 broadcast 효율
- mount 폭발 방지 — cwd 진입 시 *직계만* mount, 자식 폴더 expand 시 lazy. README/Cargo.toml/.gitignore가 보이도록 root *바로* 자식만 우선.
- file watcher에서 *방금 우리가 write한 변경*은 echo 무시 (debounce + actor 추적)
- 사용자 직접 액션 (UI 파일 생성 메뉴 등) — Phase 1 이후 별 메뉴/단축키 검토 (M11)
