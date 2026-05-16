# ADR-002: OS 뼈대 구현 언어로 Rust 채택

- **상태:** Accepted
- **일자:** 2026-05-17
- **결정자:** wwoosshh

## 맥락

GeulOS의 객체 서버는 시스템에 영원히 떠 있고, 수천 fd를 동시에 다루는 이벤트 루프이며, *어떤 mutate도 다른 컴포넌트의 안전을 깰 수 없어야 한다*. 메모리 안전성·동시성 안전성을 컴파일러가 *강제*하지 않으면 "안정성" 약속이 코드 리뷰의 성실성에 의존하게 된다.

선택지:
1. Rust
2. Zig
3. C
4. C++

## 결정

**Rust.**

핵심 근거:
- 메모리 안전 + GC 없음 — *코드 리뷰가 아니라 컴파일러가 차단*
- async/await + tokio — 이벤트 루프 일급 지원
- C FFI 최강 — virtio·libdrm·libinput 통합 자연스러움
- 선례 검증: Redox OS(100% Rust), Asahi Linux GPU 드라이버, Windows 커널 일부 재작성, Linux 커널 본체가 Rust 드라이버 수용
- 빅3 OS가 모두 Rust로 *이주 중* — 2026년에 새 OS를 시작하면서 C로 가는 것은 시대 역행

## 결과

### 긍정적

- 컴파일이 통과한 코드는 메모리 안전 보장
- 거대한 크레이트 생태계 (`tokio`, `serde`, `wgpu`, `uuid` 등)
- 사용자가 비판한 "Windows의 불안정한 GUI" 문제의 근본 원인을 컴파일러로 차단

### 부정적

- 학습 곡선: 빌림 검사기와 친해지는 데 시간
- 컴파일 시간이 길 수 있음 (대안 언어보다 느림)

### 중립적

- 글 언어로 *코어*를 작성하지 않음. 글은 별도 위치(AI 글루)에서 활용 — ADR-004 참조.

## 대안 검토

- **Zig:** pre-1.0 (현재 0.13). 1~2년 안에 ABI/문법 깰 가능성. OS 뼈대 같은 장기 코드베이스에 부담.
- **C:** 메모리 안전 0. "Windows의 불안정한 GUI" 비판 정신과 정면 충돌. 객체 서버에서 단 한 번의 UAF가 WWE 대본 비유 전체를 망가뜨림.
- **C++:** 새 OS 프로젝트에서 2026년에 채택할 이유가 약함. 복잡도 대비 안전성 이득 적음.

## 참고

- 관련 스펙: `docs/specs/2026-05-17-geulos-design.md` §3 원칙 4
- Redox OS: https://www.redox-os.org
- Microsoft Rust in Windows: https://msrc.microsoft.com/blog/2019/07/we-need-a-safer-systems-programming-language/
