# ADR-016: GeulOS가 PID 1을 직접 점유하되 *최소 init 책임만* 수행

- **상태:** Accepted
- **일자:** 2026-05-17
- **결정자:** wwoosshh

## 맥락

설계 §2.1의 *"PID 1을 GeulOS 객체 서버가 점유한다"* 약속을 *어떻게 실현*할지가 M6의 첫 결정. 외부 분석 메모 (`docs/review/2026-05-17-claude-analysis.md`) §위험 D는 *완전 대체*의 위험을 지적:

> "/sbin/init를 GeulOS 코어로 *완전히* 대체하면 udev, dbus, mount, network 같은 *기본 시스템 서비스*가 모두 사라짐. 'ssh도 안 되는 OS'가 됨."

선택지:

1. **순수 PID 1** — server-host를 PID 1로. 다른 어떤 init도 안 함.
2. **GeulOS init layer** — 작은 PID 1 wrapper (`geulos-init`)가 mount/network 책임. 그 위에 server-host + 앱을 spawn. *우리가 "BusyBox init이 하는 일"의 최소 집합을 직접 작성*.
3. **systemd 위 service** — systemd가 PID 1, GeulOS는 그 위 service. *"PID 1 점유" 약속 불일치*.
4. **BusyBox init coexist** — BusyBox init이 PID 1, GeulOS 자동 시작. *우리 약속의 핵심과 어긋남*.

## 결정

**선택지 2: GeulOS init layer.** 새 크레이트 `geulos-init`가 PID 1.

책임:
1. /proc, /sys, /dev (devtmpfs) mount
2. hostname 설정 (옵션)
3. virtio-net 인터페이스 셋업 (QEMU user-mode 활용)
4. `geulosd` (server-host) spawn
5. `geulos-echo-app` spawn
6. SIGCHLD 수신, 좀비 reaping
7. 자식 프로세스 monitoring

*하지 않는* 것:
- udev (정적 device set만 가정)
- dbus (필요 없음)
- 로그인 매니저
- cron
- DHCP 구현 (QEMU user-mode 네트워킹 사용)

## 결과

### 긍정적

- *"PID 1 = GeulOS"* 약속 유지 — 외부 분석가가 지적한 부분이 정확히 우리 응답에 일치
- 부담스럽지 않은 코드 — 추정 ~500줄 Rust (mount + network + spawn + signal)
- 모든 시스템 서비스가 *우리 통제 하*에 있어 *AI 가시성* 보장 (장기 비전)
- BusyBox/systemd 의존 0 — 깔끔한 단일 OS 아이덴티티

### 부정적

- ssh 등 표준 Linux 도구 없음 — *디버깅이 어렵다*. 콘솔(virtio-console) 또는 wire 프로토콜로만 진단 가능
- 부팅 실패 시 *처음에 매우 어려울 가능성*. 단계별 작은 검증 필수
- udev 없음 → 새 device 동적 인식 안 됨. 정적 device set만 활용

### 중립

- 후속에 udev 통합 (M8+) 또는 ssh 추가 (M7+)는 가능. 지금은 *최소 부팅*만 책임.
- DHCP 구현은 *QEMU user-mode* 환경에서 *불필요*. 베어메탈 시점에 재검토.

## 대안 검토

- **선택지 1 (순수 PID 1):** 너무 빈약 — mount조차 없으면 server-host가 /proc 못 읽음. 비현실적.
- **선택지 3 (systemd):** "PID 1 = GeulOS" 약속 불일치. 후속 베어메탈 가는 길에 systemd 의존이 짐. 정체성 약화.
- **선택지 4 (BusyBox coexist):** 깔끔하지만 *우리 약속의 핵심*과 어긋남. *내가 PID 1이라고 했는데 BusyBox가 PID 1*인 모순.

## 참고

- 설계 §2.1 (Linux 위 PID 1 점유)
- 외부 분석 메모 §위험 D (M6 위험)
- ADR-001 (Linux 커널 채용 결정의 후속)
