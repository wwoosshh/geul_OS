# GeulOS 호스트 브리지 설계 (Model B) — v1: 호스트 C:/D: 읽기 탐색

> 상태: 설계 승인 대기 → writing-plans 전환 예정
> 일자: 2026-05-29

## 배경 / 동기

GeulOS의 비전: OS가 AI에게 실제 작업 상태를 **구조화된 데이터(객체 트리)**로 직접 제공해,
기존 Claude Code / 유사 도구처럼 "사용자 화면을 캡처해 실행 상태를 눈으로 확인"할 필요가 없게 한다.
이를 위해 GeulOS는 **작업용 OS**여야 하며, 기존 Windows의 `C:`, `D:` 등 최상위 저장소의 모든 파일에
접근해 양방향(Windows에서 만든 파일을 GeulOS에서 작업, 그 결과를 Windows에서 확인)으로 일할 수 있어야 한다.
GeulOS 내부에만 격리된 저장소를 두면 "생태계에 갇혀" 아무도 쓰지 않는다.

### 핵심 제약 (조사 결과)

현재 Windows용 QEMU 빌드는 호스트 파일시스템을 게스트에 직접 마운트하는 길이 전부 막혀 있다:
- `-fsdev`/virtio-9p/virtfs: **`fsdev support is disabled`** (이 빌드에서 비활성)
- virtio-fs / vhost-user-fs device: 없음
- user-net `smb=`: 옵션은 파싱되나 Windows 호스트엔 samba 데몬이 없어 실동작 안 함

→ 게스트 안에서 `mount`로 호스트 드라이브를 붙이는 표준 방법이 불가능. 따라서 **호스트 측 브리지
프로세스**가 호스트 파일/실행을 네트워크로 VM에 제공하는 아키텍처(Model B)로 간다.

### Model B를 택한 이유 (Model A 기각)

- **Model A** (파일은 호스트, 실행은 VM): npm 등이 `node_modules` 파일 수천 개를 네트워크 왕복으로
  읽어 빌드가 비현실적으로 느려짐(WSL2의 `/mnt/c` 문제와 동일) + inotify/POSIX 락/심볼릭링크가 안 맞아
  깨짐 + copy-in/copy-out batch 방식이라 중간 유실·개입 시 복구 불가. → 기각.
- **Model B** (실행도 호스트, GeulOS는 AI/제어 평면): npm이 호스트 파일을 **로컬로** 빠르게 읽고,
  네트워크로는 명령·출력·디렉터리 목록(작고 가끔)만 오감 → 빠르고 실시간. 옛 winit GeulOS가 Windows
  네이티브 앱으로 C/D에 React 설치→빌드→dev 서버를 돌리던 방식의 정통 계승.

## v1 범위

**파일관리자가 호스트 C:/D:를 읽기 전용으로 탐색**하는 것까지를 v1으로 한다(기반 + 첫 증분).
호스트 접근 범위는 **전체 드라이브**, 권한은 **읽기 전용**. 쓰기·실행은 이후 증분.

### 비범위 (이후 증분)

- 호스트 파일 **쓰기**(저장/생성/삭제) — AI 발신 시 기존 Dialog 확인 동반.
- 호스트 **명령 실행**(`npm install`/`build`/`run dev`) — 기존 ShellRunner/JobObject 코드를
  브리지로 이식, ConsoleWindow 스트리밍 재사용, AI Dialog 확인.
- dev 서버 접근/프록시, 파일 변경 watch(inotify 대체).

## 아키텍처

```
┌─ Windows 호스트 ──────────────────────────────┐
│  geulos-host-bridge.exe                        │
│   - 127.0.0.1:5560 listen (프레임 JSON RPC)    │
│   - list_drives / list_dir / stat / read_file  │
│   - 읽기 전용, 절대경로만                       │
│   - launch.ps1이 QEMU와 함께 기동              │
└───────────────▲────────────────────────────────┘
                │ TCP (slirp 게이트웨이 10.0.2.2:5560)
┌─ QEMU VM (GeulOS / Linux) ─────────────────────┐
│  desktop-shell                                  │
│   - host_bridge_client: 10.0.2.2:5560 다이얼   │
│   - drives.rs: 호스트 드라이브 + VM 루트 나열   │
│   - lazy_mount: 호스트 경로면 RPC, /면 std::fs  │
│   - Folder/File 객체 합성 → geulosd 트리 mount  │
│  geulosd(객체 서버) ← compositor(파일관리자 렌더)│
└─────────────────────────────────────────────────┘
```

### 컴포넌트

1. **`crates/geulos-host-bridge`** (신규, Windows 호스트 바이너리)
   - `main.rs`: TCP listener(127.0.0.1:5560), 연결당 요청 루프.
   - `protocol.rs`: 요청/응답 타입 + 프레임 인코딩(기존 `geulos-proto`의 length-prefixed
     `encode_frame`/`decode` 재사용 — 일관성 + 검증된 코드).
   - `fs_ops.rs`: `list_drives`(Win32 `GetLogicalDrives`), `list_dir`(std::fs::read_dir),
     `stat`, `read_file`. 순수 로직은 단위테스트.
   - 읽기 전용: 어떤 쓰기 op도 v1엔 없음. 경로는 절대경로만 허용(상대/`..` 정규화 검증).

2. **`apps/desktop-shell/src/host_bridge_client.rs`** (신규)
   - `connect()` → `10.0.2.2:5560` TCP. 실패 시 `Option<Client>` None(브리지 없음 = graceful).
   - `list_drives()`, `list_dir(path)`, `stat(path)`, `read_file(path)` 동기/async 래퍼.
   - 단위테스트: mock stream으로 프레임 round-trip.

3. **`apps/desktop-shell/src/drives.rs`** (수정)
   - Linux 분기: 기존 `vec!["/"]` → `bridge.list_drives()` 결과(`C:\`, `D:\`, …) **+ VM 루트 `/`**.
     브리지 None이면 `vec!["/"]`로 폴백(현행과 동일).

4. **`apps/desktop-shell/src/lazy_mount.rs`** (수정)
   - `expand_folder(path)`: 경로가 **호스트(드라이브 문자 `X:\` 또는 `X:/`로 시작)**면
     `bridge.list_dir(path)` 결과로 Folder/File 객체 합성, 그 외(`/`...)는 기존 `std::fs::read_dir`.
   - 객체 합성 로직(`std_types::folder`/`file`)은 동일 — 데이터 소스만 분기.

### 경로 모델 / 호스트 태깅

- **호스트 경로**: Windows 스타일(`C:\Users\...`). 드라이브 문자로 시작(`^[A-Za-z]:[\\/]`).
- **VM 로컬 경로**: `/`로 시작.
- desktop-shell은 폴더의 path 접두로 라우팅 결정. 파일관리자/컴포지터 렌더는 **무변경**
  (Folder/File 객체를 그대로 그림 — path 문자열만 다름).
- 파일관리자 최상위 = 호스트 드라이브들(C:, D:, …) + VM 루트 `/`. (사용자 요청 시 호스트 전용으로 축소 가능.)

## 프로토콜 (v1)

length-prefixed 프레임(geulos-proto `encode_frame`) 안에 JSON 1건.

요청:
```json
{ "op": "list_drives" }
{ "op": "list_dir",  "path": "C:\\Users" }
{ "op": "stat",      "path": "C:\\Users\\x.txt" }
{ "op": "read_file", "path": "C:\\Users\\x.txt", "max_bytes": 1048576 }
```
응답:
```json
{ "ok": true, "drives": ["C:\\", "D:\\"] }
{ "ok": true, "entries": [ {"name":"x.txt","is_dir":false,"size":12,"modified_ms":...} ] }
{ "ok": true, "stat": {"is_dir":true,"size":0,"modified_ms":...} }
{ "ok": true, "content_base64": "...", "truncated": false }
{ "ok": false, "error": "권한 거부: C:\\System Volume Information" }
```
- `read_file`은 `max_bytes`로 상한(대용량 방지). 초과 시 `truncated:true`.
- 바이너리 안전 위해 `read_file` 본문은 base64.

## 데이터 흐름 (v1 탐색)

1. desktop-shell 시작 → `host_bridge_client::connect("10.0.2.2:5560")`.
   - 성공: `list_drives` → `["C:\\","D:\\",...]`.
   - 실패(브리지 미기동): None → 드라이브는 `/`만(폴백).
2. `open_file_manager_window`: FileTree 자식 = 호스트 드라이브 Folder들(path=`C:\` 등) + VM 루트 Folder(`/`).
3. 사용자가 호스트 폴더 펼침 → `expand_folder` → 호스트 경로 판정 → `bridge.list_dir` →
   entries → Folder/File 객체 합성 → mount → 컴포지터 렌더. UX는 현행과 동일, 데이터만 호스트.
4. (옵션) 파일 더블클릭으로 열람 → `bridge.read_file` → Window content. v1 포함/다음 증분 택1(아래 미해결).

## 에러 처리

- **브리지 미기동/연결 실패**: desktop-shell은 호스트 드라이브 없이 VM 루트 `/`만 노출 + 로그.
  GeulOS 부팅·동작에 지장 없음(하드 실패 금지). 브리지를 켜면 다음 파일관리자 열기부터 드라이브 등장.
- **list_dir 권한 거부/IO 오류**: 브리지가 `ok:false` 반환 → desktop-shell은 빈 폴더 처리
  (현행 `expand_folder`의 read_dir 실패 처리와 동일).
- **잘못된 경로**(상대/`..`): 브리지가 거부(절대경로만).
- **연결 끊김 중간**: client는 재연결 시도 1회 후 None 취급(드라이브 사라짐, 부팅엔 무영향).

## 보안

- v1 **읽기 전용** — 어떤 쓰기/실행 op도 없음. 위험 표면 최소.
- 브리지는 `127.0.0.1`만 listen(외부 노출 금지). slirp 특성상 게스트만 10.0.2.2로 도달.
- 절대경로만 허용 + `..` 컴포넌트 거부로 단순 traversal 방지.
- 이후 쓰기/실행 증분에서 AI 발신 op는 기존 Dialog 확인(AI=사용자2 안전장치) 재사용.

### 자동 보안 리뷰 결과 (2026-05-29) — v1 수용 + 강화 게이트

자동 리뷰가 2건 지적:
1. **[HIGH] 무인증 임의 파일 읽기**: 브리지가 루프백 연결자에게 인증 없이 전체 fs를 읽기 제공.
2. **[MEDIUM] 심볼릭 링크 traversal**: `is_safe_absolute`가 canonicalize 안 함.

**v1 판단(수용):**
- (1) 루프백 bind + **읽기 전용** + 사용자가 명시적으로 "VM의 호스트 전 파일 접근"을 요청.
  단일 사용자 개발 머신에선 로컬 프로세스가 이미 사용자 권한으로 동일 파일을 읽으므로 **권한 상승 아님**;
  게스트 VM의 읽기 자체가 의도된 기능.
- (2) v1은 **base-dir 허용목록 없이 전체 드라이브를 의도적으로 노출** → canonicalize가 막을 경계가 없어 무관.

**쓰기/실행 증분 진입 전 MANDATORY 강화(아래 없이는 쓰기/실행 op 추가 금지):**
- per-launch **인증 토큰**(launch.ps1이 난수 토큰 생성 → 게스트에 전달[payload/커널 cmdline] → 첫 프레임 검증).
  무인증 임의 *쓰기/명령 실행*은 심각하므로 토큰 필수.
- `std::fs::canonicalize` + **base-dir 허용목록** 재검사(실경로가 허용 base 하위인지). 심볼릭 링크 escape 차단.
- 루프백 bind 유지(127.0.0.1).

## 테스트

- **브리지 fs_ops 단위테스트**(Windows dev, `cargo test`): temp dir에 list_dir/stat/read_file,
  없는 경로 에러, `..` 거부, max_bytes truncation.
- **프로토콜 round-trip**: 요청/응답 encode→decode 동일성.
- **host_bridge_client 단위테스트**: mock stream으로 프레임 송수신·파싱.
- **drives.rs**: 브리지 None 폴백(`/`), Some 시 드라이브+루트 합성.
- **lazy_mount 분기**: 호스트 경로 판정(`C:\` vs `/`) 단위테스트.
- **수용 검증(사용자)**: 브리지 켜고 VM 부팅 → 파일관리자에 C:/D: 등장 → 펼쳐 실제 파일 보임;
  브리지 끄고 부팅 → `/`만 보이고 정상 동작.

## 결정 사항 (확정 — 사용자 검토에서 변경 가능)

1. **`read_file`(파일 열람) v1 포함**: 목록/탐색 + stat + `read_file`까지 v1. 텍스트 파일을 더블클릭해
   Window로 여는 것까지 v1 "탐색"에 포함(읽기 전용이라 위험 없음, 구현 비용 작음). 쓰기는 비범위 유지.
2. **파일관리자 최상위 = 호스트 드라이브(C:, D:, …) + VM 루트 `/`**: GeulOS 자체 fs도 함께 탐색 가능
   하도록 둘 다 노출(호스트 드라이브가 위, VM 루트가 아래). 추후 사용자 요청 시 호스트 전용으로 축소 가능.
