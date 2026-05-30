# GeulOS 호스트 브리지 v1.5 — 보안 강화 + 쓰기 (Model B 증분 ①)

> 상태: 설계 승인 대기 → writing-plans 전환 예정
> 일자: 2026-05-30
> 이전: [v1 spec](2026-05-29-geulos-host-bridge.md) (호스트 C:/D: 읽기 탐색)

## 배경

v1으로 파일관리자가 호스트 C:/D:를 *읽기 전용*으로 탐색하고 텍스트 파일을 본문 표시까지 가능해졌다(spec: read_file 포함됐으나 v1 plan에선 deferred). Model B 비전을 끝까지 이루려면 ① 보안 강화 + 쓰기 → ② 실행(npm/build/dev)으로 두 증분으로 진행하기로 했고, 본 spec은 **증분 ①**(보안 + 쓰기 + 파일 열람 path 완성)을 다룬다. ②(실행)는 별도 spec.

## v1.5 범위 결정 사항 (brainstorm 결과)

- **분해**: 증분 ① = 보안 + 쓰기 (이 spec). 증분 ② = 실행 (다음 spec).
- **쓰기 트리거 UX**: 파일관리자 창 상단 툴바 4 버튼 + **단일클릭=선택 / 더블클릭=열기** 전환 (Explorer.state.selected_item 추가, 더블클릭 감지). 발견성 좋고 새 인프라(컨텍스트 메뉴) 불필요. AI=사용자2 — 같은 객체 메서드.
- **보안 토큰 전달**: launch.ps1 per-launch 난수 토큰 → 브리지엔 env, 게스트엔 **커널 cmdline** `geulos.bridge_token=<hex>`. geulos-init이 파싱해 `/run/geulos/bridge.token`에 저장, desktop-shell이 시작 시 읽어 첫 프레임 Auth.
- **허용목록 기본**: 전체 드라이브(현 read v1 범위와 동일). 사용자 설정은 후속.
- **AI Dialog 확인**: 기존 ShellRunner 패턴(AI 발신 op → Dialog mount → 사용자 응답) 그대로 재사용.

## 아키텍처

```
┌─ Windows 호스트 ──────────────────────────────────────┐
│  launch.ps1: 난수 토큰 생성 → bridge env, guest cmdline│
│  geulos-host-bridge.exe                                │
│   - 127.0.0.1:5560 listen                              │
│   - 첫 프레임 Auth{token} 검증 → 아니면 연결 종료      │
│   - canonicalize + base-dir 허용목록 재검사            │
│   - 읽기: list_drives/list_dir/stat/read_file (v1)     │
│   - 쓰기(NEW): write_file/create_dir/remove/rename     │
└────────────────────▲───────────────────────────────────┘
                     │ TCP 10.0.2.2:5560 + token Auth
┌─ QEMU VM (GeulOS) ──┴───────────────────────────────────┐
│  geulos-init: /proc/cmdline → /run/geulos/bridge.token  │
│  desktop-shell                                           │
│   - host_bridge_client: 시작 시 토큰 읽음 → 첫 Auth     │
│   - 파일관리자 toolbar: [+파일][+폴더][이름변경][삭제]  │
│   - Explorer.selected_item + 더블클릭 감지 (compositor) │
│   - 호스트 파일 open → bridge.read_file → Notepad 표시  │
│   - Notepad save → 호스트 path면 bridge.write_file 라우팅│
│   - AI 발신 write → 기존 Dialog 흐름(허용 시 실행)     │
│  compositor: 선택 row 하이라이트, 더블클릭 감지         │
└──────────────────────────────────────────────────────────┘
```

### 컴포넌트 변경/추가

1. **`crates/geulos-host-bridge`** (수정)
   - `protocol.rs`: `Auth{token}` 요청 추가, `WriteFile`/`CreateDir`/`Remove{recursive}`/`Rename`. `Response::Auth{ok}`, `Response::Ok`.
   - `fs_ops.rs`: write_file/create_dir/remove/rename. 모두 canonicalize + 허용목록 재검사.
   - `auth.rs`(신규): 토큰 보관(env에서 받음) + 첫 프레임 검증.
   - `main.rs`: connection state에 `authed: bool`. 첫 프레임이 Auth 성공이어야 다른 op 처리.

2. **`crates/geulos-init`** (수정)
   - `/proc/cmdline` 파싱해 `geulos.bridge_token=<hex>` 추출 → `/run/geulos/bridge.token`에 644 권한으로 write (initramfs 단계).

3. **`apps/desktop-shell/src/host_bridge_client.rs`** (수정)
   - 시작 시 `/run/geulos/bridge.token` 읽음 → `static AUTH_TOKEN`. 연결 후 첫 프레임 `Request::Auth{token}` 전송, `Response::Auth{ok:true}` 확인. 실패 시 None 반환(graceful fallback).
   - 신규 메서드: `write_file(path, bytes)`, `create_dir(path)`, `remove(path, recursive)`, `rename(from, to)`.

4. **`apps/desktop-shell/src/handlers/explorer_methods.rs`** + 컴포지터
   - Explorer 객체에 `selected_item: ObjectId?` state 추가(server-side 객체 모델).
   - 새 메서드: `Explorer.select(folder_id)`, `Explorer.create_file(name)`, `Explorer.create_folder(name)`, `Explorer.rename_selected(new_name)`, `Explorer.delete_selected()`.
   - 컴포지터 BTN_LEFT 핸들러: 같은 row 500ms 내 재클릭 = 더블클릭 → 기존 navigate_to/open_file. 단일 = `Explorer.select(id)` invoke.
   - 렌더: selected_item id와 일치하는 row 하이라이트(파란 배경).

5. **파일 열람 경로** (file_read 또는 open_file handler)
   - 대상 File의 path가 호스트(드라이브 문자)면 `host_bridge_client::read_file` 사용. 그 외는 std::fs::read. Notepad/Window mount + content.

6. **파일 저장 경로** (file_write/save handler)
   - 대상 File의 path가 호스트면 `host_bridge_client::write_file`. 그 외 std::fs::write. AI 발신 시 기존 Dialog 흐름.

7. **파일관리자 창 툴바** (FileManager 렌더 + hit_test)
   - 창 상단(타이틀 아래)에 height=28 툴바 영역. 4개 버튼: [+ 새 파일][+ 새 폴더][이름 변경][삭제]. 각 버튼 클릭 = 그 메서드 invoke.

## 프로토콜 추가 (v1.5)

요청:
```json
{ "op": "auth",       "token": "<32 hex>" }
{ "op": "write_file", "path": "C:\\...", "content_base64": "..." }
{ "op": "create_dir", "path": "C:\\..." }
{ "op": "remove",     "path": "C:\\...", "recursive": false }
{ "op": "rename",     "from": "C:\\...", "to":   "C:\\..." }
```
응답:
```json
{ "ok": true,  "auth": { "ok": true } }
{ "ok": false, "auth": { "ok": false } }   // 토큰 mismatch → 다음 프레임에 즉시 연결 종료
{ "ok": true,  "ok2": {} }                  // 쓰기 op 성공
{ "ok": false, "error": "..." }
```

- v1.5 추가 op는 **Auth 성공 후에만** 허용. 첫 프레임이 Auth가 아니거나 토큰 mismatch면 브리지가 연결 종료.

## 보안 (KI-028 해소)

- **토큰**: 32 hex char(128 bit) 난수, per-launch. launch.ps1이 `[System.Web.Security.Membership]::GeneratePassword` 또는 `[byte[]]::new(16) | %{Get-Random ...} | %{$_.ToString("x2")}`로 생성.
  - 브리지 기동: `Start-Process $BridgeExe -ArgumentList @() -Env @{ GEULOS_BRIDGE_TOKEN = $token }` (또는 stdin 1줄). 브리지가 env에서 읽음.
  - 게스트 전달: QEMU args에 `-append "console=ttyS0 video=1280x800 geulos.bridge_token=<hex>"` 추가.
  - geulos-init이 `/proc/cmdline` 파싱 → `/run/geulos/bridge.token`에 write(rw로 root만, 그 외 r — 일반 사용자도 접근 가능하게).
  - desktop-shell `host_bridge_client::startup_load_token()`에서 read.
- **canonicalize + 허용목록**: 모든 fs op는 `std::fs::canonicalize(path)?` 후 결과 경로가 허용된 base("C:\\","D:\\",...) 하위인지 검사. 심볼릭 링크 escape 차단.
- **AI Dialog 확인**: write op invoke의 sender가 `ai:*`면 desktop-shell이 기존 ShellRunner 패턴(`dialog_ops::PendingFs::FileWrite{path,bytes}`)으로 Dialog mount → 사용자 응답 후 bridge.write_file 호출. 사용자 발신은 즉시 실행.
- **루프백 bind 유지**(127.0.0.1).

## 데이터 흐름

### 파일 더블클릭(열람)
1. 사용자가 파일관리자에서 file.txt 더블클릭.
2. compositor: 500ms 내 같은 row 재클릭 감지 → `Explorer.open_file(file_id)` invoke.
3. desktop-shell handler: file.path가 호스트 path → `bridge.read_file(path, max_bytes=1MB)` → content. Notepad Window mount + state.content = decoded UTF-8.
4. compositor가 Notepad 렌더.

### Notepad에서 편집 후 저장(Ctrl+S)
1. 사용자가 Notepad에서 텍스트 수정.
2. Ctrl+S → `File.save(content)` invoke (또는 Notepad의 save handler).
3. desktop-shell handler: 호스트 path → AI면 Dialog → 사용자 발신 또는 허용 후 → `bridge.write_file(path, bytes)` → ok.
4. 성공 시 File.state.dirty=false broadcast.

### 새 파일/폴더 생성
1. 사용자가 툴바 [+ 새 파일] 클릭 → `Explorer.create_file(name="untitled.txt")` invoke (이름 입력 Dialog 후).
2. handler: active_folder의 path + name → bridge.write_file(path, b""). 성공 시 새 File 객체 mount, parent.children 갱신.

### 이름 변경 / 삭제
1. 사용자 단일클릭 한 행 → `Explorer.select(id)`.
2. 툴바 [이름 변경] → name 입력 Dialog → `Explorer.rename_selected(new_name)` → bridge.rename. 객체 path/name 갱신.
3. 툴바 [삭제] → 확인 Dialog → `Explorer.delete_selected()` → bridge.remove(recursive=is_dir). 객체 destroyed=true, parent.children에서 제거.

## 에러 처리

- **Auth 실패**(토큰 mismatch): 브리지가 Response::Auth{ok:false} 1회 보낸 후 연결 종료. 클라이언트는 None 반환 → v1 폴백 동작(VM 루트만, 쓰기 비활성).
- **토큰 파일 미존재**: 클라이언트가 startup_load_token에서 None → 모든 RPC None → 폴백.
- **write 실패**(권한 거부/디스크 가득/canonicalize 실패): bridge가 `{ok:false, error}` 반환. handler는 사용자에게 에러 dialog 또는 객체 state.last_error 설정.
- **AI Dialog 거부**: handler가 PendingFs entry 정리, write 호출 안 함, file.state.last_error 설정.

## 보안 (이번 spec에서 KI-028 해소)

이 spec 구현 완료 시 KI-028의 v1 수용 단서가 **해소**됨:
- ✅ per-launch 인증 토큰.
- ✅ canonicalize + base-dir 허용목록.
- ✅ 루프백 bind 유지.
- ✅ AI write op는 Dialog 확인.

이후 증분 ②(실행)는 같은 인증·canonicalize 기반 위에서 새 op만 추가하면 됨.

## 테스트

- **브리지 fs_ops 단위테스트**(temp dir): write_file/create_dir/remove/rename 정상 + 권한거부 + 잘못된 경로 + canonicalize 허용목록 경계 + recursive remove.
- **auth 단위테스트**: 토큰 match/mismatch 검증, missing token, 잘못된 형식.
- **프로토콜 round-trip**: 새 요청/응답 직렬화/역직렬화.
- **host_bridge_client**: mock stream으로 Auth handshake (positive+negative), write_file 라운드트립.
- **integration**: launch.ps1 토큰 생성 → 게스트 cmdline → init 파싱 → desktop-shell load → 브리지 Auth 성공 1회 시나리오.
- **수용 검증 (사용자)**:
  1. 호스트 텍스트 파일 더블클릭 → Notepad 표시.
  2. 편집 + Ctrl+S → 호스트 파일 변경 확인(Windows에서 열어 검증).
  3. 툴바 [+ 새 파일/폴더] → 호스트에 생성 확인.
  4. 단일클릭 후 [이름 변경][삭제] → 호스트 반영 확인.
  5. AI가 write 호출 → Dialog 뜨고 [허용] 시 반영, [거부] 시 미반영.
  6. 토큰 mismatch(예: 토큰 파일 수동 변조) → 모든 op 폴백 확인.

## 범위 / YAGNI

- **IN v1.5**: 보안 토큰 + canonicalize + 허용목록 / 쓰기 op(write/create_dir/remove/rename) / 파일 열람·편집·저장(Notepad) / FM 선택+툴바+더블클릭 감지 / 더블클릭 detection.
- **OUT (다음 증분 또는 별도)**:
  - 실행/ShellRunner 이식 (증분 ②).
  - copy/move 별도 op (rename으로 동일 디렉터리 내 이름 변경만 v1.5; 다른 디렉터리는 ②).
  - 권한/ACL 표시.
  - undo/휴지통.
  - fs_watcher 호스트 path skip 정리 — 노이즈 로그만 남고 기능 정상이므로 v1.5엔 cosmetic 정리만 (조용히 skip).

## 종속/사전 작업 (이미 완료된 prerequisite)

- ✅ FM dedup + close 후 자식 서브트리 destroyed cascade — 2026-05-30 hot-fix `5650a43` 외.
