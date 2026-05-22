# ADR-034 — 파일·폴더 시각 구분 아이콘 (Lucide MIT 16x16 PNG)

**Status:** Accepted (2026-05-22)

## Context

M8 part 2 (T8.13~T8.20, ADR-033) 마감 후 도그푸딩에서 사용자 직접 보고:

> "어떤게 파일이고 어떤게 폴더인지 아이콘이미지가 없어서 보기불편한점"

현재 좌측 FileTree와 우측 Explorer는 모든 노드를 *동일한 텍스트 한 줄*로 표시한다. Folder는 `[+] 이름` / `[-] 이름` 같은 prefix로만 구분되고, File은 `  이름` (선행 공백 2개)으로 표시 — *시각적 무게*가 거의 없어 한 화면에 수십 개가 나열되면 type을 한눈에 파악 못 한다. 게다가 .md / .rs / .toml / .png 등 *파일 카테고리*도 모두 동일 글꼴·색으로 그려져 *어디부터 무엇인지* 인지 비용이 높다.

M8 part 2가 viewer/스크롤로 *내용 접근*은 풀었지만, *목록 탐색*의 시각적 부하는 미해결로 남았다. M9 (편집·저장·권한 다이얼로그) 전에 *별 task*로 해소하기로 결정 — UX 부담이 가장 적은 기능 한 가지에 집중.

세 갈래 결정 필요:

1. **아이콘 표현 매체:** vector glyph (폰트) vs raster (PNG) vs 빌드 시 SVG → raster
2. **자산 출처:** 직접 디자인 vs 오픈소스 라이브러리 (Lucide / Feather / Phosphor)
3. **타입 결정 로직:** mime 일임 vs 확장자 기반 vs 화이트리스트 혼합

각 갈래의 결정 근거:

### 1. 16x16 raster PNG (vector glyph X, 런타임 SVG 변환 X)

- *Unicode glyph (폰트 내장)*는 흑백 단색 + 폰트 codepoint 의존 + `.notdef` 위험 (Noto Sans KR에 폴더·파일 글리프 없음) → 즉시 글자 깨짐.
- *런타임 SVG → raster* (resvg crate)는 빌드 의존성 폭증 + 시작 시 디코드 비용 + 렌더 시점 캐시 관리 복잡. v1엔 과한 인프라.
- *PNG raster 16x16 정적 임베드*는 시작 시 1회 decode → ARGB u32 [256] 캐시 후 매 프레임 메모리 카피만. softbuffer 픽셀 버퍼와 같은 ARGB 형식이라 alpha blend가 자연스럽다.
- 16x16 고정 — DPI 스케일/사용자 줌은 v2로 위임. 현 컴포지터의 행 높이(24px)와 잘 맞고, 양옆 4~6px padding이면 산뜻하다.

### 2. Lucide MIT (ISC) — 직접 디자인 X, Feather/Phosphor 대신

- *직접 디자인*은 v1에 시간 낭비 + 디자인 품질 낮음 (개발자의 그림 실력으로 16x16 picto 만들기는 *프로토타입 티 강하게* 남는다).
- Lucide는 *Feather의 적극 유지되는 fork*로 1000+ 아이콘 라이브러리. ISC 라이선스 — 사실상 MIT 동등으로 임베드/재배포 자유. 동일 stroke 두께·viewBox 24x24 일관성으로 16x16 raster 변환 시에도 디자인 호환.
- Phosphor도 후보였으나 *세 가지 weight* 중 선택해야 하는 추가 결정이 v1엔 군더더기. Lucide는 *단일 weight* — 결정 줄임.

### 3. type-aware 라우팅 (mime + 확장자 + dotfile 화이트리스트)

- *mime 단일*은 부족 — T8.19의 `guess_mime` 휴리스틱이 `application/octet-stream` fallback이 빈번하고, `.toml` `.yaml` 같은 *설정* 카테고리를 mime으로는 분리 못 함.
- *확장자 단일*도 부족 — dotfile (`.env`/`.gitignore`)은 확장자가 없거나 *오인*되기 쉽다.
- 따라서 *3단 캐스케이드*: ① Folder (type_uri 기준) → ② dotfile 화이트리스트 (T8.19와 동일 셋) → ③ mime `text/markdown` 특화 → ④ 확장자 (rs/py/toml/png/zip 등) → ⑤ mime `text/*` → ⑥ generic. 첫 매치 우선.

## Decision

`compositor/icons/`에 16x16 RGBA PNG 9종을 정적 임베드한다. Lucide MIT/ISC를 출처로 cairosvg 변환으로 생성.

### 9종 매핑 (spec §4)

| IconKind | 자산 | Lucide source SVG | 트리거 |
|---|---|---|---|
| FolderClosed | folder-closed.png | folder | type_uri = aios.std/Folder@1 + expanded=false |
| FolderOpen | folder-open.png | folder-open | type_uri = aios.std/Folder@1 + expanded=true |
| Markdown | markdown.png | file-text | mime = text/markdown |
| Code | code.png | code | 확장자 rs/py/js/ts/html/htm/css |
| Config | config.png | settings | 확장자 toml/yaml/yml/json |
| Text | text.png | file-text (재사용) | mime = text/* (markdown 제외) |
| Image | image.png | image | 확장자 png/jpg/jpeg/gif/svg/webp/bmp |
| Archive | archive.png | package | 확장자 zip/tar/gz/7z/rar/bz2/xz |
| Dotfile | dotfile.png | key-round | name ∈ {.env, .envrc, .gitignore, .gitattributes, .dockerignore, .editorconfig, .prettierrc, .eslintrc} |
| Generic | generic.png | file | 그 외 (fallback) |

총 9개 자산 (Markdown/Text가 같은 Lucide source `file-text`를 재사용하지만 *별 파일*로 보관 — v2에서 Markdown만 색 차별 가능). `Generic`은 9종 매핑 외 fallback이라 자산 수에 포함 — 결과적으로 10 파일이나 *카테고리는 9*.

### 코드 경로

- `compositor/src/icons.rs` (T-icon.2) — `IconKind` enum + `icon_for_file(type_uri, name, mime, is_expanded) -> IconKind` 라우팅 + `OnceLock<IconCache>` 시작 시 1회 decode + `blit_icon_at` softbuffer ARGB 버퍼에 src-over alpha blend.
- `compositor/src/render.rs` (T-icon.3) — Folder/File 분기에 `blit_icon_at` 호출. layout rect 변경 없음 — *텍스트 시작 x만 shift*.

### Cargo 의존

`image = { version = "0.25", default-features = false, features = ["png"] }` — JPEG/GIF/TIFF/BMP/WebP 등 미활성, *PNG decoder만* 컴파일. binary size 영향 약 200KB 이하 추정.

### 자산 생성 파이프라인 (T-icon.1)

1. Lucide GitHub raw에서 9개 SVG 다운로드 (`raw.githubusercontent.com/lucide-icons/lucide/main/icons/`).
2. `stroke="currentColor"` → `stroke="#1f2937"` (slate-800) 치환. stroke-width 2 → 2.5로 살짝 두껍게 — 16x16 raster에서 선이 흐려지지 않도록.
3. cairosvg로 `output_width=output_height=16` PNG 변환. Pillow로 RGBA 보정 + 16x16 검증 + `optimize=True` 저장.
4. 결과 9 PNG (300~650 bytes 각) + `compositor/icons/LICENSE-LUCIDE` (Lucide repo의 ISC LICENSE 원본 사본).

## Alternatives rejected

- **Unicode glyph (폰트 내장)** — Noto Sans KR에 폴더·파일 codepoint 없음 → `.notdef` 글자 깨짐. 별 아이콘 폰트 (Font Awesome 등)를 추가 임베드하면 *폰트 파일 한 개 더* + 글리프 색이 폰트 색에 종속 (다중 색 X). Lucide 아이콘이 v2에서 *색 차별* 여지를 남기는 게 raster의 장점이라 폰트 채택 X.
- **직접 raster (fill_rect const)** — 컴포지터 코드에 16x16 픽셀 array를 hex 상수로 박아 넣기. 디자인 품질 낮음 (개발자가 그림으로 *프로토타입 느낌*만 줌). 사용자가 "보기 불편" 보고를 *시각 품질*까지 묶어 한 것이므로 미달.
- **빌드 시 SVG → PNG (resvg crate)** — 빌드 시점 의존성 + build.rs 추가 + cargo cache 무효화 빈번 + cross-compile 시 cairo/freetype 등 native lib 부담. 자산 *수급은 빌드 시점*, *디코드는 런타임* 형태가 단순 — *완성된 PNG를 commit*하면 빌드 측에 부담 0.
- **vector resize (런타임 SVG 디코드 후 임의 크기)** — 16x16 고정 v1로 충분. DPI 스케일 / 사용자 줌은 *전체 UI*가 픽셀 단위에 묶여 있어 (Window 좌표·폰트 24px 등) 아이콘만 vector로 빼 봤자 의미 없음. v2에서 *모든 UI*가 vector 화 되는 시점에 같이 검토.
- **Phosphor / Feather** — Phosphor는 *3 weight* 중 선택 추가 결정 비용. Feather는 *유지보수 정체* (Lucide가 fork된 이유). Lucide가 *적극 유지 + 단일 weight + ISC*로 최소 결정.

## Consequences

- **사용자 가치:** 한 화면에 수십 개 노드가 나열돼도 *type을 한눈에 파악*. dotfile / 설정 / 코드 / 이미지가 *시각적으로 분리* — 도그푸딩 보고 직접 해소.
- **binary size:** image crate (PNG only) + 9 PNG 자산 = ~200KB 이하 증가 추정. 9 PNG 합계 약 4.8KB (압축 후), image crate 자체가 대부분. 솔로 dogfooding 범위에선 무해.
- **시작 시간:** `OnceLock<IconCache>::get_or_init`로 *첫 호출 시 1회 decode* — 약 9 × `image::load_from_memory(16×16)` ≈ < 5ms. 첫 frame에서만 발생, 이후 array lookup.
- **자산 영구 보존:** `compositor/icons/*.png` 9개 + LICENSE-LUCIDE는 *commit 자산*. Lucide upstream이 사라지더라도 빌드는 영향 X. upstream upgrade는 *명시적 task*로 별도 진행 (style drift 방지).
- **v2 영역:** 다크 모드 (현 stroke = slate-800 고정), 사용자 커스텀 (홈 디렉터리 override), 20x20 / 32x32 크기, Window title bar 아이콘, *mime → IconKind* 사용자 매핑 (예: `application/x-yaml` 추가). v1 범위 의도적 제한.
- **테스트 영역:** `icon_for_file` 라우팅은 *순수 함수* → 단위 테스트 다수 (T-icon.2). PNG decode는 `decode_all_icons_succeeds` 한 건으로 모든 자산이 *fallback 0-array가 아닌지* 검증. *시각 픽셀 정확도*는 컴포지터 acceptance(T-icon.4) 수동 검증 — 컴포지터 단위 테스트의 한계는 본 마일스톤에도 유지.

## 참고

- 관련 ADR: ADR-026 (Window 객체 모델 — File mime 정보가 본 라우팅의 입력), ADR-027 (M8 read-only — viewer-only인 v1 정신과 일관), ADR-033 (M8 part 2 마감 후 도그푸딩 보고가 본 ADR의 출발점)
- 관련 T8.19: dotfile 화이트리스트 (`lazy_mount::guess_mime`) — 본 라우팅의 dotfile 셋이 동일.
- 관련 spec: `docs/specs/2026-05-20-geulos-icons.md`
- 관련 plan: `docs/plans/2026-05-22-geulos-icons.md` T-icon.1 ~ T-icon.5
- 외부 출처: Lucide (https://lucide.dev) — ISC License, sources at `lucide-icons/lucide` GitHub. 본 ADR이 채택한 9개 source SVG는 folder / folder-open / file-text / code / settings / image / package / key-round / file.
