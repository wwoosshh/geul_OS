# ADR-010: M2의 와이어 프로토콜 전송은 TCP localhost, UDS는 M6 production에서

- **상태:** Accepted
- **일자:** 2026-05-17
- **결정자:** wwoosshh

## 맥락

설계 문서 §5.3과 §9.2는 클라이언트가 Unix 도메인 소켓(`/run/aios/{ai,app}.sock`)으로 객체 서버에 접속한다고 명시. 그러나 dev 환경(Windows 11 Home)에서는 다음 제약이 있다:

- tokio의 `UnixListener`는 `#[cfg(unix)]`로 컴파일됨. Windows에서 미지원.
- Windows 10+에는 AF_UNIX가 있지만 tokio가 자동으로 추상화하지 않음.
- WSL2가 있어도, 호스트의 IDE/터미널에서 직접 디버깅 가능한 편이 개발 사이클이 짧음.

## 결정

**M2의 와이어 프로토콜 1차 전송은 TCP localhost로 한다.** 동일 프로토콜이 UDS와도 호환되도록 codec·핸드셰이크를 *전송 비종속(transport-agnostic)*으로 설계한다.

- 서버 바이너리 (`server-host`)는 `--listen tcp://127.0.0.1:5550` 으로 기본 시작.
- 와이어 메시지 형식 자체는 UDS와 동일 (4바이트 빅엔디언 길이 접두사 + JSON 본문).
- M6 시점에 같은 codec을 UDS 리스너로 한 줄 차이만으로 노출.

## 결과

### 긍정적

- Windows 11 Home dev에서 즉시 빌드·실행
- TCP는 디버깅 용이 (netcat/curl로 raw 메시지 검사 가능)
- 미래 *원격 머신에서 GeulOS VM 조작* 시나리오와도 자연 호환

### 부정적

- Production 시에는 TCP를 외부에 노출하지 않도록 방화벽/바인딩 주의
- mTLS는 M6+에서 추가 (지금은 토큰 기반 인증만)

### 중립

- 와이어 형식 자체가 전송 비종속이므로 M6 마이그레이션 비용 작음

## 대안 검토

- **UDS만:** Windows dev 막힘.
- **명명된 파이프(Windows) + UDS(Linux) 듀얼:** 코드 복잡도 증가 + Windows에서도 dev 외 사용 시나리오 없음.
- **stdio/named pipe in-process:** 멀티 클라이언트 어려움.

## 참고

- 설계 문서 §5.3 (와이어 프로토콜)
- 와이어 프로토콜 v0.1: `docs/specs/wire-protocol-v0.1.md`
