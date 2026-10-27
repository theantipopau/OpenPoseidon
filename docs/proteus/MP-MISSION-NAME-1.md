# MP-MISSION-NAME-1 — a mission template name that leaves the mission tree no longer resolves

Status: done-unverified (CPU-only; nothing to verify on a GPU)

Found by `PATH-JOIN-AUDIT-1` as finding 2. Sibling of `MODINSTALL-PATH-1`: the same rule,
a read/mount sink instead of a write.

## Problem

Two places join a mission template name onto a directory and then touch the filesystem:

- `ResolveMPMissionTemplateBase` (`NetworkServerMission.cpp`) builds
  `std::filesystem::path(dir) / templateName` for every lookup directory and tests
  `<base>.pbo`;
- the dedicated-server mission loop (`NetworkServerSimulate.cpp`) tests
  `GetMPMissionsDir() + t + "/mission.sqm"` with the raw `"<mission>.<world>"` string.

Neither inspected the name. `..\..\somepbo.Intro` makes the join resolve outside the
MPMissions tree, so the server would test, mount and parse a PBO (or read a
`mission.sqm`) from anywhere on disk and treat it as one of its own templates.

The name is not packet text: it comes from the mission list the server selects from
(`server.cfg`'s `Missions/*/template`, or the host's mission browser, whose entries are
directory names). That makes this a content/config trust boundary rather than a remote
one — but it is also exactly the string a mod pack or a hand-edited config controls, and
nothing checked it. The fix is the shared component rule, so any future caller that does
receive the name over the wire is covered too.

## Approach

The predicate from `MODINSTALL-PATH-1` moved to
`Foundation/Common/Filenames.hpp` — it is a path-string rule, not an installer rule — and
is shared:

```cpp
inline bool Foundation::IsSinglePathComponent(const std::string& name);
bool Poseidon::IsMissionTemplateNameValid(RString templateName); // "mission.world"
```

Both halves of the resolver's arguments are checked separately, *before* the lookup loop:
`mission` and `world` are validated as one component each. Checking the concatenated
string would not do — a mission named `..` and a world named `Eden` concatenate to
`...Eden`, which is a legal (if odd) single component. The whole `"<mission>.<world>"` is
checked where that string itself is used as a path component (the simulate loop), via
`IsMissionTemplateNameValid`.

Refusal is an empty result, and every caller stops rather than substituting a name:

| site | on refusal |
| --- | --- |
| `ResolveMPMissionTemplateBase` | returns empty and logs; no directory is consulted, so the answer does not depend on whether an escaped file happens to exist |
| `NetworkServer::CreateMission` | logs, clears `_originalName`/`_missionBank` and `RemoveBank(MPCurrentPrefix)` — the "no mission" state |
| `NetworkServer::InitMission` | leaves `_missionBank` empty (its senders already treat an unreadable bank as "nothing to transfer") |
| `NetworkServer::SimulateDS` | `FailSimulateSmokeCheck("mission template name is not a single path component")` and stops before either join |

## Behaviour

| template | result |
| --- | --- |
| `addon_lookup`, world `Eden` | resolves as before (mod lookup, then the documented fallback) |
| `mfcti 1.16 everon`, world `eden` | resolves — interior dots and spaces are one component |
| mission `..`, world `Eden` | refused (empty) |
| mission `../..`, `..\..\x`, `@../../x`, `a/b`, `C:\missions\x` | refused (empty) |
| any mission, world `..` or `../..` | refused (empty) |
| `"<mission>.Eden"` containing `/`, `\`, or a drive | `IsMissionTemplateNameValid` false |

Nested missions keep working the way they already did: `GetMPMissionLookupDirectories`
enumerates the category folders under MPMissions and hands each as a lookup *directory*,
so a mission two folders deep is found by walking directories, never by putting a
separator in its name.

## Files

- `engine/Poseidon/Foundation/Common/Filenames.hpp`: `IsSinglePathComponent` (moved here
  from `Core/ModInstall.hpp`, where `MODINSTALL-PATH-1` first added it, so the network side
  shares one rule instead of a copy).
- `engine/Poseidon/Core/ModInstall.{hpp,cpp}`: use the shared predicate; no behaviour change.
- `engine/Poseidon/Network/NetworkServerCommon.hpp`: `IsMissionTemplateNameValid` and the
  "empty means refused" contract on the resolver.
- `engine/Poseidon/Network/NetworkServerMission.cpp`: per-half validation in the resolver;
  `CreateMission` / `InitMission` stop on a refused name.
- `engine/Poseidon/Network/NetworkServerSimulate.cpp`: the name is validated before the
  template path or `<dir>/mission.sqm` is built.
- `tests/unit/engine/Poseidon/Network/test_network_server.cpp`: the case below.

## Toggle

n/a. Path containment: a switch restoring the raw join would restore the escape.

## Evidence

| run | result |
| --- | --- |
| `PoseidonTests "MP mission template names that leave the mission tree are refused"` | 18 assertions pass — 7 hostile mission names, 2 hostile worlds, 5 hostile whole-template strings, and the two positive names with their two result checks |
| `PoseidonTests [network]` | 218 cases: 217 passed, 1 failed — `MP weapon-index OOB read in optics block`, the deliberately hidden `[.]` reproducer that reads out of bounds on purpose (excluded from the default run); 2,983 assertions |
| `PoseidonTests [mods]` | 72 cases: 70 passed, 0 failed, 2 skipped; 412 assertions (unchanged by the shared predicate) |
| `PoseidonCoreTests` full | 896 cases: 872 passed, 0 failed, 24 skipped; 16,008 assertions (unchanged) |
| `PoseidonTests` full | 4,082 cases: 3,925 passed, 3 failed, 154 skipped; 1,310,771 assertions (+18 = the new case). The 3 are the known non-mission failures (local DayZ corpus, `DrainInstanceOps` ordering, dev-panel `ImGui::` invariant) |
| `cmake --build … --target PoseidonGame` | exit 0 |
| `python scripts/proteus/check-format.py` | OK |
| `python scripts/proteus/check-all.ps1` | OK |

## Risks

- A mission whose *name* contains a separator was never resolvable through this path
  anyway (the lookup builds `<dir>/<name>.pbo`, so a separator simply pointed somewhere
  else). No supported layout loses anything.
- Names ending in `.` or a space are now refused. Windows trims those, so the old lookup
  and the actual file could disagree; refusing is the honest answer.
- `CreateMission` on a refused name clears `_originalName`. Callers that display "current
  mission" will show the empty state rather than a name the server did not mount, which
  matches what the server actually has.
- The resolver no longer reports a path for an invalid name, so a caller that ignored the
  empty return would see a dead-end fallback instead of a traversal. All current callers
  handle it.

## Follow-ups

- `server.cfg` mission entries are the remaining authoring surface: a load-time warning in
  the mission-list config parser would tell an admin *why* their template was refused,
  instead of the simulate loop failing with the smoke-check message.
- `AddonSystem::ResolveAddonFile` still takes a free-form name and is saved only by its one
  literal caller; guard it with the same predicate or narrow the signature.
- The master server accepts a publish manifest's `folderName` without the equivalent check
  (`mserver/MasterService/src/mods.rs:1024`); rejecting it there keeps traversal-shaped
  names out of every catalog.

## Verify on GPU

n/a — no rendering, no device state. Run `PoseidonTests [network][mission]` (CPU) instead.
