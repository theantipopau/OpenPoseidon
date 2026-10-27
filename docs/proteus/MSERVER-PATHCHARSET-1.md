# MSERVER-PATHCHARSET-1 — a published name is installable by construction

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Follow-up of `MSERVER-FOLDERNAME-1` (the write side of `PATH-JOIN-AUDIT-1` finding 1): the
server had *a* rule for the two manifest-supplied names, but not the engine's rule.

## Problem

`is_safe_segment` accepted any non-empty value with no `/`, no `\` and no two consecutive
dots. The engine accepts a name only when `Foundation::IsSinglePathComponent`
(`engine/Poseidon/Foundation/Common/Filenames.hpp`) says it is one relative path component.
The two lists disagreed in both directions:

| value | engine (`IsSinglePathComponent`) | server before |
| --- | --- | --- |
| `a*b`, `a?b`, `a"b`, `a<b`, `a>b`, `a|b` | refused (Windows-reserved) | **published** |
| `a:b` | refused (drive designator / NTFS stream) | **published** |
| `a` + C0/DEL + `b` | refused (control character) | **published** |
| `trailing.` | refused (Windows trims the dot) | **published** |
| `Sth..else` | accepted (one component) | **refused** |

The first four rows are the damaging direction. `mod_id` is also the object-store key
(`<mod_id>/<mod_id>.pbo.zst`), so the store inherits the same portability constraints as the
installer, and `folderName` becomes `<modsRoot>/<folderName>` on every client, where
`ModInstallDir` refuses it — the entry downloads and then fails to install. The last row is the
harmless direction but still wrong: the server refused a name the installer accepts, purely
because `contains("..")` could not tell `..` from `Sth..else`.

## Approach

One predicate, rewritten to the engine's definition — same name at the time, every existing
call site, no signature change (`mods.rs:1038`):

```rust
fn is_safe_segment(value: &str) -> bool
```

It has since moved out of the service into `papa_bear_client::catalog::is_single_path_component`
so the publisher validates with the same code (`MSERVER-CLI-RULE-1`); the body below is what
moved.

- reject empty, `.`, `..`;
- reject a trailing `.` or ` ` (Windows trims those, so `".. "` collapses back to `..`);
- reject C0 (`< 0x20`) and DEL, and the reserved set `/ \ : * ? " < > |` — exactly the
  characters `IsSinglePathComponent` rejects. C1 (`0x80..=0x9f`) stays allowed there, so it
  stays allowed here; using `char::is_control()` would have silently been stricter.

Both identities now go through it wherever they did before (the same eight functions): the publish paths
(`finalize_mod`, `finalize_revision`), and the read/delete paths, which already treat a
refusal as "not found" rather than an error.

`folderName` additionally keeps the `MSERVER-FOLDERNAME-1` behaviour — the manifest value is
trimmed by `normalize_folder_name` (`mods.rs:1061`) before the predicate, absent/empty stays
`None` for the `@<modId>` fallback, and a refusal is a 400 through the existing mapping.
`mod_id` is not trimmed (it is an identity, and trimming would alias two different ids), so a
trailing space reaches the predicate and is refused.

## Behaviour

| name | before | now |
| --- | --- | --- |
| `csla`, `@CSLA`, `carwars2.5`, `Mod Folder 2`, `Sth..else` | `Sth..else` refused | all published |
| `a*b`, `a:b`, `trailing.`, `a\u{7}b` as `mod_id` | **published** | `400 invalid mod id '<value>'` |
| the same as `folderName` | published; every client's install then refused it | `400 invalid folder name '<value>'` |
| `..\..\Documents\x`, `..`, `.` | already refused (`MSERVER-FOLDERNAME-1`) | unchanged |

## Files

- `mserver/MasterService/src/mods.rs` — `is_safe_segment`, its doc, the new store test.
- `mserver/MasterService/src/lib.rs` — one more assertion in the existing endpoint test.
- `docs/proteus/MSERVER-PATHCHARSET-1.md`

## Evidence

`mods::tests::publish_holds_both_manifest_names_to_the_engine_component_set`
(`mods.rs:1479`) pushes 13 reserved/trailing/whole-dot names through `finalize_mod` as
`mod_id` — each must fail with the exact message and leave the catalog empty — then 6 of them
as `folderName`, with no `character-mod` directory created. It ends on the widening: a
publish with `mod_id` `Sth..else` and `folderName` `ver2.0` succeeds, and the artifact
round-trips through `artifact_bytes("Sth..else")`. The names are the same set the C++ test
`ModInstallDir refuses catalog names that are not one path component`
(`test_mod_install.cpp:210`) checks against `IsSinglePathComponent`, so the two languages are
pinned to the same boundary.

`tests::http_mod_publish_rejects_a_folder_name_that_could_leave_the_mods_root` (`lib.rs:1314`)
now also publishes `folderName: bad?name` and requires **400** with
`invalid folder name 'bad?name'` before the legitimate `@folder-name-mod` publish — the
widened rule must reach the client as a validation error, not a 500.

Broken-state delta, measured: with the old body restored (file copied back afterwards) the new
test fails on its first name with
`unwrap_err() on an Ok value: ModCatalogEntry { mod_id: "a*b", … download_url: Some("/v1/mods/a*b/revisions/1/download") }`
— i.e. it observes the entry that used to be published, and that its URL then carries a `*`.

- `cargo test -p papa-bear-master-service --lib`: **74 passed / 0 failed** (73 before).
- `cargo clippy -p papa-bear-master-service --all-targets`: no error; the crate denies
  `clippy::all`, and the first draft's `!(a) && !(b)` was rewritten to the single negation it
  asked for. The remaining warnings are the pre-existing ones (40 in `lib test`, 31 in `lib`).
- `check-format.py --base HEAD`: OK (44 changed files; the new test array is rustfmt's
  vertical form). `check-all.ps1`: OK — asset registry, roadmap freshness, both selftests and
  the format gate.

## Risks

- Stricter for `mod_id`: an id containing `:` or ending in `.` can no longer be published or
  served. No catalog entry in this repo's fixtures uses one.
- An entry published before this change with such an id or folder name is still *served*; only
  a re-publish is refused. Same shape as the `MSERVER-FOLDERNAME-1` follow-up.
  `MSERVER-CHECK-NAMES-1` now enumerates those entries (`check-names`), but it does not change
  what is served.
- The predicate is duplicated by *language*: C++ (`IsSinglePathComponent`) and Rust (one copy,
  in `papa_bear_client::catalog`). The *cases* are no longer: all three suites read
  `tests/fixtures/mod_names/catalog_name_cases.json` after `MSERVER-NAME-CASES-1`, so a case
  added there fails every suite. A rule changed on one side only still needs a reviewer — the
  fixture catches divergence, it cannot prevent it.

## Follow-ups

- (done) The publisher CLI used its own `contains("..")` check; it now calls the shared
  predicate, as does its install-side folder check — see `MSERVER-CLI-RULE-1`.
- (done) The `--check-names` store scan now reports ids and folder names published before both
  server checks — see `MSERVER-CHECK-NAMES-1`.

## Verify

Nothing to verify on a GPU. Re-run `cargo test -p papa-bear-master-service --lib` and, against
a live local service, `POST /v1/mods` with `folderName=a?b` — expect
`400 invalid folder name 'a?b'` — and with `modId=Sth..else` + `folderName=ver2.0`, expect 200.
