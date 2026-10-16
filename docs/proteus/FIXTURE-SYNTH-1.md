# FIXTURE-SYNTH-1 — Synthetic texture family + shape fuzz reproducers (done-unverified)

## Scope (FIXTURE-FONT-1 follow-up: per-test archaeology, authored data only)

The full `PoseidonTests` binary aborted at `paa/synthetic_ai88_lzss.pac`. Walking
the abort forward file-by-file, this ticket authors every fixture whose tests
assert parser behavior and structural properties — no Bohemia content:

- `tests/fixtures/paa/make_pac_ai88_fixture.py` (new): the 8x8 AI88 marker PAC
  both marker tests assert (`synthetic_ai88_lzss.pac`, 163 bytes)
- `tests/fixtures/paa/make_synthetic_textures.py` (new): DXT1/DXT3 64x64,
  DXT5 32x32 (multi-level chain — the mip-chain test requires levels > 1),
  AI88 64x64, ARGB4444 32x32, DXT5-marker `synthetic_default`/`synthetic_dark`
  64x64 PACs, and three fuzz reproducers (truncated DXT, over-long LZW match
  with signed-byte checksum, short ARGB8888)
- `tests/fixtures/mlod/make_p3dm_fixtures.py` (extended): three `fuzz_shape`
  reproducers using the existing P3DM writer — 64-non-NUL `#Property#` key
  (TAGG stack-OOB), ODOL-magic texture-count guard, ODOL-magic LOD-count guard
- `tests/unit/engine/Poseidon/CMakeLists.txt` (+1): `test_mlod_tagg.cpp`
  existed but was never compiled into any target — wired into `PoseidonTests`

Format notes the archaeology pinned down: the "PAC" fixtures are PAA-layout
files (the `0x8080`/`0xFF05` descriptor selects PAA level data regardless of
suffix; `.pac` vs `.paa` is reported from the suffix); DXT payloads are raw
exactly as `PAAEncoder::WritePAA` emits; Bin16 payloads are LZSS all-literal +
checksum; `StrToInt("ODOL")` == MSVC `'LODO'` == `0x4C4F444F`.

## Verification

- `[fuzz]` 11 cases pass (incl. the newly wired MLOD TAGG reproducer).
- `[graphics][decode],[Graphics][PAADecoder],[Formats][PAA],[Formats][PAC]`
  30 cases / 9873 assertions pass; both marker-bearing tests pass in their
  respective binaries (decode in `PoseidonTests`, binding in `PoseidonCoreTests`).
- Full binary advances past every texture/shape-fuzz gap. No engine change
  (test list + fixtures only).
- `check-all` OK.

## Follow-ups (deliberately NOT fabricated here)

- `p3d/` render/loader corpus (`simple_proxy`, `complex_vehicle`, `flat_quad`,
  …): needs ODOL/MLOD files with proxies, weighted selections, multi-LOD —
  a model-generator ticket with format research, not a bytes ticket.
- `mlod/complex_vehicle_mlod.p3d`: tests assert original content (Czech
  selection names, 2617 points) — needs owner decision (retarget tests to
  synthetic fixtures vs. provide data); same for any test asserting shipped
  counts/names per `ASSET_SOURCES.md`.
- `wrp/test_world.wrp` (world probes), `jpg`/`pbo`/`rtm` dirs still absent.
- `PoseidonCoreTests` post-build copy still dies on the missing `pbo` dir
  (exe links; same warn-not-fail treatment as FIXTURE-FONT-1 may apply).
