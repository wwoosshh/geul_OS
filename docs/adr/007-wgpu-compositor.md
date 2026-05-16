# ADR-007: 컴포지터 렌더링 백엔드로 wgpu 채택

- **상태:** Accepted (잠정, M4 완료 시 재검토)
- **일자:** 2026-05-17
- **결정자:** wwoosshh

## 맥락

컴포지터는 객체 트리를 사용자 모니터에 그린다. GeulOS는 VM 게스트이므로 호스트 GPU에 virtio-gpu로 접근한다. Rust 생태계에서 GPU 추상화 옵션은 다음과 같다:

1. wgpu (Rust 표준 GPU 추상화, WebGPU 호환)
2. Linux DRM/KMS 직접 사용
3. winit + softbuffer (소프트웨어 렌더링)

## 결정

**wgpu.** Rust 생태계의 사실상 표준 GPU 추상화이며, virtio-gpu를 자연스럽게 활용한다.

## 결과

### 긍정적

- Rust 생태계 표준 — 풍부한 라이브러리·예제·문서
- 크로스플랫폼 (개발은 Windows 호스트, 배포는 VM 안 Linux)
- WebGPU 사양 기반 — 미래에 브라우저 컴포지터 변종도 가능
- GPU 가속 → 매끄러운 UI

### 부정적

- 학습 곡선 가파름 (M4 직전 1주 학습 스파이크 권장)
- virtio-gpu 드라이버 안정성에 의존

### 중립적

- 만약 wgpu가 부담스러우면 M4 시점에 softbuffer로 *대체* 가능. 객체 트리 ↔ 렌더링 경계가 명확하므로 백엔드 교체는 국소적

## 대안 검토

- **DRM/KMS 직접:** Linux 종속. 저수준 부담 큼. 글-네이티브 커널 마이그레이션 시 다시 짜야 함.
- **softbuffer:** 가장 단순하지만 GPU 미활용. 백업 옵션으로 유지.

## 참고

- 관련 스펙: `docs/specs/2026-05-17-geulos-design.md` §5.4
- 재결정 시점: M4 완료 시 (설계 문서 §9.7)
- wgpu: https://wgpu.rs
