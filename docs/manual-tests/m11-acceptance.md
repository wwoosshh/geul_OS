# M11 Acceptance — 수동 회귀 시나리오

**전제:** `.\target\debug\geulos.exe` (launcher) 빌드 + 실행. AI 시나리오는 ANTHROPIC_API_KEY 또는 `/ai start` awaiting flow로 설정.

각 시나리오 실행 후 결과 기록란에 ✓/✗ 표시.

## 시나리오 A — compositor 사용자 동작 (회귀)

### A1. Explorer.navigate_to
1. launcher 띄움.
2. 좌측 트리에서 C: 폴더명 클릭.
3. **기대:** 우측 Explorer에 C: 내부가 표시.

### A2. Cli.submit_input
1. CLI 영역 클릭 → "hello" 타이핑 → Enter.
2. **기대:** lines 히스토리에 "> hello" + 응답 표시.

### A3. Window 클릭 close
1. 우측에서 파일 더블클릭 → Window 열림.
2. Window close 동작.
3. **기대:** Window 사라짐.

## 시나리오 B — AI granted/ungranted 경계

### B1. AI Filesystem@1 (path 무관)
1. `/ai start test1` → "임의 경로의 파일을 읽어달라"는 요청 (cwd 밖).
2. **기대:** Dialog 없이 통과 (Filesystem@1.read_external은 항상 허용).

### B2. AI Folder.create_file (granted dir 안 + Dialog 동의)
1. AI에게 "현재 폴더에 hello.txt 만들어줘" 요청.
2. **기대:** Dialog "AI가 <dir>에서 파일 작업 허용?" 표시 → "허용" → 파일 생성 성공.

### B3. AI 같은 Folder 후속 호출 (grant 캐시)
1. B2 직후 "이번엔 world.txt 만들어줘" 요청.
2. **기대:** *Dialog 없이* 통과 (granted_dirs + server GrantStore 캐시).

### B4. AI 다른 Folder 호출 (ungranted)
1. cwd의 다른 sub-folder를 대상으로 "거기에 파일 만들어줘" 요청.
2. **기대:** *새 Dialog* 표시 (디렉터리별 grant).

## 시나리오 C — KI-001 차단 검증 (핵심)

### C1. 외부 geulosh로 Dialog.respond 시도
1. AI에게 write 요청 → Dialog 표시 상태에서 멈춤.
2. 별 터미널에서 `geulosh --connect <addr>` → `query type aios.builtin/Dialog@1` → 응답으로 Dialog ID 얻음.
3. `geulosh invoke <dialog_id> respond '{"choice":"allow"}'`.
4. **기대:** `PermissionDenied` 응답. Dialog는 *여전히* 사용자 응답 대기.

### C2. 외부 geulosh로 Window.close 시도
1. 어떤 파일 Window 열어둠.
2. `geulosh invoke <window_id> close '{}'`.
3. **기대:** `PermissionDenied`. Window 그대로.

### C3. 외부 geulosh로 set_state Window.title
1. `geulosh invoke <window_id> set_state '{"key":"title","value":"hijacked"}'`.
   (또는 wire 명령 형식 — geulosh 정확 syntax 따라.)
2. **기대:** `PermissionDenied`. title 그대로.

## 시나리오 D — invariants

### D1. desktop-shell SetState 통과
- 스크롤 동작 → scroll_y SetState → 통과 (compositor → SetState).
- 새 파일 외부 생성 → fs_watcher → desktop-shell이 child_count SetState → 통과.

### D2. AI invoke Window/Explorer/Cli/Dialog 거부
- AI prompt: "Window 닫아줘" 또는 "Explorer.navigate_to 호출해줘".
- **기대:** AI 측에서 `PermissionDenied` 메시지 수신 → AI가 사용자에게 "차단됨" 안내.

## 통과 기준

12개 시나리오 모두 기대대로. 하나라도 실패 시 T9-T11 디버그.

## 결과 기록 (T16에서 채움)

| 시나리오 | 통과 | 비고 |
|---|---|---|
| A1 | | |
| A2 | | |
| A3 | | |
| B1 | | |
| B2 | | |
| B3 | | |
| B4 | | |
| C1 | | |
| C2 | | |
| C3 | | |
| D1 | | |
| D2 | | |
