# GeulOS

> *AI-네이티브 OS의 참조 구현 시도.* 사용자에게는 GUI, AI에게는 CLI. 모든 상호작용 요소가 1급 객체이고 모든 동작이 이벤트인 — *시간 여행이 OS에 1급 기능으로 박힌* 작업 환경.

## 무엇이 다른가 (한 줄씩)

1. **풀 OS 대체** — PID 1을 GeulOS가 점유. 프레임워크가 아니라 OS.
2. **단일 라이터 이벤트 루프** — 모든 동작(클릭/AI 호출/앱 변경)이 직렬화된 이벤트 로그. *공짜로 따라오는 것:* 매크로 녹화·리플레이·undo/redo가 OS 1급 기능. Apple App Intents도 MCP도 이걸 못 함.
3. **사용자 클릭 ≡ AI 호출** — 마우스 입력과 외부 AI의 호출이 *동일한 이벤트 파이프라인*을 통과. 같은 결과, 같은 권한 검사, 같은 감사 로그. 어디서도 본 적 없는 대칭성.
4. **AI-agnostic, 로컬 LLM 1급 지원** — Claude/GPT(클라우드), Ollama(호스트), VM 내부 LLM, 브라우저 오케스트레이션 UI — 네 토폴로지 모두 지원. AI는 GeulOS에 *결합되지 않음*.

## 핵심 아이디어 — 문제 정의

OpenClaw 류의 AI 자동화가 비효율적인 이유는 AI를 *고려하지 않은 시대의 환경* 위에서 사람의 행동을 모방하기 위해 픽셀 좌표 계산·스크린샷 검증을 매 단계마다 반복하기 때문이다. 의미(semantic) → 픽셀(pixel) → 의미 재구성의 *왕복*이 시간·토큰·오류율의 주범.

GeulOS는 이 왕복을 *원천 차단*한다 — UI의 모든 요소가 객체 ID로 식별되고, AI는 좌표가 아닌 의미로 시스템과 대화한다. 비유: *시각장애인에게 옆에서 말로 설명하기 (기존)* vs *점자로 된 정식 설명서를 손에 쥐어주기 (GeulOS)*.

설계 문서: [`docs/specs/2026-05-17-geulos-design.md`](docs/specs/2026-05-17-geulos-design.md)

## 상태

| 마일스톤 | 분량 | 상태 |
|---|---|---|
| M0 부트스트랩 | 4주 | ✅ |
| M1 객체 서버 + 이벤트 버스 | 8주 | ✅ |
| M1.5 geulosh 검증 도구 | 1주 | ✅ |
| M2 와이어 프로토콜 + TCP | 4주 | ✅ |
| M3 앱 런타임 + 권한 매니저 | 4주 | ✅ |
| Toolchain Bump (Rust 1.95) | 1일 | ✅ |
| **M4 컴포지터 GUI** | **8주** | **✅** |
| M5 AI 어댑터 인프라 (ADR-015 재배치) | 3주 | 진행 중 |
| M5.5 글 VM 임베드 (글 G1~G4 완료 후) | 5주 | 연기 |
| M6 VM 부팅 통합 | 4주 | |
| M7 도그푸딩 (메모장) | 4주+ | |

총 ~11.5개월 일정 중 약 **6.25개월** (54%) 진척. 일정은 *낙관적 추정*. 외부 약속은 마일스톤 단위로만.

## 아키텍처 4층

```
AI 클라이언트  ──▶  글 AI I/O 드라이버  ──▶  GeulOS 코어  ──▶  Linux 커널  ──▶  하이퍼바이저
   (외부)            (Rust + 글 VM)         (Rust, PID 1)     (보이지 않음)
```

- **③ Linux 커널** — 드라이버·FS·네트워크 (보이지 않는 층)
- **② GeulOS 코어** — 객체 서버 + 이벤트 버스 + 컴포지터 + 권한 매니저 + 앱 런타임 (Rust)
- **① 글 AI I/O 드라이버** — AI의 자연어 스크립트를 OS 동작으로 번역 (Rust + 글 VM 임베드)
- **(외부)** — Claude / GPT / 로컬 LLM (Ollama 등) 무엇이든 가능

## 글 언어의 위치

[글 언어](https://github.com/wwoosshh/geul-lang)는 한글 자연어 문법(SOV·조사 바인딩)으로 동작하는 별도 프로그래밍 언어 — **GeulOS의 *선택적 흐름 제어 레이어***이지 표준 인터페이스가 아니다.

| 누가 | 무엇으로 GeulOS와 대화하나 |
|---|---|
| 표준 인터페이스 | **와이어 프로토콜** (JSON over TCP/Unix sock) — Hello/Mount/Invoke/Subscribe/Query/Event |
| 단발 호출 (90%) | 와이어 프로토콜 직통 (마이크로초 단위) |
| 다단계 자동화 | 글 코드 한 덩어리를 Glscript 메시지로 (글 VM이 호스트 함수로 OS와 대화) |

글이 약해져도 OS는 살고, 글이 강해지면 OS도 강해지는 *디커플드 구조*.

### 왜 글 언어인가

- **사람-AI 공유 매체** — 자동화 스크립트는 *사람도 읽어야* 한다. AppleScript가 25년간 살아남은 이유와 동일.
- **프로젝트 정체성** — "AI와 OS가 자연어로 대화한다"는 *이야기*는 외부 청중에게 강력.
- **장기 옵션** — M7 이후 글-네이티브 시스템 컴포넌트로 점진 마이그레이션 경로 (설계 문서 §9.7).

*"LLM이 자연어 코드 생성을 잘한다"*는 정당화는 일부러 *사용하지 않는다* — 2026년 현재 LLM은 JSON/Python/JS 생성이 더 정확하고, Korean SOV는 토큰 효율도 불리. 글의 가치는 *기술적 LLM 친화성이 아니라 인간 친화성과 정체성*에 있다.

## 비교 (요약)

| 측면 | GeulOS | AIOS (Rutgers) | MCP (Anthropic) | App Intents (Apple) |
|---|---|---|---|---|
| 형태 | 풀 OS 대체 (PID 1) | LLM-as-kernel 미들웨어 | AI ↔ 도구 통신 표준 | iOS 앱 액션 매니페스트 |
| 단일 객체 트리 | ✅ | ❌ | ❌ (앱별 파편화) | ❌ (앱별) |
| 사용자 클릭 = AI 호출 | ✅ (동일 파이프라인) | ❌ | ❌ | ❌ |
| 매크로/undo OS 1급 | ✅ | ❌ | ❌ | ❌ |
| AI-agnostic | ✅ (Claude/GPT/Ollama/…) | LLM 종속 | ✅ | iOS 종속 |
| 시장 점유 의도 | ❌ (참조 구현 자리) | 학계 | 광범위 | 거대 |

**GeulOS는 시장 점유 경쟁 안 함** — *AI-네이티브 OS 아키텍처의 정답을 가장 먼저, 가장 명확하게 보여준 참조 구현* 자리를 노린다.

## 직접 실행

### 1. 인터랙티브 셸 (M1.5 이후 가능)

```powershell
cd C:\GeulOS  # 또는 클론한 위치
cargo run -p geulos-shell
> mount text "안녕 GeulOS"
> mount button "확인"
> ls
> invoke #2 press
> exit
```

### 2. 자동 검증 시나리오

```powershell
cargo run -p geulos-shell -- --script tools/geulosh/scripts/m1_smoke.gsh
```

### 3. 3터미널 전체 시스템 (M4 이후)

```powershell
# 터미널 A — OS 코어
cargo run -p geulos-server-host

# 터미널 B — 앱
cargo run -p geulos-echo-app

# 터미널 C — 컴포지터 (GUI 윈도우)
cargo run -p geulos-compositor
```

자세한 절차: [`docs/manual-tests/m4-acceptance.md`](docs/manual-tests/m4-acceptance.md)

## 크레이트 구조

| 크레이트 | 역할 | 마일스톤 |
|---|---|---|
| `core` | 객체 서버, 이벤트 버스, 권한 매니저 (TCB) | M1 |
| `proto` | 와이어 프로토콜 타입 + JSON 길이 접두사 codec | M2 |
| `server-host` | 비동기 TCP 서버 + ObjectServer 액터 | M2 |
| `compositor` | 사용자 GUI 컴포지터 (winit + softbuffer + fontdue) | M4 |
| `ai-bridge` | LLM 어댑터(Claude 등) + 와이어 클라이언트 + 세션 매니저 | M5 |
| `apps/echo-app` | 데모 앱 (count 버튼) | M3 |
| `tools/geulosh` | CLI 검증 셸 (REPL + 스크립트 모드) | M1.5 |

## 빌드

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

테스트 ~160개 + 1개 ignored subprocess acceptance.

## 알려진 한계 / 추적 중

[`docs/known-issues.md`](docs/known-issues.md) 참고.

## 라이선스

MIT OR Apache-2.0
