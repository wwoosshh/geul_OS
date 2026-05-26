# M12 Acceptance — 수동 회귀 시나리오 + auto_react_project

**전제:** ANTHROPIC_API_KEY 설정. launcher (`.\target\debug\geulos.exe`) 띄운 상태.
git/npm/node/npx 호스트 PATH에 있음.

## 시나리오 1 — git --version 단순 호출

1. AI CLI에서 "git 버전 확인해줘" 요청 (또는 wire로 직접 invoke)
2. Dialog "AI가 다음 명령 실행: git --version, cwd: <cwd>" → 허용
3. 기대: state.last_exit_code=0, last_stdout에 "git version 2.x.x"

## 시나리오 2 — 화이트리스트 외 binary 거부

1. AI에게 "rm 명령 실행해줘" 요청 (또는 wire로 ShellRunner.run(cmd="rm"))
2. 기대: state.last_error = "화이트리스트 외 binary: 'rm'..."
3. Dialog 띄우지 않음 (검증 단계에서 거부)

## 시나리오 3 — cwd 존재하지 않음

1. AI가 cwd="D:/존재안함" run 시도
2. 기대: state.last_error = "cwd 존재하지 않음: 'D:/존재안함'"

## 시나리오 4 — Dialog 거부

1. AI가 git status run 시도 → Dialog → 거부
2. 기대: state.last_error = "사용자 거부", last_exit_code=-1

## 시나리오 5 — npm install 성공

1. 임시 디렉터리 + package.json 준비
2. AI가 npm install run 시도 → Dialog → 허용
3. 기대: 60-90초 후 node_modules/ 생성 + exit_code=0

## 시나리오 6 — 보안: AI가 ShellRunner의 run 외 method 시도

1. AI가 invoke_method(sr_id, "set_state", ...) 또는 method 이름 임의 시도
2. 기대: PermissionDenied (AiSession Exact "run"만 Allow)

## auto_react_project end-to-end

`cargo run --example auto_react_project -p geulos-ai-bridge`

기대 (5-10분):
- D:/GeulOS/tmp-react-app/ 생성
- package.json + node_modules/react/ 존재
- src/App.jsx에 "Hello GeulOS React" 포함

## 결과 기록

| 시나리오 | 통과 (✓/✗) | 비고 |
|---|---|---|
| 1 git --version | 미실행 | |
| 2 화이트리스트 거부 | 미실행 | |
| 3 cwd 없음 | 미실행 | |
| 4 Dialog 거부 | 미실행 | |
| 5 npm install | 미실행 | |
| 6 보안 차단 | 미실행 | |
| auto_react_project | 미실행 | |
