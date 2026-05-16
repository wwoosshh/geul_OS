# GeulOS — Toolchain Bump (Rust 1.78 → 1.85+) 실행 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to execute task-by-task.

**Goal:** Rust 토큰체인을 edition2024 지원 버전(1.85 이상)으로 올려, M1.5/M2/M3에서 누적된 *세 번의 의존성 핀 우회*를 정리하고, hand-rolled TOML parser를 표준 `toml` 크레이트로 교체. M4(컴포지터 + wgpu) 진입 전 인프라 부담 제거.

**Architecture:** 인프라성 변경. 코드 동작 변화 없음 (기능 동일). 다음 4개의 우회/제약 모두 해소:
- M1.5: `uuid` 1.11.0 강제 핀
- M2: `proptest` 1.4.0 강제 핀
- M3: hand-rolled TOML parser (`core/src/object/manifest.rs`)
- 일반: 빅3 OS도 Rust 신버전으로 이주 중인데 우리만 18개월 묵힌 1.78에 갇혀 있는 상황

**Tech Stack:** 기존 그대로. 단지 toolchain 버전과 일부 dep 버전이 바뀜.

**Selection criteria (완료 조건):**
- `rust-toolchain.toml`의 `channel`이 1.85 이상
- `Cargo.lock`에서 uuid/proptest가 *최신 안정 버전*으로 자유롭게 해석되어도 빌드 성공
- `core/src/object/manifest.rs`가 표준 `toml` 크레이트로 다시 작성됨 (또는 `toml_edit`)
- 157개 일반 테스트 + 1개 ignored acceptance 모두 그린
- CI 그린

---

## Task 1: 토큰체인 + 워크스페이스 rust-version 업데이트

**Files:**
- Modify: `rust-toolchain.toml`
- Modify: `Cargo.toml` (workspace.package의 rust-version)
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/adr/002-rust-skeleton.md` (선택 — 토큰체인 버전 기록 갱신)

- [ ] **Step 1: 현재 환경에서 사용 가능한 stable 확인**

Run: `rustup install stable && rustup show`
Expected: 최신 stable이 표시됨 (예상: 1.85 ~ 1.88 사이).

- [ ] **Step 2: `rust-toolchain.toml` 업데이트**

기존:
```toml
[toolchain]
channel = "1.78"
components = ["rustfmt", "clippy"]
```

다음으로 교체 (1.85 이상의 *현재 안정 채널* — Step 1에서 확인한 버전 사용):

```toml
[toolchain]
channel = "1.85"
components = ["rustfmt", "clippy"]
```

(`channel = "stable"`도 옵션이지만, 재현성을 위해 명시적 버전 추천. Step 1에서 본 최신 stable이 1.88이라면 1.88 사용.)

- [ ] **Step 3: 루트 `Cargo.toml`의 `[workspace.package]` 업데이트**

```toml
rust-version = "1.85"
```

(또는 Step 2에서 선택한 버전과 일치.)

- [ ] **Step 4: `.github/workflows/ci.yml` 업데이트**

기존:
```yaml
- name: Install Rust toolchain
  uses: dtolnay/rust-toolchain@stable
  with:
    components: clippy, rustfmt
```

다음으로 교체:
```yaml
- name: Install Rust toolchain
  uses: dtolnay/rust-toolchain@master
  with:
    toolchain: "1.85"
    components: clippy, rustfmt
```

(M0 quality reviewer가 "CI 토큰체인 명시 핀 권고"로 남긴 백로그도 함께 해결.)

- [ ] **Step 5: 빌드 sanity**

```bash
cargo check --workspace
```
Expected: 새 toolchain으로 컴파일 성공. *clippy 신규 lint*나 deprecated 경고가 뜰 수 있음 — 이건 Task 4에서 처리.

- [ ] **Step 6: 커밋 (의도적으로 단순한 변경만 — 후속 정리는 별 커밋)**

```bash
git add rust-toolchain.toml Cargo.toml .github/
git commit -m "build: Rust 토큰체인 1.78 → 1.85 + CI 명시 핀"
```

---

## Task 2: 의존성 핀 해제 + cargo update

**Files:**
- Modify: `Cargo.lock` (cargo update로 자동 갱신)

- [ ] **Step 1: 현재 핀 상태 확인**

```bash
cargo tree --workspace | grep -E "uuid|proptest"
```
Expected: uuid 1.x.x (M1.5에서 1.11.0으로 핀했던 것), proptest 1.4.0.

- [ ] **Step 2: 모든 의존성 업데이트**

```bash
cargo update
```
Expected: uuid이 최신 1.x로, proptest가 최신 1.x로, 다른 dep들도 최신으로 해석. edition2024가 들어와도 OK.

- [ ] **Step 3: 전체 빌드 확인**

```bash
cargo build --workspace --all-targets
```
Expected: 성공. 새 clippy 경고가 있어도 빌드는 통과해야 함 (clippy는 다음 태스크에서).

- [ ] **Step 4: 전체 테스트 (일반)**

```bash
cargo test --workspace --all-targets
```
Expected: 157개 그린. 만약 깨지는 게 있다면 새 dep 버전의 API 차이일 가능성. 보고만 하고 fix는 별 커밋.

- [ ] **Step 5: 커밋**

```bash
git add Cargo.lock
git commit -m "build: Cargo.lock 갱신 — 모든 의존성 최신 stable"
```

---

## Task 3: hand-rolled TOML parser → 표준 `toml` 크레이트

`core/src/object/manifest.rs`는 M3에서 toml 0.8이 edition2024 필요로 안 깔리는 문제를 우회하기 위해 손수 짠 미니 parser를 쓰고 있음. Rust 1.85+가 되면 그럴 필요 없음.

**Files:**
- Modify: `core/Cargo.toml` (toml dep 유지 — workspace.dependencies에 이미 있음)
- Modify: `core/src/object/manifest.rs`

- [ ] **Step 1: 기존 hand-rolled parser 코드 확인**

```bash
cat C:/AiOS/core/src/object/manifest.rs
```
hand-rolled 부분의 *대략적인 길이와 형식*을 파악.

- [ ] **Step 2: 표준 toml 크레이트로 교체**

`core/src/object/manifest.rs`의 `from_toml` / `to_toml`을 다음 형태로 변경:

```rust
//! 앱 매니페스트 (`aios.toml`).

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::identity::TypeUri;

/// 매니페스트 raw 표현 (toml에서 deserialize).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestRaw {
    id: String,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    ui_types: Vec<String>,
}

/// 앱 매니페스트.
#[derive(Debug, Clone, PartialEq)]
pub struct AppManifest {
    pub id: String,
    pub permissions: Vec<String>,
    pub ui_types: Vec<TypeUri>,
}

/// 매니페스트 파싱 오류.
#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("TOML parse error: {0}")]
    Toml(String),
    #[error("bad TypeUri: {0}")]
    BadTypeUri(String),
}

impl AppManifest {
    pub fn from_toml(s: &str) -> Result<Self, ManifestError> {
        let raw: ManifestRaw =
            toml::from_str(s).map_err(|e| ManifestError::Toml(e.to_string()))?;
        let mut ui_types = Vec::new();
        for t in raw.ui_types {
            let parsed =
                TypeUri::parse(&t).map_err(|_| ManifestError::BadTypeUri(t.clone()))?;
            ui_types.push(parsed);
        }
        Ok(Self {
            id: raw.id,
            permissions: raw.permissions,
            ui_types,
        })
    }

    pub fn to_toml(&self) -> Result<String, ManifestError> {
        let raw = ManifestRaw {
            id: self.id.clone(),
            permissions: self.permissions.clone(),
            ui_types: self.ui_types.iter().map(|t| t.as_str().to_string()).collect(),
        };
        toml::to_string(&raw).map_err(|e| ManifestError::Toml(e.to_string()))
    }

    pub fn declares_type(&self, type_uri: &TypeUri) -> bool {
        self.ui_types.iter().any(|t| t == type_uri)
    }
}
```

- [ ] **Step 3: manifest 테스트 6개 통과 확인**

```bash
cargo test -p geulos-core --test manifest_test
```
Expected: 6개 모두 PASS — 표준 toml 크레이트로 round-trip이 더 깔끔하게 작동해야 함.

- [ ] **Step 4: 전체 테스트**

```bash
cargo test --workspace
```
Expected: 157개 그대로 그린.

- [ ] **Step 5: 커밋**

```bash
git add core/
git commit -m "refactor(core): hand-rolled TOML parser → 표준 toml 크레이트"
```

---

## Task 4: 신규 clippy 경고 처리

Rust 1.85+의 clippy는 1.78 대비 새 lint이 추가되어 있을 수 있음. `-D warnings`로 빌드를 깨는 경고가 나오면 *최소한의 변경*으로 fix.

**Files:**
- 수정 위치는 clippy 출력에 따라 가변

- [ ] **Step 1: 전체 clippy 실행**

```bash
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tee clippy.log
```
Expected: 그린이거나, 새 경고 목록이 나옴.

- [ ] **Step 2: 각 경고 처리**

*"if 경고가 0개라면 이 태스크는 즉시 건너뛰고 Task 5로 이동."*

가능한 신규 경고 종류와 처리 방침:

| 경고 종류 | 처리 |
|---|---|
| `needless_borrow` | 참조 제거 |
| `clone_on_copy` | clone() 제거 |
| `useless_conversion` | `into()` 제거 |
| `redundant_pattern_matching` | `matches!` 또는 `if let` 사용 |
| `derivable_impls` | `#[derive(Default)]` 사용 |
| `unnecessary_lazy_evaluations` | `or` / `unwrap_or` 사용 |

**원칙:** 코드 *의미*는 절대 바꾸지 않음. 경고는 스타일 권고이므로 가장 작은 변경으로 처리.

- [ ] **Step 3: 다시 clippy + fmt**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```
Expected: 둘 다 그린.

- [ ] **Step 4: 커밋 (변경이 있을 때만)**

```bash
git add -A
git commit -m "style: Rust 1.85 clippy 신규 lint 대응"
```

(*변경이 없으면 이 태스크의 커밋도 없음. 그래도 OK.*)

---

## Task 5: 최종 스모크 + 푸시

- [ ] **Step 1: 전체 검증**

```bash
cargo build --workspace --all-targets
cargo test --workspace --all-targets
cargo test -p geulos-server-host --test m3_acceptance -- --include-ignored
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```
모두 그린.

- [ ] **Step 2: 푸시 + CI 그린 확인**

```bash
git push origin main
```

GitHub Actions 그린 확인.

- [ ] **Step 3: 완료 선언**

다음 사실이 모두 성립:
- Rust 1.85+ 토큰체인
- uuid/proptest/toml 모든 핀 해제
- 표준 toml 크레이트로 manifest.rs 단순화
- 157개 일반 테스트 + 1개 acceptance 모두 PASS
- CI 그린

M4 (컴포지터) 진입 환경이 깨끗해짐.

---

## 자체 점검 결과

**스펙 커버리지:** 사용자 백로그 3건 모두 해결 (uuid 핀, proptest 핀, hand-rolled toml). M0 quality 백로그 1건도 해결 (CI 명시 핀).

**플레이스홀더 스캔:** TBD/TODO 없음.

**리스크:**
- Rust 1.85 신규 lint이 예상보다 많을 경우 Task 4가 부풀 수 있음. 그래도 코드 의미는 동일하므로 메커니컬 작업.
- 새 의존성 버전의 미묘한 동작 차이 — 테스트가 잡아낼 것으로 기대.
- 이건 *기능 변경이 0인 인프라 작업*. 만약 테스트 하나라도 깨지면 그건 새 toolchain의 버그가 아니라 우리 코드의 toolchain-non-portable 가정.
