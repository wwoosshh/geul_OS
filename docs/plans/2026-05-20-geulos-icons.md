> **Status:** superseded (2026-05-22)
> **Note:** 같은 spec의 v2 plan(`2026-05-22-geulos-icons.md`)이 실 구현에 사용 — folder closed/open 변형 + dotfile 등 IconKind 확장됨.

# GeulOS 아이콘 (파일·폴더 시각 구분) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> **NEVER push** — controller가 마일스톤 끝에 batch push. subagent는 commit만.

**Goal:** 좌측 FileTree와 우측 Explorer의 폴더·파일 행에 16x16 PNG raster 아이콘 9종(Lucide MIT)을 blit해서 파일 타입을 한눈에 구분하게 만든다. 편집·저장은 무관(M9), 다크 모드/사용자 커스텀은 v2.

**Architecture:** 컴포지터에 신규 `icons.rs` 모듈 1개 추가 (IconKind enum + 시작 시 1회 디코드 후 캐시되는 9 PNG + `icon_for_file` 라우팅 함수 + `blit_icon_at` alpha-blend 헬퍼). `render.rs`의 `Folder@1`/`File@1` 분기가 텍스트 x를 shift하고 아이콘 픽셀을 blit. 그 외 모든 컴포넌트는 무변경.

**Tech Stack:**
- `image` crate 0.25 (default-features off, features = ["png"]) — PNG 디코드만
- `std::sync::LazyLock` (Rust 1.80+, workspace의 1.95에서 사용 가능) — 시작 시 1회 디코드
- Lucide MIT 아이콘 9종 (체크인, 출처는 LICENSE-LUCIDE에 기록)
- 기존 softbuffer ARGB u32 픽셀 버퍼 + 수동 src-over blend

**Spec parent:** `docs/specs/2026-05-20-geulos-icons.md`

---

## File Structure

| 신규/수정 | 경로 | 책임 |
|---|---|---|
| Create | `docs/adr/034-icons.md` | ADR-034 본문 (PNG raster 결정 근거) |
| Create | `compositor/icons/folder-closed.png` ~ `generic.png` (9개) | Lucide 16x16 RGBA PNG 자산 |
| Create | `compositor/icons/LICENSE-LUCIDE` | Lucide MIT 사본 + 출처 URL |
| Create | `compositor/src/icons.rs` | IconKind, icon_for_file, IconCache, blit_icon_at, 단위 테스트 |
| Modify | `compositor/Cargo.toml` | `image = { version = "0.25", default-features = false, features = ["png"] }` |
| Modify | `compositor/src/lib.rs` | `pub mod icons;` 추가 |
| Modify | `compositor/src/render.rs` | `Folder@1`/`File@1` 분기에 blit + 텍스트 x shift, label에서 `[+]/[-]` 단독 분리 |

`compositor/src/layout.rs` 무변경 (rect 폭/높이 그대로). `hit_test.rs` 무변경 (ExpandToggle 36px 영역 그대로 — 아이콘은 Body 안).

---

## Task 1: ADR-034 + image dep + lib 노출

**Files:**
- Create: `docs/adr/034-icons.md`
- Modify: `compositor/Cargo.toml` (line 32 뒤에 추가)
- Modify: `compositor/src/lib.rs`

- [ ] **Step 1.1: ADR-034 작성**

Create `docs/adr/034-icons.md`:

```markdown
# ADR-034 — 파일·폴더 아이콘 (Lucide 16x16 PNG, type-aware)

- **상태:** Accepted
- **결정일:** 2026-05-20
- **부모 spec:** `docs/specs/2026-05-20-geulos-icons.md`

## Context

M8 마감 후 사용자 보고: *"어떤게 파일이고 어떤게 폴더인지 아이콘이미지가 없어서 보기불편한점"*. 좌측 FileTree와 우측 Explorer 모두 텍스트 라벨만 있어 시각적 구분이 약함.

## Decision

16x16 PNG raster 아이콘 9종을 `compositor/icons/`에 체크인하고, 시작 시 `LazyLock`으로 1회 디코드해서 `[u32; 256]` 픽셀 배열로 캐시. `icon_for_file(type_uri, name, mime, is_expanded)` 라우팅이 IconKind 결정. softbuffer ARGB 버퍼에 수동 src-over alpha blend.

## Alternatives 검토

- **Unicode 글리프 (📁 📄)** — 폰트 의존, 색상 고정, 플랫폼 렌더 차이. 거부.
- **직접 raster (Rust 코드로 도형 그리기)** — 9개 작성 부담 + 미적 일관성 낮음. 거부.
- **SVG → 빌드 시 변환 (resvg)** — `resvg` crate 의존성 무거움(~2MB 코드). v1 거부, v2 dark/custom 도입 시 재검토.
- **`include_bytes!` + PNG 디코드** — 채택. 9 PNG = ~5KB, 디코드 1회로 끝.

## Consequences

- `compositor` 바이너리 크기 ~200KB 증가 (image crate + 9 PNG)
- 새 아이콘 추가 = PNG 추가 + IconKind variant + 라우팅 분기 (3곳)
- v2 후속: dark 모드 세트, title bar 아이콘, 사용자 커스텀, vector resize

## Trade-offs

- 16x16 raster는 HiDPI 디스플레이에서 약간 흐릴 수 있음 — v2에서 24x24 또는 vector resize
- light bg 전용 — dark 모드는 v2

## 매핑 (9종)

| IconKind | Lucide 글리프 | 사용 케이스 |
|---|---|---|
| `FolderClosed` | folder | `aios.std/Folder@1`, expanded 아님 |
| `FolderOpen` | folder-open | `Folder@1`, expanded |
| `Markdown` | file-text | `.md`/`.markdown` |
| `Code` | code | `.rs`/`.py`/`.js`/`.ts`/`.html`/`.css` |
| `Config` | settings | `.toml`/`.yaml`/`.yml`/`.json` |
| `Text` | file-text | `.txt`/`.log`/`.ini`/`.cfg` |
| `Image` | image | `.png`/`.jpg`/`.gif`/`.svg`/`.webp` |
| `Archive` | package | `.zip`/`.tar`/`.gz`/`.7z`/`.rar` |
| `Dotfile` | key-round | `.env`/`.gitignore`/`.editorconfig` 등 |
| `Generic` | file | 기타 |

(`Markdown`과 `Text`가 같은 Lucide 글리프 file-text를 *공유*하지만 별 PNG로 체크인 — 향후 분리 변경 자유 확보.)

라이센스: Lucide MIT — `compositor/icons/LICENSE-LUCIDE`에 사본.
```

- [ ] **Step 1.2: image crate dependency 추가**

Edit `compositor/Cargo.toml` — line 32(`arboard = "3"`) 뒤에 추가:

```toml
# ADR-034: 16x16 Lucide PNG 디코드 (default-features off로 size 절감 — png만 필요)
image = { version = "0.25", default-features = false, features = ["png"] }
```

- [ ] **Step 1.3: icons 모듈을 lib에 노출**

Read `compositor/src/lib.rs` (10 lines 정도). 기존 `pub mod` 선언들 옆에 추가:

```rust
pub mod icons;
```

- [ ] **Step 1.4: 빌드 검증 (의존성만, icons.rs는 아직 없음 — 빌드 실패 예상)**

Run:
```powershell
cd F:\GeulOS
cargo check -p geulos-compositor 2>&1 | Select-Object -Last 10
```

Expected: `error[E0583]: file not found for module 'icons'` (Step 1.3에서 선언만 했고 아직 파일 없음). 다음 Task 2에서 파일 생성.

- [ ] **Step 1.5: 빈 icons.rs 임시 생성해서 빌드 그린 만들기**

Create `compositor/src/icons.rs`:

```rust
//! 파일·폴더 아이콘 — Lucide MIT 16x16 PNG. ADR-034.
//!
//! Task 2~5에서 IconKind + 라우팅 + 디코드 캐시 + blit 구현.
```

Run:
```powershell
cargo check -p geulos-compositor 2>&1 | Select-Object -Last 5
```

Expected: `Finished` (warning 0).

- [ ] **Step 1.6: Commit**

```powershell
cd F:\GeulOS
git add docs/adr/034-icons.md compositor/Cargo.toml compositor/src/lib.rs compositor/src/icons.rs
git commit -m "docs(adr)+(compositor): ADR-034 + 아이콘 모듈 스켈레톤 (image dep + 빈 icons.rs)"
```

---

## Task 2: Lucide PNG 9개 수급 + LICENSE

**Files:**
- Create: `compositor/icons/folder-closed.png`
- Create: `compositor/icons/folder-open.png`
- Create: `compositor/icons/markdown.png`
- Create: `compositor/icons/code.png`
- Create: `compositor/icons/config.png`
- Create: `compositor/icons/text.png`
- Create: `compositor/icons/image.png`
- Create: `compositor/icons/archive.png`
- Create: `compositor/icons/dotfile.png`
- Create: `compositor/icons/generic.png`
- Create: `compositor/icons/LICENSE-LUCIDE`

배경: spec §5.1에서 *implementer 사전 변환*이 결정됨. PowerShell + ImageMagick (`magick` CLI)가 가장 robust. ImageMagick 미설치 시 https://lucide.dev/icons/ 에서 SVG → 온라인 SVG→PNG 변환기로 16x16 변환.

- [ ] **Step 2.1: 아이콘 디렉터리 생성**

Run:
```powershell
New-Item -ItemType Directory -Force -Path F:\GeulOS\compositor\icons | Out-Null
```

- [ ] **Step 2.2: ImageMagick 설치 확인 (없으면 안내)**

Run:
```powershell
$mg = Get-Command magick -ErrorAction SilentlyContinue
if ($mg) { "OK: $($mg.Source)" } else { "ImageMagick 없음 — winget install ImageMagick.ImageMagick 또는 수동 변환" }
```

Expected: `OK: ...` 출력. 없으면 winget으로 설치하거나 Step 2.3 우회 흐름(수동).

- [ ] **Step 2.3: 9 SVG 다운로드 + 16x16 PNG 변환**

다음 PowerShell 스크립트를 그대로 실행:

```powershell
cd F:\GeulOS\compositor\icons

$icons = @{
    "folder-closed" = "folder"
    "folder-open"   = "folder-open"
    "markdown"      = "file-text"
    "code"          = "code"
    "config"        = "settings"
    "text"          = "file-text"
    "image"         = "image"
    "archive"       = "package"
    "dotfile"       = "key-round"
    "generic"       = "file"
}

foreach ($name in $icons.Keys) {
    $lucide = $icons[$name]
    $svgUrl = "https://raw.githubusercontent.com/lucide-icons/lucide/main/icons/$lucide.svg"
    $svgPath = "$name.svg"
    $pngPath = "$name.png"
    Invoke-WebRequest -Uri $svgUrl -OutFile $svgPath -UseBasicParsing
    # Lucide SVG는 stroke만 — 16x16 fit + 배경 투명 + stroke 색 #333 권장
    magick -background none -density 384 $svgPath -resize 16x16 -strip $pngPath
    Remove-Item $svgPath
}

Get-ChildItem *.png | Format-Table Name, Length
```

Expected: 10줄(9 PNG + 헤더), 각 파일 ~300-800 bytes.

ImageMagick 없는 경우 우회: https://lucide.dev/icons/folder 등에서 각 SVG 다운로드 + 온라인 도구(예: cloudconvert.com)로 16x16 PNG 변환 후 같은 이름으로 디렉터리에 저장.

- [ ] **Step 2.4: LICENSE-LUCIDE 작성**

Create `compositor/icons/LICENSE-LUCIDE`:

```
ISC License

Copyright (c) 2024 Lucide Contributors

Permission to use, copy, modify, and/or distribute this software for any purpose
with or without fee is hereby granted, provided that the above copyright notice
and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR
OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
PERFORMANCE OF THIS SOFTWARE.

---

GeulOS uses 10 icons from Lucide (https://lucide.dev/), which is licensed under
the ISC License. The original Lucide icons are SVG vectors; the PNG files in
this directory are 16x16 rasterizations produced via ImageMagick from the
upstream SVG sources (lucide-icons/lucide on GitHub, main branch as of
2026-05-20). Mapping:

  folder-closed.png ← folder.svg
  folder-open.png   ← folder-open.svg
  markdown.png      ← file-text.svg
  code.png          ← code.svg
  config.png        ← settings.svg
  text.png          ← file-text.svg
  image.png         ← image.svg
  archive.png       ← package.svg
  dotfile.png       ← key-round.svg
  generic.png       ← file.svg
```

(Lucide는 ISC 라이센스 — MIT보다 약간 다른 문구지만 호환. spec §4의 "MIT" 표기는 보수적 분류; 실제는 ISC.)

- [ ] **Step 2.5: PNG 9개 + LICENSE staged 확인**

Run:
```powershell
Get-ChildItem F:\GeulOS\compositor\icons | Format-Table Name, Length
```

Expected: 11개 (9 PNG + LICENSE-LUCIDE + ... 디렉터리 entry는 제외). 모든 PNG 200-1000 bytes.

- [ ] **Step 2.6: Commit**

```powershell
cd F:\GeulOS
git add compositor/icons/
git commit -m "feat(compositor): T-icon.2 Lucide 16x16 PNG 자산 9종 + LICENSE-LUCIDE (ISC)"
```

---

## Task 3: IconKind enum + icon_for_file 라우팅 (TDD)

**Files:**
- Modify: `compositor/src/icons.rs`

- [ ] **Step 3.1: 실패할 라우팅 테스트 11개 작성**

Overwrite `compositor/src/icons.rs`:

```rust
//! 파일·폴더 아이콘 — Lucide MIT 16x16 PNG. ADR-034.

/// 아이콘 종류. 9 variant + Generic = 10. spec §4 매핑.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconKind {
    FolderClosed,
    FolderOpen,
    Markdown,
    Code,
    Config,
    Text,
    Image,
    Archive,
    Dotfile,
    Generic,
}

/// 객체 + 파일명 + mime + expanded 상태로 IconKind 결정.
///
/// 라우팅 순서 (spec §5.4):
/// 1. Folder@1 → expanded? Open : Closed
/// 2. Dotfile 화이트리스트 (`.env`, `.gitignore`, ...)
/// 3. mime == "text/markdown" → Markdown
/// 4. 확장자 .rs/.py/.js/.ts/.html/.css → Code
/// 5. 확장자 .toml/.yaml/.yml/.json → Config
/// 6. 확장자 .png/.jpg/.jpeg/.gif/.svg/.webp → Image
/// 7. 확장자 .zip/.tar/.gz/.7z/.rar → Archive
/// 8. mime이 "text/"로 시작 → Text
/// 9. 기타 → Generic
pub fn icon_for_file(type_uri: &str, name: &str, mime: &str, is_expanded: bool) -> IconKind {
    // Task 3.3에서 구현.
    let _ = (type_uri, name, mime, is_expanded);
    todo!("Task 3.3")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_unexpanded_returns_folder_closed() {
        assert_eq!(
            icon_for_file("aios.std/Folder@1", "src", "", false),
            IconKind::FolderClosed
        );
    }

    #[test]
    fn folder_expanded_returns_folder_open() {
        assert_eq!(
            icon_for_file("aios.std/Folder@1", "src", "", true),
            IconKind::FolderOpen
        );
    }

    #[test]
    fn md_extension_returns_markdown() {
        assert_eq!(
            icon_for_file("aios.std/File@1", "README.md", "text/markdown", false),
            IconKind::Markdown
        );
    }

    #[test]
    fn rs_extension_returns_code() {
        assert_eq!(
            icon_for_file("aios.std/File@1", "lib.rs", "text/rust", false),
            IconKind::Code
        );
    }

    #[test]
    fn toml_extension_returns_config() {
        assert_eq!(
            icon_for_file("aios.std/File@1", "Cargo.toml", "text/plain", false),
            IconKind::Config
        );
    }

    #[test]
    fn env_dotfile_returns_dotfile() {
        assert_eq!(
            icon_for_file("aios.std/File@1", ".env", "text/plain", false),
            IconKind::Dotfile
        );
    }

    #[test]
    fn png_extension_returns_image() {
        assert_eq!(
            icon_for_file("aios.std/File@1", "photo.png", "application/octet-stream", false),
            IconKind::Image
        );
    }

    #[test]
    fn zip_extension_returns_archive() {
        assert_eq!(
            icon_for_file("aios.std/File@1", "src.zip", "application/octet-stream", false),
            IconKind::Archive
        );
    }

    #[test]
    fn txt_extension_returns_text() {
        assert_eq!(
            icon_for_file("aios.std/File@1", "notes.txt", "text/plain", false),
            IconKind::Text
        );
    }

    #[test]
    fn unknown_extension_returns_generic() {
        assert_eq!(
            icon_for_file("aios.std/File@1", "weird.xyz", "application/octet-stream", false),
            IconKind::Generic
        );
    }

    #[test]
    fn dotfile_check_runs_before_extension() {
        // ".env" 자체에는 확장자 없음. 그러나 ".env.local" 같은 경우는 *Dotfile이 우선*되지
        // 않아야 함 (확장자 .local로 fallback). Spec §5.4의 화이트리스트는 *정확한 이름 매칭*.
        assert_eq!(
            icon_for_file("aios.std/File@1", ".env.local", "text/plain", false),
            IconKind::Text  // .local 확장자 unknown이지만 mime text/plain → Text
        );
    }
}
```

- [ ] **Step 3.2: 테스트 실행 — 모두 실패 확인**

Run:
```powershell
cd F:\GeulOS
cargo test -p geulos-compositor icons:: 2>&1 | Select-Object -Last 20
```

Expected: 11 tests failed (panic: `not yet implemented: Task 3.3`).

- [ ] **Step 3.3: icon_for_file 구현**

Replace the `icon_for_file` body in `compositor/src/icons.rs` (keep tests, keep enum):

```rust
pub fn icon_for_file(type_uri: &str, name: &str, mime: &str, is_expanded: bool) -> IconKind {
    if type_uri == "aios.std/Folder@1" {
        return if is_expanded { IconKind::FolderOpen } else { IconKind::FolderClosed };
    }

    // 정확한 이름 매칭 — `.env.local` 같은 변형은 fallback (Step 3.1 11번째 테스트 참고)
    const DOTFILES: &[&str] = &[
        ".env",
        ".envrc",
        ".gitignore",
        ".gitattributes",
        ".dockerignore",
        ".editorconfig",
        ".prettierrc",
        ".eslintrc",
    ];
    if DOTFILES.contains(&name) {
        return IconKind::Dotfile;
    }

    if mime == "text/markdown" {
        return IconKind::Markdown;
    }

    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "rs" | "py" | "js" | "ts" | "html" | "css" => return IconKind::Code,
        "toml" | "yaml" | "yml" | "json" => return IconKind::Config,
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" => return IconKind::Image,
        "zip" | "tar" | "gz" | "7z" | "rar" => return IconKind::Archive,
        _ => {}
    }

    if mime.starts_with("text/") {
        return IconKind::Text;
    }

    IconKind::Generic
}
```

- [ ] **Step 3.4: 테스트 통과 확인**

Run:
```powershell
cd F:\GeulOS
cargo test -p geulos-compositor icons:: 2>&1 | Select-Object -Last 10
```

Expected: `test result: ok. 11 passed; 0 failed`.

- [ ] **Step 3.5: clippy + fmt 그린**

Run:
```powershell
cargo fmt -p geulos-compositor
cargo clippy -p geulos-compositor --all-targets -- -D warnings 2>&1 | Select-Object -Last 5
```

Expected: `Finished` 무경고.

- [ ] **Step 3.6: Commit**

```powershell
git add compositor/src/icons.rs
git commit -m "feat(compositor): T-icon.3 IconKind + icon_for_file 라우팅 (11 단위 테스트)"
```

---

## Task 4: IconCache (LazyLock 디코드) + decode 테스트

**Files:**
- Modify: `compositor/src/icons.rs`

- [ ] **Step 4.1: decode 테스트 작성 (실패 예상)**

Edit `compositor/src/icons.rs`. `mod tests` 블록 안에 추가:

```rust
    #[test]
    fn decode_all_icons_succeeds() {
        // 9 PNG 모두 16x16으로 디코드되어 256 픽셀이 나와야 함.
        for kind in [
            IconKind::FolderClosed,
            IconKind::FolderOpen,
            IconKind::Markdown,
            IconKind::Code,
            IconKind::Config,
            IconKind::Text,
            IconKind::Image,
            IconKind::Archive,
            IconKind::Dotfile,
            IconKind::Generic,
        ] {
            let pixels = get_icon_pixels(kind);
            assert_eq!(pixels.len(), 256, "{kind:?} expected 256 px, got {}", pixels.len());
        }
    }
```

Run:
```powershell
cargo test -p geulos-compositor icons::tests::decode_all_icons_succeeds 2>&1 | Select-Object -Last 5
```

Expected: 컴파일 에러 (`get_icon_pixels` 없음).

- [ ] **Step 4.2: IconCache + get_icon_pixels 구현**

Edit `compositor/src/icons.rs`. `mod tests` 위에 추가:

```rust
use std::sync::LazyLock;

/// 9 PNG 자산 — `compositor/icons/`에서 컴파일 시점에 embed.
const PNG_FOLDER_CLOSED: &[u8] = include_bytes!("../icons/folder-closed.png");
const PNG_FOLDER_OPEN: &[u8] = include_bytes!("../icons/folder-open.png");
const PNG_MARKDOWN: &[u8] = include_bytes!("../icons/markdown.png");
const PNG_CODE: &[u8] = include_bytes!("../icons/code.png");
const PNG_CONFIG: &[u8] = include_bytes!("../icons/config.png");
const PNG_TEXT: &[u8] = include_bytes!("../icons/text.png");
const PNG_IMAGE: &[u8] = include_bytes!("../icons/image.png");
const PNG_ARCHIVE: &[u8] = include_bytes!("../icons/archive.png");
const PNG_DOTFILE: &[u8] = include_bytes!("../icons/dotfile.png");
const PNG_GENERIC: &[u8] = include_bytes!("../icons/generic.png");

/// 16x16 = 256 ARGB u32 픽셀. spec §5.5.
type IconPixels = [u32; 256];

/// 시작 시 1회 디코드해서 캐시. LazyLock으로 첫 호출 시 9 PNG 동시 디코드.
static ICON_CACHE: LazyLock<IconCache> = LazyLock::new(IconCache::decode_all);

struct IconCache {
    folder_closed: IconPixels,
    folder_open: IconPixels,
    markdown: IconPixels,
    code: IconPixels,
    config: IconPixels,
    text: IconPixels,
    image: IconPixels,
    archive: IconPixels,
    dotfile: IconPixels,
    generic: IconPixels,
}

impl IconCache {
    fn decode_all() -> Self {
        Self {
            folder_closed: decode_png_to_argb(PNG_FOLDER_CLOSED, "folder-closed"),
            folder_open: decode_png_to_argb(PNG_FOLDER_OPEN, "folder-open"),
            markdown: decode_png_to_argb(PNG_MARKDOWN, "markdown"),
            code: decode_png_to_argb(PNG_CODE, "code"),
            config: decode_png_to_argb(PNG_CONFIG, "config"),
            text: decode_png_to_argb(PNG_TEXT, "text"),
            image: decode_png_to_argb(PNG_IMAGE, "image"),
            archive: decode_png_to_argb(PNG_ARCHIVE, "archive"),
            dotfile: decode_png_to_argb(PNG_DOTFILE, "dotfile"),
            generic: decode_png_to_argb(PNG_GENERIC, "generic"),
        }
    }
}

fn decode_png_to_argb(bytes: &[u8], name: &'static str) -> IconPixels {
    let img = image::load_from_memory(bytes)
        .unwrap_or_else(|e| panic!("{name}.png 디코드 실패: {e}"));
    let rgba = img.to_rgba8();
    assert_eq!(
        (rgba.width(), rgba.height()),
        (16, 16),
        "{name}.png은 16x16이어야 함 — 현재 {}x{}",
        rgba.width(),
        rgba.height()
    );
    let mut out = [0u32; 256];
    for (i, px) in rgba.pixels().enumerate() {
        // image::Rgba<u8> = [R, G, B, A]. softbuffer는 ARGB u32 (A << 24 | R << 16 | G << 8 | B).
        let [r, g, b, a] = px.0;
        out[i] = (u32::from(a) << 24)
            | (u32::from(r) << 16)
            | (u32::from(g) << 8)
            | u32::from(b);
    }
    out
}

/// IconKind에 해당하는 16x16 ARGB 픽셀 배열. 첫 호출 시 9 PNG 모두 디코드 (~1ms).
pub fn get_icon_pixels(kind: IconKind) -> &'static IconPixels {
    let c = &*ICON_CACHE;
    match kind {
        IconKind::FolderClosed => &c.folder_closed,
        IconKind::FolderOpen => &c.folder_open,
        IconKind::Markdown => &c.markdown,
        IconKind::Code => &c.code,
        IconKind::Config => &c.config,
        IconKind::Text => &c.text,
        IconKind::Image => &c.image,
        IconKind::Archive => &c.archive,
        IconKind::Dotfile => &c.dotfile,
        IconKind::Generic => &c.generic,
    }
}
```

- [ ] **Step 4.3: 테스트 통과 확인**

Run:
```powershell
cargo test -p geulos-compositor icons:: 2>&1 | Select-Object -Last 10
```

Expected: 12 tests passed (11 routing + 1 decode).

- [ ] **Step 4.4: clippy + fmt 그린**

Run:
```powershell
cargo fmt -p geulos-compositor
cargo clippy -p geulos-compositor --all-targets -- -D warnings 2>&1 | Select-Object -Last 5
```

Expected: `Finished` 무경고.

- [ ] **Step 4.5: Commit**

```powershell
git add compositor/src/icons.rs
git commit -m "feat(compositor): T-icon.4 IconCache LazyLock + 9 PNG 디코드 (decode 회귀 테스트)"
```

---

## Task 5: blit_icon_at + blend_argb 헬퍼 (TDD)

**Files:**
- Modify: `compositor/src/icons.rs`

- [ ] **Step 5.1: blend + blit 테스트 작성 (실패 예상)**

Edit `compositor/src/icons.rs`. `mod tests` 안에 추가:

```rust
    #[test]
    fn blend_argb_opaque_replaces_bg() {
        // alpha 0xFF → 전경이 배경을 완전히 덮음
        let bg = 0xFF_00_00_00; // black
        let fg = 0xFF_FF_FF_FF; // white
        assert_eq!(blend_argb(bg, fg), 0xFF_FF_FF_FF);
    }

    #[test]
    fn blend_argb_transparent_keeps_bg() {
        // alpha 0x00 → 호출자가 사전 skip하지만 함수 호출 시 안전해야
        let bg = 0xFF_AA_AA_AA;
        let fg = 0x00_00_00_00;
        assert_eq!(blend_argb(bg, fg), bg);
    }

    #[test]
    fn blend_argb_half_alpha_mixes() {
        // 흰색 위에 0x80 알파의 검정 → 중간 회색에 가까운 값
        let bg = 0xFF_FF_FF_FF; // white
        let fg = 0x80_00_00_00; // black @ ~50% alpha
        let result = blend_argb(bg, fg);
        let r = (result >> 16) & 0xFF;
        // 정확히 0x80이 아닌 ~0x7F (정수 라운딩). 0x70~0x90 범위.
        assert!((0x70..=0x90).contains(&r), "expected ~0x80 mid-gray, got 0x{r:02X}");
    }

    #[test]
    fn blit_icon_skips_out_of_bounds() {
        // 4x4 버퍼에 16x16 아이콘을 음수 좌표로 — 크래시 없이 일부만 그려야
        let mut buf = [0xFF_00_00_00u32; 16]; // 4x4
        blit_icon_at(&mut buf, 4, 4, -2, -2, IconKind::Generic);
        // 단순 안전성 — panic 없으면 OK. 버퍼는 변경됐을 수도, 아닐 수도.
    }
```

Run:
```powershell
cargo test -p geulos-compositor icons:: 2>&1 | Select-Object -Last 5
```

Expected: 컴파일 에러 (`blend_argb`, `blit_icon_at` 없음).

- [ ] **Step 5.2: blend_argb + blit_icon_at 구현**

Edit `compositor/src/icons.rs`. `get_icon_pixels` 함수 뒤에 추가:

```rust
/// src-over alpha compositing — `fg`를 `bg` 위에 alpha만큼 섞는다.
///
/// ARGB u32 형식. softbuffer 버퍼 직접 호환.
pub fn blend_argb(bg: u32, fg: u32) -> u32 {
    let a = (fg >> 24) & 0xFF;
    if a == 0 {
        return bg;
    }
    if a == 0xFF {
        return fg;
    }
    let inv = 255 - a;
    let bg_r = (bg >> 16) & 0xFF;
    let bg_g = (bg >> 8) & 0xFF;
    let bg_b = bg & 0xFF;
    let fg_r = (fg >> 16) & 0xFF;
    let fg_g = (fg >> 8) & 0xFF;
    let fg_b = fg & 0xFF;
    let r = (fg_r * a + bg_r * inv) / 255;
    let g = (fg_g * a + bg_g * inv) / 255;
    let b = (fg_b * a + bg_b * inv) / 255;
    0xFF_00_00_00 | (r << 16) | (g << 8) | b
}

/// 16x16 아이콘을 softbuffer 픽셀 배열의 (x, y)에 blit. 경계 밖 픽셀은 skip.
pub fn blit_icon_at(
    buffer: &mut [u32],
    buf_w: usize,
    buf_h: usize,
    x: i32,
    y: i32,
    kind: IconKind,
) {
    let icon = get_icon_pixels(kind);
    for iy in 0..16i32 {
        for ix in 0..16i32 {
            let tx = x + ix;
            let ty = y + iy;
            if tx < 0 || ty < 0 || tx >= buf_w as i32 || ty >= buf_h as i32 {
                continue;
            }
            let px = icon[(iy * 16 + ix) as usize];
            if (px >> 24) & 0xFF == 0 {
                continue;
            }
            let idx = ty as usize * buf_w + tx as usize;
            buffer[idx] = blend_argb(buffer[idx], px);
        }
    }
}
```

- [ ] **Step 5.3: 테스트 통과 확인**

Run:
```powershell
cargo test -p geulos-compositor icons:: 2>&1 | Select-Object -Last 10
```

Expected: 16 tests passed (11 routing + 1 decode + 4 blend/blit).

- [ ] **Step 5.4: clippy + fmt 그린**

Run:
```powershell
cargo fmt -p geulos-compositor
cargo clippy -p geulos-compositor --all-targets -- -D warnings 2>&1 | Select-Object -Last 5
```

Expected: `Finished` 무경고.

- [ ] **Step 5.5: Commit**

```powershell
git add compositor/src/icons.rs
git commit -m "feat(compositor): T-icon.5 blit_icon_at + blend_argb src-over (4 단위 테스트)"
```

---

## Task 6: render.rs Folder@1 분기 — 아이콘 blit + 텍스트 shift

**Files:**
- Modify: `compositor/src/render.rs` (line 103~113)

Spec §5.3:
```
[+] {icon} foldername
^   ^      ^
|   |      └ 텍스트 시작 (rect.x + 60)
|   └ 16x16 아이콘 (rect.x + 40)
└ ExpandToggle 텍스트 (rect.x + 4)
```

기존 line 110~111:
```rust
let label = format!("{} {}", prefix, name);
draw_text(buffer, width, height, &label, rect.x + 4, rect.y + 6, COLOR_FOLDER_TEXT);
```

→ `[+]/[-]`만 x+4에 그리고, 아이콘을 x+40에 blit, 폴더명만 x+60에 그림.

- [ ] **Step 6.1: render.rs의 Folder@1 분기 갱신**

Edit `compositor/src/render.rs`. line 103~113를 다음으로 교체:

```rust
            "aios.std/Folder@1" => {
                let is_sel = selected_id == Some(id);
                if is_sel {
                    fill_rect(buffer, width, height, &rect, COLOR_SELECTED_BG);
                }
                let name = obj.props.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let is_expanded = is_folder_expanded(tree, id);
                let prefix = if is_expanded { "[-]" } else { "[+]" };
                // ExpandToggle 표시 (rect.x + 4)
                draw_text(buffer, width, height, prefix, rect.x + 4, rect.y + 6, COLOR_FOLDER_TEXT);
                // 16x16 아이콘 (rect.x + 40) — spec §5.3 layout
                let kind = crate::icons::icon_for_file("aios.std/Folder@1", name, "", is_expanded);
                crate::icons::blit_icon_at(buffer, width, height, rect.x + 40, rect.y + 6, kind);
                // 폴더명 (rect.x + 60)
                draw_text(buffer, width, height, name, rect.x + 60, rect.y + 6, COLOR_FOLDER_TEXT);
                draw_ai_dot_if_recent(buffer, width, height, &rect, obj, now_ms);
            }
```

- [ ] **Step 6.2: 빌드 + 기존 테스트 회귀 없음 확인**

Run:
```powershell
cd F:\GeulOS
cargo test -p geulos-compositor 2>&1 | Select-Object -Last 5
```

Expected: 모든 기존 테스트 통과 (icons 16 + 기존 layout/render/tree_model 회귀).

- [ ] **Step 6.3: clippy + fmt 그린**

Run:
```powershell
cargo fmt -p geulos-compositor
cargo clippy -p geulos-compositor --all-targets -- -D warnings 2>&1 | Select-Object -Last 5
```

Expected: `Finished` 무경고.

- [ ] **Step 6.4: Commit**

```powershell
git add compositor/src/render.rs
git commit -m "feat(compositor): T-icon.6 Folder@1 렌더에 아이콘 blit + 텍스트 x shift (spec §5.3)"
```

---

## Task 7: render.rs File@1 분기 — 아이콘 blit + 텍스트 shift

**Files:**
- Modify: `compositor/src/render.rs` (line 114~123)

Spec §5.3:
```
{icon} item_name
^      ^
|      └ 텍스트 시작 (rect.x + 24)
└ 16x16 아이콘 (rect.x + 4)
```

기존 line 119~121:
```rust
let name = obj.props.get("name").and_then(|v| v.as_str()).unwrap_or("?");
let label = format!("  {}", name);
draw_text(buffer, width, height, &label, rect.x + 4, rect.y + 4, COLOR_FILE_TEXT);
```

- [ ] **Step 7.1: render.rs의 File@1 분기 갱신**

Edit `compositor/src/render.rs`. line 114~123를 다음으로 교체:

```rust
            "aios.std/File@1" => {
                let is_sel = selected_id == Some(id);
                if is_sel {
                    fill_rect(buffer, width, height, &rect, COLOR_SELECTED_BG);
                }
                let name = obj.props.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let mime = obj.props.get("mime").and_then(|v| v.as_str()).unwrap_or("");
                // 16x16 아이콘 (rect.x + 4) — spec §5.3 layout
                let kind = crate::icons::icon_for_file("aios.std/File@1", name, mime, false);
                crate::icons::blit_icon_at(buffer, width, height, rect.x + 4, rect.y + 4, kind);
                // 파일명 (rect.x + 24)
                draw_text(buffer, width, height, name, rect.x + 24, rect.y + 4, COLOR_FILE_TEXT);
                draw_ai_dot_if_recent(buffer, width, height, &rect, obj, now_ms);
            }
```

- [ ] **Step 7.2: 빌드 + 회귀 없음 확인**

Run:
```powershell
cargo test -p geulos-compositor 2>&1 | Select-Object -Last 5
```

Expected: 모든 테스트 통과.

- [ ] **Step 7.3: clippy + fmt 그린**

Run:
```powershell
cargo fmt -p geulos-compositor
cargo clippy -p geulos-compositor --all-targets -- -D warnings 2>&1 | Select-Object -Last 5
```

Expected: `Finished` 무경고.

- [ ] **Step 7.4: Commit**

```powershell
git add compositor/src/render.rs
git commit -m "feat(compositor): T-icon.7 File@1 렌더에 아이콘 blit + 텍스트 x shift (spec §5.3)"
```

---

## Task 8: Workspace 전체 검증 + 사용자 시각 acceptance

**Files:** (변경 없음 — 검증만)

- [ ] **Step 8.1: workspace 전체 build/test/fmt/clippy 그린**

Run:
```powershell
cd F:\GeulOS
cargo build --workspace --all-targets 2>&1 | Select-Object -Last 3
cargo test --all 2>&1 | Select-Object -Last 5
cargo fmt --all -- --check 2>&1 | Select-Object -Last 3
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | Select-Object -Last 5
```

Expected 각각: `Finished`, 모든 테스트 통과, fmt drift 0, clippy 무경고.

- [ ] **Step 8.2: 사용자 시각 acceptance — 호스트 3 프로세스 데모**

3 PowerShell 창을 *이 순서로* 띄움 (KI-004 회피):

```powershell
# 터미널 1
cargo run -p geulos-server-host

# 터미널 2 (1번 띄운 후)
cargo run -p geulos-desktop-shell

# 터미널 3 (2번 띄운 후)
cargo run -p geulos-compositor
```

체크리스트 (사용자 시각 확인):
- [ ] 좌측 FileTree에 `[+] {폴더아이콘} C:\`, `[+] {폴더아이콘} D:\` 등이 보임 (folder-closed 아이콘)
- [ ] `[+]` 클릭으로 폴더 expand → 아이콘이 folder-open으로 변경됨
- [ ] 우측 Explorer에 navigate → 폴더 행은 folder-closed/open, 파일 행은 확장자별 다른 아이콘
- [ ] `.md` 파일 → markdown 아이콘
- [ ] `.rs` 파일 → code 아이콘
- [ ] `.toml` 파일 → config 아이콘
- [ ] `.png`/`.jpg` 파일 → image 아이콘
- [ ] `.zip` 파일 → archive 아이콘
- [ ] `.env`/`.gitignore` → dotfile (key-round) 아이콘
- [ ] 확장자 unknown → generic 아이콘
- [ ] 아이콘 위 클릭 = 파일/폴더 클릭과 동일 동작 (ExpandToggle 36px 영역만 [+]/[-] toggle)
- [ ] 컴포지터 크래시 X
- [ ] AI 노란 점 표시 위치 변하지 않음 (rect 우측)

- [ ] **Step 8.3: 사용자 acceptance 통과 보고**

사용자에게 *각 mime별 정확한 아이콘 등장* 확인 요청. 통과 시 다음 단계 (Task 9).

---

## Task 9: spec/quality review + known-issues 업데이트

**Files:**
- Modify: `docs/known-issues.md` (M8 종료 시점 + 아이콘 task 마감 추가)

- [ ] **Step 9.1: known-issues.md에 마감 entry 추가**

Read `docs/known-issues.md`의 line 8~16 (M8 정식 마감 entry) 확인 후, 그 아래에 추가:

```markdown
- **아이콘 task 마감 (2026-05-XX):** T-icon.1~9 완료. `compositor/src/icons.rs` 신설 +
  Lucide 9 PNG (ISC) 체크인. ADR-034. 16 단위 테스트 (11 라우팅 + 1 decode + 4 blit/blend).
  사용자 시각 acceptance 통과 — 각 mime별 정확 아이콘 노출 + 폴더 expand 시 closed/open
  전환. 잔여 v2 갭: dark 모드, title bar 아이콘, 사용자 커스텀, 24x24/vector resize.
```

(날짜는 commit 시점으로 채움.)

- [ ] **Step 9.2: dead code 마킹 확인**

Run:
```powershell
cargo clippy -p geulos-compositor --all-targets -- -W dead_code 2>&1 | Select-String "warning: function|warning: const"
```

Expected: icons.rs의 public API는 모두 사용됨 (render.rs에서 호출). 만약 사용 안 된 헬퍼가 있으면 `#[allow(dead_code)]` 또는 제거.

- [ ] **Step 9.3: 전체 workspace 최종 그린 확인**

Run:
```powershell
cargo test --all 2>&1 | Select-String "test result"
```

Expected: 모든 binary `0 failed`.

- [ ] **Step 9.4: Linux-only 코드 검증 (회귀 가드)**

이 task는 cross-platform 코드만 추가하므로 `geulos-init`에 영향 없음. 그러나 controller가 push 전 한 번 더 확인:

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo clippy --target x86_64-unknown-linux-musl -p geulos-init --all-targets -- -D warnings 2>&1 | Select-Object -Last 3
```

Expected: `Finished` (geulos-init 변경 0이므로 무경고 — 회귀 0 확인).

- [ ] **Step 9.5: Commit + controller push 대기 보고**

```powershell
git add docs/known-issues.md
git commit -m "docs(known-issues): 아이콘 task 마감 entry (T-icon.1~9 완료)"
git log --oneline -10
```

controller에게 보고:
> 아이콘 task 9 commit 완료 (T-icon.1~9). origin/main과 차이는 icon spec(fabf572) + 본 task의 9 commit = 총 10 commit. **Push는 controller 직접 수행.**

---

## Self-Review (skill 후속 체크)

**1. Spec coverage 매핑:**
| Spec §  | 내용 | 구현 task |
|---|---|---|
| §3 in-scope: 9 PNG 16x16 | Lucide 자산 9종 | Task 2 |
| §3 in-scope: routing | icon_for_file | Task 3 |
| §3 in-scope: blit | blit_icon_at + blend_argb | Task 5 |
| §3 in-scope: cache | LazyLock | Task 4 |
| §3 in-scope: FileTree/Explorer 행에 blit | render.rs Folder/File 분기 | Task 6, 7 |
| §4 9 IconKind 매핑 | IconKind enum + 라우팅 | Task 3 |
| §5.1 PNG 사전 변환 | PowerShell + ImageMagick 스크립트 | Step 2.3 |
| §5.2 크기 16x16 | decode 시 assert | Step 4.2 (decode_png_to_argb) |
| §5.3 위치 (FileTree x+40, Explorer x+4) | render 직접 좌표 | Task 6, 7 |
| §5.4 라우팅 순서 | icon_for_file 본문 | Step 3.3 |
| §5.5 디코드+캐시 | LazyLock + IconCache | Task 4 |
| §5.6 alpha blend | blend_argb src-over | Task 5 |
| §6.1 icons.rs 모듈 | 신설 | Task 1~5 |
| §6.2 9 PNG 자산 | 체크인 | Task 2 |
| §6.3 render.rs 분기 | Folder + File | Task 6, 7 |
| §6.4 layout.rs 무변경 | (확인만) | Task 6 step 8.1 회귀 통과로 보장 |
| §6.5 Cargo.toml image dep | Step 1.2 | Task 1 |
| §7 11 단위 테스트 | tests mod | Task 3 (11) + Task 4 (1) + Task 5 (4) = 16개 (spec 11 + decode 1 + blend/blit 4 보강) |
| §8 ADR-034 | 본문 작성 | Step 1.1 |

✅ Spec의 in-scope 항목 모두 task로 cover. out-of-scope (dark/title bar/custom/vector) 모두 plan에서 제외.

**2. Placeholder scan:** "TODO/TBD/implement later/fill in" — 모든 step에 실제 코드 또는 명령 포함. 0건.

**3. Type consistency:**
- `IconKind` 변형 이름 — `FolderClosed`/`FolderOpen`/`Markdown`/`Code`/`Config`/`Text`/`Image`/`Archive`/`Dotfile`/`Generic` (Step 3.1, 4.2, 5.2, 6.1, 7.1에서 일관)
- `icon_for_file(type_uri, name, mime, is_expanded)` 시그니처 — Step 3.1 정의, Step 6.1/7.1 호출 시 동일 인자 순서
- `blit_icon_at(buffer, buf_w, buf_h, x, y, kind)` — Step 5.2 정의, Step 6.1/7.1 호출 시 동일
- `get_icon_pixels(kind) -> &'static IconPixels` — Step 4.2 정의, Step 5.2에서 호출

✅ type/시그니처 일관.

---

## 실행 가이드

**추정:** 9 task × 30~60분 = 약 6~9시간 (PNG 수급에 따라). spec §9의 3~5일 추정보다 짧음 — TDD가 한 task당 30분 cap 강제.

**Critical Path:**
- Task 2(PNG 수급)가 막히면 전체 진행 정지. ImageMagick 미설치 시 *Step 2.2의 winget 명령* 또는 *온라인 변환기 우회* 둘 다 가능.
- Task 6/7은 시각 acceptance(Task 8) 통과해야 의미 — 단순히 빌드 그린만으로는 부족.

**다음 단계 (이 plan 완료 후):** M9 — 권한 다이얼로그 + write 메서드 복귀 + KI-001/002/016 일괄 해소. 별 brainstorm 필요.
