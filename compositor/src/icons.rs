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
        ".env" | ".envrc" | ".gitignore" | ".gitattributes" | ".dockerignore" | ".editorconfig"
        | ".prettierrc" | ".eslintrc" => return IconKind::Dotfile,
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

/// SP1 크롬(Dock/DesktopIcon)의 `icon` 문자열 → IconKind 라우팅.
///
/// DesktopIcon.props.icon / Dock.items[].icon은 자유 문자열("folder","file_manager" 등)이라
/// `icon_for_file`(파일 mime/확장자 기반)과 다른 단순 화이트리스트로 매핑한다. 보유 자산(9종)
/// 안에서만 매핑하고 미지정/미지원은 Generic으로 폴백 — 크롬이 빈 아이콘으로 깨지지 않게.
pub fn icon_kind_for_name(name: &str) -> IconKind {
    match name {
        "folder" | "folder-closed" | "file_manager" | "files" => IconKind::FolderClosed,
        "folder-open" => IconKind::FolderOpen,
        "markdown" | "md" => IconKind::Markdown,
        "code" | "terminal" | "cli" => IconKind::Code,
        "config" | "settings" => IconKind::Config,
        "text" | "notepad" | "memo" => IconKind::Text,
        "image" => IconKind::Image,
        "archive" => IconKind::Archive,
        "dotfile" => IconKind::Dotfile,
        _ => IconKind::Generic,
    }
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
            let alpha = (src >> 24) & 0xFF;
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

/// 16×16 아이콘을 nearest-neighbor로 `size`×`size` 픽셀로 확대하여 blit.
///
/// 바탕화면 아이콘(`DesktopIcon@1`)처럼 큰 표시가 필요할 때 사용. 화면 경계 밖 픽셀은 skip.
/// `size`가 0이면 no-op.
pub fn blit_icon_scaled(
    buffer: &mut [u32],
    buf_w: usize,
    buf_h: usize,
    x: i32,
    y: i32,
    kind: IconKind,
    size: i32,
) {
    if size <= 0 {
        return;
    }
    let pixels = icon_cache().get(kind);
    let sz = size as usize;
    for dy in 0..sz {
        for dx in 0..sz {
            // nearest-neighbor: 소스 좌표 = (dx * ICON_SIZE / sz, dy * ICON_SIZE / sz)
            let src_x = dx * ICON_SIZE / sz;
            let src_y = dy * ICON_SIZE / sz;
            let src = pixels[src_y * ICON_SIZE + src_x];
            let alpha = (src >> 24) & 0xFF;
            if alpha == 0 {
                continue;
            }
            let tx = x + dx as i32;
            let ty = y + dy as i32;
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

/// `blit_icon_scaled` 렌더 수학 테스트용: 소스 픽셀 인덱스 계산 헬퍼.
/// `blit_icon_scaled`와 동일한 nearest-neighbor 공식.
#[cfg(test)]
pub fn scaled_src_idx(dst_x: usize, dst_y: usize, size: usize) -> usize {
    let src_x = dst_x * ICON_SIZE / size;
    let src_y = dst_y * ICON_SIZE / size;
    src_y * ICON_SIZE + src_x
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
        assert_eq!(icon_for_file("aios.std/Folder@1", "docs", "", false), IconKind::FolderClosed);
    }

    #[test]
    fn icon_for_file_returns_folder_open_for_expanded_folder() {
        assert_eq!(icon_for_file("aios.std/Folder@1", "docs", "", true), IconKind::FolderOpen);
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
        assert_eq!(icon_for_file("aios.std/File@1", "main.rs", "text/rust", false), IconKind::Code);
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
    fn icon_kind_for_name_maps_chrome_names() {
        assert_eq!(icon_kind_for_name("folder"), IconKind::FolderClosed);
        assert_eq!(icon_kind_for_name("file_manager"), IconKind::FolderClosed);
        assert_eq!(icon_kind_for_name("settings"), IconKind::Config);
        assert_eq!(icon_kind_for_name("terminal"), IconKind::Code);
        // 미지정/미지원 → Generic 폴백 (빈 아이콘으로 깨지지 않게).
        assert_eq!(icon_kind_for_name(""), IconKind::Generic);
        assert_eq!(icon_kind_for_name("nonexistent"), IconKind::Generic);
    }

    #[test]
    fn scaled_src_idx_nearest_neighbor_math() {
        // size=32이면 각 소스 픽셀이 2×2 출력 픽셀로 확대.
        // dst (0,0) → src (0,0), dst (1,0) → src (0,0) (같은 소스 블록).
        // dst (2,0) → src (1,0), dst (31,31) → src (15,15).
        assert_eq!(scaled_src_idx(0, 0, 32), 0);
        assert_eq!(scaled_src_idx(1, 0, 32), 0);
        assert_eq!(scaled_src_idx(2, 0, 32), 1);
        assert_eq!(scaled_src_idx(31, 31, 32), 15 * ICON_SIZE + 15);
    }

    #[test]
    fn scaled_src_idx_identity_at_16() {
        // size=16이면 1:1 — dst (x, y) → src (x, y).
        for y in 0..ICON_SIZE {
            for x in 0..ICON_SIZE {
                assert_eq!(scaled_src_idx(x, y, ICON_SIZE), y * ICON_SIZE + x);
            }
        }
    }

    #[test]
    fn blit_icon_scaled_zero_size_no_panic() {
        // size=0이면 no-op (panic 없음).
        let mut buf = vec![0u32; 100 * 100];
        blit_icon_scaled(&mut buf, 100, 100, 10, 10, IconKind::Generic, 0);
    }

    #[test]
    fn blit_icon_scaled_paints_larger_area_than_16() {
        // 불투명 아이콘을 40×40으로 확대하면 40×40 영역 안에 비-0 픽셀이 있어야 한다.
        // Generic 아이콘은 decode_all_icons_succeeds가 비-0 픽셀을 보장하므로 여기서 사용.
        let w = 60usize;
        let h = 60usize;
        let mut buf = vec![0u32; w * h];
        blit_icon_scaled(&mut buf, w, h, 10, 10, IconKind::Generic, 40);
        // 10..50 영역 내 임의의 픽셀이 칠해졌는지 확인.
        let any_painted = buf.iter().any(|&p| p != 0);
        assert!(any_painted, "scaled blit은 최소 1개 픽셀을 칠해야 함");
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
            assert!(
                any_nonzero,
                "{:?} 아이콘이 빈 fallback — PNG decode 실패 또는 자산 누락",
                kind
            );
        }
    }
}
