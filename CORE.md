# Wardrobe runtime behavior

Wardrobe is a 32-bit Rust ASI for GTA San Andreas and SA-MP. It changes local
rendering only: matching local and remote peds keep their server model IDs while
receiving compatible cloned RenderWare clumps.

## Safe execution

The startup thread loads configuration, waits for a supported `samp.dll`,
validates GTA 1.0 US call targets, and installs the `CGame::Process` detour.
The detour calls the original function first, then creates `GameFrame` for
`Runtime`. Every GTA and RenderWare mutation runs through that frame-thread
capability; background and network callbacks do not touch game state.

## Configuration and matching

`wardrobe.json` contains skin definitions, rules, `poll_interval_ms`, and
`log_level`. Rules match name, server model, or both; combined matches take
precedence over name-only, then model-only. Valid changed JSON is reloaded about
once per second, while invalid edits preserve the active configuration.

## Skin lifecycle

`SkinManager` caches one prepared TXD/source clump per profile. Runtime clones
that source for each matching ped without changing its server model ID. It keeps
the profile, server model, and render identity for every installed clone so it
can detect server resets, restore removed rules, and suppress unchanged failed
applications.

Changed or removed sources retire first. Wardrobe restores every known user of
the old source, then releases it only after a complete liveness scan confirms
that none of its clone identities remain. This prevents freeing a source still
used by a streamed ped.

## Scanning and events

Runtime checks the throttled configuration watcher every frame and performs a
complete scan after a valid reload, a coalesced rak-samp request, or the
configured fallback interval. The optional `rak_samp.asi` listener starts after
`samp.dll` is found and coalesces `SetPlayerSkin`, player stream-in, and player
stream-out requests. Its callback always continues traffic and never accesses
Runtime or GTA.

## Verification

Run formatting, tests, and strict Clippy for source changes. For game-facing
work, inspect `wardrobe.log` and test skin application, reset recovery,
streaming, rule removal, and source reloads.
