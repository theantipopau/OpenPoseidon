#!/usr/bin/env python3
"""Generate the dummy.ttf FontRenderer test fixture (FIXTURE-FONT-1).

Authored, not copied: no retail or third-party font data is used anywhere here.
Every glyph outline is an invented box; only the *coverage* mirrors what the
tests in test_font_renderer.cpp require:

- ASCII printable range 32..126 (raster/measure/layout/atlas-packing tests),
  with space (U+0020) empty but advancing,
- Czech diacritics used by the Czech sections (U+010D U+011B U+0148 U+0159
  U+0161 U+0165 U+016F U+00ED U+00FD U+017E),
- Cyrillic Zhe (U+0416) for the Russian-coverage test (must be a real glyph:
  the test asserts it does not alias the missing-glyph fallback).

Box metrics are uniform on purpose: 400-unit ink on a 600-unit advance, so
advance always exceeds ink width (the LayoutText 'oo' step test) and the left
side bearing stays small and positive (the 'j' bearing test).

Regenerate:  python tests/fixtures/font/make_dummy_ttf_fixture.py
Requires:    fontTools (pip install fonttools)

Also writes the legacy OFP bitmap-font pair the [font][parser]/[tools][font]/
[tools-preview][font] tests need (see FIXTURE-FONT-1):

- legacy.fxy: 224 flat records of 6 little-endian uint16
  (charCode, setNum, x, y, w, h); every glyph (c, 1, 0, 0, 4, 4), i.e.
  w-1 == h-1 == 3 as the tests assert (maxWidth/maxHeight 3, one set {1},
  texture legacy-01.paa, exactly 2688 bytes). Mirrors the layout
  ParseFXY reads — no retail data involved.
- legacy-01.paa: minimal decodable DXT1 4x4 (opaque-white block, so glyph
  blits land non-background pixels), same container shape as the
  tests/fixtures/paa taggs fixtures.
"""
import pathlib
import struct

HERE = pathlib.Path(__file__).resolve().parent

UPM = 1000
INK_X0, INK_Y0, INK_X1, INK_Y1 = 100, 0, 500, 700
ADVANCE = 600
SPACE_ADVANCE = 500

CZECH_EXTRA = [0x010D, 0x011B, 0x0148, 0x0159, 0x0161, 0x0165, 0x016F, 0x00ED, 0x00FD, 0x017E]
CYRILLIC_EXTRA = [0x0416]


def glyph_name(cp: int) -> str:
    if cp == 0x20:
        return "space"
    if 0x21 <= cp <= 0x7E:
        return chr(cp) if chr(cp).isalnum() else "uni%04X" % cp
    return "uni%04X" % cp


def main() -> None:
    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.ttGlyphPen import TTGlyphPen

    codepoints = list(range(32, 127)) + CZECH_EXTRA + CYRILLIC_EXTRA

    order = [".notdef"] + [glyph_name(cp) for cp in codepoints]
    cmap = {cp: glyph_name(cp) for cp in codepoints}

    glyphs = {}
    metrics = {}
    for name in order:
        pen = TTGlyphPen(None)
        if name == ".notdef":
            metrics[name] = (ADVANCE, 0)
        elif name == "space":
            metrics[name] = (SPACE_ADVANCE, 0)
        else:
            pen.moveTo((INK_X0, INK_Y0))
            pen.lineTo((INK_X1, INK_Y0))
            pen.lineTo((INK_X1, INK_Y1))
            pen.lineTo((INK_X0, INK_Y1))
            pen.closePath()
            metrics[name] = (ADVANCE, INK_X0)
        glyphs[name] = pen.glyph()

    fb = FontBuilder(UPM, isTTF=True)
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap(cmap)
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": "ProteusDummy", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    fb.setupHead()
    fb.setupMaxp()
    out = HERE / "dummy.ttf"
    fb.save(out)
    print("wrote %s: %d bytes, %d glyphs" % (out.name, out.stat().st_size, len(order)))


def make_legacy_fxy() -> None:
    out = HERE / "legacy.fxy"
    with open(out, "wb") as f:
        for i in range(224):
            f.write(struct.pack("<HHHHHH", i, 1, 0, 0, 4, 4))
    print("wrote %s: %d bytes" % (out.name, out.stat().st_size))


def make_legacy_paa() -> None:
    # Opaque-white DXT1 block: color0 white, color1 black, all indices 0.
    block = bytes.fromhex("ffff000000000000")

    def tagg(name: str, payload: bytes) -> bytes:
        return b"GGAT" + name[::-1].encode("ascii") + struct.pack("<I", len(payload)) + payload

    taggs = (
        tagg("AVGC", bytes.fromhex("fe8080ff"))
        + tagg("MAXC", bytes.fromhex("ffffffff"))
        + tagg("OFFS", bytes(64))
    )
    mip = struct.pack("<HH", 4, 4) + len(block).to_bytes(3, "little") + block
    data = struct.pack("<H", 0xFF01) + taggs + struct.pack("<H", 0) + mip + struct.pack("<HH", 0, 0)
    out = HERE / "legacy-01.paa"
    out.write_bytes(data)
    print("wrote %s: %d bytes" % (out.name, out.stat().st_size))


if __name__ == "__main__":
    main()
    make_legacy_fxy()
    make_legacy_paa()
