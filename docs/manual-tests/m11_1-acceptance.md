# M11.1 Acceptance — 수동 회귀 시나리오

**전제:** `.\target\debug\geulos.exe` (launcher) 빌드 + 실행. ANTHROPIC_API_KEY
설정 또는 `/ai start` awaiting flow.

## 시나리오 1 — 즉시 echo

1. `/ai start test1` → AI 세션 시작.
2. CLI에 "안녕 AI" 입력 → Enter.
3. **기대:** 즉시 lines에 `[ai:test1] > 안녕 AI` + `(응답 대기 중...)` 표시.

## 시나리오 2 — 응답 도중 UI 반응

1. 시나리오 1의 `(응답 대기 중...)` 상태에서 *스크롤 / 우측 폴더 클릭 / 좌측 트리 클릭* 시도.
2. **기대:** 모든 동작이 즉시 반응 (UI 멈춤 X).

## 시나리오 3 — 응답 도착 + sentinel 제거

1. AI 응답 도착.
2. **기대:** `(응답 대기 중...)` 라인 사라짐 + AI 응답 lines에 추가.
3. 빈 응답일 경우: `[AI: (빈 응답)]` 한 줄 추가 (silent blank 회피).

## 시나리오 4 — JSONL 파일 생성

```powershell
Get-ChildItem "$HOME/.geulos/logs/ai-chat/" | Select-Object -Last 1
```

**기대:** `test1-YYYYMMDD-HHMMSS.jsonl` 파일 존재.

## 시나리오 5 — jq parse 가능

```powershell
Get-Content <file> | Select-Object -First 1
```

**기대:** valid JSON object. `kind`, `ts`, `text` 같은 필드 확인.

## 시나리오 6 — 중복 tool call 진단

1. AI에게 같은 폴더 두 번 조회 요청.
2. JSONL에서 `kind: "tool_call"` 라인 grep — 동일 args 가진 호출 2번 발견.
3. **기대:** 진단 가능 (실제 중복 호출이 있다면 grep으로 즉시 보임).

## 통과 기준

6개 모두 기대대로. 결과 표 (사용자 후속 실행):

| 시나리오 | 통과 (✓/✗) | 비고 |
|---|---|---|
| 1 즉시 echo | 미실행 | |
| 2 UI 반응 | 미실행 | |
| 3 sentinel 제거 | 미실행 | |
| 4 JSONL 파일 | 미실행 | |
| 5 jq parse | 미실행 | |
| 6 중복 진단 | 미실행 | |
