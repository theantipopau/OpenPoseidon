# MODINSTALL-PATH-1 — a catalog folder name that is not one path component is refused, not joined

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Found by `PATH-JOIN-AUDIT-1` as finding 1. Follow-up of `QSTREAM-ESCAPE-1`: same shape as
the mod-join escape, one layer further out.

## Problem

`ModInstallDir` is documented as `<modsRoot>/<folderName>`, falling back to `@<modId>`, and
implemented it as a raw string join:

```cpp
return modsRoot + "/" + folderName;   // the folder name was never inspected
```

`folderName` and `modId` are copied verbatim out of the master-server catalog
(`MasterServerServiceClient.cpp:910`) and then used as a *directory* by the install
transaction — `MakeStagedModInstall` (`ModInstall.cpp:168`) derives
`<dest>.staging-N` / `<dest>.backup-N` from it, `SwapStagedModInstalls` renames the
installed folder to the backup and the staged folder into place, and
`CommitStagedModInstalls` finishes with `remove_all(<dest>.backup-N)`.

So `folderName = "..\..\Documents\x"` makes the whole sequence operate outside `Mods/`,
and the last step deletes what it backed up there. The artifact's size and SHA-256 are
verified before the unpack; the path never was. The `@<modId>` fallback is no safer
(`@..\..\x`).

Repro without network:

```
ModInstallDir("/m", "id", "..\\..\\payload")  ==  "/m/../../payload"
```

## Approach

One predicate, applied where the name would become a path component:

```cpp
// engine/Poseidon/Foundation/Common/Filenames.hpp
inline bool IsSinglePathComponent(const std::string& name);
```

True only for a name that means the same folder on every supported filesystem: non-empty,
not `.` or `..`, none of `/ \ : * ? " < > |`, no control characters, and no trailing `.`
or space — Windows silently trims those, so `".. "` would collapse back to `".."`.
It lives with the other path-string helpers so the network side can share it
(`MP-MISSION-NAME-1`) without depending on the installer.

Both `ModInstallDir` overloads now return **empty** when the folder component, or the mod id
the manifest is keyed by, fails the predicate. Empty is a refusal, not a redirection: the
name is never repaired into something else, because "install it under a name of my
choosing" is exactly the outcome the caller did not ask for.

The refusal is then carried outward instead of being re-derived:

| layer | behaviour on an empty destination |
| --- | --- |
| `MakeStagedModInstall` (`ModInstall.cpp:168`) | returns an install with empty `stagingDir`/`backupDir` — no relative `.staging-N` beside the process cwd |
| `SwapStagedModInstalls` | refuses (`staged mod directory is missing`), so nothing is renamed or deleted |
| `FindPreservedStagedModInstall` | refuses, so a preserved download is not adopted into a bad path |
| `BuildCheckedDownloadTasks` (`OptionsUIApp.cpp:1629`) | sets `refused`, builds no tasks; the MODS screen discards the batch and says nothing was downloaded |
| `MakeModDownloadTask` (`ModDownloadSupport.hpp:78`) | returns a task with no destination, so the worker fails at the transport having written nothing |
| `DisplayMultiplayer` join (`:1184`) | refuses the join with a message before any task is created |

The MP join guard matters because there the folder name is chosen by the *server*, not by
the local catalog.

## Behaviour

| name | `IsSinglePathComponent` | `ModInstallDir("/x/mods", "fixture-mod", name)` |
| --- | --- | --- |
| `csla`, `@fixturemod`, `Mod Folder 2`, `ver2.0`, `naïve` | true | `/x/mods/<name>` |
| `..`, `.`, `../..`, `..\..\Documents`, `@../../x`, `anims/../..` | false | empty (refused) |
| `a/b`, `a\b`, `C:`, `C:/Windows`, `\\server\share` | false | empty (refused) |
| `.. ` (trailing space), `escape.` (trailing dot) | false | empty (refused) |
| `a*b` (Windows-reserved) | false | empty (refused) |

`ModInstallDir(root, modId)` with no folder name is `"@" + modId`, and `modId` is held to
the same rule, so `ModInstallDir("/x/mods", "carwars-2.5")` still gives
`/x/mods/@carwars-2.5` while a separator in the id is refused. An empty mod id, which used
to compose `"/@\"`, is now refused as well; no caller has one (every row, catalog entry and
server download carries an id).

## Files

- `engine/Poseidon/Foundation/Common/Filenames.hpp`: `IsSinglePathComponent`, next to
  `IsRelativePath`.
- `engine/Poseidon/Core/ModInstall.cpp`: both `ModInstallDir` overloads refuse;
  `MakeStagedModInstall` refuses an empty destination; `FindPreservedStagedModInstall`
  refuses one.
- `engine/Poseidon/Core/ModInstall.hpp`: the "empty means refuse" contract on the two
  overloads.
- `engine/Poseidon/UI/OptionsUIApp.cpp`: `BuildCheckedDownloadTasks` refuses the batch; the
  MODS apply handler discards staged installs and reports it.
- `engine/Poseidon/UI/ModDownloadSupport.hpp`: the catalog-to-task builder refuses.
- `engine/Poseidon/UI/DisplayUIMultiplayer.cpp`: the join flow refuses a server-chosen
  folder name before building tasks.
- `tests/unit/engine/Poseidon/Core/test_mod_install.cpp`: the two cases below.

## Toggle

n/a. This is path containment; a switch that restored the raw join would restore the
defect. The refusal is the only mode.

## Evidence

| run | result |
| --- | --- |
| `PoseidonTests "ModInstallDir refuses catalog names that are not one path component"` | 65 assertions pass — the accepted table, 14 hostile names through both overloads, and "stays directly under the root" for every accepted name |
| `PoseidonTests "a refused install destination cannot become a relative staging path"` | 8 assertions pass — empty destination/staging/backup, swap refuses, no move flags set |
| `PoseidonTests "ModInstallDir refuses*,a refused install*"` | 2 cases, 73 assertions, exit 0 |
| `PoseidonTests [mods]` | 72 cases: 70 passed, 0 failed, 2 skipped (the absent `pbo/addon_fixture.*` pair); 412 assertions pass |
| `PoseidonCoreTests` full | 896 cases: 872 passed, 0 failed, 24 skipped; 16,008 assertions pass — unchanged by this ticket (its cases live in `PoseidonTests`) |
| `cmake --build … --target PoseidonGame` | exit 0 (the UI call sites compile and link) |
| `python scripts/proteus/check-format.py` | OK |
| `python scripts/proteus/check-all.ps1` | OK |

`PoseidonTests` as a whole is **not** green, for reasons outside this ticket: the local DayZ
corpus's off-revision entries (`Compat smoke`), the `DrainInstanceOps` model-lookup ordering
assertion, and the dev-panel `ImGui::` wrapper invariant (7 needles left after
`G3c-panel-wrappers`). All three were failing before this change and none of them touches
`ModInstall`; the ticket's own tag is green.

## Risks

- A catalog entry whose folder name ends in `.` or a space, or contains one of the Windows
  reserved characters, is now refused. Such a folder is not reliably representable on
  Windows anyway — the OS trims or rejects it — so accepting it only "worked" for the
  install transaction's string comparisons, not for the directory it claimed to name.
- Refusing an empty mod id changes `ModInstallDir(root, "")` from `"/@"` to `""`. Nothing
  ever passed an empty id (the return fed `FindPreservedStagedModInstall` /
  `MakeStagedModInstall`, both of which would have been nonsense).
- Already-installed mods are untouched: `ScanModsRoot` reads real directory names from
  disk, and this predicate only gates *new* install destinations. A folder already on disk
  named `weird.` still loads.
- The exclusion rule is per component, so interior `..` in a name (`a..b`) is still
  allowed. It cannot traverse: traversal needs a separator, and separators are refused.
  (The master server now applies this same engine rule to published names —
  `papa_bear_client::catalog::is_single_path_component`, `MSERVER-PATHCHARSET-1` — so what
  it advertises and what this joins agree.)

## Follow-ups

- `MP-MISSION-NAME-1` (proposed) is the other finding from `PATH-JOIN-AUDIT-1`: a
  client-selected mission template name joined onto the MPMissions tree. Same predicate,
  different sink (read/mount, not write).
- `AddonSystem::ResolveAddonFile` still takes a free-form name and is only saved by its one
  literal caller; guard it or narrow the signature.
- (done) The master server validated `mod_id` when publishing but accepted `folderName`
  from the manifest unchecked; that is closed by `MSERVER-FOLDERNAME-1` and the rule it uses
  is now shared with the engine's set (`MSERVER-PATHCHARSET-1`).

## Verify on GPU

n/a — no rendering, no device state; a string predicate on the install path. Run
`PoseidonTests [mods]` (CPU) instead.
