# ADR-017: 커널 모듈은 Alpine apk에서 추출, `finit_module`로 로드

- **상태:** Accepted
- **일자:** 2026-05-18
- **결정자:** wwoosshh

## 맥락

ADR-016이 결정한 *"GeulOS init이 최소 책임으로 PID 1 점유"* 모델은 첫 부팅에서 검증됐다 (M6 T1~T8). 그러나 M6 acceptance를 외부 ai-bridge → VM 통신까지 확장하려는 시점에 구조적 벽 발견:

> Alpine 커널(LTS·virt 모두)은 모든 NIC 드라이버를 *모듈*로 빌드. 우리 initrd엔 모듈 0개 → `eth0`/`enp0s3` 자체가 생성 안 됨.

진단으로 확인된 사실:
- `vmlinuz-virt`: `virtio_console`만 built-in, 모든 NIC 드라이버 모듈
- `vmlinuz-lts`: `virtio_console`조차도 안 보임, *더 모듈식*
- 두 커널 모두 `/sys/bus/pci/devices`에는 NIC PCI 장치(0x8086:100e e1000 또는 0x1AF4:1000 virtio-net)가 등록되지만, 바인딩할 드라이버가 커널에 없음

이게 ADR-005("AI는 모든 배치 토폴로지 지원")와 충돌:
- T1·T2·T4 토폴로지 모두 외부 ↔ guest 네트워크 필요
- T3(VM 내부 LLM)도 결국 어딘가에서 모델 다운로드를 위해 네트워크 필요

## 결정

**커널 모듈을 Alpine apk에서 *선택적으로* 추출해 initrd에 포함시키고, geulos-init이 `finit_module` syscall로 적재한다.**

구체적으로:

1. **모듈 출처:** Alpine 공식 패키지 `linux-lts-<X.Y.Z>-r<N>.apk` (tar.gz 형식). `/lib/modules/<kernel>/` 디렉터리에 .ko.gz 파일들.
2. **추출 범위:** 부팅 직후 즉시 필요한 *최소 집합* — 현재는 NIC 1개 (e1000 또는 virtio_net + 의존). Phase D 진입 시 virtio-gpu·virtio-input·virtio-blk 추가.
3. **빌드 통합:** `boot/build.ps1`이 빌드 시작 시 `boot/modules/<kernel>/`를 체크. 없으면 `boot/modules/fetch.ps1`로 fresh 다운로드 + 추출. .ko.gz는 빌드 시 압축 풀어 .ko로 변환 (`finit_module`이 압축 형식을 항상 지원하진 않음).
4. **적재 위치:** `geulos-init`이 mount 직후 (network보다 먼저) `/lib/modules/`를 스캔하고 *하드코딩된 순서*로 `finit_module(O_RDONLY .ko fd)` 호출.
5. **의존 순서:** 우선 하드코딩. 추후 `modules.dep` 파싱으로 자동화.
6. **커널·모듈 버전 락:** vmlinuz와 apk를 항상 *같은 빌드 명령*에서 fresh 다운로드 → 버전 어긋남 방지. `fetch.ps1`이 둘을 함께 처리.

## 결과

### 긍정적

- Alpine 패키지 생태계를 *그대로 활용* — 우리가 .ko를 빌드할 필요 없음
- *미래 모든 드라이버 적재 기반* — virtio-gpu, virtio-input, USB, 디스크 컨트롤러 등 모두 같은 메커니즘
- ADR-016의 "최소 init" 약속 유지 — 모듈 로딩이 mount·network·spawn과 동급 *기본 책임*
- 사용자가 추가 모듈이 필요할 때 *.ko 파일 한 줄만 추가*로 즉시 활용 (글-네이티브 OS 마이그레이션 전 과도기에 유용)

### 부정적

- 빌드 파이프라인이 *외부 패키지 다운로드*에 의존 — Alpine CDN 가용성 또는 오프라인 환경 고려 필요 (캐시 디렉터리로 완화)
- 커널 버전 업데이트 시 모듈 재추출 필요 — `fetch.ps1`이 vmlinuz와 apk를 함께 처리해 이 위험을 최소화
- *Alpine 의존성 추가* — 향후 Alpine 외 커널 사용 시 같은 작업 필요

### 중립

- `init_module`/`finit_module` syscall은 `CAP_SYS_MODULE` 필요. PID 1 root는 이 권한 보유 → 문제 없음
- 압축 모듈(.ko.gz, .ko.zst) 지원은 빌드 시 풀기로 단순화 — finit_module의 `MODULE_INIT_COMPRESSED_FILE` flag(커널 6.x+) 사용도 가능하지만 portable한 무압축 방식이 안전

## 대안 검토

### 대안 A: Custom 커널 빌드 (built-in으로 모든 드라이버)

- Buildroot 또는 직접 컴파일로 NIC·virtio-* 드라이버 다수를 *built-in*으로 컴파일
- 거부 이유: ① Windows 호스트에서 Linux 커널 빌드 환경 구축 부담 ② 커널 업데이트 시 매번 재빌드 ③ 모듈 시스템의 *유연성* 포기 (PCI hotplug 같은 future feature 손실) ④ Phase D virtio-gpu도 결국 비-built-in일 가능성 큼 → 같은 문제 재현

### 대안 B: Alpine의 initramfs-lts를 그대로 사용 + 우리 init을 *체이닝*

- Alpine init이 PID 1으로 부팅 → 모듈 로드 → exec /bin/geulos-init
- 거부 이유: ① ADR-016의 "PID 1 = GeulOS" 약속 약화 ② Alpine init이 하는 일이 *우리가 통제 안 하는* 영역으로 들어옴 ③ Alpine 의존도가 *모듈 추출*보다 더 큼

### 대안 C: Hyper-V Synthetic NIC (`hv_netvsc`)

- 커널 부팅 로그에 `hv_netvsc` 드라이버 *built-in* 확인됨 (LTS 커널)
- 거부 이유: QEMU가 Hyper-V VMBus를 게스트에 노출하지 않음 (WHPX는 호스트 가속만 사용, 게스트 인터페이스는 일반 virtio/e1000 PCI). 즉 *드라이버는 있는데 매칭할 디바이스가 없음*

### 대안 D: Modloop 직접 마운트 (Alpine 표준 방식)

- Alpine의 `modloop.squashfs`를 다운로드, 게스트 부팅 직후 squashfs 마운트해 `/lib/modules` 노출
- 거부 이유: ① squashfs 마운트 + 모듈 적재 *둘 다* 필요 — 복잡도 증가 ② squashfs 자체가 kernel 모듈일 가능성 (chicken-and-egg) ③ 우리가 필요한 모듈은 *몇 개뿐* — 전체 모듈 트리 마운트는 과함

## 참고

- 설계 §2.1, §4.2 (AI 토폴로지)
- ADR-005 (AI는 모든 배치 토폴로지 지원)
- ADR-016 (최소 init 책임)
- Linux man `init_module(2)`, `finit_module(2)`
- Alpine wiki: https://wiki.alpinelinux.org/wiki/Kernel_modules
