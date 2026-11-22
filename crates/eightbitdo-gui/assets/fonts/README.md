# Bundled fonts

The GUI embeds three subsets of **Noto Sans CJK JP** so Japanese text renders
without depending on system fonts.

| File | Contents | Glyphs | Size |
| --- | --- | --- | --- |
| `NotoSansJP-Regular.otf` | ASCII, punctuation, JIS X 0208 level 1+2, UI strings | 7461 | 1.48 MiB |
| `NotoSansJP-Medium.otf` | ASCII, punctuation, UI strings only (section labels, controls) | 1054 | 0.11 MiB |
| `NotoSansJP-Bold.otf` | ASCII, punctuation, UI strings only (page title, strong values) | 1054 | 0.11 MiB |

- **Upstream**: Noto Sans CJK JP, version 2.004 (face 0 of
  `/usr/share/fonts/noto-cjk/NotoSansCJK-{Regular,Medium,Bold}.ttc`, package
  `fonts-noto-cjk` on Debian/Ubuntu).
- **License**: SIL Open Font License 1.1. See [`OFL.txt`](OFL.txt).
  The font name is kept as "Noto Sans CJK JP"; the reserved name "Source"
  from the upstream notice is not used.
- **Modification**: glyph subsets and removal of vertical layout tables
  (`vmtx`, `VORG`, `vhea`), hinting, and GPOS/GSUB layout features (kerning
  only). See `build_fonts.py`.

## Why JIS X 0208 level 1+2

Profile names and macro names come from the device and can contain Japanese.
Subsetting to JIS X 0208 (6879 characters) covers ordinary names while keeping
the binary small; characters outside the JIS set render as tofu. The Regular
weight is used for body text and user data. Medium is used for section labels
and controls, and Bold only for the page title and emphasized values; both are
static UI text, so they only need the UI character set.

## Regenerating

```bash
uv run crates/eightbitdo-gui/assets/fonts/build_fonts.py
```

`uv` reads the PEP 723 metadata in the script and installs `fonttools` in an
isolated environment. Set `NOTO_CJK_DIR` if the source TTC files are not in
`/usr/share/fonts/noto-cjk`. The generated `.otf` files are committed, so CI
and normal builds do **not** need fontTools or the source fonts.

egui/ab_glyph does not support WOFF2, hence the OpenType/CFF output.
