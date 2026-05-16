# GeulOS

> *AI에게 점자 설명서를 주는 OS.*

사용자에게는 GUI, AI에게는 CLI인 OS. 모든 상호작용 요소는 1급 객체이고 모든 동작은 이벤트이며, [글 언어](https://github.com/wwoosshh/geul-lang)는 AI ↔ OS의 자연어 글루로 동작한다.

## 상태

브레인스토밍 단계 완료 (2026-05-17). M0 부트스트랩 진행 중.

설계 문서: [`docs/specs/2026-05-17-geulos-design.md`](docs/specs/2026-05-17-geulos-design.md)

## 핵심 아이디어

OpenClaw 류의 AI 자동화가 비효율적인 이유는 AI를 *고려하지 않은 시대의 환경* 위에서 사람의 행동을 모방하기 위해 픽셀 좌표 계산·스크린샷 검증을 매 단계마다 반복하기 때문이다. GeulOS는 이 왕복을 *원천 차단*한다 — UI의 모든 요소가 객체 ID로 식별되고, AI는 좌표가 아닌 의미로 시스템과 대화한다.

## 아키텍처 4층

```
AI 클라이언트  ──▶  글 AI I/O 드라이버  ──▶  GeulOS 코어  ──▶  Linux 커널  ──▶  하이퍼바이저
   (외부)            (Rust + 글 VM)         (Rust, PID 1)     (보이지 않음)
```

- **③ Linux 커널** — 드라이버·FS·네트워크 (보이지 않는 층)
- **② GeulOS 코어** — 객체 서버 + 이벤트 버스 + 컴포지터 + 권한 매니저 + 앱 런타임 (Rust)
- **① 글 AI I/O 드라이버** — AI 자연어/스크립트를 OS 동작으로 번역 (Rust + 글 VM 임베드)
- **(외부)** — Claude / GPT / 로컬 LLM (Ollama 등) 무엇이든 가능

## 크레이트 구조

| 크레이트 | 역할 | 마일스톤 |
|---|---|---|
| `core` | 객체 서버, 이벤트 버스, 권한 매니저 (TCB) | M1 |
| `proto` | 와이어 프로토콜 타입 | M2 |
| `compositor` | 사용자 GUI 컴포지터 | M4 |
| `glue-ai` | AI I/O 드라이버 (글 VM 임베드) | M5 |
| `apps/echo-app` | 데모 앱 | M3 |

## 빌드

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## 라이선스

MIT OR Apache-2.0
