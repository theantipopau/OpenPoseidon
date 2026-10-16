# FIXTURE-VOICE-1 — restored omitted voice fixture files; 2 failing UI tests fixed

## Symptom

Broad `[UI]` had 2 sticking failures, both voice flags in
`test_mission_language_detector.cpp` (per-language coverage expects
`english/czech.voice`, DemoSoundSubdir expects `EN CZ`). Proven pre-existing
(stash pre/post identical), cause unknown — this ticket root-causes and fixes.

## Root cause (missing data, not a code bug)

`DetectVoiceLanguages` reads each mission's `description.ext` `CfgSounds` entry
and checks file existence: `CoverageSample.Noe` references `radio.ogg`,
`DemoSoundSubdir.Noe` references `sound/line.ogg`, with per-language
`WithLangSuffix` variants (`radio.Czech.ogg`, …). The source release shipped
**zero** sound files under either fixture dir, so every voice flag was false and
the detector — working correctly — reported no voice. Code untouched.

## Fix (5 existence-only files, content never read)

Recreated exactly the referenced names (recreate with `printf` — any bytes do;
the suite's own temp-dir test writes `"dummy"`):

- `CoverageSample.Noe/`: `radio.ogg`, `radio.Czech.ogg`, `radio.Polish.ogg`
  (no Russian file — `russian.voice == false` still asserted)
- `DemoSoundSubdir.Noe/sound/`: `line.ogg`, `line.Czech.ogg`
  (the stock sound-subdir layout the test is named for)

Committed despite the source-release binary omission because the detector only
stats existence, the suite itself treats content as irrelevant, and without them
the repo's own tests fail on checkout. `font/pac/jpg/pbo/rtm` remain unrestorable
(no source, no generator) and are ticketed, not fabricated.

## Verification

- `[missions]` 107 assertions / 24 cases pass, exit 0.
- Broad `[UI] ~[font]` 220 cases: **214 pass / 0 fail / 6 skipped, exit 0** —
  first fully-green broad UI run in this workstream. `[font]` still excluded:
  `font/dummy.ttf` has no source anywhere (needs an authored font or an explicit
  skip-with-reason).
- `check-all` OK.

## Follow-ups

- `font` decision + `pac/jpg/pbo/rtm` tickets (audit proposal item 3, remainder).
- Consider a tiny generator script (paa/mlod precedent) if these names ever need
  to be reproducible from scratch rather than committed.
