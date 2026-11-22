#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["fonttools>=4.50"]
# ///
"""Build the bundled Noto Sans JP subsets used by the GUI.

The GUI must not depend on system fonts, so two subset fonts are committed:

* ``NotoSansJP-Regular.otf``: ASCII, common punctuation, JIS X 0208 level 1+2
  (6879 characters), and every non-ASCII character used by the UI sources.
  Profile and macro names are user data, so the JIS set is included to render
  ordinary Japanese names without escaping.
* ``NotoSansJP-Bold.otf``: only the ASCII/punctuation basics and the characters
  the UI itself uses. It is used for headings and section labels.

Source font: ``Noto Sans CJK JP`` (regular/bold TTC face 0), SIL Open Font
License 1.1.  ``OFL.txt`` in this directory is the license text.

Run from anywhere:

    uv run crates/eightbitdo-gui/assets/fonts/build_fonts.py

Set ``NOTO_CJK_DIR`` if the Noto CJK TTC files live outside
``/usr/share/fonts/noto-cjk``.  The generated files are committed; CI does not
need fontTools or the source fonts.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTCollection

HERE = Path(__file__).resolve().parent
CRATE = HERE.parents[1]
SRC = CRATE / "src"
FONT_SOURCE = Path(os.environ.get("NOTO_CJK_DIR", "/usr/share/fonts/noto-cjk"))

# Ranges that the UI or Western text can reasonably need.  CJK punctuation,
# kana, half/full-width forms and general punctuation are all included because
# they appear in localized labels and user-entered names.
BASE_RANGES = [
    (0x0020, 0x007E),  # ASCII printable
    (0x00A0, 0x00FF),  # Latin-1 supplement
    (0x2010, 0x2030),  # dashes, quotes, ellipsis
    (0x2000, 0x206F),  # general punctuation
    (0x2190, 0x21FF),  # arrows
    (0x2460, 0x24FF),  # enclosed alphanumerics
    (0x25A0, 0x25FF),  # geometric shapes
    (0x3000, 0x303F),  # CJK symbols and punctuation
    (0x3041, 0x3096),  # hiragana
    (0x30A1, 0x30FF),  # katakana
    (0xFF00, 0xFFEF),  # half/full-width forms
]

# The subsetter drops vertical layout and legacy tables; egui/ab_glyph only
# needs horizontal metrics.  Dropping GPOS/GSUB saves about 2 MB.
DROP_TABLES = ["DSIG", "vmtx", "VORG", "vhea"]


def base_codes() -> set[int]:
    codes: set[int] = set()
    for start, end in BASE_RANGES:
        codes.update(range(start, end + 1))
    return codes


def jis_x_0208_codes() -> set[int]:
    """Every character in JIS X 0208 level 1+2, decoded through EUC-JP."""
    codes: set[int] = set()
    for high in range(0xA1, 0xFF):
        for low in range(0xA1, 0xFF):
            try:
                character = bytes([high, low]).decode("euc_jp")
            except UnicodeDecodeError:
                continue
            codes.add(ord(character))
    return codes


def ui_codes() -> set[int]:
    """ASCII plus every non-ASCII character in the GUI crate sources."""
    codes = base_codes()
    for path in sorted(SRC.glob("*.rs")):
        text = path.read_text(encoding="utf-8")
        codes.update(ord(character) for character in text if ord(character) > 0x7F)
    return codes


def subset_face(weight: str, codes: set[int], destination: Path) -> None:
    source = FONT_SOURCE / f"NotoSansCJK-{weight}.ttc"
    if not source.is_file():
        sys.exit(f"missing source font: {source} (set NOTO_CJK_DIR)")

    collection = TTCollection(str(source), lazy=True)
    face = collection.fonts[0]  # face 0 is "Noto Sans CJK JP"

    options = subset.Options()
    options.hinting = False
    options.name_IDs = ["*"]
    options.name_legacy = True
    options.name_languages = ["en", "ja"]
    options.notdef_outline = True
    options.layout_features = ["kern"]
    options.drop_tables += DROP_TABLES

    subsetter = subset.Subsetter(options=options)
    subsetter.populate(unicodes=codes)
    subsetter.subset(face)
    face.save(str(destination))

    size = destination.stat().st_size
    glyphs = len(face.getGlyphOrder())
    print(f"{destination.name}: {glyphs} glyphs, {size / 1024 / 1024:.2f} MiB")


def main() -> None:
    regular_codes = ui_codes() | jis_x_0208_codes()
    bold_codes = ui_codes()
    subset_face("Regular", regular_codes, HERE / "NotoSansJP-Regular.otf")
    subset_face("Bold", bold_codes, HERE / "NotoSansJP-Bold.otf")


if __name__ == "__main__":
    main()
