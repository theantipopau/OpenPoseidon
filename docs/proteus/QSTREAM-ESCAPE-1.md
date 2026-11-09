# QSTREAM-ESCAPE-1 — a mod-relative path that escapes its root can no longer resolve

Status: done-unverified (CPU-only fix; nothing to verify on a GPU)

## Problem

`FIXTURE-SKIP-1` left this visible as known failure 4:

> `PoseidonCoreTests`: `QIFStreamB FileExist` resolves
> `..\@voicemod\voice\Jari\reportstatus.wss` to a file inside the mod, i.e. a path
> that escapes its prefix still resolves.

`QIFStreamB::FileExist` / `AutoOpen` fall back to "is this a loose file in one of the
active mod folders?", which joins a caller-supplied **relative** name onto each mod
directory:

```
<root>/@voicemod  +  /  +  ../@voicemod/voice/Jari/reportstatus.wss
```

Windows collapses `@voicemod/..` at open time, so that resolves to
`<root>/@voicemod/voice/Jari/reportstatus.wss` — the file inside the mod the path was
supposed to have left. The join is the bug: the `..` is only meaningful as "go up from
the game root", and climbing above that root names something outside it.

Two helpers do this join, and both had the hole:

- `ResolveModRootAlias` (name starts with a mod folder name) — `@voicemod\..\..\@voicemod\voice\x`
  would join onto the mod directory and then climb out of it and back in.
- `ResolveModOverride` (any relative name, tried against every mod folder) — the case the
  test hits.

## Approach

One guard, applied to the part that is about to be joined, before any directory is
consulted:

```cpp
bool EscapesRoot(const char* relPath);   // true when a ".." walks above its own start
```

It walks the path's components, tracking depth: a component is `+1`, `..` is `-1`, `.`
and empty components (adjacent separators) are ignored, and a `..` at depth 0 means the
path leaves the root it will be joined onto. `ResolveModRootAlias` checks the tail after
the prefix component; `ResolveModOverride` checks the whole relative name.

This is refused *before* the filesystem is asked, so the answer does not depend on
whether the escaped-and-re-entered file happens to exist.

## Behaviour that stays

A `..` that stays inside the root is still resolved, and the same test pins that:

| path | result |
| --- | --- |
| `@voicemod\voice\Jari\reportstatus.wss` | true (mod root alias) |
| `voice\..\@voicemod\voice\Jari\reportstatus.wss` | true (collapses, never climbs above the root) |
| `voice\..\@voicemod\voice\Adam\reportstatus.wss` | false (no such file) |
| `..\@voicemod\voice\Jari\reportstatus.wss` | **false** (was true) |

The last row is the fix. `AutoOpen` goes through the same two helpers, so opening — not
just the existence check — is closed by the same guard.

## Files

- `engine/Poseidon/IO/Streams/QBStream.cpp`: `EscapesRoot` (anonymous namespace, next to
  `LastPathComponent`); guard in `ResolveModRootAlias` and `ResolveModOverride`.

## Toggle

n/a. This is a correctness/containment fix, not a behaviour behind a switch: a toggle
that restored the old path would restore the escape, which is the defect. The default
(escape refused) is the only mode.

## Evidence

| run | before | after |
| --- | --- | --- |
| `PoseidonCoreTests "QIFStreamB FileExist - mod path escaping its prefix"` | 6 assertions: 5 passed, 1 failed (`!true`) | 6 assertions pass |
| `PoseidonCoreTests [qstream]` | 132 cases: 110 passed, 22 skipped | same; 499 assertions pass |
| `PoseidonCoreTests` full | 895 cases: 870 passed, **1 failed**, 24 skipped | 895 cases: **871 passed, 0 failed**, 24 skipped; 15,994 assertions |
| `PoseidonTests` full | 3 failed / 154 skipped (pre-existing) | unchanged: 3 failed / 154 skipped |
| `check-format --base HEAD` | — | OK |

## Risks

- A mod that reached a *shared* directory by climbing out of its own folder
  (`<mod>\..\..\shared\...`) no longer resolves through the mod-join path. That is the
  same class of escape and is now refused; such content must be addressed from the game
  root instead.
- The direct loose open (`QIFStream::FileExists(name)`) is untouched: a name is allowed
  to be relative to the game root and the OS is free to collapse `..` inside it. Only
  the reinterpretation of a name as *mod*-relative is guarded.

## Follow-ups

- The same "join a caller-supplied relative name onto a directory" pattern exists in the
  bank paths (`NormalizeAddonBankPath` relies on `CollapseParentDirs`, which already
  refuses to climb above its first component). Worth one audit if another escape is
  reported.
- `FIXTURE-SKIP-1`'s known-failure list still shows this item as open; that document is
  the record of that ticket, so it is left as written.

## Verify on GPU

n/a — no rendering, no device state; the change is a string walk in the file
open/existence path. Run `PoseidonCoreTests` (CPU) instead.
