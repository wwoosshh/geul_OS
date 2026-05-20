# GeulOS 아이콘 — 파일·폴더 시각 구분 (M8 후속)

**Date:** 2026-05-20
**Status:** Design 승인됨, plan 작성 대기
**Author:** wwoosshh + Claude controller session
**Parent:** M8 (`docs/specs/2026-05-18-geulos-m8-multi-window-explorer.md` + `docs/specs/2026-05-20-geulos-m8-notepad-viewer-scroll.md`) 마감 후 별 task

---

## 1. 한 줄 요약

좌측 FileTree와 우측 Explorer의 폴더·파일 행에 *16x16 PNG raster 아이콘*을 추가해 파일 타입을 한눈에 구분. v1은 9종 (Lucide MIT). 편집·저장은 무관 (M9), 다크 모드/사용자 커스텀은 v2.

---

## 2. Motivation (사용자 결정 — 2026-05-20)

T8.20까지 사용자 보고:
> "어떤게 파일이고 어떤게 폴더인지 아이콘이미지가 없어서 보기불편한점"

M8 part 2 (메모장 viewer + 스크롤) 마무리 후 *별 task*로 추진. UX 보강.

---

## 3. Scope

### In scope
- 16x16 PNG raster 아이콘 9종 (Lucide MIT 출처)
- mime/확장자 기반 라우팅 (`icon_for_file` helper)
- 좌측 FileTree와 우측 Explorer 행에 아이콘 blit
- 폴더는 expanded 상태에 따라 closed/open 변형
- LazyLock 또는 OnceCell 기반 시작 시 1회 decode + 캐시

### Out of scope (v2 또는 후속)
- **Window title bar 아이콘** — v2 (현재 title bar는 텍스트만)
- **다크 모드** — 단일 light-bg 셋. v2.
- **사용자 커스텀 아이콘** — 무관. v2.
- **CLI 패널 아이콘** — Cli는 텍스트 viewer라 무관
- **Drag preview 아이콘** — drag 자체가 시각 피드백 없음 (M8 v1)
- **vector resize** — 16x16 raster 고정. window resize 비례 X.

---

## 4. 아이콘 셋 (9종)

| IconKind | Lucide 매핑 | 사용 케이스 |
|---|---|---|
| `FolderClosed` | `folder` | `aios.std/Folder@1`, `state.expanded`에 없음 |
| `FolderOpen` | `folder-open` | `Folder@1`, `state.expanded`에 있음 |
| `Markdown` | `file-text` (또는 `book-open`) | `.md`, `.markdown` |
| `Code` | `code` | `.rs`, `.py`, `.js`, `.ts`, `.html`, `.css` |
| `Config` | `settings` (또는 `file-cog`) | `.toml`, `.yaml`, `.yml`, `.json` |
| `Text` | `file-text` | `.txt`, `.log`, `.ini`, `.cfg` |
| `Image` | `image` | `.png`, `.jpg`, `.gif`, `.svg`, `.webp` |
| `Archive` | `package` | `.zip`, `.tar`, `.gz`, `.7z`, `.rar` |
| `Dotfile` | `key-round` | `.env`, `.gitignore`, `.editorconfig`, T8.19 화이트리스트 |
| `Generic` | `file` | 기타 — viewer가 binary 또는 unknown |

(9종 + `Markdown`/`Text`이 같은 Lucide 글리프 사용 가능 — 실제 PNG는 9개로 정리.)

**라이센스:** Lucide MIT — repo에 `LICENSE` 사본 또는 `compositor/icons/LICENSE-LUCIDE` 둠.

---

## 5. 디자인 결정

### 5.1 아이콘 소스 — Lucide PNG 사전 변환

- Lucide의 SVG 자산을 *implementer가 사전 변환* (Inkscape 또는 ImageMagick 등)해 16x16 RGBA PNG로.
- 또는 `lucide-static` npm 패키지의 사전 변환 raster 활용.
- 빌드 시점 SVG 렌더 (resvg crate 등) 안 함 — *정적 자산*.

### 5.2 크기 — 16x16

- FileTree 행 28px, Explorer 행 24px — 16x16 아이콘은 양쪽에 안전 (수직 padding 4-6px).
- 향후 다른 크기 (20x20, 24x24)는 v2. v1 단일 셋.

### 5.3 위치

**좌측 FileTree:**
```
[+] {icon} foldername
^   ^      ^
|   |      └ 텍스트 시작 (x + 36 + 4 + 16 + 4 = x + 60)
|   └ 16x16 아이콘 (x + 36 + 4 = x + 40)
└ ExpandToggle hit rect (36px 폭)
```

**우측 Explorer:**
```
{icon} item_name
^      ^
|      └ 텍스트 시작 (x + 4 + 16 + 4 = x + 24)
└ 16x16 아이콘 (x + 4)
```

**Hit rect 영향:**
- FileTree의 ExpandToggle 영역 (36px) 그대로 — 아이콘은 *Body 영역 안*
- Explorer의 Body rect 그대로 — 아이콘 위 클릭 = 파일/폴더 클릭

### 5.4 라우팅 — `icon_for_file`

```rust
pub fn icon_for_file(
    type_uri: &str,        // "aios.std/Folder@1" or "aios.std/File@1"
    name: &str,             // 파일/폴더명 (확장자 또는 dotfile 매칭)
    mime: &str,             // T8.14의 mime
    is_expanded: bool,      // folder만 의미 — file은 false
) -> IconKind
```

라우팅 순서:
1. type_uri = `Folder@1` → `is_expanded` ? `FolderOpen` : `FolderClosed`
2. T8.19 dotfile 화이트리스트 (`.env`, `.gitignore` 등) → `Dotfile`
3. mime이 `text/markdown` → `Markdown`
4. 확장자 별 — `.rs`/`.py`/`.js`/`.ts`/`.html`/`.css` → `Code`
5. 확장자 별 — `.toml`/`.yaml`/`.yml`/`.json` → `Config`
6. 확장자 별 — `.png`/`.jpg`/`.gif`/`.svg`/`.webp` → `Image`
7. 확장자 별 — `.zip`/`.tar`/`.gz`/`.7z`/`.rar` → `Archive`
8. mime이 `text/*` → `Text`
9. 기타 → `Generic`

### 5.5 디코드 + 캐시

- `OnceCell<IconCache>` 정적 — 시작 시 1회 decode
- `IconCache` = `HashMap<IconKind, Vec<u32>>` (16x16 = 256 RGBA u32)
- `include_bytes!`로 PNG 자산 9개 embed
- `image` crate가 PNG decode → RGBA → ARGB u32 변환

### 5.6 alpha blend

PNG는 alpha 채널 있음. softbuffer는 ARGB. blend:
```rust
fn blit_icon_at(buffer: &mut [u32], buf_w: usize, buf_h: usize, x: i32, y: i32, icon: &[u32]) {
    for iy in 0..16 {
        for ix in 0..16 {
            let px = icon[iy * 16 + ix];
            let alpha = ((px >> 24) & 0xFF) as u32;
            if alpha == 0 { continue; }  // 투명 픽셀 skip
            let target_x = x + ix as i32;
            let target_y = y + iy as i32;
            if target_x < 0 || target_y < 0 || target_x >= buf_w as i32 || target_y >= buf_h as i32 {
                continue;
            }
            let idx = target_y as usize * buf_w + target_x as usize;
            if alpha == 0xFF {
                buffer[idx] = px;  // opaque — 그대로
            } else {
                // 간단 alpha blend (배경과 mix)
                let bg = buffer[idx];
                buffer[idx] = blend_argb(bg, px, alpha);
            }
        }
    }
}
```

`blend_argb` 헬퍼 — 표준 src-over composition.

---

## 6. 컴포지터 변경 (4 파일)

### 6.1 신규 `compositor/src/icons.rs`
- `IconKind` enum (9 variant)
- `IconCache` struct (HashMap)
- `init_icon_cache()` LazyLock — 시작 시 9 PNG decode
- `icon_for_file(type_uri, name, mime, is_expanded) -> IconKind` 라우팅
- `get_icon_pixels(kind: IconKind) -> &[u32; 256]`
- `blit_icon_at(buffer, w, h, x, y, kind: IconKind)` 헬퍼

### 6.2 신규 `compositor/icons/*.png` (9 자산)
- `folder-closed.png`
- `folder-open.png`
- `markdown.png`
- `code.png`
- `config.png`
- `text.png`
- `image.png`
- `archive.png`
- `dotfile.png`
- `generic.png`
- `LICENSE-LUCIDE` (또는 `LICENSE.txt`)

### 6.3 수정 `compositor/src/render.rs`
- `Folder@1` 분기에서:
  - 텍스트 그리기 *전에* `blit_icon_at(buffer, w, h, rect.x + 36 + 4, rect.y + 6, icon_kind)` (FolderClosed 또는 Open)
  - 텍스트 시작 x를 `rect.x + 60`으로 (기존 `rect.x + 4`에서 +56 shift)
- `File@1` 분기에서:
  - 텍스트 그리기 *전에* `blit_icon_at(buffer, w, h, rect.x + 4, rect.y + 4, icon_kind)`
  - 텍스트 시작 x를 `rect.x + 24`로 (기존 `rect.x + 4`에서 +20 shift)
- `_ = ` 매개 갱신 — `icon_for_file` 호출 시 type_uri/name/mime/is_expanded 전달

### 6.4 수정 `compositor/src/layout.rs`
- *변경 없음* — rect의 width는 그대로, 텍스트 시작 위치만 render에서 조정.
- 단 hit_test/ExpandToggle 영역 (36px)은 보존 — 아이콘 위 클릭은 Body 영역 (toggle 아님).

### 6.5 수정 `compositor/Cargo.toml`
- `image = { version = "0.25", default-features = false, features = ["png"] }` 추가
- default-features off — 다른 file format 불필요. binary size 절감.

---

## 7. 단위 테스트

`compositor/src/icons.rs` `#[cfg(test)] mod tests`:
1. `icon_for_file_returns_folder_closed_for_unexpanded_folder`
2. `icon_for_file_returns_folder_open_for_expanded_folder`
3. `icon_for_file_returns_markdown_for_md_extension`
4. `icon_for_file_returns_code_for_rs_extension`
5. `icon_for_file_returns_config_for_toml_extension`
6. `icon_for_file_returns_dotfile_for_env`
7. `icon_for_file_returns_image_for_png_extension`
8. `icon_for_file_returns_archive_for_zip_extension`
9. `icon_for_file_returns_text_for_txt_extension`
10. `icon_for_file_returns_generic_for_unknown_extension`
11. `decode_all_icons_succeeds` — 9 PNG 모두 16x16 decode 통과

(라우팅 순서 일관성도 1-10에서 cover. 11은 자산 검증.)

---

## 8. ADR 시드

- **ADR-034 — 파일·폴더 아이콘 (Lucide 16x16 PNG, type-aware)**. PNG raster vs Unicode glyph vs 직접 raster의 trade-off. Lucide MIT 출처. 9종 매핑. v2: dark mode/title bar/custom/vector.

---

## 9. Sub-task 분해 (3~5일)

| Task | 주제 | 추정 |
|---|---|---|
| T-icon.1 | ADR-034 + Lucide PNG 자산 9개 (compositor/icons/) + image dep + LICENSE | 1일 |
| T-icon.2 | icons.rs — IconKind + icon_for_file 라우팅 + decode cache + 11 단위 테스트 | 1일 |
| T-icon.3 | render.rs Folder/File 분기 — blit_icon_at + 텍스트 x shift | 1일 |
| T-icon.4 | acceptance 시각 검증 + (선택) 후속 fix | 0.5일 |
| T-icon.5 | spec/quality review + commit cleanup | 0.5일 |

---

## 10. 자체 점검

### 스펙 커버리지 (사용자 요청)
- "어떤게 파일이고 어떤게 폴더인지" → `FolderClosed`/`FolderOpen` vs file 아이콘 ✅
- "아이콘이미지가 없어서 보기불편한점" → 9 raster 아이콘 + alpha blend ✅

### Tradeoff 위험

| 위험 | 완충 |
|---|---|
| Lucide PNG 자산 수급 (16x16 직접 변환 필요) | implementer가 SVG→PNG 변환 또는 lucide-static 활용. 단일 1회 작업 |
| image crate binary size 증가 | default-features off + png only. ~200KB 증가 추정 |
| 16x16이 한국어 폰트 18pt 옆에서 작아 보일 수 있음 | 20x20 또는 24x24 v2 검토 |
| dark 배경 아이콘이 light 배경에 안 어울림 | Lucide는 light bg 기본. 다크 모드 v2에 별 셋 |
| alpha blend 비용 매 redraw | 16*16=256 픽셀 * 행 수. 행 수 ~50 가시 → 12800 픽셀/redraw. 무시 가능 |

### 의도된 design 갭
- *드래그 시 ghost 아이콘* — drag 자체가 v1 시각 피드백 없음
- *애니메이션* (folder open/close transition) — v2
- *컴팩트/spacious 모드* — v2

---

## 11. 다음 단계

1. Spec 사용자 review
2. `writing-plans`로 plan 작성 (T-icon.1~5)
3. subagent-driven-development로 task별 implementer
4. T-icon.4 acceptance 시각 검증 — 사용자에게 *각 mime별 정확한 아이콘 등장* 확인
5. M9 (권한 다이얼로그 + 편집·저장)로 이어짐
