# compositor/fonts

This directory holds the embedded font used by the compositor for text rasterization.

## Required file

`font.ttf` — a TrueType font file. **Not committed** (license reasons).

## How to populate

### Windows

```powershell
Copy-Item C:\Windows\Fonts\arial.ttf compositor\fonts\font.ttf
```

Or any other `.ttf` from `C:\Windows\Fonts\`:

```powershell
Copy-Item C:\Windows\Fonts\consola.ttf compositor\fonts\font.ttf
```

### Linux

```bash
cp /usr/share/fonts/truetype/dejavu/DejaVuSans.ttf compositor/fonts/font.ttf
# or
cp /usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf compositor/fonts/font.ttf
```

### macOS

```bash
cp /Library/Fonts/Arial.ttf compositor/fonts/font.ttf
```

### Alternative: open-license font

Download [JetBrains Mono](https://www.jetbrains.com/lp/mono/) (OFL 1.1) and place
`JetBrainsMono-Regular.ttf` as `compositor/fonts/font.ttf`.

## Why embedded?

`compositor/src/text.rs` uses `include_bytes!("../fonts/font.ttf")` so the font is
baked into the binary — no runtime font-loading path needed.
