# ADR-014: MCP를 적이 아닌 진입 경로로 — 와이어 프로토콜 ↔ MCP 양방향 변환기

- **상태:** Proposed (M5 또는 M5.5 구현 결정)
- **일자:** 2026-05-17
- **결정자:** wwoosshh
- **트리거:** 외부 분석 메모 §4.2 ([`docs/review/2026-05-17-claude-analysis.md`](../review/2026-05-17-claude-analysis.md))

## 맥락

Anthropic의 **Model Context Protocol (MCP)**가 *"AI ↔ 도구 통신 표준"* 자리를 빠르게 점유 중이다 (2025년 발표, 2026년 5월 현재 iOS 26 시스템 통합 진행 중, 수많은 앱이 MCP 서버 노출 시작).

GeulOS의 가치 제안 중 *"구조화된 AI 인터페이스 제공"*은 MCP가 *OS 교체 없이* 풀려고 하는 문제다. 만약 *모든 앱이 MCP 서버를 갖는 것이 표준*이 되면 GeulOS의 가치 제안 상당 부분이 약화될 위험.

### MCP와 GeulOS 와이어 프로토콜의 비교

| 측면 | MCP | GeulOS 와이어 프로토콜 |
|---|---|---|
| 전송 | JSON-RPC 2.0 over stdio/SSE/HTTP | JSON over TCP (M2), UDS (M6+) |
| 메시지 종류 | tools/resources/prompts | Hello/Mount/Invoke/Subscribe/Query/Event/StateSet/Get/Glscript |
| 컨텍스트 모델 | 앱별 파편화 (각 MCP 서버는 자기 도구만) | **단일 객체 트리** (OS 차원 통합) |
| 단일 라이터 직렬화 | ❌ (각 서버 독립) | ✅ (ObjectServer 액터) |
| 매크로/undo OS 1급 | ❌ | ✅ (이벤트 로그 부수효과) |
| 사용자 클릭 = AI 호출 | ❌ | ✅ (동일 파이프라인) |
| 생태계 (2026-05) | 광범위, 성장 중 | 출발 |

### GeulOS가 잃을 것 / 지킬 것

**MCP 표준이 점유해도 사라지지 않는 GeulOS 가치:**
- OS 차원 통합 객체 트리
- 단일 라이터에서 *자동으로 따라오는* 매크로/undo/리플레이
- 사용자 클릭과 AI 호출의 동일 파이프라인 (대칭성)
- *전체 시스템*을 일관된 권한 모델로 다루는 능력

**MCP가 점유하면 GeulOS만 단독 제공 시 *덜 가치 있어지는* 것:**
- 단일 앱 단위의 AI 도구 노출 (MCP가 이미 표준이라 굳이 GeulOS 와이어 학습 부담 안 줘도 됨)
- "구조화된 AI 인터페이스"라는 *추상적 광고 문구*

## 결정

**GeulOS는 MCP를 적이 아닌 *진입 경로*로 다룬다.** 와이어 프로토콜 ↔ MCP의 *양방향 변환기*를 GeulOS 생태계의 일부로 제공한다.

구체적으로:

### MCP → GeulOS (Inbound)

외부에 *MCP 서버로 보이는 어댑터*를 제공. MCP 클라이언트(Claude Desktop, IDE 통합 등)가 *MCP를 말한다*고 생각하지만 실제로는 GeulOS와 대화.

- **변환:** MCP `tools/call` → GeulOS `Invoke`. MCP `resources/read` → GeulOS `Query` + `Get`. MCP `prompts` → 의미 매핑 검토.
- **장점:** 기존 MCP 클라이언트가 *수정 없이* GeulOS에 접속. MCP 생태계 전체가 GeulOS의 잠재 사용자.

### GeulOS → MCP (Outbound)

외부의 *MCP 서버를 GeulOS 안의 객체 트리로 노출하는 어댑터*. 예: `taskwarrior` MCP 서버가 있으면, GeulOS 안에 `app:mcp-taskwarrior:*` 객체들로 나타남.

- **변환:** MCP 서버의 tools를 GeulOS `Object.methods`로 매핑. 사용자/AI는 *GeulOS 안에서* taskwarrior와 대화. 단일 객체 트리·undo·매크로의 혜택을 *MCP 서버 작성 비용 0*으로 얻음.
- **장점:** GeulOS 가치 제안 #2 (단일 라이터의 부수효과)가 MCP 생태계 전체로 확장. 사용자는 *모든* MCP 도구를 OS 1급 undo로 다룰 수 있음.

### 의도적으로 *변환 안 함*

- **Glscript 메시지** → MCP에 대응 없음. 다단계 워크플로우 추상화는 GeulOS-only 기능.
- **사용자 클릭 ↔ Invoke 대칭성** → MCP는 사용자 입력 개념이 없음. GeulOS의 영혼이라 그대로 유지.

## 결과

### 긍정적

- **MCP 생태계가 GeulOS의 잠재 시장.** 적이 아니라 *AI 도구의 즉시 라이브러리*.
- **GeulOS의 진짜 차별점(단일 라이터의 부수효과)이 부각됨** — MCP가 못 하는 것을 GeulOS가 한다는 게 명확해짐.
- **빅테크의 MCP 채택이 GeulOS에게도 호재**가 됨 — Apple iOS 26 MCP 통합 → 그 MCP 표준을 GeulOS도 말함.
- **"단일 객체 트리 + undo" 시연 영상이 강력**해짐 — *"이미 깔린 모든 MCP 도구가 OS 차원 undo와 매크로를 자동으로 얻는다"*.

### 부정적

- **추가 구현 부담** — 양방향 변환기 = 2개 어댑터 (M5 또는 M5.5 분량 약 2~3주).
- **MCP 사양 변동 추적 의무** — MCP는 빠르게 진화. spec 변경 시 어댑터 갱신 필요.
- **GeulOS의 정체성 흐림 우려** — "MCP를 그냥 다 쓰면 GeulOS 왜 필요하냐"는 의문에 *대답이 있어야* 함 (= 위의 "잃지 않는 가치" 4개).

### 중립

- **변환기 구현은 별 크레이트 `mcp-bridge`로 분리**. GeulOS 코어와 디커플드. MCP 사양 변경의 영향 격리.
- **타이밍**: M5 (글 AI 드라이버) 완료 후 M5.5로 추가하거나, M5 안에 inbound 변환기만 우선 포함. M6 진입 전 outbound도 구현 권장.

## 대안 검토

- **MCP 무시 (GeulOS 단독 길):** 단기적으로 단순하지만 *생태계 충돌* 위험. MCP가 표준이면 사용자/AI가 GeulOS 와이어 프로토콜을 *별도로* 학습하는 부담. 빅테크 채택이 가속될수록 격차 커짐.
- **MCP 채택 (GeulOS 와이어 프로토콜 폐기):** GeulOS의 차별점(단일 객체 트리, 매크로/undo) 포기. 그러면 *GeulOS가 존재할 이유 자체가 약화*. 명백히 잘못된 결정.
- **MCP 호환층 *없이* MCP 도구를 수동 GeulOS 앱으로 포팅:** 비현실적 — MCP 도구는 빠르게 많아짐. 자동 변환기 없이는 따라잡기 불가.

## 참고

- 외부 분석 메모: [`docs/review/2026-05-17-claude-analysis.md`](../review/2026-05-17-claude-analysis.md) §4.2
- MCP 명세: https://modelcontextprotocol.io/specification
- 설계 문서 §5.3 (와이어 프로토콜)
- ADR-005 (AI는 GeulOS에 결합되지 않음 — 본 ADR은 그 원칙의 자연스러운 확장)
- 다음 결정 시점: M5 plan 작성 시 — inbound 변환기를 M5 안에 포함시킬지 결정.
