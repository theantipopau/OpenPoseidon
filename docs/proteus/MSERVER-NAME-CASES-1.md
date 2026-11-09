# MSERVER-NAME-CASES-1 — one case list for the catalog-name boundary, read by both languages

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Follow-up of `MSERVER-PATHCHARSET-1`, whose risk section said the boundary was "duplicated by
*language*": C++, the client crate and the master service each tested it against a hand-typed
list, so a newly reserved character would be noticed in one suite and silently accepted by the
others.

## Problem

Three suites asserted the same rule with three different case lists:

| suite | refused | accepted |
| --- | --- | --- |
| `test_mod_install.cpp` (`ModInstallDir refuses …`) | 14 | 6 |
| `catalog.rs` (`the_component_rule_matches_the_engine`) | 19 | 8 |
| `mods.rs` (`publish_holds_both_manifest_names_to_the_engine_component_set`) | 13 ids / 6 folders | 2 |

The sets overlapped but none contained another. `a?b` was refused by two and absent from the
third; `trailing ` only existed in the client test; the publish test checked `folderName`
against a six-name subset of its own `mod_id` list. Adding a name — or worse, changing the
predicate — meant editing three arrays in two languages and hoping nobody forgot one.

## Approach

One JSON fixture, `tests/fixtures/mod_names/catalog_name_cases.json`, with two arrays and a
`$comment` that states the verdict semantics and names the readers:

- `refused` (24): the predicate must answer `false`, and the install directory must not
  compose. Traversal shapes, separators, drive/UNC, the Windows-reserved set, C0/DEL,
  trailing dot and space, `.`/`..`/`trailing..`.
- `accepted` (10): the predicate must answer `true`, the composed directory must stay directly
  under the mods root, the published spelling must be kept verbatim and round-trip through the
  store. Includes the widened spellings (`Sth..else`, `ver2.0`, `Mod Folder 2`) and a
  non-ASCII name.

Each suite keeps its own three-line reader rather than sharing code — the *list* is the shared
artifact, and the parsers are standard (`cJSON` in C++, `serde_json` in Rust):

```cpp
// test_mod_install.cpp — runtime read of the source tree, cJSON
std::vector<std::string> LoadCatalogNameCases(const char* key)
// catalog.rs / mods.rs — compile time, so a missing file is a compile error
fn catalog_name_cases(key: &str) -> Vec<String>   // include_str!(CARGO_MANIFEST_DIR + /../../tests/…)
```

The C++ reader uses `TESTS_ROOT_DIR` (the fixture directory the suite already uses for
source-tree test data) instead of the POST_BUILD staged copy: the copy is refreshed only when
the test target relinks, so a fixture-only change could otherwise be tested against yesterday's
list. Missing or unparsable is a failure there, not a `SKIP` — this is checked-in text, unlike
the binary fixtures `FIXTURE-SKIP-1` covers. Rust reads at compile time, so the same edit
rebuilds both Rust suites.

**Blank is not in the file.** An empty `folderName` means "no folder name" (installs fall back
to `@<modId>`), while an empty `modId` is refused, so one verdict cannot describe both
spellings; each suite asserts its own blank behaviour locally.

## Behaviour

| list entry | C++ `IsSinglePathComponent` / `ModInstallDir` | Rust `is_single_path_component` | publish (`mod_id`, `folderName`) |
| --- | --- | --- | --- |
| `refused` | `false`, empty path | `false` | refused with `invalid mod id '<value>'` / `invalid folder name '<trimmed>'` |
| `accepted` | `true`, `<root>/<name>` | `true` | stored and served under that exact name |

## Files

- `tests/fixtures/mod_names/catalog_name_cases.json` — the list (new).
- `tests/unit/engine/Poseidon/Core/test_mod_install.cpp` — `LoadCatalogNameCases`, the case
  now loops the fixture instead of a local `hostile[]`/accepted array.
- `mserver/Client/src/catalog.rs` — parse at compile time, loop both lists; the local C1 and
  blank assertions stay.
- `mserver/Client/Cargo.toml` — `serde_json` as a **dev**-dependency (the crate had none).
- `mserver/MasterService/src/mods.rs` — same loader; both manifest names are looped over the
  full refused list (24 ids and 24 folder names, formerly 13 and 6), and every accepted name is
  published as both and round-tripped through `artifact_bytes`.
- `mserver/CLI/tests/cli_roundtrip.rs` — the publisher-side test drove five ids and three
  folders of its own (`a*b`, `a:b`, `trailing.`, `..`, `traversal/name`; `a*b`, `..`,
  `nested/path`); it now drives every refused name through both `--mod-id` and `--folder-name`
  and checks the positive pair against `accepted`.
- `docs/proteus/MSERVER-NAME-CASES-1.md`

## Evidence

`ModInstallDir refuses catalog names that are not one path component`: **122 assertions**, all
passed, on a clang-cl `PoseidonTests` build (the file compiles and links; `cJSON` arrives
through `Poseidon`'s `PUBLIC cjson` link). The `[mods]` tag: 72 cases, 70 passed / 2 skipped
(the absent `pbo/` fixtures), 469 assertions.

`catalog::tests::the_component_rule_matches_the_engine` reads both lists; the publish test does
the same at the store layer. Broken-state deltas, both measured by adding one name
(`"plain-name"`) to `refused` — a name both predicates currently accept, so every suite must
now fail — and restoring the file afterwards:

- C++ (no rebuild needed, the source file is read at run time):
  `test_mod_install.cpp(267): FAILED: CHECK_FALSE(IsSinglePathComponent(name))`, with
  `name: plain-name`, plus the two `ModInstallDir` checks; restored → 122 assertions green.
- Rust client: `panicked at catalog.rs:109: accepted "plain-name"`.
- Rust publish: `called Result::unwrap_err() on an Ok value: ModCatalogEntry { mod_id:
  "plain-name", … download_url: Some("/v1/mods/plain-name/revisions/1/download") }` — the entry
  a shared list entry would have published.

`publish_validates_mod_id_before_any_network_call` (unchanged name; it covers both manifest
names) now runs 48 real `papa publish` invocations, one per refused name per spelling, each
asserted to fail with the exact message *and* without `catalog lookup failed`, so the refusal
happened before the (closed) port was touched. ".. " is compared against the trimmed value,
because `--folder-name` goes through the same normalizer as the service. The positive control
stays one URL-safe pair — `Sth..else` as the id, `ver2.0` as the folder, both asserted to still
be in `accepted` — because each publish that reaches the network costs about two seconds against
`127.0.0.1:1`; sweeping the whole accepted list would add ~40 s to every CI run for a check the
unit tests already make. The suite takes 2.6 s, as before. Delta measured here too: adding
`"plain-name"` to `refused` fails the test naming it, with `catalog lookup failed:
http://127.0.0.1:1/v1/mods/plain-name: … (os error 10061)` — the name reached the network.

Suite counts: `cargo test -p papa-bear-client` **15 passed / 0 failed** (unchanged; the
boundary test now reads the fixture), `papa-bear-master-service` **74 + 7 passed** (unchanged
count; the publish test loops 58 publishes instead of 20), `papa-bear-cli` **18 + 2 passed**
(unchanged), `cargo clippy` on the three crates no new warning (client's two
`first doc comment paragraph is too long` warnings are from `MSERVER-NORMALIZE-1`; master
service still 31 lib / 40 lib test), `check-format` and `check-all` see the run log.

## Risks

- The fixture is now a build input for Rust: deleting or breaking it fails `cargo test`
  compilation / panics the client test, and fails the C++ case at run time. That is the
  intended direction (loud), but it does couple two crates to a repo path outside their
  manifest directory (`/../../tests/fixtures`).
- Two parsers, one file: `cJSON` and `serde_json` agree on every JSON this file uses (plain
  strings, `\u0007`, `\u007f`, UTF-8 letters), and each suite's first assertion is the value
  itself, so a decode difference surfaces as a named failure rather than a silent skip.
- The minimum-size floors (`>= 20` refused, `>= 8` accepted) are sanity checks against a
  truncated file, not locks — a deliberate removal does not trip them.
- The shared list cannot make the two *implementations* agree on a character neither rejects:
  `accepted` is the control for over-strictness, not a proof of completeness.
- The CLI test keeps its strong positive control to one pair by design; a name that the list
  moves from `accepted` to `refused` will fail there, but a *newly accepted* name is only
  exercised at the process level if it is one of the two asserted to remain accepted.

## Follow-ups

- The predicates are still two implementations (C++ `IsSinglePathComponent`, Rust
  `is_single_path_component`); the fixture makes a divergence fail, but a rule that changes on
  one side only still needs a reviewer to add the case.
- (done) The `--check-names` sweep now reports ids and folder names published before
  `MSERVER-PATHCHARSET-1` — see `MSERVER-CHECK-NAMES-1`.

## Verify

Nothing to verify on a GPU. Re-run:

```powershell
cargo test -p papa-bear-client -p papa-bear-master-service -p papa-bear-cli
cmake --build build/win-x64-clang-rwdi --target PoseidonTests
build/win-x64-clang-rwdi/tests/unit/engine/Poseidon/PoseidonTests.exe "ModInstallDir refuses catalog names that are not one path component"
```

Add a name to `refused`, re-run both — each must fail naming it — then remove it.
