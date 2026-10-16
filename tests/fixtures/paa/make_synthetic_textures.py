#!/usr/bin/env python3
"""Generate the synthetic texture fixture family (FIXTURE-SYNTH-1).

Authored, not copied: every pixel is invented here (opaque white blocks,
gradient ramps). Layout mirrors what the engine expects (Pactext.cpp /
PAADecoder.cpp / PAAEncoder.cpp):

  u16 magic ............ 0xFF01/03/05 DXT, 0x8080 AI88, 0x4444 ARGB4444
  u16 palette count 0 .. no palette (asserted by LoadPaaBin16)
  per mip: u16 w, u16 h, u24 dSize, payload
  u16 0, u16 0 ......... terminator

DXT payloads are raw (no 0x8000 bit, no LZO) exactly as PAAEncoder::WritePAA
emits. AI88/4444 payloads are LZSS all-literal streams with the int32
signed-byte checksum DecodeLZW verifies (same recipe as
make_pac_ai88_fixture.py). The P8 .pac uses the byte-RLE LoadPacP8 reads.

Dims asserted by tests (test_paa.cpp, decode/image/probe suites):
  synthetic_dxt1.paa ..... DXT1 64x64, full chain to 4x4
  synthetic_dxt3.paa ..... DXT3 64x64, full chain to 4x4
  synthetic_dxt5.paa ..... DXT5 32x32, full chain to 4x4 (mip-chain test
                           requires levels > 1)
  synthetic_ai88.paa ..... AI88 64x64 + chain (header-only assertions)
  synthetic_argb4444.paa . ARGB4444 32x32 + chain (header-only assertions)
  synthetic_default.pac .. P8 palette PAC 64x64 (Image/Inspect/Preview PAC path)

Regenerate:  python tests/fixtures/paa/make_synthetic_textures.py
"""
import pathlib
import struct

HERE = pathlib.Path(__file__).resolve().parent

DXT1 = 0xFF01
DXT3 = 0xFF03
DXT5 = 0xFF05
AI88 = 0x8080
ARGB4444 = 0x4444


def lzss_literal(payload: bytes) -> bytes:
    stream = bytearray()
    for i in range(0, len(payload), 8):
        stream.append(0xFF)  # 8 literals follow
        stream += payload[i : i + 8]
    checksum = sum(b if b < 128 else b - 256 for b in payload)
    return bytes(stream) + struct.pack("<i", checksum)


def mip_entry(w: int, h: int, payload: bytes) -> bytes:
    return struct.pack("<HH", w, h) + len(payload).to_bytes(3, "little") + payload


def paa(magic: int, mips: bytes) -> bytes:
    return struct.pack("<HH", magic, 0) + mips + struct.pack("<HH", 0, 0)


def chain(top: int):
    dims = []
    w = top
    while w >= 4:
        dims.append(w)
        w //= 2
    return dims


WHITE_DXT1 = struct.pack("<HHI", 0xFFFF, 0x0000, 0x00000000)


def dxt_block(fmt: int) -> bytes:
    if fmt == DXT1:
        return WHITE_DXT1
    if fmt == DXT3:
        return bytes(8 * b"\xff") + WHITE_DXT1
    # DXT5: alpha endpoints (255, 0) + all-zero alpha indices select 255.
    return bytes((255, 0)) + bytes(6) + WHITE_DXT1


def dxt_mips(fmt: int, top: int) -> bytes:
    out = bytearray()
    block = dxt_block(fmt)
    per_block = len(block)
    for w in chain(top):
        n = (w // 4) * (w // 4)
        out += mip_entry(w, w, block * n)
    return bytes(out)


def ai88_mips(top: int) -> bytes:
    out = bytearray()
    for w in chain(top):
        payload = bytearray()
        for y in range(w):
            for x in range(w):
                payload += bytes((x * 256 // w, y * 256 // w))
        out += mip_entry(w, w, lzss_literal(bytes(payload)))
    return bytes(out)


def argb4444_mips(top: int) -> bytes:
    out = bytearray()
    for w in chain(top):
        out += mip_entry(w, w, lzss_literal(struct.pack("<H", 0xFFFF) * (w * w)))
    return bytes(out)


def fuzz_dxt_oob() -> bytes:
    # Truncated DXT1 payload: 16x16 needs 16 blocks x 8 = 128 bytes, only one
    # block present. The DXT size guard must return an empty image, not read
    # past the payload.
    return paa(DXT1, mip_entry(16, 16, WHITE_DXT1))


def fuzz_lzw_overflow() -> bytes:
    # Over-long LZW match: 1 literal then a max-length (17) match whose tail
    # runs past the remaining output. The lensb bound must clip it, and the
    # stream then ends mid-decode (checksum absent) — either way, no OOB write.
    # Checksum over decoded bytes: 0x42 + 17 x 0x20 (initial text_buf fill).
    stream = bytes((0xFF, 0x42, 0x00, 0x01, 0xFF))
    body = stream + struct.pack("<i", 0x42 + 17 * 0x20)
    return (
        struct.pack("<H", AI88)
        + struct.pack("<H", 0)
        + struct.pack("<HH", 8, 8)
        + len(body).to_bytes(3, "little")
        + body
        + struct.pack("<HH", 0, 0)
    )


def fuzz_argb8888_oob() -> bytes:
    # ARGB8888 16x16 needs 1024 raw bytes, only 100 present. The payload-size
    # guard must return an empty image, not read past rawData.
    return paa(0x8888, mip_entry(16, 16, bytes(100)))


DARK_DXT5 = bytes((255, 0)) + bytes(6) + struct.pack("<HHI", 0x0000, 0x0000, 0x00000000)


def pac_file(block: bytes, top: int) -> bytes:
    # Marker-bearing PAC: DXT5 descriptor + empty palette + raw DXT chain.
    # The .pac suffix (not the marker) is what probes report as PAC/non-PAA.
    out = bytearray(struct.pack("<HH", DXT5, 0))
    for w in chain(top):
        n = (w // 4) * (w // 4)
        out += mip_entry(w, w, block * n)
    out += struct.pack("<HH", 0, 0)
    return bytes(out)


def main() -> None:
    files = {
        "synthetic_dxt1.paa": paa(DXT1, dxt_mips(DXT1, 64)),
        "synthetic_dxt3.paa": paa(DXT3, dxt_mips(DXT3, 64)),
        "synthetic_dxt5.paa": paa(DXT5, dxt_mips(DXT5, 32)),
        "synthetic_ai88.paa": paa(AI88, ai88_mips(64)),
        "synthetic_argb4444.paa": paa(ARGB4444, argb4444_mips(32)),
        "synthetic_default.pac": pac_file(dxt_block(DXT5), 64),
        "synthetic_dark.pac": pac_file(DARK_DXT5, 64),
        "fuzz_dxt_oob.paa": fuzz_dxt_oob(),
        "fuzz_lzw_overflow.paa": fuzz_lzw_overflow(),
        "fuzz_argb8888_oob.paa": fuzz_argb8888_oob(),
    }
    for name, blob in files.items():
        # synthetic_*.pac stage to fixtures/texture/pac via the pac/ dir mapping
        # in copy-fixtures.cmake; everything else via paa/ -> texture/paa.
        dest = HERE / name
        if name.endswith(".pac"):
            pac_dir = HERE.parent / "pac"
            pac_dir.mkdir(exist_ok=True)
            dest = pac_dir / name
        dest.write_bytes(blob)
        print(f"wrote {dest.relative_to(HERE.parent)}: {len(blob)} bytes")


if __name__ == "__main__":
    main()
