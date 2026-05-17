# GeulOS M6.5 — 커널 모듈 로더 + 외부 네트워크 실행 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. **NEVER push** — controller batches push at end.

**Goal:** GeulOS init이 Alpine 모듈을 적재해 외부 네트워크가 작동. M6 acceptance를 *완전한 4층 통신*까지 확장 — 호스트의 ai-bridge가 forwarded TCP로 VM의 server-host에 접속해 echo-app 객체 발견.

**Why this milestone exists:** M6 작업 중 발견된 구조적 벽 ([[adr-017-kernel-module-strategy]]). Alpine 커널이 모든 NIC 드라이버를 모듈로 빌드 → 우리 initrd엔 모듈 0개 → 외부 통신 불가. *해결 자체로 미래 마일스톤(Phase D virtio-gpu·input)의 기반*.

**Architecture:**
```
fetch.ps1                              build.ps1
  ↓ apk download                          ↓
  ↓ ↓ ↓                                   ↓
boot/modules/<kernel>/                staging
  e1000.ko                              /lib/modules/<kernel>/e1000.ko
  ...                                     /init (geulos-init)
                                          /bin/geulosd
                                          /bin/geulos-echo-app
                                            ↓ mkinitrd
                                          geulos.cpio.gz
                                            ↓ qemu -initrd
geulos-init/src/modules.rs              VM 부팅 ──→  modules.rs:load_all()
  - finit_module syscall wrapper                       ↓ finit_module
  - 하드코딩된 의존 순서                              eth0 인터페이스 등록
  - main.rs: mount → modules → network                ↓
                                                       network.rs 성공
```

**Tech Stack:**
- Alpine `linux-lts` apk 추출 (PowerShell + tar)
- `finit_module(2)` syscall via `libc::syscall`
- 압축 모듈(.ko.gz)은 빌드 시 풀기

**Selection criteria (완료 조건):**
- `boot/build.ps1 -Release` 성공 (모듈 자동 fetch + 포함)
- VM 부팅 콘솔에 `enp0s3 UP (10.0.2.15/24)` 출력
- 호스트의 `cargo run -p geulos-ai-bridge -- run --scenario ai-bridge/scenarios/01_explore.toml` 가 VM의 echo-app 객체 3개 발견 + report_done 통과
- `cargo build --workspace --exclude geulos-init` 그린
- `cargo build --target x86_64-unknown-linux-musl -p geulos-init` 그린
- `cargo test --workspace --exclude geulos-init` 그린

---

## ADR 시드

- **ADR-017 — Kernel module strategy.** Alpine apk에서 추출, `finit_module`로 적재. 본 plan 작성과 동시.

---

## 파일 구조 (사전 매핑)

```
boot/
├── modules/                           # 신규 디렉터리
│   ├── README.md                      # 모듈 추출 절차 안내
│   ├── fetch.ps1                      # apk 다운로드 + .ko 추출
│   ├── .gitignore                     # *.apk, <kernel-version>/ 무시
│   └── <kernel-version>/              # 예: 6.12.81-0-lts/
│       ├── e1000.ko                   # 추출된 모듈 (.gz 풀린 상태)
│       └── (의존 모듈들)
├── kernel/
│   └── vmlinuz                        # fetch.ps1이 함께 갱신
└── build.ps1                          # 수정: 모듈 자동 fetch + 포함

geulos-init/
├── src/
│   ├── main.rs                        # 수정: mount → modules → network
│   ├── modules.rs                     # 신규: finit_module wrapper + 로딩 로직
│   ├── mount.rs
│   ├── network.rs                     # 그대로
│   ├── spawn.rs
│   └── signal.rs
└── Cargo.toml                         # 변경 없음 (libc는 nix 의존을 통해 접근)

docs/
├── adr/
│   └── 017-kernel-module-strategy.md  # 신규
└── plans/
    └── 2026-05-18-geulos-m6-5-module-loader.md  # 이 문서
```

---

## Task T1: ADR-017 + 본 plan 작성

- [x] **이미 작성 완료** (본 plan 작성 시점)

---

## Task T2: Alpine 모듈 추출 헬퍼 (`boot/modules/fetch.ps1`)

**Files:**
- Create: `boot/modules/fetch.ps1`
- Create: `boot/modules/README.md`
- Create: `boot/modules/.gitignore`

### Step 1: 디렉터리 마커 + .gitignore

`boot/modules/.gitignore`:

```gitignore
# 다운로드 산출물 (빌드 시 재생성)
*.apk
*.tar.gz
*/

# README는 추적 (빈 줄로 디렉터리 보호 안 됨)
!README.md
!fetch.ps1
!.gitignore
```

`boot/modules/README.md`:

```markdown
# boot/modules/ — Alpine 커널 모듈 캐시

`fetch.ps1`이 Alpine apk를 다운로드해 필요한 .ko 파일들을 이 디렉터리 아래
`<kernel-version>/`에 저장한다. `boot/build.ps1`이 빌드 시 이 디렉터리를 참조해
initrd의 `/lib/modules/<kernel-version>/`에 복사.

ADR-017 참고.

## 직접 호출

```powershell
pwsh boot/modules/fetch.ps1                  # 현재 vmlinuz와 일치하는 모듈 받기
pwsh boot/modules/fetch.ps1 -RefreshKernel   # vmlinuz도 fresh download
```
```

### Step 2: `boot/modules/fetch.ps1` 본격 구현

스크립트가 처리할 것:

1. **현재 vmlinuz 버전 확인** — vmlinuz 헤더에서 추출 (또는 사용자 인자로 받음)
2. **Alpine 패키지 페이지에서 latest linux-lts 버전 확인** — `https://pkgs.alpinelinux.org/...`
3. **버전 어긋남 시 fresh kernel download** (`-RefreshKernel` 플래그)
4. **`linux-lts-<ver>.apk` 다운로드** (`https://dl-cdn.alpinelinux.org/alpine/v3.21/main/x86_64/linux-lts-<ver>.apk`)
5. **apk = tar.gz** — `.NET Compression` 또는 외부 `tar` 사용해 추출
6. **`/lib/modules/<kernel>/kernel/drivers/net/ethernet/intel/e1000/e1000.ko.gz` 같은 경로에서 .ko 파일 추출**
7. **.gz 풀기** — `System.IO.Compression.GZipStream`
8. **`boot/modules/<kernel-version>/`에 저장**

핵심 PowerShell 함수들 (상세 코드는 구현 시):
- `Resolve-LatestKernelVersion`
- `Download-AlpineApk`
- `Extract-ApkToDirectory`
- `Find-AndCopyModule -Name "e1000"`
- `Decompress-GzFile`

추출 대상 모듈 (M6.5 범위):
- `e1000.ko` (Intel 82540EM, QEMU 기본 NIC)
- (의존이 있으면 함께)

향후 추가:
- `virtio_net.ko` + `virtio_pci.ko` + `virtio.ko` + `virtio_ring.ko` (대체 NIC)
- `virtio_gpu.ko` (Phase D)
- `virtio_input.ko` (Phase D)
- `virtio_blk.ko` (Phase E 영속성)

- [ ] **Step 1: `boot/modules/.gitignore` 생성**
- [ ] **Step 2: `boot/modules/README.md` 생성**
- [ ] **Step 3: `boot/modules/fetch.ps1` 구현**
- [ ] **Step 4: 수동 호출 검증** — `pwsh boot/modules/fetch.ps1` 실행, `boot/modules/<ver>/e1000.ko` 파일 생성 확인 (~ 30~80 KB 예상)
- [ ] **Step 5: 커밋**

---

## Task T3: `boot/build.ps1` 통합

**Files:**
- Modify: `boot/build.ps1`

빌드 흐름 추가:

```
1. Cross-compile (기존)
2. (신규) 모듈 fetch — boot/modules/<kernel>/ 없으면 fetch.ps1 호출
3. (신규) Stage에 /lib/modules/<kernel>/ 디렉터리 만들고 .ko 복사
4. mkinitrd (기존)
5. 커널 체크 (기존)
```

- [ ] **Step 1: `boot/build.ps1`에 module 단계 삽입**
- [ ] **Step 2: 빌드 실행 → `geulos.cpio.gz` 크기가 약 1~2 MB 증가 확인**
- [ ] **Step 3: 커밋**

---

## Task T4: Rust 모듈 로더 구현

**Files:**
- Create: `geulos-init/src/modules.rs`
- Modify: `geulos-init/src/main.rs`

### Step 1: `modules.rs` 작성

```rust
//! 커널 모듈 적재. `finit_module` syscall 직접 호출.
//!
//! ADR-017 참고. Alpine 모듈을 `/lib/modules/<kernel>/`에서 발견해 적재.
//! 의존은 *하드코딩된 순서* — modules.dep 파싱은 후속.

use std::os::fd::AsRawFd;
use std::path::Path;
use nix::libc;
use nix::fcntl::{open, OFlag};
use nix::sys::stat::Mode;

/// finit_module syscall 번호 (x86_64 Linux).
const SYS_FINIT_MODULE: libc::c_long = 313;

/// `finit_module(fd, args, flags)` 래퍼.
pub fn finit_module(path: &Path) -> Result<(), String> {
    let fd = open(path, OFlag::O_RDONLY | OFlag::O_CLOEXEC, Mode::empty())
        .map_err(|e| format!("open {}: {}", path.display(), e))?;

    let args = b"\0";  // 빈 module params
    let res = unsafe {
        libc::syscall(SYS_FINIT_MODULE, fd.as_raw_fd(), args.as_ptr(), 0)
    };
    if res < 0 {
        return Err(format!(
            "finit_module {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// `/lib/modules/<kernel>/` 디렉터리에서 *모든 .ko*를 *하드코딩된 순서*로 적재.
///
/// 순서 정책 (M6.5 단순화):
/// 1. virtio_ring, virtio, virtio_pci (의존 그래프 하단)
/// 2. e1000, virtio_net, virtio_blk (NIC/디스크)
/// 3. 기타 .ko (알파벳 순)
///
/// 이미 적재된 모듈은 finit_module이 -EEXIST 반환 → 무시.
pub fn load_all() -> Result<(), String> {
    let mod_root = Path::new("/lib/modules");
    if !mod_root.is_dir() {
        println!("[init] no /lib/modules — skipping module load");
        return Ok(());
    }

    // 첫 서브디렉터리 = 우리가 빌드한 커널 버전
    let kernel_dir = std::fs::read_dir(mod_root)
        .map_err(|e| format!("read /lib/modules: {}", e))?
        .filter_map(|e| e.ok())
        .find(|e| e.path().is_dir())
        .ok_or("no kernel version directory under /lib/modules")?
        .path();

    println!("[init] loading modules from {}", kernel_dir.display());

    // 의존 순서 — 단순화된 하드코딩
    let priority = ["virtio_ring.ko", "virtio.ko", "virtio_pci.ko",
                    "e1000.ko", "virtio_net.ko"];

    let entries: Vec<_> = std::fs::read_dir(&kernel_dir)
        .map_err(|e| format!("read kernel dir: {}", e))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "ko"))
        .collect();

    // 우선순위 먼저
    let mut loaded = std::collections::HashSet::new();
    for name in priority {
        for entry in &entries {
            if entry.file_name().to_string_lossy() == name {
                match finit_module(&entry.path()) {
                    Ok(()) => {
                        println!("[init] loaded {}", name);
                        loaded.insert(name.to_string());
                    }
                    Err(e) => eprintln!("[init] {} load failed: {}", name, e),
                }
            }
        }
    }
    // 나머지
    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if loaded.contains(&name) { continue; }
        match finit_module(&entry.path()) {
            Ok(()) => println!("[init] loaded {}", name),
            Err(e) => eprintln!("[init] {} load failed: {}", name, e),
        }
    }

    Ok(())
}
```

### Step 2: `main.rs`에 modules 단계 삽입

mount 이후, network 이전에 호출. main.rs:

```rust
mod modules;  // 신규

// main():
// 1. mount
// 2. (신규) modules::load_all()
// 3. network
// 4. spawn
```

- [ ] **Step 1: `modules.rs` 작성**
- [ ] **Step 2: `main.rs` 갱신**
- [ ] **Step 3: `cargo build --target x86_64-unknown-linux-musl -p geulos-init` 그린**
- [ ] **Step 4: 커밋**

---

## Task T5: 부팅 + 외부 ai-bridge acceptance

- [ ] **Step 1: 전체 빌드** — `pwsh boot/build.ps1 -Release`
- [ ] **Step 2: 부팅** — `pwsh boot/qemu/launch.ps1`
- [ ] **Step 3: 콘솔에서 다음 확인:**
  ```
  [init] loading modules from /lib/modules/<ver>
  [init] loaded e1000.ko
  [init] interfaces seen: ["enp0s3", "lo"]    또는 ["eth0", "lo"]
  [init] enp0s3 UP (10.0.2.15/24)
  ```
- [ ] **Step 4: 별 PowerShell에서 외부 ai-bridge 실행**
  ```powershell
  cargo run -p geulos-ai-bridge -- run --scenario ai-bridge/scenarios/01_explore.toml
  ```
- [ ] **Step 5: ai-bridge가 echo-app의 3개 객체(container/text/button) 발견하고 report_done 통과 확인**
- [ ] **Step 6: 커밋**

---

## Task T6: 정리

**Files:**
- Modify: `docs/known-issues.md`
- Modify: `README.md`

- [ ] **Step 1: `docs/known-issues.md`에 KI-012 추가:**
  - "Alpine 커널이 모든 NIC 드라이버를 모듈로 빌드 — M6.5 module loader로 해소"
  - 향후 Phase D에서 virtio-gpu·input 모듈 동일 메커니즘으로 추가 필요 명시

- [ ] **Step 2: `README.md` 마일스톤 표 갱신:**
  - M5 ✅
  - M6 ✅ (외부 ai-bridge 통신 포함)
  - M6.5 ✅ (커널 모듈 로더)

- [ ] **Step 3: 커밋 + push (controller가 일괄)**

---

## 자체 점검

**스펙 커버리지:**
- ADR-005 (AI 모든 토폴로지) — 외부 네트워크 작동으로 T1·T2·T4 지원 ✓
- ADR-016 (최소 init) — modules 단계가 mount/network/spawn과 동급으로 등록 ✓
- ADR-017 (본 plan과 동시) — 핵심 기술적 결정 영구 기록 ✓

**플레이스홀더 스캔:** TBD/TODO 없음. 의존 그래프 자동화는 후속 M으로 명시 분리.

**위험 인정:**
- Alpine CDN 가용성 의존 — 캐시로 완화
- 커널·모듈 버전 락 — fetch.ps1이 둘 다 처리해 자동 일치
- 사용자가 다른 배포판으로 옮길 시 — 본 plan은 Alpine 특화, ADR-017이 그 경계 명시
