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
