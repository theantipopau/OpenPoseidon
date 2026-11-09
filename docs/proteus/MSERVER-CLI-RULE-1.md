# MSERVER-CLI-RULE-1 — the publisher validates with the service's rule, not its own copy

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Follow-up of `MSERVER-PATHCHARSET-1`, which brought the master service's rule in line with the
engine. The publisher still carried two private copies of it, so "client and server agree" was
true at the service and false at the tool everyone publishes with.

## Problem

`papa publish --mod-id <name>` checked the name with hand-written string tests
(`mserver/CLI/src/main.rs:320-331`): non-empty, no `/`, no `\`, no `..`. That is the same
coarse shape the service had *before* `MSERVER-PATHCHARSET-1`, which disagreed with
`Foundation::IsSinglePathComponent` in both directions:

| `--mod-id` | service (after `MSERVER-PATHCHARSET-1`) | CLI before |
| --- | --- | --- |
| `a*b`, `a:b`, `trailing.` | `400 invalid mod id` | **sent to the service** |
| `Sth..else` | accepted | **refused before upload** |

The install side had a third copy (`main.rs:894-898`): `folderName` from the catalog was checked
for empty/`/`/`\`/`.`/`..` only, so a Windows-reserved name or a drive-prefixed one (`C:`)
passed the check and was handed to `Path::join`. The service has refused those since
`MSERVER-PATHCHARSET-1`, but a catalog written before that could still carry one.

## Approach

The predicate moved to the one crate both ends already depend on:

```rust
papa_bear_client::catalog::is_single_path_component(name: &str) -> bool   // mserver/Client/src/catalog.rs:22
```

- a port of `Foundation::IsSinglePathComponent`, identical character ranges (C1 stays
  allowed, as in C++);
- `MasterService/src/mods.rs` imports it and its private `is_safe_segment` is gone — the
  eight functions that used it (both publish paths, the read/delete paths,
  `normalize_folder_name`) call the shared rule unchanged;
- `papa publish` uses it for `--mod-id`, and refuses a bad `--folder-name` before packing —
  the service's `normalize_folder_name` trims and then applies the same rule, so the CLI now
  trims the same way and reports `invalid folder name '<value>'` locally instead of uploading
  an artifact that would be rejected at finalize;
- `papa install` uses it for the catalog's `folderName` before `Path::join`.

No wire change, no new dependency: `papa-bear-cli` and `papa-bear-master-service` already
depend on `papa-bear-client`.

## Files

- `mserver/Client/src/catalog.rs` (new) — the shared predicate and its boundary test.
- `mserver/Client/src/lib.rs` — `pub mod catalog`.
- `mserver/MasterService/src/mods.rs` — import the shared rule, delete `is_safe_segment`.
- `mserver/CLI/src/main.rs` — publish `--mod-id` and `--folder-name`, install `folderName`.
- `mserver/CLI/tests/cli_roundtrip.rs` — the binary-level test.
- `docs/proteus/MSERVER-CLI-RULE-1.md`

## Evidence

`catalog::tests::the_component_rule_matches_the_engine` (`catalog.rs:46`) pins the boundary
once for every Rust consumer: 8 accepted names (`@CSLA`, `carwars2.5`, `ver2.0`,
`Mod Folder 2`, `Sth..else`, non-ASCII letters …) and 19 refused ones (`.`, `..`, `.. `,
separators, `:`, the reserved set, C0/DEL, trailing dot/space), plus the C1-allowed assertion
that keeps a future `char::is_control()` rewrite from drifting from C++. It lists the same
cases as `ModInstallDir refuses catalog names that are not one path component`
(`tests/unit/engine/Poseidon/Core/test_mod_install.cpp`).

`publish_validates_mod_id_before_any_network_call` (`cli_roundtrip.rs:64`) drives the built
binary against an unreachable master (`127.0.0.1:1`, connection refused, no server needed):

- `a*b`, `a:b`, `trailing.`, `..`, `traversal/name` as `--mod-id` → non-zero exit with
  `invalid mod id '<value>'`;
- `a*b`, `..`, `nested/path` as `--folder-name` → `invalid folder name '<value>'`;
- `--mod-id Sth..else --folder-name ver2.0` passes validation and stops at
  `catalog lookup failed … /v1/mods/Sth..else` — proving the formerly refused name is now
  accepted, and that the accepted path really is the network call.

Broken-state delta, measured: with the old inline `--mod-id` check restored (file copied back
afterwards), the test fails on its first name with
`Error: catalog lookup failed: http://127.0.0.1:1/v1/mods/a*b: Connection Failed …` — the CLI
sent the name to the service, which is exactly the disagreement this closes.

- `cargo test -p papa-bear-client`: 14 passed / 0 failed (13 before).
- `cargo test -p papa-bear-cli`: 18 unit + 2 integration passed / 0 failed.
- `cargo test -p papa-bear-master-service`: 74 + 7 + 0 passed / 0 failed, unchanged — the
  shared rule is the body the service already had.
- `cargo clippy -p papa-bear-client -p papa-bear-cli -p papa-bear-master-service --all-targets`:
  no error.
- `check-format.py` and `check-all`: see the run log entry.

## Risks

- The CLI now refuses names it used to upload successfully (`a*b`, `a:b`, `trailing.`) — those
  uploads were rejected by the service since `MSERVER-PATHCHARSET-1`, so the change moves the
  error earlier and stops it costing a pack + upload.
- `Sth..else` can now be published by the CLI; the service accepts it, and the engine installs
  it. No behavior change for names both sides already agreed on.
- The trim-then-validate `folderName` logic was still mirrored in the CLI when this landed;
  `MSERVER-NORMALIZE-1` moved it into the shared module.

## Follow-ups

- (done) Moved the `folderName` normalization (trim, empty → `None`, then the predicate) into
  the shared catalog module — see `MSERVER-NORMALIZE-1`.
- (done) The C++ and Rust tests now read one shared case file, so a reserved character added
  there fails every suite at once — see `MSERVER-NAME-CASES-1`.
- (done) A catalog sweep now reports ids and folder names published before
  `MSERVER-PATHCHARSET-1` that the rule rejects — see `MSERVER-CHECK-NAMES-1`.

## Verify

Nothing to verify on a GPU. Re-run the three suites above, and against a live master
`papa publish <dir> --admin-key … --mod-id a*b` — expect the local refusal, no upload.
