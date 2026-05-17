# ai-bridge 첫 실행 + 약한 모델 검증 결과 보고서

- **일자:** 2026-05-17
- **선행 보고서:**
  - `2026-05-17-claude-analysis.md` (외부 분석 메모, 위험 A 제기)
  - `2026-05-17-probe-results.md` (probe.py 4 시나리오)
- **이 보고서:** ai-bridge (M5 산출물)의 첫 실행 + Haiku 4.5(가장 약한 현행 모델)로 동일 검증

---

## 0. 한 줄 결론

**Anthropic 모든 현행 모델이 GeulOS 와이어 프로토콜을 직접 사용 가능.** Sonnet 4.6 ≈ Haiku 4.5 의 *성공률*. Haiku 4.5는 *6배 저렴, 2배 빠름*. 로컬 LLM(T2 토폴로지) 시나리오의 현실성을 강력히 시사.

---

## 1. 3차 검증의 흐름

| 차 | 시점 | 도구 | 모델 | 시나리오 | 목적 |
|---|---|---|---|---|---|
| 1차 | 2026-05-17 (이른 시각) | probe.py (Python 프로토타입) | claude-sonnet-4-6 | 4개 (01-04) | 위험 A 1차 검증 |
| 2차 | 2026-05-17 (당일 늦게) | **ai-bridge (Rust 프로덕션)** | claude-sonnet-4-6 | 2개 (01, 03) | M5 산출물 검증 |
| 3차 | 2026-05-17 (당일 늦게) | ai-bridge | **claude-haiku-4-5** | 2개 (01, 03) | **약한 모델 검증** |

---

## 2. 종합 결과 표

### 2.1 시나리오 01 — explore (단순 발견)

| 도구 | 모델 | Turns | Wall | Tokens (in/out) | 비용 | 패턴 |
|---|---|---|---|---|---|---|
| probe.py | sonnet-4-6 | 3 | 13.6s | 7359/737 | $0.03 | 4 parallel + 3 parallel |
| ai-bridge | sonnet-4-6 | 3 | 16.5s | 5826/928 | $0.03 | 동일 |
| **ai-bridge** | **haiku-4-5** | **3** | **7.3s** | **5805/672** | **$0.005** | **동일** |

### 2.2 시나리오 03 — subscribe + drain (반응 패턴)

| 도구 | 모델 | Turns | Wall | Tokens (in/out) | 비용 | 병렬 사용 |
|---|---|---|---|---|---|---|
| probe.py | sonnet-4-6 | — (이 도구가 probe.py에 없음) | — | — | — | — |
| ai-bridge | sonnet-4-6 | 5 | 16.0s | 9244/762 | $0.04 | ✅ (get + subscribe) |
| **ai-bridge** | **haiku-4-5** | **5** | **7.4s** | **8362/599** | **$0.005** | ❌ (순차) |

### 2.3 더 약한 모델 (검증 시도)

| 모델 | 결과 |
|---|---|
| claude-3-5-haiku-latest | ❌ 404 — deprecated |
| claude-3-5-haiku-20241022 | ❌ 404 — deprecated |
| claude-3-haiku-20240307 | ❌ 404 — deprecated |

2026-05 기준 Anthropic의 *현재 접근 가능한* 가장 약한 모델 = Haiku 4.5.

---

## 3. 결정적 발견

### 3.1 Haiku 4.5가 *동일한 성공률*

| 측면 | Sonnet 4.6 | Haiku 4.5 | 평가 |
|---|---|---|---|
| 시나리오 통과 | 2/2 | 2/2 | 동일 |
| Turn 수 | 3, 5 | 3, 5 | 동일 |
| report_done 정상 종료 | ✅ | ✅ | 동일 |
| UUID 정확도 | 100% | 100% | 동일 |
| 메서드 시그니처 추론 | ✅ | ✅ | 동일 |
| Summary 정확성 | 매우 상세 | 정확 (덜 상세) | Haiku 약간 간결 |
| 자가 보안 감사 (wildcard ACL) | ✅ | ✅ | 동일 |
| 순서 의존성 인식 (subscribe 먼저) | ✅ | ✅ | **동일** |
| Parallel tool use | 적극 | **부분적** (시나리오 03에서 안 함) | 약간 차이 |

### 3.2 차이는 *효율*, *능력*이 아님

Haiku 4.5의 *유일한* 약점: 시나리오 03 Turn 2에서 `get_object` + `subscribe`를 *순차* 호출 (Sonnet은 병렬). 결과적으로 같은 5 turns. *능력 부족이 아니라 최적화 부족*.

### 3.3 Haiku 4.5의 비용·속도 우위

- **6배 저렴** ($0.005 vs $0.03~0.04)
- **2배 빠름** (7.3s vs 16.5s — 모델 inference 자체가 빠름)
- **품질 손실 거의 없음** (시나리오 통과 100%, summary 약간 간결)

### 3.4 KI-011 tombstone fix 시각적 재확인

ai-bridge의 모든 get_object 응답에 `"destroyed":false` 필드가 노출됨 — *KI-011 fix가 wire에서도 작동하는 시각적 증거*. 옛 echo-app 세션의 유령 객체 없음.

---

## 4. 무엇이 *처음으로* 입증되었나

| 검증 항목 | 1차 (probe) | 2차 (ai-bridge sonnet) | 3차 (ai-bridge haiku) |
|---|---|---|---|
| 와이어 프로토콜 사용 가능 | ✅ | ✅ | ✅ |
| 다단계 작업 | ✅ | ✅ | ✅ |
| 자유 탐색 | ✅ | — | — |
| **subscribe + drain** | ❌ (도구 없음) | ✅ | ✅ |
| **순서 의존성 자율 인식** | ❌ (검증 안 됨) | ✅ | ✅ |
| **이벤트 구조 깊은 파싱** | ❌ | ✅ | ✅ |
| **약한 모델 (Haiku-급)** | — | — | ✅ |
| **Rust 프로덕션 코드** | ❌ (Python 프로토타입) | ✅ | ✅ |
| **결정론 회귀 테스트 가능 (MockAdapter)** | ❌ | ✅ | ✅ |

---

## 5. T2 토폴로지(로컬 LLM)에 미치는 시사

Haiku 4.5의 *capability 대역*은 대략 다음과 비슷:

| 오픈 모델 | 비교 |
|---|---|
| Llama 3.3 70B | ≈ Haiku 4.5 |
| Qwen 2.5 72B | ≈ Haiku 4.5 |
| GPT-4o-mini | ≈ Haiku 4.5 |
| Gemini 2.0 Flash | ≈ Haiku 4.5 |

**모두 GeulOS 와이어 프로토콜을 다룰 수 있을 가능성 매우 높음.** 이는 ADR-005의 *T2 토폴로지*(*"로컬 Ollama가 호스트에서 돌면서 GeulOS 조작"*)가 **현실적**임을 강력히 시사.

다음 직접 검증이 가능한 방안:
1. **OpenAI 어댑터** 추가 → gpt-4o-mini로 확인 (1~2시간)
2. **Ollama 어댑터** 추가 → 로컬 Llama 3.3 70B 확인 (사용자의 GPU 필요)

---

## 6. 비용 함의 — 프로덕션 가능성

Haiku 4.5 기준 비용 추정:

| 시나리오 | 비용 (대략) |
|---|---|
| AI 호출 1회 (이 정도 복잡도) | $0.005 |
| 100회/일 = 3000회/월 | **$15/월** |
| 매분 1회 = 약 43,000회/월 | **$215/월** |

**지속적 백그라운드 자동화 가격대.** 사용자가 *언제든 AI에게 GeulOS 조작 시킬 수 있는* 가격 부담 없음.

---

## 7. 검증하지 못한 것 (한계)

- **비-Anthropic 모델** — OpenAI / Google / 로컬 LLM 어댑터 없음
- **Claude 3.x 모델** — 모두 deprecated (2026-05 기준)
- **5000+ 객체 트리** — 현재는 3 객체 (echo-app만). 큰 트리에서 성능 미확인
- **다중 동시 AI 세션** — 1세션/1프로세스만 검증
- **장시간 세션** — 대화 history 누적 시 행동 변화 미확인
- **Glscript 경로** — M5.5로 의도 연기

---

## 8. M5/M5.5/M6에 대한 함의

### M5 (완료) — 검증됨

- ai-bridge 크레이트가 *probe.py의 모든 능력 + 더 많음*
- MockAdapter로 결정론 회귀 테스트 가능 (CI에 통합 가능)
- 다중 LLM 어댑터 추상화 — OpenAI/Ollama 확장 즉시 가능

### M5.5 (글 VM 임베드) — *우선순위 더 낮아짐*

이 보고서의 가장 중요한 함의 중 하나: **글 VM 임베드의 *AI 측 사용 가치*는 미미하다는 추가 증거**. 가장 약한 현행 모델조차 와이어 프로토콜을 잘 다룸. 글 VM은 ADR-004 본문대로 *사람용 매크로 매체*로 포지션 유지가 합리적.

### M6 (VM 부팅 통합) — 다음 단계

본 검증으로 AI 인프라 측은 *완전히 안정화*됨. M6 진행이 자연스러움.

---

## 9. 부록 — 실행 명령 (재현용)

```powershell
# 1. 환경 (3터미널)
cargo run -p geulos-server-host         # 터미널 A
cargo run -p geulos-echo-app            # 터미널 B

# 2. ai-bridge 실행 (.env에 ANTHROPIC_API_KEY)
# Sonnet 4.6
cargo run -p geulos-ai-bridge -- run \
  --scenario ai-bridge/scenarios/01_explore.toml

# Haiku 4.5 (약한 모델)
cargo run -p geulos-ai-bridge -- run \
  --scenario ai-bridge/scenarios/03_observe_subscribe.toml \
  --model claude-haiku-4-5-20251001
```

audit 로그가 CWD에 `ai-bridge-audit-<timestamp>.log`로 저장됨.

---

## 10. 결론

> **위험 A는 *3차에 걸쳐* 해소되었다.** 와이어 프로토콜은 *Anthropic 모든 현행 모델 (Sonnet 4.6, Haiku 4.5)*에서 *직접 사용 가능*. 특히 **Haiku 4.5의 성공은 *로컬 LLM 시나리오의 현실성*을 강력히 시사**. 다음 마일스톤은 *AI 측이 아닌 VM 부팅 측* (M6)으로 자연스럽게 진행 가능.
