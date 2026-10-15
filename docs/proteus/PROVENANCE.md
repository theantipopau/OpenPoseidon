# Proteus provenance log

Every line of code or art taken from outside this fork (upstream koosoli/OpenPoseidon
included) gets an entry here BEFORE merge: source repo, commit hash, file path, licence.
If the licence can't be confirmed, the ticket becomes a proposal instead.

## Inherited at fork time (present before any Proteus work)

- `assets/ui/openposeidon_logo.paa`, `assets/ui/openposeidon_logo.png` —
  upstream koosoli/OpenPoseidon, predates the fork (exact upstream hash not recorded at
  fork time; treat as GPL-3.0-or-later per the repo LICENSE). Superseded by Proteus art;
  no new uses. UNVERIFIED: upstream hash.
- `resources/sky/bright_stars.pstar`, `resources/sky/milkyway.pskytex` —
  converted public-domain data (Yale BSC via CDS/ADC; NASA SVS/ESA Gaia DR2); converters in
  `tools/sky/`. See THIRD_PARTY_NOTICES.md and `assets/registry.yaml`.
- `assets/fonts/BarlowCondensed-*.ttf` — Jeremy Tribby, SIL OFL 1.1 (bundled; OFL allows).

## Algorithm choices (no code taken, recorded per ticket spec)

- T19 `scripts/proteus/imgdiff.py` — SSIM (Wang, Bovik, Sheikh & Simoncelli, 2004)
  implemented from scratch from the published formula; no copied code, no dependency.
  FLIP (Netflix) considered and rejected: needs a display model and heavier numerics
  for no benefit at golden-threshold use. PNG codec likewise hand-written (zlib + struct
  only). Nothing to license.

## Study references (no code taken, design influence only)

- W0 `pmndrs/upscaler` (https://github.com/pmndrs/upscaler), MIT licence —
  WebGPU temporal upscaler for three.js, FSR 2/3-style architecture. Reference commit
  `96184a6812bc5fd5e5fd7898257788a1663105a9` (main, 2026-10-12; commit/date via web
  lookup 2026-10-15, UNVERIFIED against a local clone). Files studied (design only):
  `src/Upscaler.ts` (pass orchestration), `src/shaders/accumulate.ts` (history
  reprojection, variance clipping, reactive response), `src/shaders/reconstruct.ts`
  (depth/motion reconstruction, disocclusion), `src/shaders/generateReactive.ts`
  (reactive mask), `src/shaders/easu.ts` + `src/shaders/rcas.ts` (spatial path).
  No code copied or ported in W0a; if W0b ports any pass, this entry grows a
  per-file record first.
