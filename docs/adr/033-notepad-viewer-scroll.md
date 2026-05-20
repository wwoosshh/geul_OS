# ADR-033 — Window 내장 텍스트 viewer + 라인 단위 스크롤 (M8 part 2)

**Status:** Accepted (2026-05-20)

## Context

M8 part 1(ADR-026, 멀티-윈도우 객체 모델)에서 `aios.builtin/Window@1`이 1급 객체로 도입됐다. 컴포지터는 Window 본문에 *File 객체의 `state.preview`* (앞 512바이트) 만 렌더링한다 — 그 이상은 spec/구현 모두 없음. v1 한계로 *의도된* 동작이었지만 도그푸딩 중 사용자가 막힌다:

> "txt외에 md파일을 열수가 없다" / "스크롤 없어서 잘림"

`.md`/`.rs` 등 텍스트 파일이 1KB만 넘어도 본문이 절단되고, Explorer/FileTree도 자식이 화면 높이를 초과하면 아래쪽이 잘려 *접근 불가*. M8의 핵심 가치인 "전체 파일시스템 탐색 + 파일 viewer"가 절반만 작동하는 셈.

세 가지 갈래 결정 필요:

1. **viewer 위치:** Window 내장 vs 별 객체 `aios.std/Notepad@1`
2. **content 전달:** Window가 직접 file 본문 보유 vs File.state.preview 확장 (512 → 1MB)
3. **스크롤 입력 흐름:** invoke (Window.scroll(delta))를 통한 통합 vs 컴포지터 직접 SetState

각 갈래의 결정 근거:

### 1. Window 내장 viewer (별 객체 X)

- 별 `Notepad@1` 객체는 *M9에서 편집·저장·앱 lifecycle*과 함께 도입하는 게 자연스럽다 — viewer-only인 v1에 별 type을 새로 정의하면 M9에서 다시 리네임/재설계해야 함.
- Window의 책임이 *"file 한 건을 시각화"*라면 viewer 로직을 Window 자체에 묶어도 ADR-026의 정체성(파일과 일대일)과 모순 없음.
- 컴포지터의 `render_window`가 이미 file mime 분기를 가질 예정이라, 별 객체로 빼면 한 객체 더 lookup해야 해서 *구현이 늘 뿐*.

### 2. Window가 직접 content 보유

- File 객체는 `aios.std/*` — 사용자/AI 누구나 query 가능. preview를 1MB로 늘리면 *file 일람만 보는* 모든 클라이언트가 매번 1MB씩 받음 (현 preview 512바이트는 fs 스캔 시 채워짐 — File 객체 수 × 1MB는 비현실).
- Window는 *특정 mount 시점에만* 본문이 필요. Window mount 직전에 desktop-shell이 한 번만 file을 read해서 Window.state.content 채우면 됨 — 본문이 Window lifecycle에 묶임.
- 또한 1MB cap이 *클라이언트마다* 다를 수 있는데 (앞으로 image viewer는 다른 cap), Window가 보유하면 type-aware cap이 자연스럽다.

### 3. 컴포지터 직접 SetState (invoke 우회)

- 마우스 휠 / PageDown은 *눌릴 때마다 발생* — invoke 라운드트립(컴포지터 → server → desktop-shell → server broadcast)을 매번 돌리면 latency가 누적된다. 스크롤은 사용자 즉각 반응이 필요.
- ADR-026에서 Window drag move/resize도 *drag end 시점에만 invoke* 하기로 결정했는데, 같은 흐름이 스크롤에도 적용된다 — 컴포지터가 state를 *직접* 갱신해 즉시 그리고, 그 갱신을 SetStateMsg로 server에 push해 AI도 본다.
- v1은 *컴포지터 → server에 SetState 메시지 직송* — server-host의 `state_set` 핸들러를 컴포지터 actor가 호출. desktop-shell의 단일 라이터(ADR-003)는 *Window의 props/메서드 트리거 갱신*에 한정되며, scroll_y는 *UI hint*에 가까워 컴포지터가 라이터인 게 자연스럽다. (M9에 invoke 통합 SetState 재검토 가능 — KI 후보.)

## Decision

세 갈래 결정 종합:

### Schema 변경 (T8.13 본 task)

`std_types::window` 팩토리에 신규 state 3건:

| 키 | 타입 | 기본값 | 의미 |
|---|---|---|---|
| `scroll_y` | `i32` (라인 단위) | `0` | 첫 가시 라인 번호. 컴포지터가 24px 곱해 픽셀 오프셋 계산. |
| `content` | `String` (≤ 1MB UTF-8) | `""` | 파일 본문. Window mount 직전 desktop-shell이 채움. |
| `content_too_large` | `bool` | `false` | 1MB cap에 걸려 잘렸으면 `true`. 컴포지터가 "[일부만 표시]" 안내 렌더. |

`std_types::file_tree` 와 `std_types::explorer` 팩토리에도 동일한 의미의 `scroll_y: i32 = 0` 추가 — *세 영역 공통 스크롤* 메커니즘.

- `scroll_y`는 *라인 단위*. 픽셀 단위는 휠 1 notch당 24px = 1 line 같은 *전환 환산만* 컴포지터에서. AI가 "Window를 3라인 아래로 스크롤" 같은 자연어를 invoke로 표현하기 쉬워짐 (M9+ 일 수도).
- `content`는 *Window라는 mount 단위에만* 본문을 묶음 — File.state.preview는 그대로 512바이트 유지 (전체 query 비용 영향 X).
- `content_too_large`는 *호출자 책임* 필드 — `std_types`는 cap을 강제하지 않고 default `false`만 둠. desktop-shell 측 file_read 헬퍼(T8.14)가 1MB 초과 시 `true`로 set.
- 음수 `scroll_y`는 *컴포지터에서 clamp* — 모델은 negative를 허용해도 렌더가 안전. max clamp는 안 함 (총 라인 수가 자식 mount 후에야 알려짐 — render 시 자연 안 그림).

### Cross-component 책임 분담 (이 ADR이 plan 전체에 걸쳐 정의)

- **core/std_types (T8.13):** schema만. cap 강제 X, validation X.
- **desktop-shell (T8.14):** Window mount 직전 file 본문 read — mime이 `text/*`만 통과, UTF-8 검증, 1MB cap, char boundary 안전 truncate. 실패 시 사용자 안내 메시지를 content에 담음.
- **compositor render (T8.15):** Window.state.content를 라인 단위 split, `scroll_y` 기점부터 가시 라인만 그림. content_too_large=true면 끝에 안내. FileTree/Explorer는 scroll_y * 24px만큼 자식 y를 위로 밀어 layout (T8.16).
- **compositor input (T8.17):** MouseWheel + PageUp/Down → SetStateMsg 직송 (UiAction::SetState 신규 variant).

## Alternatives rejected

- **별 `aios.std/Notepad@1` 객체** — M8 viewer-only 단계에 별 type을 정의하면 M9 편집·저장·앱 lifecycle 도입 시 *재정의/리네임* 필요. M9에서 별 프로세스 메모장 앱 (TextArea/Memo 패턴 재활용)을 도입할 때 함께 다루는 게 자연스러움.
- **File.state.preview 확장 (512바이트 → 1MB)** — 모든 File 객체가 *항상* 1MB content를 보유하면 fs 스캔/탐색 비용이 폭증. preview는 Explorer 미리보기 (현 v1) 용도로 남기고, Window 본문만 별 채널로.
- **invoke 통합 SetState (`Window.scroll(delta)`)** — 매 휠 이벤트마다 invoke 라운드트립 → 가시적 lag. ADR-026의 drag *commit 시점에만 invoke* 패턴과 일관되게, *스크롤도 컴포지터 직접 갱신*. v1 단순화. M9 권한 모델에서 *모든 SetState도 ACL 검사*하는 시점에 재검토.
- **type-aware 별 viewer 객체 (Image@1 / Pdf@1)** — viewer마다 별 객체로 분리하면 M8 part 2 범위를 넘어선다. Window가 mime 분기로 *직접* 렌더(text 분기만 우선)하는 게 v1엔 충분. image/pdf는 후속.

## Consequences

- **사용자 가치:** 텍스트 viewer가 *실제로 작동* — `.md`/`.rs`/`.toml` 등을 *전체* 본문으로 볼 수 있고, FileTree/Explorer에서 화면 밖 자식을 *스크롤로 접근* 가능. 도그푸딩 차단 해소.
- **AI 가시성:** Window.state.content가 *server tree에 있음* — AI가 `get_object`로 *현재 열려있는 윈도우 본문 전체*를 볼 수 있다. 시나리오 풍부화 (예: "지금 본 코드에서 이 함수 설명해줘"). 동시에 *민감 파일을 사용자가 모르고 열면 AI가 즉시 본다* — M9 권한 다이얼로그에서 *Window mount 시점에 사용자 확인* 가능성 검토 필요 (KI 후보).
- **wire 트래픽:** Window mount 한 번당 최대 1MB content가 server-host → 모든 subscriber로 broadcast. Window가 *수십 개*면 누적 비용. M8 솔로 dogfooding 범위에선 무해. M9+에 다중 사용자 / 원격 actor 도입 시 *content를 별 채널 (lazy read endpoint)*로 빼는 방안 검토.
- **스크롤 일관성:** scroll_y를 컴포지터가 *직접 SetState*로 갱신하므로 ADR-003 단일 라이터의 *형식적 위배*. 단 scroll_y는 UI hint(server 비즈니스 로직 영향 X)이고 컴포지터 actor도 server에 인증되어 있어 *책임 라이터*가 명확. M9 ACL 정식화 시 *컴포지터 actor에게 Window.state.scroll_y만 SetState 허용* 룰로 표현 가능.
- **FileTree/Explorer 통일성:** 세 영역 모두 `state.scroll_y` 라는 *같은 이름*과 *같은 의미(라인 단위)*. 컴포지터의 휠 핸들러가 hit-test 결과에 따라 target만 다르고 *처리 로직 동일*.
- **테스트 영역:** schema(core 단위 테스트) + file_read(desktop-shell 단위 테스트) + layout scroll(compositor 단위 테스트)는 자동화. *render 픽셀*과 *마우스 휠 통합*은 acceptance(T8.18) 수동 검증 — 컴포지터 단위 테스트의 한계는 이 마일스톤에도 유지.

## 참고

- 관련 ADR: ADR-026 (멀티-윈도우 객체 모델 — 본 ADR의 base), ADR-027 (M8 read-only — viewer-only는 그 정신과 일관), ADR-020 (셸 빌트인 정책), ADR-003 (단일 라이터 — 부분 완화 명시)
- 관련 known-issues: KI-001 (wildcard ACL — 스크롤 SetState 보호의 ACL 기반은 M9), KI-015 (AI 컨텍스트의 민감 데이터 노출 — Window content 노출 우려가 유사)
- 관련 spec: `docs/specs/2026-05-20-geulos-m8-notepad-viewer-scroll.md`
- 관련 plan: `docs/plans/2026-05-20-geulos-m8-notepad-viewer-scroll.md` T8.13~T8.18
