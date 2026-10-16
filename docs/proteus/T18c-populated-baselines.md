# T18c — Populated benchmarks: admitted-count gate + re-baseline (done-unverified)

## Problem

SHUTDOWN-T-1's proof run was judged VALID with `admitted=0 (offered=13)`,
so T18b fps numbers could not tell a populated world from an empty one.
The gate (`check-capture.py --min-admitted`, wired into both capture
scripts since W0-ORDER) was toothless for exactly the scenes that needed
it: `scenes.yaml`/`shots.yaml` pinned `min_admitted: 0` on all four
Intro-island scenes.

## Finding (live GPU, RX 9070 XT / vulkan)

The Intro missions are bare terrain **by design**, not by bug:

- `terrain-brush.Intro/mission.sqm` is 674 bytes: one `SoldierWB` player,
  no statics. A 30 s settled run offers 13 objects, all refused
  `not-static`, 0 admitted — and the PNG is NOT the menu backdrop
  (EMPTY_KNOWN denylist does not trip), render thread busy at 139 fps on
  real terrain pixels. Nothing more will ever admit there, however long
  the settle: there is nothing static to admit.
- `smoke-room.noe` (same one-soldier shape, on Nogova) admits
  **173,540 / 178,263** on the current binary — statics stream and bake.
  The 150000 gate passes with headroom.

So the two scene families need different guards: census for Nogova
(interior/coast keep 150000), pixels for Intro (open-group keeps 0 with
the md5 denylist as the tripwire). A blanket 500 was tried first and
reverted — it INVALIDates legitimate bare-terrain captures (verified:
settled open run goes VALID@0 / INVALID@500 while rendering real
terrain). `check-capture.py` default 500 stays for unknown shots.

## Change

Comments only, values restored: `min_admitted: 0` + rationale on the
four Intro entries in `scripts/proteus/scenes.yaml` and
`scripts/proteus/shots.yaml`. No engine, judge, or threshold change.

## Re-baseline (binary 5ec44b3, 1080p/vulkan, Iters 1)

| scene | bench_fps | admitted / offered | verdict |
|---|---|---|---|
| open | 89.3 (300f / 3.4 s) | 0 / 13 (bare terrain, expected) | VALID |
| interior | 71.1 (300f / 4.2 s) | 173,540 / 178,263 | VALID |

CSVs + sidecars: `docs/proteus/perf/20261016-141805-*open.*`,
`20261016-142222-*interior.*`. Menu-era 1440p numbers stay superseded,
not deleted. Remaining scenes (woodland/ride/coast/manual) re-baseline
with the same one-command runs; multi-iter p95/p99 still owed per T18b
(`-Iters 5+`).

## Follow-ups

- T19 goldens on the current binary (bless predates the merge; thresholds
  stand, pixels need re-verifying).
- `offered=13` dynamics (soldier) refused not-static while standing in
  frame: correct-by-design (dynamic path) but never explicitly asserted.
