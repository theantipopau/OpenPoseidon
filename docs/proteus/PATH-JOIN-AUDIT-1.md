# PATH-JOIN-AUDIT-1 — every directory join that takes a supplied name, and what it does with `..`

Status: done-unverified (docs + one test-only change; no engine behaviour touched)

Follow-up of `QSTREAM-ESCAPE-1`, which closed the two *mod*-relative joins and left its
ledger entry reading "audit the remaining directory-join sites (bank paths)".

## The rule this audit applies

`QSTREAM-ESCAPE-1` fixed one shape: **a relative name is joined onto a base directory
that is not the game root, and the OS collapses `..` at open time, so the name leaves
that base and the result is still attributed to it.** `..\\@voicemod\\voice\\x` opened
`@voicemod`'s own file: a name that escapes its prefix still resolved, and the answer
("that file is in this mod") was wrong in a way nothing reported.

That rule only bites where the join *reinterprets* the name. A name joined onto the game
root is just a root-relative path — `QIFStream::FileExists` deliberately allows those to
contain `..`, and the same caller can spell any root-relative path directly, so no
capability is gained. The audit therefore separates two populations:

- **A — namespace joins.** The base names a content root (mod folder, addon folder, addon
  bank) and the join decides which namespace answers. Escape here is the QSTREAM defect.
- **B — ordinary path building.** The base is a directory, a literal leaf is appended, or
  the name comes from an enumeration; escaping lands somewhere the caller could already
  name. Not the defect, but listed so the next reader does not have to re-derive that.

## A — namespace joins

| site | join | name comes from | verdict |
| --- | --- | --- | --- |
| `QBStream.cpp:136` `ResolveModRootAlias` | `<mod>/` + rest of the name | any relative name starting with a mod folder | **closed** — `EscapesRoot(rest)` (`:149`) |
| `QBStream.cpp:229` `ResolveModOverride` | `<mod>/` + name, tried per mod | any relative name | **closed** — `EscapesRoot` (`:240`) |
| `QBStream.cpp:181` `CollapseParentDirs` | collapses in-name `<seg>/..` pairs, then the loose retry opens the result | any name containing `..` | **sound** — refuses a `..` at depth 0, so the collapsed name never walks above the game root; the retry is root-relative by design |
| `QBStream.cpp:218` `NormalizeAddonBankPath` → bank fallback (`AutoOpen` `:2574`, `FileExist` `:2661`) | bank *name* + member, looked up in `QFBank::_files` (`:1002`) | any name that collapses to `addons\\…` | **sound** — the result indexes an in-archive table, never a directory, and the collapse cannot pass the root (row above) |
| `AddonSystem.cpp:39` `ResolveAddonFile` | addon prefix + name | the caller's `const char*` | **closed** — refuses a `name` that is not one relative component (`:41`) before the join or the `directory_iterator` scan, so the helper no longer depends on its caller passing a literal |
| `AddonSystem.cpp:177/213/256` `ParseAddonConfig` | addon prefix + `config.cpp` / `config.bin` / `stringtable.csv` | engine literals | cleared (literal leaves) |
| `ConfigParsers.cpp:33` `ResolveFileInDir` | mod dir + name | `config.cpp`, `config.bin`, `resource-extra.cpp`, `config-extra.cpp` | cleared (literal leaves) |
| `GameState.cpp:258` `BankList::Load(path + bName)`; `:331` `GFileBanks.Load(root + stem)` | directory + bank file | `_findfirst` / `directory_iterator` names | cleared (a real filename from an enumeration cannot contain a separator) |
| `GameState.cpp:285` `LoadBanksCallback` | mod dir + `ctx->path` | engine literals (`addons`, `dta`, `Campaigns`) | cleared (literal) |
| `Font.cpp:200` | `ResolveModOverride(ttfPath)` | font path from the config | cleared (goes through the guarded joiner) |
| `ModCollection.cpp:416/472/487` `ResolveModPathList` | mods dir + requested path | `mods.json` / CLI `--mod` | **no boundary to violate** — an absolute mod path is explicitly supported, so a `..` segment grants nothing that `C:\\…` does not |
| `ModArchive.cpp:209/229` `Unpack` | destination root + archive entry name | PBO entry names (content) | **guarded** — `IsSafeEntryPath` (`:130`) rejects absolute, drive-qualified and `..` names before the write loop |

## B — ordinary path building (checked, not changed)

| site | why it is not the QSTREAM defect |
| --- | --- |
| `Shots.cpp:1932`, `SoundScene.cpp:442`, `UIControlsExt.cpp:1296`, `DisplayUI.cpp:124/150`, `UIArcadeWaypoint.cpp:924` — mission dir + name | the base is inside the game root, and escaping it yields a root-relative name the same mission content can write directly |
| `SoldierOldMove.cpp:1022/1219`, `NetworkClientOnMessage.cpp:320`, `NetworkClientActions.cpp:586` — `scripts\\` + name | same: `..` only walks back to the root, which is already nameable |
| `EmatMaterialAdapter.hpp:173` `DeployedEmatPath` | content-supplied texture path joined onto a deploy root; escape reads a root-relative file the material can already name (note: `IsUnderEmatDeployRoot` classifies by prefix, so `root\\..\\x` counts as "under root" — a labelling wrinkle, not a traversal) |
| `NetworkServerSimulate.cpp:570/586`, `NetworkServerMission.cpp:260/270` | **exposed** — see finding 2 |
| `ModInstall.cpp:318/323` `ModInstallDir` | **exposed** — see finding 1 |
| save/profile/campaign joins (`GetSaveDirectory()` + literal, `GetCampaignSaveDirectory(campaign)` + literal) | fixed leaves; `campaign` comes from the enumerated campaign list or a local save archive, not from the network |

## Finding 1 (exposed, destructive) — the workshop installer joins a remote folder name onto the mods root

`ModInstall.hpp:38` documents `ModInstallDir` as `<modsRoot>/<folderName>, falling back to
@<modId>`, and that is exactly what `ModInstall.cpp:323` does: `modsRoot + "/" + folderName`.
`folderName` and `modId` are copied verbatim out of the master-server catalog
(`MasterServerServiceClient.cpp:910`), then used as a *directory* by:

- `ModDownloadSupport.hpp:77` and `OptionsUIApp.cpp:1628/2193` → `ModInstallDir`,
- `MakeStagedModInstall` (`ModInstall.cpp:168`) → `<dest>.staging-N`, `<dest>.backup-N`,
- `SwapStagedModInstalls` / `RestoreStagedModInstalls` / `DiscardStagedModInstalls`
  (`:180-295`) → `fs::remove_all`, `fs::rename`, and `ModArchive::Unpack` into the staging
  directory.

A catalog entry with `folderName: "..\\..\\Documents\\x"` therefore resolves to
`<modsRoot>/../../Documents/x`. `SwapStagedModInstalls` (`:197`) renames that directory
to `<dest>.backup-N` and renames the freshly unpacked staging directory into its place;
`CommitStagedModInstalls` (`:231`) then deletes the backup, so the original directory's
contents are gone, and `RestoreStagedModInstalls`' failure path `remove_all`s the
destination it had just replaced. The artifact's size and SHA-256 are verified
(`:100-160`) — the *path* never is. The `@<modId>` fallback is no safer: `@..\\..\\x`
still has `..` as its second component.

Repro, no network needed: `ModInstallDir("/m", "id", "..\..\payload")` returns
`/m/../../payload`; `MakeStagedModInstall(that)` returns staging/backup paths outside
`/m`; `SwapStagedModInstalls` renames them and `CommitStagedModInstalls` deletes the
backup.

Proposed shape: one predicate (`single relative component`: non-empty, no `/` or `\\`, not
`.`/`..`, no `:`, no control chars) shared by both overloads; reject at the task builders
so the row is reported as unusable instead of installed somewhere unexpected.

**Closed by `MODINSTALL-PATH-1`.** `IsSinglePathComponent` now guards both overloads, which
return empty instead of a joined path; every layer below treats empty as a refusal,
`MakeStagedModInstall` refuses an empty destination, and both task builders plus the MP join
refuse the batch. See `docs/proteus/MODINSTALL-PATH-1.md`.

## Finding 2 (exposed, read-only) — a client-supplied mission template name is joined onto the MPMissions tree

`ResolveMPMissionTemplateBase` (`NetworkServerMission.cpp:243`) takes the mission+world
string the client asked for and does `std::filesystem::path(dir) / templateName` (`:260`),
and `NetworkServerSimulate.cpp:586` does `GetMPMissionsDir() + t + "/mission.sqm"`. A
template name of `..\\..\\somepbo.Intro` makes the server test and mount a `.pbo` outside
its missions directory. The sink is a read/mount (plus the `mission.sqm` existence test),
so the impact is template selection outside the intended tree, not a write.

**Closed by `MP-MISSION-NAME-1`.** `ResolveMPMissionTemplateBase` validates the mission and
world halves separately before any lookup directory is joined and returns empty on refusal,
the dedicated-server loop checks the whole `"<mission>.<world>"` component before building
either join, and both callers stop instead of substituting a name. See
`docs/proteus/MP-MISSION-NAME-1.md`.

## Evidence

New test — the bank namespace had none, and it is the helper the ledger named:

| run | result |
| --- | --- |
| `PoseidonCoreTests "QIFStreamB FileExist - bank path names never climb above the game root"` | 14 assertions pass |
| `PoseidonCoreTests [qstream]` | 133 cases: 111 passed, 22 skipped; 513 assertions pass (the 22 skips are the absent-fixture cases, none of them this one) |
| `cmake --build … --target PoseidonCoreTests` | exit 0 |
| `PoseidonCoreTests` (full) | 896 cases: 872 passed, 0 failed, 24 skipped; 16,008 assertions pass |
| `python scripts/proteus/check-format.py` | OK |

The test writes its own one-member PBO in `%TEMP%` (`WriteSingleMemberPbo`) rather than
using `pbo/addon_fixture.pbo`, which is one of the families this checkout does not ship —
a bank-namespace test that skips is no evidence at all. It pins:

| name | expected | why |
| --- | --- | --- |
| `addon_fixture\\stringtable.csv` | resolves | the bank's own prefix |
| `anims\\..\\addons\\addon_fixture\\stringtable.csv` | resolves | the documented purpose of the fallback: a root-relative name that reaches `addons/` the long way |
| `addons\\..\\addons\\addon_fixture\\stringtable.csv` | resolves | same collapsed form |
| `..\\addons\\addon_fixture\\stringtable.csv` | does not resolve | climbs above the game root; `CollapseParentDirs` refuses at depth 0 |
| `x\\..\\..\\addons\\addon_fixture\\stringtable.csv` | does not resolve | climbs above the root after collapsing `x` |
| `..\\addon_fixture\\stringtable.csv` | does not resolve | same, without the `addons` spelling |
| `AutoOpen` on the resolving / climbing names | opens / fails | `AutoOpen` shares the fallback, so both entry points must agree |

Observation, not asserted: the plain root-relative spelling `addons\\addon_fixture\\…`
(no `..`) does **not** reach the bank, because the fallback only runs for names
`CollapseParentDirs` finds a `..` in. That asymmetry is harmless today (the name is also
tried as a loose file) and is recorded here rather than pinned, so a future cleanup may
change it deliberately.

## Risks

- Apart from the `ResolveAddonFile` guard (which only changes what a name the sole caller
  never passes would have done), no behaviour changed, so the risks are of the audit being
  wrong, not of the code moving. The two "cleared" rows that a reviewer should re-check
  first are `ModCollection` (the absolute-path allowance is deliberate — verify that is still
  the intended contract) and the mission/script joins (they assume content may name any
  root-relative path).
- The new test asserts existing behaviour; if a future ticket makes the bank fallback
  refuse every `..`, the three positive rows fail and the reason is written down here.

## Follow-ups

- `MODINSTALL-PATH-1` (**done**): the catalog folder name / mod id is validated before it
  becomes an install path; the sink was `remove_all`/`rename`, so it was fixed as a bug, not
  filed as hardening. See `docs/proteus/MODINSTALL-PATH-1.md`.
- `MP-MISSION-NAME-1` (**done**): the mission template name is one relative component or it
  is refused, at the resolver and in the dedicated-server loop. See
  `docs/proteus/MP-MISSION-NAME-1.md`.
- `AddonSystem::ResolveAddonFile` (**done**): guarded with the same
  `Foundation::IsSinglePathComponent` predicate, at the top of the helper, so both the join
  and the case-insensitive directory scan are unreachable for a `..`/separator/absolute name.
  The signature is unchanged, so the literal caller needs no change and the refusal is
  invisible to it (it already treats `false` as "no stringtable"). No test was added: the
  helper lives in an anonymous namespace in a translation unit no test target links, and the
  predicate it now calls is already covered by `test_mod_install.cpp`.
- Directory enumeration elsewhere (`DirScanner`, `_find*`) supplies real filenames and is
  not audited further; a filesystem entry containing a separator is not representable.
