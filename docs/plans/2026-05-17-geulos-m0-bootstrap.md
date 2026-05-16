# GeulOS M0 — 부트스트랩 실행 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** GeulOS의 Cargo workspace, 모든 크레이트 스켈레톤, CI 파이프라인, 와이어 프로토콜 스펙 v0.1, ADR 9장, 첫 TDD 사이클 한 개를 갖춘 *컴파일·테스트·린트가 모두 그린인 부트스트랩 상태*를 만든다.

**Architecture:** Rust 단일 Cargo workspace. 크레이트 5개(`core`, `proto`, `compositor`, `glue-ai`, `apps/echo-app`)가 단방향 의존(core ◀── 나머지). 이 milestone은 *실행 가능한 컴포넌트는 만들지 않고* 토대만 깐다. 단 ObjectId 타입을 TDD 사이클 한 번으로 만들어 "이 토대가 실제로 작동한다"는 스모크 테스트를 제공한다.

**Tech Stack:** Rust (stable), Cargo workspace, `uuid` crate, GitHub Actions CI, clippy, rustfmt.

**Selection criteria (완료 조건):**
- `cargo build --workspace` 성공
- `cargo test --workspace` 통과
- `cargo clippy --workspace --all-targets -- -D warnings` 무경고
- CI 그린
- 와이어 프로토콜 스펙 ≥ 1500자
- ADR 9장 작성 완료

---

## 파일 구조 (전체 사전 매핑)

이 마일스톤이 끝나면 다음 구조가 존재한다:

```
geul_OS/
├── Cargo.toml                     # workspace root
├── rust-toolchain.toml            # Rust 버전 고정
├── clippy.toml                    # clippy 설정
├── rustfmt.toml                   # 코드 포맷 설정
├── .gitignore                     # Rust 표준
├── README.md                      # 프로젝트 소개
├── .github/
│   └── workflows/
│       └── ci.yml                 # build / test / clippy
├── core/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs
│   │   └── object.rs              # ObjectId (첫 TDD 산출물)
│   └── tests/
│       └── object_test.rs
├── proto/
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs                 # placeholder
├── compositor/
│   ├── Cargo.toml
│   └── src/
│       └── main.rs                # placeholder
├── glue-ai/
│   ├── Cargo.toml
│   └── src/
│       └── main.rs                # placeholder
├── apps/
│   └── echo-app/
│       ├── Cargo.toml
│       └── src/
│           └── main.rs            # placeholder
└── docs/
    ├── specs/
    │   ├── 2026-05-17-geulos-design.md   # (이미 존재)
    │   └── wire-protocol-v0.1.md          # 신규
    ├── plans/
    │   └── 2026-05-17-geulos-m0-bootstrap.md  # (이 문서)
    └── adr/
        ├── 000-template.md
        ├── 001-linux-kernel-host.md
        ├── 002-rust-skeleton.md
        ├── 003-single-writer-event-loop.md
        ├── 004-geul-only-as-glue.md
        ├── 005-ai-deployment-topology.md
        ├── 006-manifest-based-permissions.md
        ├── 007-wgpu-compositor.md
        ├── 008-geul-bytecode-vm.md
        └── 009-ai-untrusted-default.md
```

---

## Task 1: Cargo workspace 루트 + Rust 버전 고정

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `rustfmt.toml`

- [ ] **Step 1: 루트 `Cargo.toml` 생성**

`Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
    "core",
    "proto",
    "compositor",
    "glue-ai",
    "apps/echo-app",
]

[workspace.package]
edition = "2021"
rust-version = "1.78"
license = "MIT OR Apache-2.0"
repository = "https://github.com/wwoosshh/geul_OS"

[workspace.dependencies]
uuid = { version = "1.8", features = ["v4", "serde"] }
serde = { version = "1.0", features = ["derive"] }
thiserror = "1.0"
```

- [ ] **Step 2: `rust-toolchain.toml` 생성**

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.78"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 3: `rustfmt.toml` 생성**

`rustfmt.toml`:

```toml
edition = "2021"
max_width = 100
use_small_heuristics = "Max"
```

- [ ] **Step 4: workspace 멤버가 아직 없으므로 정의만 검증**

Run: `cargo check --workspace`
Expected: `error: failed to load manifest for workspace member ./core` — 정상. 다음 태스크에서 멤버 크레이트 추가.

- [ ] **Step 5: 커밋**

```bash
git add Cargo.toml rust-toolchain.toml rustfmt.toml
git commit -m "build: Cargo workspace 루트 + Rust 1.78 toolchain 고정"
```

---

## Task 2: `core/` 크레이트 + ObjectId (첫 TDD 사이클)

**Files:**
- Create: `core/Cargo.toml`
- Create: `core/src/lib.rs`
- Create: `core/src/object.rs`
- Create: `core/tests/object_test.rs`

- [ ] **Step 1: `core/Cargo.toml` 생성**

`core/Cargo.toml`:

```toml
[package]
name = "geulos-core"
version = "0.0.1"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "GeulOS core: object server, event bus, permission gate (TCB)"

[dependencies]
uuid.workspace = true
serde.workspace = true
thiserror.workspace = true
```

- [ ] **Step 2: `core/src/lib.rs` 생성 (object 모듈 노출)**

`core/src/lib.rs`:

```rust
//! GeulOS core crate.
//!
//! 이 크레이트는 TCB(Trusted Computing Base)에 해당하는 컴포넌트들을 담는다:
//! 객체 서버, 이벤트 버스, 권한 매니저. 모든 외부 컴포넌트(컴포지터, 앱 런타임,
//! 글 AI I/O 드라이버)는 이 크레이트의 공개 API를 통해서만 코어와 대화한다.

pub mod object;

pub use object::ObjectId;
```

- [ ] **Step 3: 실패하는 테스트 작성 (TDD 첫 단계)**

`core/tests/object_test.rs`:

```rust
use geulos_core::ObjectId;

#[test]
fn object_id_new_returns_unique_ids() {
    let a = ObjectId::new();
    let b = ObjectId::new();
    assert_ne!(a, b, "두 번 호출한 ObjectId::new()가 같으면 안 됨");
}

#[test]
fn object_id_is_displayable() {
    let id = ObjectId::new();
    let s = format!("{}", id);
    assert!(!s.is_empty(), "ObjectId Display는 비어있지 않은 문자열을 내야 함");
}

#[test]
fn object_id_serializes_to_string() {
    let id = ObjectId::new();
    let json = serde_json::to_string(&id).expect("ObjectId는 serde 직렬화 가능해야 함");
    assert!(json.starts_with('"') && json.ends_with('"'));
}
```

`core/Cargo.toml`에 dev-dependency 추가 (Step 1 파일을 수정):

```toml
[dev-dependencies]
serde_json = "1.0"
```

- [ ] **Step 4: 테스트 실행 → 실패 확인**

Run: `cargo test -p geulos-core`
Expected: 컴파일 실패 ("`object` 모듈 비어있음" 또는 "`ObjectId` not found").

- [ ] **Step 5: 최소 구현 작성**

`core/src/object.rs`:

```rust
//! 객체 ID와 객체 타입의 기본 정의.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 시스템 전역에서 유일한 객체 식별자.
///
/// 객체는 한 번 생성되면 ID가 변하지 않는다. 객체가 *소멸*해도 ID는 재사용되지
/// 않는다 — 이벤트 로그의 인과성을 깨지 않기 위함.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectId(Uuid);

impl ObjectId {
    /// 새로운 임의 ObjectId를 발급한다.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ObjectId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
```

- [ ] **Step 6: 테스트 실행 → 통과 확인**

Run: `cargo test -p geulos-core`
Expected: 3개 테스트 모두 PASS.

- [ ] **Step 7: 커밋**

```bash
git add core/
git commit -m "feat(core): ObjectId 타입 + 첫 TDD 사이클 (uuid v4 기반)"
```

---

## Task 3: `proto/` 크레이트 스켈레톤

**Files:**
- Create: `proto/Cargo.toml`
- Create: `proto/src/lib.rs`

- [ ] **Step 1: `proto/Cargo.toml` 생성**

`proto/Cargo.toml`:

```toml
[package]
name = "geulos-proto"
version = "0.0.1"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "GeulOS wire protocol types (Hello/Mount/Invoke/Subscribe/Query/Event/Glscript)"

[dependencies]
geulos-core = { path = "../core" }
serde.workspace = true
```

- [ ] **Step 2: `proto/src/lib.rs` 생성 (placeholder)**

`proto/src/lib.rs`:

```rust
//! GeulOS 와이어 프로토콜 타입.
//!
//! 이 크레이트의 책임: AI 클라이언트, 앱, 컴포지터가 객체 서버와 주고받는
//! 메시지의 타입을 정의한다. 메시지 종류 7개:
//! Hello, Mount, Invoke, Subscribe, Query, Event, Glscript.
//!
//! 본 구현은 M2 마일스톤에서 작성된다. M0에서는 크레이트 자리만 잡는다.

// 의도적으로 placeholder. M2에서 실제 타입을 추가한다.
```

- [ ] **Step 3: 빌드 확인**

Run: `cargo build -p geulos-proto`
Expected: 빌드 성공.

- [ ] **Step 4: 커밋**

```bash
git add proto/
git commit -m "build(proto): placeholder 크레이트 추가 (M2에서 채울 예정)"
```

---

## Task 4: `compositor/` 바이너리 스켈레톤

**Files:**
- Create: `compositor/Cargo.toml`
- Create: `compositor/src/main.rs`

- [ ] **Step 1: `compositor/Cargo.toml` 생성**

`compositor/Cargo.toml`:

```toml
[package]
name = "geulos-compositor"
version = "0.0.1"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "GeulOS user-facing GUI compositor (M4)"

[[bin]]
name = "geulos-compositor"
path = "src/main.rs"

[dependencies]
geulos-core = { path = "../core" }
```

- [ ] **Step 2: `compositor/src/main.rs` 생성 (placeholder)**

`compositor/src/main.rs`:

```rust
//! GeulOS 컴포지터: 객체 트리를 사용자 모니터에 그리는 별 프로세스.
//!
//! 본 구현은 M4 마일스톤에서 작성된다. M0에서는 바이너리 자리만 잡는다.

fn main() {
    println!("geulos-compositor placeholder (M4에서 구현 예정)");
}
```

- [ ] **Step 3: 빌드·실행 확인**

Run: `cargo run -p geulos-compositor`
Expected: `geulos-compositor placeholder (M4에서 구현 예정)` 출력.

- [ ] **Step 4: 커밋**

```bash
git add compositor/
git commit -m "build(compositor): placeholder 바이너리 추가 (M4에서 채울 예정)"
```

---

## Task 5: `glue-ai/` 바이너리 스켈레톤

**Files:**
- Create: `glue-ai/Cargo.toml`
- Create: `glue-ai/src/main.rs`

- [ ] **Step 1: `glue-ai/Cargo.toml` 생성**

`glue-ai/Cargo.toml`:

```toml
[package]
name = "geulos-glue-ai"
version = "0.0.1"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "GeulOS glue-AI driver: embeds geul bytecode VM, handles AI socket (M5)"

[[bin]]
name = "geulos-glue-ai"
path = "src/main.rs"

[dependencies]
geulos-core = { path = "../core" }
geulos-proto = { path = "../proto" }
```

- [ ] **Step 2: `glue-ai/src/main.rs` 생성 (placeholder)**

`glue-ai/src/main.rs`:

```rust
//! 글 AI I/O 드라이버: AI 클라이언트의 RPC/글스크립트를 받아 객체 서버로 전달.
//!
//! 본 구현은 M5 마일스톤에서 작성된다 (G1~G4 글 측 의존성 필요).

fn main() {
    println!("geulos-glue-ai placeholder (M5에서 구현 예정)");
}
```

- [ ] **Step 3: 빌드·실행 확인**

Run: `cargo run -p geulos-glue-ai`
Expected: placeholder 메시지 출력.

- [ ] **Step 4: 커밋**

```bash
git add glue-ai/
git commit -m "build(glue-ai): placeholder 바이너리 추가 (M5에서 채울 예정)"
```

---

## Task 6: `apps/echo-app/` 데모 앱 스켈레톤

**Files:**
- Create: `apps/echo-app/Cargo.toml`
- Create: `apps/echo-app/src/main.rs`

- [ ] **Step 1: `apps/echo-app/Cargo.toml` 생성**

`apps/echo-app/Cargo.toml`:

```toml
[package]
name = "geulos-echo-app"
version = "0.0.1"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "GeulOS demo app: minimal text + button publisher (M3 milestone driver)"

[[bin]]
name = "geulos-echo-app"
path = "src/main.rs"

[dependencies]
geulos-proto = { path = "../../proto" }
```

- [ ] **Step 2: `apps/echo-app/src/main.rs` 생성 (placeholder)**

`apps/echo-app/src/main.rs`:

```rust
//! echo-app: 가장 간단한 데모 앱. 텍스트와 버튼 1개 게시.
//! 버튼을 누르면 카운터가 +1 되고, 그 변화를 구독자에게 통보한다.
//!
//! 본 구현은 M3 마일스톤(앱 런타임 + 권한 매니저)에서 작성된다.

fn main() {
    println!("geulos-echo-app placeholder (M3에서 구현 예정)");
}
```

- [ ] **Step 3: 빌드·실행 확인**

Run: `cargo run -p geulos-echo-app`
Expected: placeholder 메시지 출력.

- [ ] **Step 4: 커밋**

```bash
git add apps/
git commit -m "build(echo-app): placeholder 데모 앱 추가 (M3에서 채울 예정)"
```

---

## Task 7: clippy 설정

**Files:**
- Create: `clippy.toml`

- [ ] **Step 1: `clippy.toml` 생성**

`clippy.toml`:

```toml
# 인지 복잡도(cognitive complexity)가 25를 넘으면 경고.
# OS 코어는 가독성이 정확성에 직결되므로 함수가 너무 복잡해지면 분리하라는 신호.
cognitive-complexity-threshold = 25

# 너무 많은 인자를 받는 함수는 분리 신호.
too-many-arguments-threshold = 6

# pub fn의 doc 누락 경고.
missing-docs-in-crate-items = true
```

- [ ] **Step 2: clippy 실행하여 워크스페이스 전체가 깨끗한지 확인**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 경고 0. (placeholder 크레이트들의 코드가 단순하므로 통과.)

- [ ] **Step 3: 커밋**

```bash
git add clippy.toml
git commit -m "build: clippy 정책 (cognitive-complexity 25, args 6)"
```

---

## Task 8: `.gitignore`

**Files:**
- Create: `.gitignore`

- [ ] **Step 1: `.gitignore` 생성**

`.gitignore`:

```gitignore
# Rust
/target/
**/*.rs.bk
Cargo.lock.bak

# IDE
.idea/
.vscode/
*.iml

# OS
.DS_Store
Thumbs.db

# Local env
.env
.env.local

# 빌드 산출물
*.exe
*.dll
*.so
*.dylib
*.pdb

# 로그
*.log
logs/

# 임시
*.tmp
*.swp
*~

# CI/dev local
/.coverage/
/coverage.xml
```

- [ ] **Step 2: 커밋**

```bash
git add .gitignore
git commit -m "build: .gitignore (Rust + IDE + OS + 로그)"
```

---

## Task 9: `README.md`

**Files:**
- Create: `README.md`

- [ ] **Step 1: `README.md` 생성**

`README.md`:

```markdown
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
```

- [ ] **Step 2: 커밋**

```bash
git add README.md
git commit -m "docs: README (프로젝트 소개 + 4층 아키텍처 + 크레이트 지도)"
```

---

## Task 10: CI 워크플로 (GitHub Actions)

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: `.github/workflows/ci.yml` 생성**

`.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: "-D warnings"

jobs:
  build-and-test:
    name: Build & Test (Linux)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
          restore-keys: |
            ${{ runner.os }}-cargo-

      - name: Build
        run: cargo build --workspace --all-targets

      - name: Test
        run: cargo test --workspace --all-targets

      - name: Clippy
        run: cargo clippy --workspace --all-targets -- -D warnings

      - name: Format check
        run: cargo fmt --all -- --check
```

- [ ] **Step 2: 로컬에서 동일 명령으로 검증**

Run: `cargo build --workspace --all-targets && cargo test --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: 모두 성공.

- [ ] **Step 3: 커밋**

```bash
git add .github/
git commit -m "ci: GitHub Actions (build + test + clippy + fmt)"
```

- [ ] **Step 4: 푸시 후 CI 그린 확인**

Run: `git push origin main`
다음 작업 전 https://github.com/wwoosshh/geul_OS/actions 에서 CI 그린 확인.

---

## Task 11: ADR 템플릿

**Files:**
- Create: `docs/adr/000-template.md`

- [ ] **Step 1: `docs/adr/000-template.md` 생성**

`docs/adr/000-template.md`:

```markdown
# ADR-NNN: [결정 제목]

- **상태:** Proposed | Accepted | Deprecated | Superseded by ADR-XXX
- **일자:** YYYY-MM-DD
- **결정자:** wwoosshh, [기타]

## 맥락 (Context)

이 결정이 필요해진 배경. 어떤 문제·제약·기회가 있었는지. 가능하면 구체적 사례·인용 포함.

## 결정 (Decision)

선택된 옵션과 그 이유. 다른 옵션을 *왜 선택하지 않았는지*도 포함.

## 결과 (Consequences)

### 긍정적

- 이 결정으로 얻는 것.

### 부정적

- 이 결정으로 잃는 것 / 감수하는 비용.

### 중립적

- 영향은 있으나 좋고 나쁨이 명확하지 않은 효과.

## 대안 검토 (Alternatives Considered)

- **대안 A:** 설명. 왜 선택 안 했는지.
- **대안 B:** 설명. 왜 선택 안 했는지.

## 참고

- 관련 ADR: ...
- 관련 스펙: ...
- 외부 자료: ...
```

- [ ] **Step 2: 커밋**

```bash
git add docs/adr/000-template.md
git commit -m "docs(adr): ADR 템플릿"
```

---

## Task 12: ADR-001 Linux 커널 호스트 채택

**Files:**
- Create: `docs/adr/001-linux-kernel-host.md`

- [ ] **Step 1: `docs/adr/001-linux-kernel-host.md` 생성**

```markdown
# ADR-001: Linux 커널을 호스트로 채택, GeulOS는 PID 1 점유

- **상태:** Accepted
- **일자:** 2026-05-17
- **결정자:** wwoosshh

## 맥락

GeulOS는 진짜 OS여야 하지만, 베어메탈 OS는 드라이버 지옥(GPU·Wi-Fi·블루투스·프린터)으로 99%가 실패하는 지점이다. 그러나 VM 게스트 환경에서는 virtio 한 세트만 지원하면 모든 하이퍼바이저(QEMU/Hyper-V/VirtualBox)에서 동작한다.

선택지:
1. 커널부터 직접 작성 (글 또는 Rust로)
2. Linux 커널 채용 + 전통 사용자 공간 일체 배제
3. Linux + 기존 데스크톱 환경 위에 얹는 응용 (= 그냥 데스크톱 환경)

## 결정

**옵션 2: Linux 커널을 채용하되, PID 1을 GeulOS의 객체 서버가 점유한다.** POSIX 셸·X·Wayland·GNOME 같은 전통 사용자 공간을 일체 올리지 않는다.

## 결과

### 긍정적

- Linux의 모든 드라이버·FS·네트워크 스택을 무료로 사용
- 6~12개월 안에 실사용 가능한 데모 도달 가능
- 사용자 입장에서 부팅 후 보는 환경은 100% GeulOS — *진짜 OS의 정체성 유지*
- 선례 있음: macOS = Darwin 커널 + macOS 유저랜드, ChromeOS = Linux 커널 + Chrome 환경

### 부정적

- Linux ABI의 POSIX 가정이 객체 모델 순도를 일부 흐림
- 장기적으로 Linux 컴포넌트를 글-네이티브로 점진 교체할 경로가 필요 (BSD → Darwin 경로)

### 중립적

- 베어메탈 부팅은 MVP 범위 밖. M7 이후 재결정.

## 대안 검토

- **자체 커널 (옵션 1):** 비전 일치도 가장 높지만 18~36개월 추가 작업. 드라이버 0개 작성 부담. *과한 야망.*
- **데스크톱 환경 (옵션 3):** 사용자가 명시 거부 — "진짜 OS여야 한다"는 요구와 충돌.

## 참고

- 관련 스펙: `docs/specs/2026-05-17-geulos-design.md` §2.1
```

- [ ] **Step 2: 커밋**

```bash
git add docs/adr/001-linux-kernel-host.md
git commit -m "docs(adr): ADR-001 Linux 커널 채용, PID 1 점유"
```

---

## Task 13: ADR-002 Rust로 OS 뼈대 작성

**Files:**
- Create: `docs/adr/002-rust-skeleton.md`

- [ ] **Step 1: `docs/adr/002-rust-skeleton.md` 생성**

```markdown
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
```

- [ ] **Step 2: 커밋**

```bash
git add docs/adr/002-rust-skeleton.md
git commit -m "docs(adr): ADR-002 Rust로 OS 뼈대 작성"
```

---

## Task 14: ADR-003 단일 라이터 이벤트 루프

**Files:**
- Create: `docs/adr/003-single-writer-event-loop.md`

- [ ] **Step 1: `docs/adr/003-single-writer-event-loop.md` 생성**

```markdown
# ADR-003: 단일 라이터(single-writer) 이벤트 루프 채택

- **상태:** Accepted
- **일자:** 2026-05-17
- **결정자:** wwoosshh

## 맥락

객체 트리는 사용자 클릭, AI 호출, 앱의 자체 변경이 *동시에* 일으킬 수 있는 자료구조다. 동시 mutate를 어떻게 조율할지가 "Windows의 불안정한 GUI" 비판이 정확히 가리키는 지점.

선택지:
1. 단일 라이터: 한 스레드가 직렬 처리
2. 다중 라이터 + 락
3. 다중 라이터 + CRDT

## 결정

**단일 라이터 이벤트 루프.** 한 스레드가 이벤트 큐를 직렬로 처리한다. 모든 mutate는 이벤트로 큐잉되어 직렬 적용된다.

## 결과

### 긍정적

- 락 없음, 데이터 레이스 없음
- 추론하기 쉬움 — Node.js · Wayland 컴포지터 · Redis와 동일 모델
- **이벤트 로그 = 시스템 전체 매크로/녹화/리플레이가 자연스럽게 가능**
- **undo/redo가 OS 1급 기능**으로 가능
- **AI에게 "방금 어떤 일이 일어났는가"를 정확히 보고**할 수 있음 (스크린샷 diff 불필요)
- 결정론 (같은 이벤트 로그 → 같은 트리 상태) — 테스트·복구·디버깅 모두 강화

### 부정적

- 코어가 한 CPU 코어에 묶임 — 객체 트리 자체 mutate는 단일 스레드
- 단, 렌더링·네트워크 I/O 같은 부수 작업은 별도 스레드 가능. 실용적 병목은 거의 없음

### 중립적

- 향후 NUMA·매니코어 워크로드에서 *읽기*는 락-프리 스냅샷, *쓰기*만 직렬 식으로 확장 가능 (RCU 패턴)

## 대안 검토

- **다중 라이터 + 락:** 표준이지만 데드락·우선순위 역전·"Windows GUI"식 미정의 동작의 원천.
- **CRDT:** 분산 시스템에서 강력하지만 단일 머신에서는 과한 복잡도. 결정론 약화. 후속 마일스톤(멀티 머신 분산)에서 재검토.

## 참고

- 관련 스펙: `docs/specs/2026-05-17-geulos-design.md` §5.2
- Event Sourcing 패턴: Martin Fowler https://martinfowler.com/eaaDev/EventSourcing.html
```

- [ ] **Step 2: 커밋**

```bash
git add docs/adr/003-single-writer-event-loop.md
git commit -m "docs(adr): ADR-003 단일 라이터 이벤트 루프"
```

---

## Task 15: ADR-004 글 언어는 AI↔OS 글루로만 사용

**Files:**
- Create: `docs/adr/004-geul-only-as-glue.md`

- [ ] **Step 1: `docs/adr/004-geul-only-as-glue.md` 생성**

```markdown
# ADR-004: 글 언어를 OS 뼈대에 사용하지 않고, AI ↔ OS 글루로만 사용

- **상태:** Accepted
- **일자:** 2026-05-17
- **결정자:** wwoosshh

## 맥락

글 언어는 자체호스팅 컴파일러를 갖고 C 대비 1.3~2.8x 성능을 내는 성숙한 *응용 언어*다. 그러나 OS 자체를 작성하기에는 아직 미성숙하다 (포인터 산술 크래시, 백슬래시 이스케이프 버그 등 [`문제점.txt`](https://github.com/wwoosshh/geul-lang/blob/main/문제점.txt) 잔존 항목 다수).

선택지:
1. 글로 모든 것을 작성 (커널 + 코어 + 글루)
2. Rust로 뼈대 + 글로 글루
3. 글을 아예 사용하지 않음

## 결정

**옵션 2: Rust로 뼈대, 글은 AI ↔ OS 자연어 글루 위치에만 배치.**

글 코드는 AI 클라이언트가 보내고, 글 AI I/O 드라이버 안의 임베드 VM이 실행한다. OS 코어와 커널 부근은 글이 닿지 않는다.

## 결과

### 긍정적

- OS 안정성 약속을 지킴 — 글의 잔존 버그가 시스템 코어에 영향 없음
- 글이 자신이 빛나는 영역(자연어 자동화 스크립트)에 집중
- 글 언어의 *성숙 속도*가 OS 진행을 막지 않음 (병렬 트랙)
- LLM이 자연어로 작업을 표현하기 좋은 매체 유지

### 부정적

- "100% 글로 작성된 OS"라는 강력한 정체성을 단기적으로 포기
- 글 측에 추가 작업 필요 (G1~G4: 바이트코드 VM, 호스트 함수 ABI, 안전 모드)

### 중립적

- 장기적으로 글-네이티브 시스템 컴포넌트로 점진 마이그레이션 경로 열려 있음 (M7 이후 재결정 — ADR-001 참조)

## 대안 검토

- **글로 모든 것:** 사용자 본인이 "글은 아직 OS 작성에 미성숙"이라고 판단. 안정성 약속과 충돌.
- **글을 사용하지 않음:** 글 언어의 강점(LLM 친화적 자연어 문법)을 활용 못함. AI ↔ OS 인터페이스가 단순 JSON RPC로 축소되어 사용자 비전과 멀어짐.

## 참고

- 관련 스펙: `docs/specs/2026-05-17-geulos-design.md` §4.1
- 글 프로젝트: https://github.com/wwoosshh/geul-lang
- G 마일스톤: 설계 문서 §9.3
```

- [ ] **Step 2: 커밋**

```bash
git add docs/adr/004-geul-only-as-glue.md
git commit -m "docs(adr): ADR-004 글은 AI↔OS 글루로만 사용"
```

---

## Task 16: ADR-005 AI 배치 토폴로지 (모든 토폴로지 지원)

**Files:**
- Create: `docs/adr/005-ai-deployment-topology.md`

- [ ] **Step 1: `docs/adr/005-ai-deployment-topology.md` 생성**

```markdown
# ADR-005: AI는 GeulOS에 결합되지 않음, 모든 배치 토폴로지 지원

- **상태:** Accepted
- **일자:** 2026-05-17
- **결정자:** wwoosshh

## 맥락

AI 클라이언트가 *어디에* 살 것인지가 OS 디자인에 큰 영향을 준다. 클라우드 API, 호스트의 로컬 LLM 서버(Ollama 등), VM 내부의 로컬 LLM, 브라우저 UI 오케스트레이션 — 사용자는 이 중 어느 것이든 원하는 대로 쓸 수 있어야 한다.

선택지:
1. 클라우드 API 전용
2. 로컬 LLM 전용
3. AI 자체를 OS 안에 번들
4. 다 지원 (와이어 프로토콜만 노출)

## 결정

**옵션 4: GeulOS는 LLM을 번들/내장하지 않는다. 와이어 프로토콜만 노출하고 AI 서버가 어디서 도는지는 사용자 선택.** 토폴로지 네 가지를 명시적으로 지원:

- **T1.** 클라우드 API → 호스트 어댑터 → TCP → GeulOS VM
- **T2.** 호스트의 로컬 LLM 서버(Ollama 등) → 어댑터 → TCP → GeulOS VM
- **T3.** VM 내부의 로컬 LLM 서버 → Unix 소켓 → GeulOS 코어
- **T4.** 브라우저 UI가 Ollama + GeulOS 양쪽에 동시 접속해 오케스트레이션

AI 엔드포인트는 Unix 소켓(`ai.sock`)과 TCP(mTLS 또는 발급 토큰 인증) 양쪽에 동일 프로토콜로 노출된다.

## 결과

### 긍정적

- AI-agnostic — 모델 교체 자유, OS 측 변경 불필요
- 로컬 LLM 우대 — 오프라인·무료·프라이버시 시나리오 1급 지원
- 브라우저 기반 오케스트레이션 도구(Open-WebUI 류)와 자연 통합
- OS 코어가 LLM 자체에 대한 책임을 지지 않음 → TCB 축소

### 부정적

- 어댑터 작성 책임을 사용자/생태계에 떠넘김 (단, 어댑터 자체는 작음)
- TCP 노출 시 인증·암호화 책임 ('M6 마일스톤 작업에 포함)

### 중립적

- 향후 GeulOS 표준 어댑터 컬렉션(Claude/OpenAI/Ollama용)을 OS와 별도 저장소에서 제공 가능

## 대안 검토

- **클라우드 전용:** 오프라인·프라이버시 시나리오 배제. 사용자 명시 거부.
- **로컬 LLM 전용:** 최강 모델 활용 불가. 비현실적 제약.
- **AI 번들:** OS 크기 폭증. 모델 업데이트가 OS 업데이트와 묶임. 모델 교체 자유 박탈.

## 참고

- 관련 스펙: `docs/specs/2026-05-17-geulos-design.md` §3 원칙 5, §4.2
- M6 마일스톤: 설계 문서 §9.2 (AI 엔드포인트 TCP 노출)
```

- [ ] **Step 2: 커밋**

```bash
git add docs/adr/005-ai-deployment-topology.md
git commit -m "docs(adr): ADR-005 AI 배치 토폴로지 (T1~T4 모두 지원)"
```

---

## Task 17: ADR-006 매니페스트 기반 권한

**Files:**
- Create: `docs/adr/006-manifest-based-permissions.md`

- [ ] **Step 1: `docs/adr/006-manifest-based-permissions.md` 생성**

```markdown
# ADR-006: 매니페스트 기반 권한 (Capability + Consent)

- **상태:** Accepted
- **일자:** 2026-05-17
- **결정자:** wwoosshh

## 맥락

GeulOS는 AI가 모든 객체를 조작할 수 있다는 강력한 능력을 부여한다. 동시에 사용자는 *AI가 의도치 않은 객체를 건드리지 않는다*는 보장이 필요하다. 앱과 AI 세션 양쪽에서 권한 모델이 필요.

선택지:
1. Unix 식 사용자/그룹/모드 권한
2. Role-Based Access Control (RBAC)
3. Capability-Based + 사용자 Consent

## 결정

**Capability + Consent 모델.**

```
권한 = (ObjectId 또는 Pattern, MethodName 또는 *, 만료시각, 사용횟수한도)
```

- **앱:** 매니페스트(`aios.toml`)에 카테고리 권한 선언. 설치 시 1회 사용자 동의.
- **AI 세션:** 발급 시 권한 범위가 박힌 토큰. 만료 가능, 회수 가능.
- 사용자 동의 다이얼로그: 1회 / 이 세션 / 영구

## 결과

### 긍정적

- *신생 앱이 자동으로 AI-접근 가능*해지는 핵심 메커니즘 (커스텀 객체 타입을 매니페스트에 선언하면 AI가 즉시 이해)
- 권한이 *명시적 계약*이므로 사후 감사 가능
- AI 세션을 *좁게* 만들 수 있어 LLM 실수 비용 제한
- 사용자가 언제든 회수 가능

### 부정적

- 매니페스트 작성 부담을 앱 개발자가 짐
- 권한 다이얼로그가 과해지면 사용자 피로

### 중립적

- 매니페스트 형식·권한 카테고리는 시간이 지나며 풍부해질 것

## 대안 검토

- **Unix 권한:** 객체 단위 세밀 제어 불가. AI 시대 모델로 부족.
- **RBAC:** 역할 정의의 행정 부담. 개인용 OS에 과함.

## 참고

- 관련 스펙: `docs/specs/2026-05-17-geulos-design.md` §5.7, §7.5
- 시나리오 D (앱 게시): 설계 문서 §6.4
```

- [ ] **Step 2: 커밋**

```bash
git add docs/adr/006-manifest-based-permissions.md
git commit -m "docs(adr): ADR-006 매니페스트 기반 권한 (Capability + Consent)"
```

---

## Task 18: ADR-007 컴포지터 렌더링 (wgpu)

**Files:**
- Create: `docs/adr/007-wgpu-compositor.md`

- [ ] **Step 1: `docs/adr/007-wgpu-compositor.md` 생성**

```markdown
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
```

- [ ] **Step 2: 커밋**

```bash
git add docs/adr/007-wgpu-compositor.md
git commit -m "docs(adr): ADR-007 컴포지터 wgpu 채택 (M4 시점 재검토 잠정)"
```

---

## Task 19: ADR-008 글 바이트코드 VM 모드

**Files:**
- Create: `docs/adr/008-geul-bytecode-vm.md`

- [ ] **Step 1: `docs/adr/008-geul-bytecode-vm.md` 생성**

```markdown
# ADR-008: 글 언어에 바이트코드 VM 모드 추가 (AOT 외)

- **상태:** Accepted
- **일자:** 2026-05-17
- **결정자:** wwoosshh

## 맥락

AI가 보내는 글 코드는 다음 두 가지 패턴이다:
- *단발 RPC*: "이 버튼을 눌러줘" — 글을 거치지 않고 직통
- *다단계 글 스크립트*: 여러 RPC를 묶는 흐름 제어 — 글 코드로 표현

다단계 글 스크립트를 매번 AOT 컴파일하면 첫 호출 latency가 폭증한다 (글 → C → MSVC → exe → spawn). 인터프리트 가능한 백엔드가 필수.

## 결정

**글 컴파일러에 *바이트코드 VM 모드*를 추가한다.** 문법 재설계 없음. 파서·AST 공유, 백엔드만 신규.

```
.글 → 파서 → AST → IR ──┬─→ ir_codegen.gl → C → MSVC
                         ├─→ pe_gen.gl → PE 직접
                         └─→ [신규] bytecode_gen.gl → 바이트코드 VM (Rust 임베드)
```

또한 "안전 모드" 의미 검사기를 두어, 모래상자 컨텍스트에서 포인터 산술·임의 메모리 접근을 *의미 분석 단계*에서 거부한다 (Rust의 `unsafe` 블록과 유사한 패턴).

## 결과

### 긍정적

- 단발 호출 latency 마이크로초 단위 유지 (글 우회)
- 다단계 스크립트도 바이트코드 캐싱으로 반복 호출 이득
- 모래상자에서 안전 보장
- 글 사용자 코드 변경 없음 (문법 동일)

### 부정적

- 글 측에 추가 작업 4건 (G1~G4): AST→바이트코드, VM, 호스트 함수 ABI, 안전 모드
- 백엔드가 둘이 됨 → 의미 차이 발생 가능성 (수동 검증 필요)

### 중립적

- 살아있는 선례 다수: Python(CPython 바이트코드 VM), Lua, Java/Kotlin(JVM), C#(CoreCLR JIT)

## 대안 검토

- **매번 AOT 컴파일 + 캐시:** 첫 호출 latency가 큼. AOT 산출물 spawn 비용도 매번 발생.
- **트리워킹 인터프리터:** 바이트코드 VM보다 단순하지만 2~10x 느림. AI 호출 빈도를 생각하면 바이트코드 VM의 추가 복잡도가 보상됨.
- **JIT:** 최고 성능이지만 구현 부담 매우 큼. 후속 마일스톤에서 재검토.

## 참고

- 관련 스펙: `docs/specs/2026-05-17-geulos-design.md` §5.5, §9.3 (G1~G4)
- 글 프로젝트: https://github.com/wwoosshh/geul-lang
```

- [ ] **Step 2: 커밋**

```bash
git add docs/adr/008-geul-bytecode-vm.md
git commit -m "docs(adr): ADR-008 글 바이트코드 VM 모드 추가"
```

---

## Task 20: ADR-009 AI 기본 불신 + Capability + Consent

**Files:**
- Create: `docs/adr/009-ai-untrusted-default.md`

- [ ] **Step 1: `docs/adr/009-ai-untrusted-default.md` 생성**

```markdown
# ADR-009: AI 클라이언트는 기본 불신 (Capability + Consent로 좁게 권한 부여)

- **상태:** Accepted
- **일자:** 2026-05-17
- **결정자:** wwoosshh

## 맥락

AI가 OS 단에서 시스템을 조작할 수 있다는 능력은 강력한 만큼 위험하다. LLM의 실수, 프롬프트 인젝션, 토큰 유출 등의 위협이 있다.

선택지:
1. 사용자가 신뢰한 AI에게는 사용자 본인의 권한을 그대로 부여
2. AI는 기본 불신, 권한은 명시적 동의로만 부여

## 결정

**옵션 2: AI는 기본 불신.** "AI는 도구이지 주인이 아니다."

- AI 세션은 발급 시 *권한 범위가 박힌* 토큰을 받는다 (예: "오후 6시까지, 메모 앱만, 읽기·쓰기").
- 권한 매니저는 *AI 클라이언트에게 어떤 RPC도 노출하지 않는다* — 권한 변경은 사용자 GUI·설정 파일로만.
- 모든 호출은 권한 게이트를 통과해야 객체 서버에 도달.
- `aios.secret/*` 타입 객체는 AI 세션 그래프에 사용자 명시 허용 없이 등장하지 않음.

## 결과

### 긍정적

- LLM 실수의 비용 제한
- 사용자가 위협을 *명시적으로* 이해할 수 있음 (동의 다이얼로그가 보임)
- AI 토큰 유출 시 피해 범위 제한 (토큰 회수, 범위 한정)
- 사용자가 *언제든 통제권 회수* 가능

### 부정적

- 동의 다이얼로그 피로 가능성 (적당한 기본값 + 영구 동의 옵션으로 완화)
- AI가 사용자 의도를 추론하기 어려운 경우, 다이얼로그가 흐름을 끊을 수 있음

### 중립적

- 향후 ML 기반 *위험 점수*로 자동 승인/거부 옵션 추가 가능 (단, 사용자 옵트인)

## 대안 검토

- **사용자 권한 그대로 부여 (옵션 1):** AI 실수의 비용 폭증. 단 한 번의 잘못된 호출이 전체 시스템 데이터 손실. *AI 시대의 새 OS가 채택할 수 없는 모델.*

## 참고

- 관련 스펙: `docs/specs/2026-05-17-geulos-design.md` §7.1, §7.5, §7.7
- 보안 불변식 S1~S6: 설계 문서 §7.8
```

- [ ] **Step 2: 커밋**

```bash
git add docs/adr/009-ai-untrusted-default.md
git commit -m "docs(adr): ADR-009 AI 기본 불신 + Capability + Consent"
```

---

## Task 21: 와이어 프로토콜 스펙 v0.1

**Files:**
- Create: `docs/specs/wire-protocol-v0.1.md`

- [ ] **Step 1: `docs/specs/wire-protocol-v0.1.md` 생성**

```markdown
# GeulOS 와이어 프로토콜 스펙 v0.1

- **상태:** Draft v0.1
- **일자:** 2026-05-17
- **저자:** wwoosshh, with Claude
- **재결정 시점:** M2 완료 시 v1.0 동결 여부 검토

## 0. 위치

이 스펙은 외부 프로세스(AI 클라이언트, 앱, 컴포지터)와 GeulOS 객체 서버 사이의 *유일한* 메시지 포맷을 정의한다. 본 스펙의 구현은 M2 마일스톤의 산출물.

## 1. 전송 (Transport)

| 클라이언트 | 엔드포인트 | 인증 |
|---|---|---|
| AI (VM 내부) | `/run/aios/ai.sock` (Unix) | 세션 토큰 |
| AI (VM 외부) | TCP `:<port>` (M6에서 결정) | mTLS 또는 세션 토큰 |
| 앱 | `/run/aios/app.sock` (Unix) | 매니페스트 |
| 컴포지터 | (커널 내부 IPC, 단일 신뢰 채널) | 권한 매니저가 직접 발급 |

연결은 양방향 스트림. 프레이밍은 *4바이트 빅엔디언 길이 접두사 + 본문 JSON*. 본문은 UTF-8 인코딩.

향후 v1.0에서 바이너리 포맷(MessagePack 또는 CBOR) 전환 검토 — 지금은 디버깅 용이성을 우선.

## 2. 핸드셰이크

연결 직후 클라이언트가 가장 먼저 `Hello`를 보낸다. 서버는 `HelloAck` 또는 `HelloReject`로 응답.

### Hello (client → server)

```json
{
  "kind": "Hello",
  "version": "0.1",
  "role": "ai" | "app" | "compositor",
  "auth": { "token": "..." } | { "manifest": { ... } },
  "client_id": "<sender 자유 식별자>"
}
```

### HelloAck (server → client)

```json
{
  "kind": "HelloAck",
  "session_id": "<UUID>",
  "actor_id": "<UUID>",
  "server_version": "0.1",
  "capabilities": ["mount", "invoke", "subscribe", "query", "glscript"]
}
```

### HelloReject (server → client)

```json
{
  "kind": "HelloReject",
  "reason": "version_mismatch" | "auth_failed" | "role_unknown" | "...",
  "detail": "사람이 읽을 수 있는 설명"
}
```

## 3. 메시지 종류 (7개)

### 3.1 Mount (app → server)

앱이 자기 객체 서브트리를 객체 서버에 게시.

```json
{
  "kind": "Mount",
  "root_object_id": "<ObjectId, 클라이언트 발급 임시 ID 또는 서버 위임>",
  "tree": {
    "id": "...",
    "type_uri": "aios.std/Window@1",
    "props": { "title": "메모장" },
    "state": {},
    "methods": [...],
    "children": [...]
  }
}
```

응답: `MountAck { server_assigned_ids: {...} }` 또는 `MountReject`.

### 3.2 Invoke (client → server)

객체의 메서드 호출.

```json
{
  "kind": "Invoke",
  "request_id": "<ULID, 클라이언트가 발급, 응답 매칭용>",
  "target": "<ObjectId>",
  "method": "press",
  "args": { ... }
}
```

응답: `InvokeAck { request_id, event_id, result }` 또는 `InvokeError { request_id, kind: "permission" | "no_such_object" | ..., detail }`.

### 3.3 Subscribe (client → server)

객체/서브트리 변화 구독.

```json
{
  "kind": "Subscribe",
  "subscription_id": "<클라이언트 발급 ID>",
  "target": "<ObjectId 또는 패턴>",
  "kinds": ["StateSet", "Lifecycle"],
  "include_initial": true
}
```

응답: `SubscribeAck { subscription_id }`.

### 3.4 Unsubscribe (client → server)

```json
{
  "kind": "Unsubscribe",
  "subscription_id": "<발급 시 받은 ID>"
}
```

### 3.5 Query (client → server)

상태 단발 조회.

```json
{
  "kind": "Query",
  "request_id": "<ULID>",
  "query": {
    "type": "type=Memo",  // 또는 다른 쿼리 형식
    "depth": 2
  }
}
```

응답: `QueryResult { request_id, objects: [...] }`.

### 3.6 Event (server → client)

객체 상태 변화 통보 (Subscribe된 클라이언트에게).

```json
{
  "kind": "Event",
  "subscription_id": "<발급 시 받은 ID>",
  "event": {
    "id": "<EventId, 단조 증가>",
    "actor": "<ActorId>",
    "target": "<ObjectId>",
    "kind": "Invoke" | "StateSet" | "Lifecycle",
    "payload": { ... },
    "causation": "<EventId 또는 null>"
  }
}
```

### 3.7 Glscript (AI → server)

AI가 보낸 글 코드 한 덩어리 실행.

```json
{
  "kind": "Glscript",
  "request_id": "<ULID>",
  "source": "<글 소스 코드 문자열>",
  "budget": {
    "max_opcodes": 100000,
    "max_memory_bytes": 16777216,
    "max_wall_ms": 5000
  }
}
```

응답: `GlscriptResult { request_id, exit_code, events: [...], stdout, stderr }` 또는 `GlscriptError`.

## 4. 액터(ActorId) 모델

| Actor 종류 | ActorId 형식 | 발급 시점 |
|---|---|---|
| 사용자 (콘솔 사용자) | `user:local` | 부팅 시 고정 |
| AI 세션 | `ai:<UUID>` | Hello 시 |
| 앱 | `app:<manifest.id>:<instance UUID>` | Mount 시 |
| 컴포지터 | `system:compositor` | 부팅 시 고정 |

## 5. 권한 검사

모든 `Invoke`와 `Mount` 안의 메서드 정의는 권한 매니저의 ACL을 통과해야 한다. 거부 시 `InvokeError { kind: "permission" }`.

## 6. 호환성

v0.1은 *깨질 수 있는 버전*. M2 완료 시 v1.0으로 동결 검토. 동결 후 메이저 버전은 의미 변경 시에만 증가.

## 7. 미해결 항목 (M2 작업으로 이관)

- 바이너리 포맷 (MessagePack vs CBOR)
- 스트리밍 응답 (긴 `Glscript`의 중간 stdout 흐름)
- 압축
- 멀티플렉싱 (한 연결에 여러 세션)

## 8. 참고

- 설계 문서: `docs/specs/2026-05-17-geulos-design.md` §5.3
```

- [ ] **Step 2: 문자 수 확인 (≥ 1500자)**

Run: `wc -m docs/specs/wire-protocol-v0.1.md` (Windows: `(Get-Content docs/specs/wire-protocol-v0.1.md | Measure-Object -Character).Characters`)
Expected: ≥ 1500.

- [ ] **Step 3: 커밋**

```bash
git add docs/specs/wire-protocol-v0.1.md
git commit -m "docs(spec): 와이어 프로토콜 v0.1 (7 메시지 + 핸드셰이크)"
```

---

## Task 22: M0 최종 스모크 테스트 + 푸시

**Files:** (확인용, 신규 작성 없음)

- [ ] **Step 1: 전체 빌드**

Run: `cargo build --workspace --all-targets`
Expected: 성공, 경고 없음.

- [ ] **Step 2: 전체 테스트**

Run: `cargo test --workspace --all-targets`
Expected: ObjectId 관련 3개 테스트 모두 PASS.

- [ ] **Step 3: clippy 전체**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 경고 0.

- [ ] **Step 4: 포맷 체크**

Run: `cargo fmt --all -- --check`
Expected: 모든 파일 포맷 일치 (차이 없음).

차이가 있으면: `cargo fmt --all` 실행 후 변경분 커밋:

```bash
git add -A
git commit -m "style: cargo fmt 적용"
```

- [ ] **Step 5: 푸시**

Run: `git push origin main`
Expected: 원격 동기화 성공.

- [ ] **Step 6: 원격 CI 그린 확인**

브라우저에서 https://github.com/wwoosshh/geul_OS/actions 를 열어 가장 최근 워크플로우가 그린인지 확인.

CI가 빨강이면, 실패 원인을 확인하고 수정 후 푸시. CI 그린 전까지 M0 미완료.

- [ ] **Step 7: M0 완료 선언**

이 시점에서 다음이 사실이어야 한다:
- 5개 크레이트 모두 빌드 통과
- ObjectId 테스트 PASS
- clippy 경고 0
- 포맷 일치
- ADR 9개 + 와이어 프로토콜 v0.1 + README 작성
- GitHub Actions 그린

축하한다. M1 — 객체 서버 + 이벤트 버스 단계로 진입할 준비가 됐다.

---

## 자체 점검 결과

**스펙 커버리지:**
- 설계 문서 §9.2의 M0 완료 조건 4개 모두 본 plan으로 달성 (cargo build, CI 그린, 스펙 ≥ 1500자, ADR 9개) ✓
- §10.1의 ADR-001~009 본문 모두 포함 ✓
- §5.3의 와이어 프로토콜 메시지 7종 모두 정의 ✓

**플레이스홀더 스캔:** TBD/TODO 없음. "Similar to" 참조 없음. 모든 코드·문서·명령이 인라인 ✓

**타입 일관성:** ObjectId(Task 2)는 후속 모든 ADR과 와이어 프로토콜 스펙(Task 21)에서 일관되게 참조 ✓
