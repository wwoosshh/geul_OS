# compositor/fonts

This directory holds the embedded font used by the compositor for text rasterization.

## Bundled font

`NotoSansKR-Regular.otf` — **Noto Sans Korean Regular** (SubsetOTF, Korean).
한글(Hangul) + 라틴(Latin) 글리프 모두 포함. 컴포지터의 단일 폰트로 사용.

- License: SIL Open Font License 1.1 — full text in `LICENSE-NotoSansKR`.
- Source: https://github.com/notofonts/noto-cjk (`Sans/SubsetOTF/KR/NotoSansKR-Regular.otf`).
- File is committed to the repo — OFL allows redistribution as part of a larger work.

`compositor/src/text.rs` uses `include_bytes!("../fonts/NotoSansKR-Regular.otf")` so the
font is baked into the binary — no runtime font-loading path.

## Legacy `font.ttf` (optional, local-only)

이전에는 OS에서 복사한 `font.ttf`를 사용했습니다. 현재는 `NotoSansKR-Regular.otf`로
완전 대체되었고 `font.ttf`는 더 이상 빌드에 사용되지 않습니다.

`.gitignore`의 `compositor/fonts/font.ttf` 규칙은 과거 워크플로우 호환을 위해
남겨두었습니다 — 로컬에서 실험용으로 다른 폰트를 시도해도 git status를 더럽히지
않습니다. 새 워크플로우에서는 이 파일을 사용하지 않아도 됩니다.

## Why embedded?

런타임 폰트 로딩 경로(OS 의존)를 피하고, 한국어 사용자 환경(특히 Windows에서
한글 시스템 폰트 경로가 일정치 않은 케이스)에서도 동일하게 렌더되도록 보장하기 위함.
