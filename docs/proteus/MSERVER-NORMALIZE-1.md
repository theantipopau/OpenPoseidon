# MSERVER-NORMALIZE-1 — one `folderName` normalizer for the publisher and the service

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Follow-up of `MSERVER-CLI-RULE-1`, which shared the *character* rule. The normalization around
it — trim, blank means "no folder name" — was still written twice, and the two sides had to
keep the same message by hand.

## Problem

`folderName` is optional in the publish manifest. "No folder name" is spelled three ways
(absent, empty, whitespace) and means the client falls back to `@<modId>`. After
`MSERVER-CLI-RULE-1` each side did this itself:

```rust
// MasterService/src/mods.rs
fn normalize_folder_name(value: Option<&str>) -> Result<Option<String>>   // trim, blank -> None
// CLI/src/main.rs — inline, for the same rule
args.folder_name.as_deref().map(str::trim).filter(|folder| !folder.is_empty())
```

The error text was also duplicated (`invalid folder name '<value>'` in both). The CLI
validated the trimmed value but uploaded the *raw* one; harmless only because the service
trims again. Any future change — say, also collapsing inner whitespace, or accepting a
leading `@` for a different reason — would have to be made twice, and the two sides already
spelled the manifest value differently (raw on the wire, trimmed in the catalog).

## Approach

The normalizer moved next to the predicate, in the crate both ends depend on
(`mserver/Client/src/catalog.rs`):

```rust
pub fn normalize_folder_name(raw: Option<&str>) -> Result<Option<String>, InvalidFolderName>  // catalog.rs:43
pub struct InvalidFolderName(pub String);                                                     // catalog.rs:58
```

- trim, then absent/blank → `Ok(None)`;
- otherwise the trimmed value must be one component, or `Err(InvalidFolderName(trimmed))`;
- `InvalidFolderName`'s `Display` *is* `invalid folder name '<value>'`, and it implements
  `std::error::Error`, so both callers get the identical message without re-wording it:
  the service wraps it in `anyhow` (`return Err(error.into())` when it has to delete the temp
  upload, `?` on the revision path), the CLI propagates it with `?`.

The service's private helper is deleted (both publish paths import the shared one). The CLI
now *assigns* the normalized value:

```rust
args.folder_name = normalize_folder_name(args.folder_name.as_deref())?;   // main.rs:325
```

so it uploads exactly the value the service would store, and a blank `--folder-name` sends no
field at all instead of an empty one.

`papa install` deliberately does **not** use this: a catalog value is used verbatim by the
engine, so trimming it on the client would install into a different directory than the game
would. It keeps the raw-value component check from `MSERVER-CLI-RULE-1`.

## Behaviour

| `folderName` | result |
| --- | --- |
| absent, `""`, `"   "`, `"\t\n"` | `Ok(None)` — no field on the wire, `@<modId>` fallback |
| `"  CSLA  "` | `Ok(Some("CSLA"))`, uploaded trimmed |
| `"@CSLA"`, `"Sth..else"`, `"ver2.0"` | `Ok(Some(..))` unchanged |
| `".. "`, `".."`, `"."`, `"a*b"`, `"nested/path"`, `"C:\mods"`, `"trailing."` | `Err(invalid folder name '<trimmed>')` |

## Files

- `mserver/Client/src/catalog.rs` — `normalize_folder_name`, `InvalidFolderName`, the test.
- `mserver/MasterService/src/mods.rs` — import the shared normalizer, delete the local one.
- `mserver/CLI/src/main.rs` — assign the normalized value; drop the inline trim/filter.
- `docs/proteus/MSERVER-NORMALIZE-1.md`

## Evidence

`catalog::tests::folder_names_normalize_or_carry_one_message` (`catalog.rs:123`): the four
blank spellings normalize to `None`; four valid names normalize (trimmed) unchanged; seven
refused values carry the trimmed value, print exactly
`invalid folder name '<trimmed>'`, and satisfy `&dyn std::error::Error` (which is what lets
both `anyhow` callers use `?`).

Broken-state delta, measured: with the trim removed from the normalizer
(`raw.map(|value| value)`, file restored afterwards) the test fails on the first blank case —
`left: Err(InvalidFolderName("   "))`, `right: Ok(None)` — so the test observes the trim
rather than just the predicate.

The exact-message assertions from the earlier tickets are unchanged and still pass, which is
the point of putting `Display` on the error: `publish_rejects_a_folder_name_that_could_leave_the_mods_root`,
`publish_holds_both_manifest_names_to_the_engine_component_set`, the endpoint case, and the
CLI's `invalid folder name 'a*b'` / `'..'` / `'nested/path'` cases.

- `cargo test -p papa-bear-client`: 15 passed / 0 failed (14 before).
- `cargo test -p papa-bear-cli`: 18 unit + 2 integration passed.
- `cargo test -p papa-bear-master-service`: 74 + 7 + 0 passed.
- `cargo clippy -p papa-bear-client -p papa-bear-cli -p papa-bear-master-service --all-targets`:
  no error.
- `check-format.py` and `check-all`: see the run log entry.

One flake was seen and is recorded rather than swept: a parallel `--lib` run failed
`concurrent_updates_allocate_distinct_revisions_without_regressing_latest` with
`LocalFileSystem … Unable to rename file: Access is denied`, the same Windows rename race
noted in `MSERVER-FOLDERNAME-1`. The test passes a `ModUploadMeta::default()` (no folder
name), and the immediately following runs are green (74 passed three times), so it is not a
consequence of this change.

## Risks

- The CLI's error now comes from a shared `Display`; any re-wording happens in one place, for
  both the publisher and the service. The existing tests pin the exact text.
- A `--folder-name` that is invalid *and* an invalid `--mod-id` now reports the folder first,
  because normalization happens before the id check. Both are refused; only which message
  appears changed.
- The service's error type changed from `anyhow`-built inside the helper to a shared error
  wrapped at the call site; HTTP status mapping is untouched (still 400 by the existing
  "any error that is not already-exists/does-not-exist" rule).

## Follow-ups

- The `@<modId>` fallback itself is still composed in the CLI and in the engine
  (`ModInstallDir`); a shared spelling would make "no folder name" one concept end to end.
- (done) The C++ and Rust tests now read one shared case file — see `MSERVER-NAME-CASES-1`.
- (done) The `check-names` sweep reports names published before `MSERVER-PATHCHARSET-1` that the
  rule now rejects — see `MSERVER-CHECK-NAMES-1`.

## Verify

Nothing to verify on a GPU. Re-run the three suites, then `papa publish <dir> --admin-key …
--folder-name "   "` against a local service — expect no `folderName` part and a stored
`folder_name: null`; `--folder-name ".."` — expect a local refusal before upload.
