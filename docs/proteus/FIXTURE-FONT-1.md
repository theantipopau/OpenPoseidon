# FIXTURE-FONT-1 — authored font fixtures + tolerant post-build copy

## Symptom

`[font]` aborted on the missing `font/dummy.ttf`, and the `PoseidonTests`
post-build step hard-failed the whole tests-target build on the first omitted
fixture dir — so neither the font tests nor a green build were reachable
in-tree.

## Fix, part 1: authored fixtures (`tests/fixtures/font/make_dummy_ttf_fixture.py`)

New generator (fontTools, same sanctioned pattern as the paa/mlod generators;
fully invented outlines, no retail/third-party data):

- `dummy.ttf` (107 glyphs): box outlines for ASCII 32–126 plus the Czech set
  (U+010D/011B/0148/0159/0161/0165/016F/00ED/00FD/017E) and Cyrillic Zhe
  (U+0416); uniform 400-on-600 metrics so advance always exceeds ink width.
- `legacy.fxy`: 224 flat 6×u16 records `(c, 1, 0, 0, 4, 4)` = exactly 2688
  bytes, maxWidth/maxHeight 3, single set {1} — the precise shape
  `ParseFXY` reads and the tests assert.
- `legacy-01.paa`: minimal decodable DXT1 4×4 (opaque-white block, so glyph
  blits land non-background pixels), same container shape as the taggs fixtures.

## Fix, part 2: tolerant staging (`scripts/proteus/copy-fixtures.cmake`)

The 12-dir `copy_directory` block in `tests/unit/engine/Poseidon/CMakeLists.txt`
is now one `cmake -P` call that copies what exists and warns on what does not.
Justification: the source release omits dirs by design (`ASSET_SOURCES.md`), so
one missing dir should not fail everyone's build; tests needing unrestored data
still abort at runtime via `GetTestFixturePath` (loud, not silent).

## Verification

- `PoseidonTests` target builds **fully green, post-build included** (ninja
  exit 0) — first time in this workstream.
- `[font]` 4640 assertions / 56 cases pass, exit 0 (was: FATAL abort).
- Broad `[UI]` with **no exclusions**: 230 cases, 224 pass / 6 skipped /
  0 fail, exit 0.
- Full binary still aborts, now at `paa/synthetic_ai88_lzss.pac` (next gap).
- `check-all` OK (after the routine `gen-roadmap.py` regen for the new
  ledger tickets).

## Follow-ups (remaining unrestorable gaps — ticketed, not fabricated)

- `pac/synthetic_*.pac`, `paa/synthetic_*` (pixel variants), `jpg/*`
  (Pillow-generatable), `pbo/addon_fixture.pbo` (buildable with in-tree PBO
  tools?), `rtm/*`, `p3d/*` + `mlod/*` beyond current generators (ODOL and
  fuzzer-minimized inputs need per-test archaeology). Each wants the
  FIXTURE-VOICE-1 treatment: read the asserting test, author the minimum the
  parser needs, prove with the test itself.
