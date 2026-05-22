# GeulOS 아이콘 — 파일·폴더 시각 구분 (Implementation Plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement task-by-task. Steps use checkbox (`- [ ]`) syntax. **NEVER push** — controller batches push at task-set end.

**Spec:** `docs/specs/2026-05-20-geulos-icons.md`

**Goal:** 좌측 FileTree와 우측 Explorer의 폴더·파일 행에 *16x16 PNG raster 아이콘* 9종 (Lucide MIT). 폴더는 expand 상태에 따라 closed/open 변형.

**Architecture:** PNG 자산을 `compositor/icons/`에 정적 임베드 (`include_bytes!`). `image` crate로 PNG decode → ARGB u32 캐시 (OnceLock). `icons.rs::icon_for_file` 라우팅 — mime/확장자/dotfile 화이트리스트로 IconKind 결정. `render.rs::render_frame`의 Folder/File 분기가 *텍스트 그리기 전*에 `blit_icon_at` 호출. layout rect 변경 없음 — 텍스트 시작 x만 shift.

**Tech Stack:** `image = "0.25"` (default-features off + `png` feature only). 신규 외부 자산: Lucide MIT 9개 PNG. 그 외 기존 그대로.

---

## 파일 구조

```
compositor/Cargo.toml              # 수정: image dep 추가
compositor/icons/folder-closed.png # 신규 (Lucide MIT)
compositor/icons/folder-open.png   # 신규
compositor/icons/markdown.png      # 신규
compositor/icons/code.png          # 신규
compositor/icons/config.png        # 신규
compositor/icons/text.png          # 신규
compositor/icons/image.png         # 신규
compositor/icons/archive.png       # 신규
compositor/icons/dotfile.png       # 신규
compositor/icons/generic.png       # 신규
compositor/icons/LICENSE-LUCIDE    # 신규 (MIT 라이선스 사본)
compositor/src/icons.rs            # 신규: IconKind + icon_for_file + cache + blit_icon_at
compositor/src/lib.rs              # 수정: pub mod icons
compositor/src/render.rs           # 수정: Folder/File 분기에 blit + 텍스트 x shift

docs/adr/034-icons.md              # 신규
docs/manual-tests/icons-acceptance.md # 신규 (T-icon.4)
```

---

## Task T-icon.1 — ADR-034 + Lucide PNG 자산 + image dep

**Files:**
- Create: `docs/adr/034-icons.md`
- Create: `compositor/icons/*.png` (9 자산) + `compositor/icons/LICENSE-LUCIDE`
- Modify: `compositor/Cargo.toml`

### 단계

- [ ] **Step 1: ADR-034 작성.** Status / Context / Decision / Alternatives rejected / Consequences / 참고. 핵심:
  - Decision: 16x16 PNG raster (Lucide MIT). type-aware 라우팅 (mime/확장자/dotfile 화이트리스트). 9종 매핑. OnceLock decode cache.
  - Alternatives rejected:
    - Unicode glyph — 흑백 + 폰트 codepoint 의존 + .notdef 위험
    - 직접 raster (fill_rect const) — 디자인 품질 낮음
    - 빌드 시 SVG → PNG (resvg crate) — 빌드 부담 + 런타임 메모리
    - vector resize — 16x16 고정 v1
  - Consequences: image crate 의존 1개 추가, binary size ~200KB 증가, 9 PNG 자산 영구 보존. v2 (다크 모드, 사용자 커스텀, 20x20, title bar 아이콘).
  - Cross-refs: ADR-026 (Window 객체), ADR-027 (M8 read-only), T8.19 dotfile 화이트리스트.
  - 기존 020~033 컨벤션 일치 (영문 헤더 + 한국어 본문).

- [ ] **Step 2: Lucide PNG 자산 9개 수급.** 두 옵션 — implementer 선택:
  - **(A) npm `lucide-static`에서 raster** — `npm install lucide-static` 또는 `https://unpkg.com/lucide-static/icons/`에서 16x16 PNG 직접 다운로드. 매핑:
    - folder.png → folder-closed.png
    - folder-open.png → folder-open.png (라이브러리에 있음)
    - file-text.png → markdown.png + text.png (별 사본 또는 같은 자산)
    - code.png → code.png
    - settings.png → config.png
    - image.png → image.png
    - package.png → archive.png
    - key-round.png → dotfile.png
    - file.png → generic.png
  - **(B) Lucide SVG → 직접 PNG 변환** — `https://github.com/lucide-icons/lucide/tree/main/icons`에서 SVG 받아 ImageMagick/Inkscape로 16x16 PNG. 더 깨끗.

  최종 9 PNG는 *16x16 RGBA* (alpha 채널 보존). PowerShell에서 크기 확인:
  ```powershell
  Get-ChildItem compositor/icons/*.png | ForEach-Object { Write-Host "$($_.Name): $($_.Length) bytes" }
  ```

- [ ] **Step 3:** `compositor/icons/LICENSE-LUCIDE` — Lucide MIT 라이선스 사본:
  ```
  ISC License
  Copyright (c) 2024 Lucide Contributors
  
  Permission to use, copy, modify, and/or distribute this software for any
  purpose with or without fee is hereby granted, provided that the above
  copyright notice and this permission notice appear in all copies.
  
  THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
  WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
  MERCHANTABILITY AND FITNESS. ...
  ```
  (Lucide repo의 `LICENSE` 파일 그대로 복사. 실제로는 ISC가 표준.)

- [ ] **Step 4: `compositor/Cargo.toml` 갱신.**

기존 `[dependencies]` 섹션에 추가:
```toml
image = { version = "0.25", default-features = false, features = ["png"] }
```

`default-features = false`로 PNG만 활성 — JPEG/GIF/etc 미사용 (binary size 절감).

- [ ] **Step 5: 빌드 확인.** image crate가 컴파일되는지:
```powershell
cargo build -p geulos-compositor
```
Expected: clean. PNG 자산이 *아직 코드에서 import 안 됨*이라 unused 경고 X (자산은 코드와 무관).

- [ ] **Step 6: Commit.**
```bash
git add docs/adr/034-icons.md compositor/icons/ compositor/Cargo.toml
git commit -m "feat(compositor): T-icon.1 — ADR-034 + Lucide MIT PNG 자산 9종 + image dep"
```
끝에:
```
Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
```

### 디자인 결정
- Lucide MIT — 라이선스 명시 (`LICENSE-LUCIDE`). 자산 영구 보존.
- 16x16 고정 v1 — 다른 크기는 v2.

---

## Task T-icon.2 — icons.rs (IconKind + 라우팅 + cache + 단위 테스트)

**Files:**
- Create: `compositor/src/icons.rs`
- Modify: `compositor/src/lib.rs` (`pub mod icons;`)

### 단계

- [ ] **Step 1: `compositor/src/icons.rs` 신규 — 핵심 구조 + 라우팅.**

```rust
//! 파일·폴더 아이콘 (ADR-034).
//!
//! Lucide MIT 16x16 PNG 9종을 정적 임베드. 시작 시 1회 decode (OnceLock 캐시) →
//! ARGB u32 [256] 배열. `icon_for_file`로 mime/확장자/dotfile 화이트리스트 라우팅.
//! `blit_icon_at`로 softbuffer ARGB buffer에 alpha blend로 그림.

use std::collections::HashMap;
use std::sync::OnceLock;

/// 아이콘 종류 — 9종 (folder closed/open + 7 파일 카테고리).
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

// 정적 PNG 자산 (include_bytes!) — Cargo.toml의 compositor crate 루트 기준 상대 경로.
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

/// 16x16 = 256 픽셀.
pub const ICON_SIZE: usize = 16;
const ICON_PIXELS: usize = ICON_SIZE * ICON_SIZE;

/// 디코드된 아이콘 — ARGB u32 256개.
pub struct IconCache {
    icons: HashMap<IconKind, [u32; ICON_PIXELS]>,
}

impl IconCache {
    fn build() -> Self {
        let mut icons = HashMap::new();
        for (kind, bytes) in [
            (IconKind::FolderClosed, PNG_FOLDER_CLOSED),
            (IconKind::FolderOpen, PNG_FOLDER_OPEN),
            (IconKind::Markdown, PNG_MARKDOWN),
            (IconKind::Code, PNG_CODE),
            (IconKind::Config, PNG_CONFIG),
            (IconKind::Text, PNG_TEXT),
            (IconKind::Image, PNG_IMAGE),
            (IconKind::Archive, PNG_ARCHIVE),
            (IconKind::Dotfile, PNG_DOTFILE),
            (IconKind::Generic, PNG_GENERIC),
        ] {
            let pixels = decode_png_16x16(bytes).unwrap_or_else(|e| {
                eprintln!("[icons] PNG decode 실패 ({:?}): {} — 빈 아이콘 사용", kind, e);
                [0u32; ICON_PIXELS]
            });
            icons.insert(kind, pixels);
        }
        Self { icons }
    }

    pub fn get(&self, kind: IconKind) -> &[u32; ICON_PIXELS] {
        self.icons.get(&kind).expect("모든 IconKind이 IconCache::build에 등록되어야 함")
    }
}

/// 정적 캐시 — 시작 시 1회 decode.
static ICON_CACHE: OnceLock<IconCache> = OnceLock::new();

pub fn icon_cache() -> &'static IconCache {
    ICON_CACHE.get_or_init(IconCache::build)
}

/// PNG 바이트 → 16x16 ARGB u32 [256].
/// `image` crate의 `load_from_memory` + RGBA → ARGB(softbuffer 형식) 변환.
fn decode_png_16x16(bytes: &[u8]) -> Result<[u32; ICON_PIXELS], String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    if rgba.width() != ICON_SIZE as u32 || rgba.height() != ICON_SIZE as u32 {
        return Err(format!("아이콘 크기 {}x{} (16x16 기대)", rgba.width(), rgba.height()));
    }
    let mut out = [0u32; ICON_PIXELS];
    for (i, pixel) in rgba.pixels().enumerate() {
        let [r, g, b, a] = pixel.0;
        // softbuffer: ARGB (A는 0xFF 가정 — 우리는 alpha blend로 처리해 보관)
        out[i] = ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
    }
    Ok(out)
}

/// type_uri + name + mime + is_expanded → IconKind 라우팅 (spec §5.4).
pub fn icon_for_file(type_uri: &str, name: &str, mime: &str, is_expanded: bool) -> IconKind {
    // 1) Folder?
    if type_uri == "aios.std/Folder@1" {
        return if is_expanded { IconKind::FolderOpen } else { IconKind::FolderClosed };
    }

    // 2) Dotfile 화이트리스트 (T8.19 lazy_mount::guess_mime과 일관)
    match name {
        ".env" | ".envrc" | ".gitignore" | ".gitattributes" | ".dockerignore"
        | ".editorconfig" | ".prettierrc" | ".eslintrc" => return IconKind::Dotfile,
        _ => {}
    }

    // 3) mime = text/markdown
    if mime == "text/markdown" {
        return IconKind::Markdown;
    }

    // 4) 확장자
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "rs" | "py" | "js" | "ts" | "html" | "htm" | "css" => return IconKind::Code,
        "toml" | "yaml" | "yml" | "json" => return IconKind::Config,
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" => return IconKind::Image,
        "zip" | "tar" | "gz" | "7z" | "rar" | "bz2" | "xz" => return IconKind::Archive,
        _ => {}
    }

    // 5) mime = text/*
    if mime.starts_with("text/") {
        return IconKind::Text;
    }

    // 6) Generic
    IconKind::Generic
}

/// softbuffer ARGB buffer에 아이콘 alpha blend로 blit.
/// 좌상단 (x, y)에 16x16. 화면 경계 밖 픽셀은 skip.
pub fn blit_icon_at(
    buffer: &mut [u32],
    buf_w: usize,
    buf_h: usize,
    x: i32,
    y: i32,
    kind: IconKind,
) {
    let pixels = icon_cache().get(kind);
    for iy in 0..ICON_SIZE {
        for ix in 0..ICON_SIZE {
            let src = pixels[iy * ICON_SIZE + ix];
            let alpha = ((src >> 24) & 0xFF) as u32;
            if alpha == 0 {
                continue;
            }
            let tx = x + ix as i32;
            let ty = y + iy as i32;
            if tx < 0 || ty < 0 || tx >= buf_w as i32 || ty >= buf_h as i32 {
                continue;
            }
            let idx = ty as usize * buf_w + tx as usize;
            if alpha == 0xFF {
                buffer[idx] = src;
            } else {
                buffer[idx] = blend_argb(buffer[idx], src, alpha);
            }
        }
    }
}

/// 표준 src-over composition (alpha=0..255).
fn blend_argb(bg: u32, src: u32, src_alpha: u32) -> u32 {
    let inv = 255 - src_alpha;
    let bg_r = (bg >> 16) & 0xFF;
    let bg_g = (bg >> 8) & 0xFF;
    let bg_b = bg & 0xFF;
    let src_r = (src >> 16) & 0xFF;
    let src_g = (src >> 8) & 0xFF;
    let src_b = src & 0xFF;
    let r = (src_r * src_alpha + bg_r * inv) / 255;
    let g = (src_g * src_alpha + bg_g * inv) / 255;
    let b = (src_b * src_alpha + bg_b * inv) / 255;
    0xFF_00_00_00 | (r << 16) | (g << 8) | b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_for_file_returns_folder_closed_for_unexpanded_folder() {
        assert_eq!(
            icon_for_file("aios.std/Folder@1", "docs", "", false),
            IconKind::FolderClosed
        );
    }

    #[test]
    fn icon_for_file_returns_folder_open_for_expanded_folder() {
        assert_eq!(
            icon_for_file("aios.std/Folder@1", "docs", "", true),
            IconKind::FolderOpen
        );
    }

    #[test]
    fn icon_for_file_returns_markdown_for_md_extension() {
        assert_eq!(
            icon_for_file("aios.std/File@1", "README.md", "text/markdown", false),
            IconKind::Markdown
        );
    }

    #[test]
    fn icon_for_file_returns_code_for_rs_extension() {
        assert_eq!(
            icon_for_file("aios.std/File@1", "main.rs", "text/rust", false),
            IconKind::Code
        );
    }

    #[test]
    fn icon_for_file_returns_config_for_toml_extension() {
        assert_eq!(
            icon_for_file("aios.std/File@1", "Cargo.toml", "text/plain", false),
            IconKind::Config
        );
    }

    #[test]
    fn icon_for_file_returns_dotfile_for_env() {
        assert_eq!(
            icon_for_file("aios.std/File@1", ".env", "text/plain", false),
            IconKind::Dotfile
        );
        assert_eq!(
            icon_for_file("aios.std/File@1", ".gitignore", "text/plain", false),
            IconKind::Dotfile
        );
    }

    #[test]
    fn icon_for_file_returns_image_for_png_extension() {
        assert_eq!(
            icon_for_file("aios.std/File@1", "photo.png", "image/png", false),
            IconKind::Image
        );
    }

    #[test]
    fn icon_for_file_returns_archive_for_zip_extension() {
        assert_eq!(
            icon_for_file("aios.std/File@1", "data.zip", "application/zip", false),
            IconKind::Archive
        );
    }

    #[test]
    fn icon_for_file_returns_text_for_txt_extension() {
        assert_eq!(
            icon_for_file("aios.std/File@1", "notes.txt", "text/plain", false),
            IconKind::Text
        );
    }

    #[test]
    fn icon_for_file_returns_generic_for_unknown_extension() {
        assert_eq!(
            icon_for_file("aios.std/File@1", "weird.xyz", "application/octet-stream", false),
            IconKind::Generic
        );
    }

    #[test]
    fn decode_all_icons_succeeds() {
        let cache = IconCache::build();
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
            let pixels = cache.get(kind);
            // 빈 [0u32; 256] fallback이 발동했으면 모든 픽셀 0 — 검출.
            let any_nonzero = pixels.iter().any(|&p| p != 0);
            assert!(any_nonzero, "{:?} 아이콘이 빈 fallback — PNG decode 실패 또는 자산 누락", kind);
        }
    }
}
```

- [ ] **Step 2: `compositor/src/lib.rs`에 `pub mod icons;` 추가.** 알파벳 순 또는 기존 패턴 (hit_test/layout/messages 옆 자연 위치).

- [ ] **Step 3: 빌드 + 테스트.**
```powershell
cargo build -p geulos-compositor
cargo test -p geulos-compositor --lib icons
```
Expected: 11 tests pass (10 라우팅 + 1 decode).

만약 `decode_all_icons_succeeds`가 실패하면 — T-icon.1의 PNG 자산이 *16x16 아님* 또는 *잘못 저장됨*. 자산 검증 필요.

- [ ] **Step 4: 전체 회귀.**
```powershell
cargo test --all
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```
모두 클린.

- [ ] **Step 5: Commit.**
```bash
git add compositor/src/icons.rs compositor/src/lib.rs
git commit -m "feat(compositor): T-icon.2 — icons.rs (IconKind + icon_for_file + cache + 11 tests)"
```
끝에:
```
Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
```

### 디자인 결정
- `OnceLock` (Rust 1.70+) — 매 시작 1회 decode, thread-safe.
- PNG decode 실패 시 *빈 [0; 256] fallback + eprintln* — 컴포지터 크래시 방지. 테스트가 fallback 검출.
- alpha blend는 표준 src-over. 16x16 = 256 픽셀, 비용 무시.

---

## Task T-icon.3 — render.rs Folder/File 분기 통합

**Files:**
- Modify: `compositor/src/render.rs`

### 단계

- [ ] **Step 1: 기존 render.rs의 `aios.std/Folder@1` 분기 read.**
현재 (대략):
```rust
"aios.std/Folder@1" => {
    let is_sel = selected_id == Some(id);
    if is_sel {
        fill_rect(buffer, width, height, &rect, COLOR_SELECTED_BG);
    }
    let name = obj.props.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    let prefix = if is_folder_expanded(tree, id) { "[-]" } else { "[+]" };
    let label = format!("{} {}", prefix, name);
    draw_text(buffer, width, height, &label, rect.x + 4, rect.y + 6, COLOR_FOLDER_TEXT);
    draw_ai_dot_if_recent(buffer, width, height, &rect, obj, now_ms);
}
```

새 (아이콘 추가):
```rust
"aios.std/Folder@1" => {
    let is_sel = selected_id == Some(id);
    if is_sel {
        fill_rect(buffer, width, height, &rect, COLOR_SELECTED_BG);
    }
    let name = obj.props.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    let is_expanded = is_folder_expanded(tree, id);

    // FileTree (rect.x < ft_threshold) vs Explorer 영역 구분 — Explorer는 `[+]`/`[-]` 없음.
    // ft_threshold는 layout의 left_w (window_w * 0.25) — render에는 buffer 폭(width) 사용.
    let ft_threshold = (width as f32 * 0.25) as i32;
    let in_filetree = rect.x < ft_threshold;

    let icon = crate::icons::icon_for_file("aios.std/Folder@1", name, "", is_expanded);
    if in_filetree {
        // [+]/[-] prefix 그대로 + icon + name
        let prefix = if is_expanded { "[-]" } else { "[+]" };
        draw_text(buffer, width, height, prefix, rect.x + 4, rect.y + 6, COLOR_FOLDER_TEXT);
        // 36px = ExpandToggle hit rect 폭 (UX fix T8.6) — 그 뒤 4px 여백 + icon
        crate::icons::blit_icon_at(buffer, width, height, rect.x + 40, rect.y + 6, icon);
        // 텍스트 시작 — icon 16px + 4px 여백
        draw_text(buffer, width, height, name, rect.x + 60, rect.y + 6, COLOR_FOLDER_TEXT);
    } else {
        // Explorer — prefix 없음. icon + name.
        crate::icons::blit_icon_at(buffer, width, height, rect.x + 4, rect.y + 4, icon);
        draw_text(buffer, width, height, name, rect.x + 24, rect.y + 6, COLOR_FOLDER_TEXT);
    }
    draw_ai_dot_if_recent(buffer, width, height, &rect, obj, now_ms);
}
```

- [ ] **Step 2: `aios.std/File@1` 분기도 유사 처리.**

기존:
```rust
"aios.std/File@1" => {
    let is_sel = selected_id == Some(id);
    if is_sel {
        fill_rect(buffer, width, height, &rect, COLOR_SELECTED_BG);
    }
    let name = obj.props.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    let label = format!("  {}", name);
    draw_text(buffer, width, height, &label, rect.x + 4, rect.y + 4, COLOR_FILE_TEXT);
    draw_ai_dot_if_recent(buffer, width, height, &rect, obj, now_ms);
}
```

새:
```rust
"aios.std/File@1" => {
    let is_sel = selected_id == Some(id);
    if is_sel {
        fill_rect(buffer, width, height, &rect, COLOR_SELECTED_BG);
    }
    let name = obj.props.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    let mime = obj.props.get("mime").and_then(|v| v.as_str()).unwrap_or("application/octet-stream");
    let icon = crate::icons::icon_for_file("aios.std/File@1", name, mime, false);

    // FileTree는 File 노드 *layout에서 skip* (T8.4 layout_tree_node_folders_only) —
    // 즉 File 분기에 도달하면 Explorer 영역 또는 echo-app 호환. ft_threshold 분기 불필요.
    crate::icons::blit_icon_at(buffer, width, height, rect.x + 4, rect.y + 4, icon);
    draw_text(buffer, width, height, name, rect.x + 24, rect.y + 4, COLOR_FILE_TEXT);
    draw_ai_dot_if_recent(buffer, width, height, &rect, obj, now_ms);
}
```

- [ ] **Step 3: 빌드 + 테스트.**
```powershell
cargo build -p geulos-compositor
cargo test --all
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```
모두 클린. 기존 layout/render 단위 테스트 회귀 X (rect 변경 없음, 텍스트 좌표만 shift).

- [ ] **Step 4: Commit.**
```bash
git add compositor/src/render.rs
git commit -m "feat(compositor): T-icon.3 — Folder/File 분기에 icon blit + 텍스트 x shift"
```
끝에:
```
Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
```

### 디자인 결정
- FileTree vs Explorer 분기 — `rect.x < width * 0.25` 휴리스틱 (layout의 left_w와 일관)
- File 분기는 *Explorer 전용* 가정 (FileTree는 folders_only로 File skip — T8.4)
- `[+]`/`[-]` 텍스트는 *그대로 유지* (UX 익숙성)
- 텍스트 y 좌표는 기존 그대로 — 아이콘 y는 +2 또는 +4 padding으로 시각 균형

---

## Task T-icon.4 — Acceptance (시각 검증)

**Files:**
- Create: `docs/manual-tests/icons-acceptance.md`

### 단계

- [ ] **Step 1: acceptance 문서 작성.**

```markdown
# 아이콘 Acceptance (ADR-034)

**Spec:** `docs/specs/2026-05-20-geulos-icons.md`
**Plan:** `docs/plans/2026-05-22-geulos-icons.md`

## 사전 조건
- 3 프로세스 (server-host → desktop-shell → compositor)
- AI 무관

## 시나리오 A — 폴더 아이콘
1. 컴포지터 띄움 → 좌측 트리 `[+] {folder-closed icon} C:\` 식 표시
2. `[+]` 클릭 → expand → `[-] {folder-open icon} C:\` (closed → open 전환)
3. 다시 클릭 → collapse → folder-closed 복귀

## 시나리오 B — 파일 타입별 아이콘 (우측 Explorer)
4. `D:\GeulOS\docs` navigate → 우측 list에 각 .md 파일 옆 markdown 아이콘
5. `D:\GeulOS\src` navigate (.rs 파일들) → code 아이콘
6. `D:\GeulOS\Cargo.toml` 같은 경로 → config 아이콘
7. `D:\GeulOS\compositor\icons` navigate → png 파일들 → image 아이콘
8. `.env`/`.gitignore` 등 → dotfile 아이콘
9. README/LICENSE 등 → text 또는 generic

## 시나리오 C — 미지원 / 기본값
10. `.png` 같은 binary mime 파일 → image 아이콘 (icon은 표시, viewer는 미지원)
11. 확장자 없는 파일 (예: `Makefile`) → text 아이콘 (T8.19 guess_mime이 text/plain 반환)
12. 알 수 없는 확장자 (`.xyz`) → generic 아이콘

## 통과 조건
- 시나리오 A/B/C 모두 *시각적으로* 정확한 아이콘 등장
- 아이콘 클릭 시 *기존 동작* (폴더 navigate, 파일 open Window) 회귀 없음
- 텍스트 좌표 shift로 *글자가 안 잘리는지* 확인 (좁은 폭에서)
- Window 본문 viewer/스크롤은 *영향 없음*

## 알려진 한계 (v2)
- Window title bar 아이콘 없음
- 다크 모드 단일 셋
- 사용자 커스텀 X
```

- [ ] **Step 2:** Commit.
```bash
git add docs/manual-tests/icons-acceptance.md
git commit -m "test(icons): T-icon.4 — acceptance 시나리오 3종 (A/B/C)"
```
끝에:
```
Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
```

- [ ] **Step 3: 시각 검증은 controller가 별도 spawn 단계.** Implementer는 문서만.

---

## Task T-icon.5 — spec/quality review + push 준비

**Files:** (없음 — review만)

### 단계

- [ ] **Step 1:** spec compliance reviewer 디스패치 (subagent-driven-development의 spec-reviewer-prompt.md).

- [ ] **Step 2:** code quality reviewer 디스패치.

- [ ] **Step 3:** 발견된 issues fix (있다면).

- [ ] **Step 4:** controller가 push (3~4 commits — T-icon.1~T-icon.4).

---

## 자체 점검

### 스펙 커버리지
| Spec 섹션 | 커버 task |
|---|---|
| §4 9종 매핑 | T-icon.1 (PNG 자산) + T-icon.2 (라우팅) |
| §5.1 Lucide PNG 소스 | T-icon.1 |
| §5.2 16x16 | T-icon.1 (자산 크기) + T-icon.2 (ICON_SIZE 상수) |
| §5.3 위치 (FileTree/Explorer) | T-icon.3 |
| §5.4 라우팅 9단계 | T-icon.2 (icon_for_file) |
| §5.5 OnceLock cache | T-icon.2 (ICON_CACHE static) |
| §5.6 alpha blend | T-icon.2 (blit_icon_at + blend_argb) |
| §6.1 신규 icons.rs | T-icon.2 |
| §6.2 신규 PNG 자산 | T-icon.1 |
| §6.3 render.rs Folder/File 분기 | T-icon.3 |
| §6.4 layout.rs 변경 없음 | T-icon.3 (확인) |
| §6.5 image dep | T-icon.1 |
| §7 단위 테스트 11건 | T-icon.2 |
| §8 ADR-034 | T-icon.1 |
| §9 sub-task 5개 | T-icon.1~T-icon.5 ✅ |

### Placeholder scan
- 모든 step에 actual code 또는 정확한 명령. TBD/TODO 없음.
- T-icon.1 Step 2는 PNG 자산 *수급 방법 2개 옵션* 명시 — implementer 선택. placeholder 아니라 *판단 영역*.

### Type 일관성
- `IconKind` (T-icon.2) — T-icon.3에서 매개 그대로.
- `icon_for_file(type_uri, name, mime, is_expanded) -> IconKind` (T-icon.2) — T-icon.3 호출 시그니처 일치.
- `blit_icon_at(buffer, w, h, x, y, kind)` (T-icon.2) — T-icon.3 호출 일치.
- `ICON_SIZE = 16` 상수 (T-icon.2) — T-icon.3 좌표 계산이 같은 값 사용.

### 위험
- T-icon.1 PNG 자산 *수급*이 implementer 환경에 의존 (npm 또는 SVG 변환 도구). 실패 시 임시 *간단한 16x16 raster 직접 픽셀 그림*도 OK — `decode_all_icons_succeeds` 테스트가 모든 픽셀 0이 아닌지 검증해 *완전 누락*만 잡음.

---

## 다음 단계

1. Spec ✅ + Plan ✅
2. **이 plan 진입** — subagent-driven-development로 T-icon.1부터
3. T-icon.4 acceptance 후 controller 시각 검증
4. T-icon.5 review + push
5. 그 다음 M9 (권한 다이얼로그 + 편집·저장) brainstorm
