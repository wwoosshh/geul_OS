# ai-probe

GeulOS의 *위험 A* 검증 도구: 실제 Claude API에게 GeulOS 와이어 프로토콜로 조작을 시켜보고 무엇이 깨지는지 관찰한다.

이건 *실험 도구*다. 정식 GeulOS 컴포넌트가 아니다. 결과가 *M5 (글 AI I/O 드라이버) plan 작성의 기초 데이터*가 된다.

## 작동 원리

```
사용자 → 시나리오 파일 → probe.py → Claude API (tool use)
                                ↓ (4개 도구를 통해)
                          GeulOS server-host (TCP)
                                ↓
                          ObjectServer + echo-app 등
```

Claude에게 노출되는 도구:
1. `list_objects_by_type(type_uri)` — wire `Query::ByType`
2. `get_object(object_id)` — wire `Get`
3. `invoke_method(target, method, args)` — wire `Invoke`
4. `report_done(summary)` — 종료 신호

모든 turn (Claude 응답 + tool call + result)이 `results/YYYYMMDD_HHMMSS_<scenario>.log` 에 기록된다.

## 준비 (한 번만)

### 1. Python 의존성 설치

```powershell
cd C:\AiOS\tools\ai-probe
pip install -r requirements.txt
```

(virtualenv 쓰고 싶으면 `python -m venv .venv` 후 `.\.venv\Scripts\activate`.)

### 2. API 키

이미 `C:\AiOS\.env`에 `ANTHROPIC_API_KEY=sk-ant-...` 형태로 저장되어 있어야 한다 (workspace .gitignore에 `.env` 포함).

## 실행

### Step 1 — GeulOS 실행 (별 터미널)

```powershell
# 터미널 A
cd C:\AiOS
cargo run -p geulos-server-host

# 터미널 B
cd C:\AiOS
cargo run -p geulos-echo-app
```

### Step 2 — probe 실행

```powershell
# 터미널 C (어디서든)
cd C:\AiOS\tools\ai-probe
python probe.py --scenario 02_press_button
```

다른 시나리오: `01_list_all`, `03_multi_press`, `04_discover`.

옵션:
- `--server 127.0.0.1:5550` (기본값)
- `--model claude-sonnet-4-6` (기본값) — `claude-opus-4-7` 등 다른 모델 가능
- `--max-turns 12` (기본값) — 너무 길어지는 것 방지

## 결과 해석

각 실행은 `results/` 디렉터리에 로그 파일을 남긴다. 보고 싶은 부분:

- **Tool call 패턴** — Claude가 어떤 순서로 도구를 호출했나? 효율적인가?
- **에러 회복** — `permission` / `not_found` / `unknown_method` 에러를 만났을 때 어떻게 대응했나?
- **UUID 처리** — Claude가 query 결과의 UUID를 *정확히 다시 보냈는가*, 아니면 잘라 먹거나 변형했는가?
- **종료 품질** — `report_done`의 summary가 *실제로* 일어난 일을 정확히 묘사하는가?

성공·실패 모두 *데이터*. 실패 사례야말로 M5 plan 작성에 가장 유용하다.

## 시나리오 4개

| 파일 | 목적 | 검증할 가설 |
|---|---|---|
| `01_list_all` | 발견 | 와이어 프로토콜이 LLM에게 *자기 검색 가능*한가 |
| `02_press_button` | 단순 invoke | 단발 action을 LLM이 *정확히 실행*하는가 |
| `03_multi_press` | 다단계 + 관찰 | 반복 action + 상태 검증을 LLM이 *직접 추론*하는가 |
| `04_discover` | 자유 탐색 | 명시 task 없이 LLM이 *목적을 발명*하는가 |

## 한계 / 알려진 이슈

- **Subscribe 미사용** — 현재 4개 도구는 polling 모델. 추후 `subscribe` + `drain` 도구 추가 가능.
- **에러 메시지 한국어** — 서버가 한국어 에러 메시지를 반환 (`권한 없음`, `찾을 수 없음`). Claude가 영어로 의미를 추론하는지가 관찰 포인트.
- **시간 제한 없음** — `--max-turns`로만 제한. Claude가 무한 polling에 빠지면 max-turns에 도달할 때까지 진행.
- **subscribe-driven 시나리오 부재** — 외부 클라이언트 입장에서 *수동적 관찰*은 다음 단계에서.

## 다음 단계 (이 도구 자체의)

여기서 발견된 문제에 따라:
- 와이어 프로토콜 v0.1 → v0.2 개정 (필요시)
- `system_prompt.md` 개선 (LLM 혼란 패턴 해소)
- M5 plan 작성 시 *경험으로 입증된 abstraction* 선택
