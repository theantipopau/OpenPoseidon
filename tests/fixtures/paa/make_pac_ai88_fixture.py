#!/usr/bin/env python3
"""Generate the marker-bearing AI88 PAC fixture (FIXTURE-PAC-1).

Authored, not copied: an 8x8 intensity+coverage texture with invented pixel
data. Layout mirrors what the engine's PAA path expects (Pactext.cpp /
PAADecoder.cpp):

  u16 desc 0x8080 ......... AI88 format marker: selects PAA-style level data
                             regardless of the .pac suffix
  u16 palette count 0 ..... no palette (asserted by LoadPaaBin16)
  mip: u16 w=8, u16 h=8 ... LoadPaaBin16 reads dims directly (no LZW magic
                             words on this path), then always DecodeLZWs
  u24 dSize ............... compressed stream size (Init skips it; Load
                             re-seeks, but keep it exact)
  LZSS stream ............. all-literal emit for 128 bytes (16 x 0xFF flag
                             bytes), then the int32 signed-byte checksum
                             DecodeLZW verifies
  u16 0, u16 0 ............ mip terminator

Pixel (x, y) is two bytes: intensity x*32+16, coverage y*32+16 — the exact
ramp the two consuming tests assert (test_paa_decode_fixtures.cpp,
test_retail_pac_source_binding.cpp).

Regenerate:  python tests/fixtures/paa/make_pac_ai88_fixture.py
"""
import pathlib
import struct

HERE = pathlib.Path(__file__).resolve().parent


def main() -> None:
    payload = bytearray()
    for y in range(8):
        for x in range(8):
            payload += bytes((x * 32 + 16, y * 32 + 16))
    assert len(payload) == 128

    stream = bytearray()
    for i in range(0, len(payload), 8):
        stream.append(0xFF)  # 8 literals follow
        stream += payload[i : i + 8]

    checksum = sum(b if b < 128 else b - 256 for b in payload)
    body = bytes(stream) + struct.pack("<i", checksum)

    blob = (
        struct.pack("<H", 0x8080)  # AI88 marker
        + struct.pack("<H", 0)  # empty palette
        + struct.pack("<HH", 8, 8)  # mip dims
        + len(body).to_bytes(3, "little")  # dSize
        + body
        + struct.pack("<HH", 0, 0)  # terminator
    )
    out = HERE / "synthetic_ai88_lzss.pac"
    out.write_bytes(blob)
    print(f"wrote {out.name}: {len(blob)} bytes (dSize={len(body)})")


if __name__ == "__main__":
    main()
