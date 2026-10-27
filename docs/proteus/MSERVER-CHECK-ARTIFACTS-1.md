# MSERVER-CHECK-ARTIFACTS-1 — the `check-names` sweep also reports missing and tampered artifacts

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Follow-up of `MSERVER-CHECK-NAMES-1`. That sweep could ask which stored *names* the service would
refuse today. It could not ask whether the entries it lists can actually be downloaded.

## Problem

`/v1/mods` lists an entry from its `mod.json` and advertises
`/v1/mods/<id>/revisions/<rev>/download`, but nothing checks that the object that route reads
still exists, and nothing re-hashes it against the `sha256` stored beside it. Both failures stay
invisible until a client tries:

- a missing artifact is a catalog row with a link that 404s — a failed download from a service
  that still advertises the mod;
- a replaced or truncated artifact downloads completely and is then refused by `papa install`
  (`download SHA-256 mismatch: expected …, got …`), after the whole file has crossed the network.

A pre-`sha256` legacy entry is a third case: `parse_metadata` backfills the hash when it reads the
entry, but only when the artifact can be read — a missing object leaves the entry listed with no
hash at all, so nothing downstream can verify it either.

## Approach

- One pass. `ModStore::scan_catalog()` (was `scan_names()`) keeps the name rows and adds an
  artifact check per entry, so the two halves cannot see different catalogs. `NameScan` becomes
  `CatalogScan { entries_checked, artifacts_checked, rejected_names, damaged_artifacts }`;
  `InvalidCatalogName` rows are unchanged.
- The object is resolved by a new private `artifact_object(mod_id, revision)`, extracted from the
  three download helpers (`revision_stream`, `signed_revision_url`, `revision_bytes`) that each
  carried the same "revision artifact, or `<id>/<id>.pbo.zst` for a legacy revision 1" fallback.
  The sweep now reads through the same resolver, so it cannot report an entry missing while a
  download would serve it — or the reverse.
- Faults are one enum, one row per entry: `ArtifactFault::Missing`, or
  `Sha256Mismatch { expected, actual }` when the stored hash is not the hash of the object. Hex case
  is not a difference (`papa install` compares with `eq_ignore_ascii_case`); both values are
  reported verbatim.
- A legacy entry with no stored `sha256` is existence-checked only. `parse_metadata` would backfill
  the hash on its next read; a sweep must not write, and comparing against a hash it computed
  itself would report nothing.
- Only the current revision is checked. Older revisions are history; the current one is what
  `/v1/mods` advertises and what a client installs next.
- `check-names` keeps its name (the previous record and operator runbooks refer to it) and prints
  both kinds of row, then one combined summary:

```
missing-mod: revision 1 artifact is missing
tampered-mod: revision 1 artifact sha256 is '0682c5…' but the object hashes to '6c1aa5…'
Error: check-names: 2 damaged artifacts in 3 catalog entries
```

A clean sweep prints `check-names: checked N catalog entries, no rejected names, every artifact
intact` and exits 0. Names and artifacts both drive the non-zero exit and are counted separately
(`1 rejected name, 1 damaged artifact in 2 catalog entries`).

## Behaviour

| current entry | object state | reported |
| --- | --- | --- |
| any revision | absent at the revision path and, for revision 1, the legacy path | `Missing` |
| revision 1, legacy `<id>/<id>.pbo.zst` | present, hash matches | no |
| any revision | present, stored `sha256` differs | `Sha256Mismatch` |
| any revision | present, no stored `sha256` | no (existence only) |
| `mod.json` unreadable | — | the run fails with `parsing mod.json for <dir>` |

## Files

- `mserver/MasterService/src/mods.rs` — `ArtifactFault`, `DamagedArtifact`, `CatalogScan`,
  `scan_catalog`, `artifact_object`, `stored_entry` (now returns the revision too), the three
  download helpers rewired to the shared resolver, three new store tests.
- `mserver/MasterService/src/main.rs` — the artifact rows, the combined summary, `plural`.
- `mserver/MasterService/tests/check_names.rs` — entries now carry an artifact and a matching
  `sha256`; three new binary-level cases.
- `docs/proteus/MSERVER-CHECK-ARTIFACTS-1.md`

## Evidence

Store tests (`mods.rs`): `scan_catalog_is_clean_for_a_published_catalog` publishes two entries and
asserts `artifacts_checked == 2` with no rows; `scan_catalog_reports_a_current_artifact_that_is_missing`
publishes, deletes `revisions/1/<id>.pbo.zst`, asserts `list_mods` **still** lists the entry and the
sweep reports `Missing` at the published revision;
`scan_catalog_reports_a_current_artifact_whose_sha256_no_longer_matches` replaces the object with
`replaced bytes` and asserts the exact expected/actual hex;
`scan_catalog_finds_a_legacy_artifact_at_its_pre_revision_path` builds a revision-1 entry with no
`revisions/` directory and a matching stored hash — without the legacy fallback it would report
`Missing`, so the case pins the shared resolver. The four name cases are the previous ones, renamed
to `scan_catalog_*` and reading `rejected_names`.

Binary tests (`tests/check_names.rs`, via `CARGO_BIN_EXE_…`): five run on Windows — a clean
two-entry catalog exits 0 with `check-names: checked 2 catalog entries, no rejected names, every
artifact intact`; a traversal-shaped `folderName` exits 1 with the exact report line and
`1 rejected name in 1 catalog entry`; a deleted artifact exits 1 with
`broken-mod: revision 1 artifact is missing` and `1 damaged artifact in 1 catalog entry`; an
overwritten artifact exits 1 with the exact `sha256 is '…' but the object hashes to '…'` line; one
catalog with both kinds of problem prints `check-names: 1 rejected name, 1 damaged artifact in 2
catalog entries`. The sixth (directory named `a*b`) is `#[cfg(not(windows))]` — Windows cannot
create that directory, so only the Linux job executes it.

Manual run over three legacy entries (clean, missing, replaced) on Windows:

```
missing-mod: revision 1 artifact is missing
tampered-mod: revision 1 artifact sha256 is '0682c5f2076f099c34cfdd15a9e063849ed437a49677e6fcc5b4198c76575be5' but the object hashes to '6c1aa50442a93e42c0eb2907cf4e017cd19547891fa190f3ea473582b0479290'
Error: check-names: 2 damaged artifacts in 3 catalog entries
```

and over a clean two-entry directory:

```
check-names: checked 2 catalog entries, no rejected names, every artifact intact
```

Counts: `papa-bear-master-service` **80 lib + 7 bin + 5 integration**, 0 failed (77 + 7 + 2
before; +3 store, +3 integration). On Linux the `cfg(not(windows))` cases add one to lib and one to
the integration binary. Clippy exit 0 with no warning in the new code (the `doc_markdown` and
`could be a const fn` notes near it are on `ModStore`/S3 docs, `list_prefix` and
`has_compatibility_filter`, all pre-existing). `check-format` OK; `check-all` OK; roadmap
regenerated.

Two different tests flaked while the suite was re-run repeatedly: once
`http_mod_revision_routes_keep_stable_identity_and_history` (the revision POST answered 400 — the
publish error mapping turns any store error into 400) and once, in ten full `--lib` runs,
`mods::tests::catalog_selects_latest_compatible_revision` (`add_revision(…).unwrap()`). Both are
the parallel Windows filesystem race already recorded in `MSERVER-FOLDERNAME-1`,
`MSERVER-NORMALIZE-1` and `MSERVER-CHECK-NAMES-1`; neither test calls the sweep and the new cases
perform no renames, but the additional parallel I/O changes the scheduling that triggers it. Every
rerun was green.

## Risks

- The sweep reads every current artifact once and hashes it (mods reach tens of GB). That is what
  the check is for, but it makes `check-names` a maintenance-window command, not a cheap health
  probe to run on every deploy.
- Extracting `artifact_object` changed how the download helpers treat a failed `head`: the old
  code fell back to the legacy path on *any* head error, the helper falls back only on `NotFound`
  and propagates the rest. A permissions or transient error now surfaces instead of silently
  reading a different object. Same bytes on every healthy path.
- An entry without a stored `sha256` is existence-checked only, and `sizeBytes` is not compared
  with the object. A hash match already implies the bytes; only metadata that lies about size
  would slip through.
- A corrupt `mod.json` aborts the whole sweep — inherited from the name sweep, and the intended
  flow is to fix the entry and re-run.
- Read-only: a damaged entry stays listed and keeps its `downloadUrl` until it is re-published or
  deleted.

## Follow-ups

- Compare `sizeBytes` with the object as well; a hash match already proves the content, so this
  would only catch metadata that misstates the size.
- (done) Report two entries whose `folderName` resolve to the same install directory — a
  catalog-level collision the per-entry pass cannot see, because both entries are individually
  valid; see `MSERVER-CHECK-COLLISIONS-1`.
- A `--no-hash` switch (existence only) for a bucket where the full read proves too slow. Not
  included: no measurement of a production catalog yet.

## Verify

Nothing to verify on a GPU.

```powershell
cargo test -p papa-bear-master-service
cargo run -p papa-bear-master-service -- check-names --mods-dir <mods root>
```

Expect exit 0 and `… every artifact intact` on a healthy catalog; one line per missing or
mismatched artifact plus a non-zero exit otherwise. Against production, set `MODS_S3_*` exactly as
the server does and omit `--mods-dir`; budget one full read of every current artifact.
