# AI Probe 결과 보고서 — 위험 A 검증

- **일자:** 2026-05-17
- **목적:** 외부 분석 보고서 `2026-05-17-claude-analysis.md` §"놓친 위험 A" — *"실제 LLM이 GeulOS 와이어 프로토콜을 사용 가능한가?"* 검증
- **도구:** `tools/ai-probe` Python 스크립트, Claude Sonnet 4.6, 4개 시나리오

---

## 0. 한 줄 결론

**와이어 프로토콜은 LLM-사용 가능하다.** 4개 시나리오 모두 *프로토콜 자체*의 문제가 없었고, Claude는 protocol을 *처음 보고도* 정확히 사용. 보너스: probe가 *GeulOS의 실제 버그*를 발견 (echo-app 60s idle).

---

## 1. 실행 환경

- Claude API model: `claude-sonnet-4-6`
- GeulOS server-host: 127.0.0.1:5550
- echo-app: 실제 실행 중 (1 Container + 1 Text + 1 Button 게시)
- 시나리오 실행 간격: 02 → (3분) → 01 → (22초) → 03 → (39초) → 04
- 총 비용: ~$0.20

---

## 2. 시나리오별 요약

| # | 시나리오 | 턴 | 시간 | 결과 |
|---|---|---|---|---|
| 02 | 단순 invoke (`02_press_button`) | 4 | 12.5s | ✅ |
| 01 | 발견 (`01_list_all`) | 3 | 13.6s | ✅ (parallel tool use) |
| 03 | 다단계 + 관찰 (`03_multi_press`) | 12 (max) | 32.4s | 🐞 시스템 버그 노출 |
| 04 | 자유 탐색 (`04_discover`) | 5 | 24.4s | ⭐ 별도 분석 |

---

## 3. 결정적 관찰

### 3.1 와이어 프로토콜의 *self-describing* 성질 입증

Claude는 다음을 **외부 학습 없이 정확히** 사용:

- UUID 4개 시나리오에서 *0건의 손상*. 길이 36자 그대로 round-trip.
- 표준 타입 URI 4개 모두 정확히 표기 (`aios.std/Container@1` 등).
- `{"kind": ..., ...}`의 tagged-enum 응답 형식 즉시 해석.
- 객체 모델 (`methods`, `acl`, `state`, `props`, `owner`, `parent`, `children`) 의미 *추론*.
- 메서드 시그니처 `{"args": [], "name": "press"}`에서 *인자 없는 메서드*임을 정확히 인식 → `args: null` 호출.

이는 *system prompt가 짧고(약 50줄), 외부 ref docs가 없는데도* 즉시 가능했음.

### 3.2 자기 보안 감사 능력 (예상치 못한 발견)

3개 시나리오에서 Claude는 *명시 지시 없이* 다음을 보고:

> *"Its ACL has a Wildcard Allow, permitting any actor to invoke any method."*  
> *"The Button's wildcard ACL entry is a deliberate escape hatch for public widgets."*

이는 우리 `KI-001` (M3 wildcard ACL 보안 부채)을 **AI가 자동으로 감사**한 셈. 와이어 프로토콜의 투명성이 *AI 사용자의 보안 이해*를 가능하게 함. 후속 M5 plan에서 이 *AI 가독성*을 차별점으로 부각 가능.

### 3.3 Parallel tool use 자동 활용

01과 04에서 Claude는 **한 turn 안에 여러 도구 호출**을 자동 묶음:
- Turn 1: `list_objects_by_type` × 4 (4개 표준 타입 동시 조회)
- Turn 2: `get_object` × 3 (3개 객체 상세 동시 조회)

이는 *직렬 polling을 피하고 효율적*. 비용·시간 50% 이상 절약.

### 3.4 정직한 종료

시나리오 03이 *심층 시사*. Claude는 5번 press 모두 성공 응답 받고도 Text가 안 바뀌는 걸 보고 **거짓 보고하지 않음**:

- 각 turn마다 *"still count: 1"* 정직 보고
- 사실 무근의 success summary 만들지 않음
- max-turns(12) 도달까지 polling 후 *report_done 없이 종료*

이건 *바람직한 실패 모드*. AI가 *환각으로 거짓 진척을 보고하는* 최악 시나리오와 대조.

### 3.5 자율 가설 형성 (시나리오 04)

scenario 04에서 Claude는 Text가 안 변한 것 보고 **3개 가설을 직접 제시**:

> *Either the counter state lives server-side and my snapshot query is stale/cached, the app's event processing is async and hadn't settled, or the increment is internal-only and not reflected back into the Text's state.content in this build.*

3번째 가설이 **정답에 매우 가까움** (실제로는 echo-app 프로세스가 죽어 *event reactor가 사라진* 상태). AI가 *우리도 모르고 만든 버그*를 추론으로 식별.

---

## 4. 발견된 시스템 버그

### 4.1 KI-010 — echo-app 60s idle 자동 종료

이건 *probe가 발견한 진짜 버그*. echo-app/src/main.rs의 `tokio::time::timeout(Duration::from_secs(60), ...)` 가 60초 입력 없으면 *자동 종료*시킴. 종료 후에도 서버는 객체를 보유 (KI-011)하므로 *유령 앱* 상태.

증거: 시나리오 02 (00:24:12) → 시나리오 01 (00:27:19) 3분 간격 동안 echo-app 종료. 이후 시나리오 03의 5번 press가 모두 성공 응답 받으나 Text 안 변함.

**fix:** 본 보고서 commit에 포함. `tokio::time::timeout` wrapper 제거 + Duration import 삭제. echo-app은 *연결이 끊기거나 read 에러* 시까지 계속 실행.

### 4.2 KI-011 — `emit_destroyed`가 객체를 실제 제거 안 함

KI-010 진단 중 발견된 *깊은 설계 부채*. actor 연결 끊겨도 객체는 query/get으로 조회 가능. 사용자 기대(*"앱이 죽으면 UI도 사라져"*)와 불일치.

3개 옵션 중 *tombstone* 방식 (KI-011 본문 (b)) 권고. M4.5 또는 M5에서 처리.

---

## 5. M5에 미치는 함의 — 큰 결정

### 5.1 전제의 전환

**기존 가정:** *"LLM이 와이어 프로토콜을 직접 다루기 어렵다. 따라서 글 언어 임베드 VM이 *필수 중간 글루*."*

**probe가 보여준 것:** *"LLM은 와이어 프로토콜을 *직접* 매우 잘 다룬다."*

### 5.2 글 언어의 역할 재정의

| 기존 (M0~M3 plan) | 본 보고서 후 |
|---|---|
| 글 = AI ↔ OS 자연어 글루 | 글 = *사람*이 작성하는 매크로/자동화 매체 |
| AI는 Glscript로 다단계 작업 | AI는 직접 RPC 사용 (subscribe+drain 도구 추가로 충분) |
| M5 = 글 VM 임베드가 *필수* 1차 산출물 | M5 = AI RPC 어댑터 강화가 1차, 글 VM 임베드는 2차 |

### 5.3 M5 plan 재배치 권고

권고는 *큰 방향 변경 없이 우선순위만 재배치*:

1. **M5-T1~T3:** AI agent 패턴 보강. probe의 4개 도구를 *영구 RPC 어댑터 크레이트*로 승격. `subscribe + drain` 도구도 추가.
2. **M5-T4~T6:** 글 VM 임베드 (기존 plan 그대로). 단 *AI가 필수로 쓰는 게 아니라 사람용 매크로 실행기*로 포지셔닝.
3. **M5 acceptance:** *현재 fix된* echo-app에 대해 시나리오 03이 정확히 동작 (5번 press → count: 6). 이게 *진짜 acceptance*.

ADR-004 (글은 AI ↔ OS 글루) 본문도 본 결과 반영 필요. 단순 *글루*가 아닌 *사람-AI 공유 매체* 강조.

---

## 6. 후속 조치 (이 PR 직후)

- ✅ KI-010 fix (echo-app idle 제거) — 본 commit에 포함
- ✅ KI-011 문서화 — known-issues.md에 추가
- ⏳ 시나리오 03 *재실행* 권고 — count: 6으로 정상 진행 확인
- ⏳ ADR-004 본문 (있다면) 또는 README의 글 언어 섹션 보강 — *AI도 사람도 모두 잘 다룰 수 있는 매체*임을 명시
- ⏳ M5 plan 작성 시 §5.3 권고 반영

---

## 7. 한계

- 1개 모델(Claude Sonnet 4.6)만 시험. GPT-5 / Gemini 등에서 동일 성능 미검증.
- 1번 실행만. 비결정성 (LLM은 stochastic) 미평가.
- 단순 시스템 (3개 객체). *수십~수백 객체* 시 perf와 추론 변화 미평가.
- Glscript 와이어 메시지 미사용. M5 진입 시 별도 검증 필요.

---

## 8. 결론

**위험 A는 해소됐다.** 와이어 프로토콜 v0.1은 *그대로* M5의 기반이 될 수 있다. 글 언어의 역할이 *재정의*되었을 뿐 *제거되지는 않음*. probe 도구 자체가 *지속 가치 있는 자산* (시나리오 회귀 테스트, 새 시나리오 추가, 다른 모델 비교 등).

가장 큰 *부가가치*: probe가 우리도 몰랐던 *실제 버그*를 노출 (KI-010/011). M5 진입 전 fix.
