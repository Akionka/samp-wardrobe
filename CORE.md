# Wardrobe core features

Wardrobe is a 32-bit Rust ASI plugin for GTA San Andreas and SA-MP. It changes
only local rendering: the server remains authoritative and other players never
receive the custom assets. Every matching streamed ped, local or remote, keeps
its SA-MP server model ID and receives a compatible cloned RenderWare clump.

This document describes the implemented behavior. The public installation and
JSON guide is [README.md](README.md); detailed ownership is in
[ARCHITECTURE.md](ARCHITECTURE.md).

## Startup and safe execution context

[`DllMain`](src/lib.rs) starts `plugin_thread` only on `DLL_PROCESS_ATTACH`.
The background thread initializes logging, loads the configuration, waits for a
supported `samp.dll`, verifies GTA 1.0 US code fingerprints, waits for GTA's
TXD system, and installs the frame hook. An unknown executable or patched
target leaves Wardrobe inactive.

`runtime::install` detours `CGame::Process`. The detour calls the original
function first, constructs [`GameFrame`](src/game_frame.rs), then runs
`Runtime::process_game_frame`. GTA and RenderWare mutations—including
[`gta::load_skin_source`](src/gta.rs),
[`gta::apply_skin_source`](src/gta.rs),
[`gta::restore_skin_source`](src/gta.rs), and
[`gta::release_skin_source_resources`](src/gta.rs)—therefore occur only on the
GTA frame thread.

## Configuration and matching

`wardrobe.json` is represented by [`SkinDefinition`](src/config.rs),
[`SkinRule`](src/config.rs), and [`SkinConfig`](src/config.rs). A definition
contains the enabled state, TXD/DFF paths, and donor model ID. A donor need
only be a valid GTA model ID in JSON; the game-thread loader additionally
rejects donors that are not `CPedModelInfo` ped models. The UI applies the same
numeric range validation.

Rules match exact player names, server model IDs, or both. Matching priority is
name plus model, then name, then model. Disabled rules and profiles do not
match. [`ConfigWatcher`](src/config.rs) accepts valid changed JSON about once
per second and retains the previous configuration after invalid edits.

## Universal clump skinning

For every matching [`StreamedPed`](src/samp.rs), Runtime reads the current
server model and render-object identity. It asks `SkinManager` for the
profile's prepared source. The cache holds one TXD, prepared source clump, and
validated donor per profile; `gta::apply_skin_source` clones that source for
each matching ped without changing `m_nModelIndex`.

Before installation, the bridge validates the source hierarchy, skin geometry,
all required ped bone tags, and live AnimBlend frame count. It prepares the
clone, transfers live animation associations, aborts secondary IK, replaces the
ordinary render object through GTA's virtual lifecycle, preserves the entity's
streaming/effect bookkeeping, copies bone pointers, and updates RenderWare and
HAnim state. The source and clone retain the server model ID, so local and
remote peds follow the same tested path.

`AppliedPlayer` stores only the profile ID, remembered server model ID, and
installed render-object identity. A changed render-object pointer, an address
reused with different geometry, or a changed model ID means SA-MP/GTA has
already reset the ped. Wardrobe drops the stale state and may apply the current
rule to the normal server representation. When a rule/profile is removed or
disabled, `gta::restore_skin_source` destroys the matching clone and invokes
`CPed::SetModelIndex` to rebuild the remembered normal clump while preserving
live animation associations.

## Reloading, failures, and retirement

[`SkinManager`](src/skin_loader.rs) owns one
[`SourceCache`](src/skin_loader/source_cache.rs). It checks each profile's
[`SkinSourceRevision`](src/config.rs)—the profile plus TXD/DFF metadata—at most
once per second. An unchanged source is reused for all peds. A source load
failure is suppressed until the profile or asset revision changes.

When a loaded profile changes, the cache retires its old source before a
replacement can load. Runtime restores every currently streamed user of that
profile to its normal server clump, retrying after transient restore failures.
Retained applied state also keeps its source desired when a transient ped read
fails. The old TXD/source is released only after a complete SA-MP ped scan
proves that none of the exact clone identities recorded for that source is
still live. Only then can a later scan load and apply the replacement source.

If a particular clone install fails, Wardrobe leaves or recovers the normal
server clump and stores an attempt fingerprint: profile, source generation,
server model, and render-object identity. It avoids retrying that unchanged
combination, preventing repeated work and log spam. A server reset, changed
render object/model, or new source generation makes the ped eligible again.

## SA-MP discovery and responsiveness

[`Samp::wait_for_load`](src/samp.rs) recognizes SA-MP 0.3.7-R1, R3-1, R4, and
0.3.DL-R1 by entry point. `Samp::streamed_peds` returns every readable streamed
ped with player ID, optional name, and GTA ped address. A missing name only
prevents name-based matching; model-based rules still work. A failed structural
scan is not treated as an empty scan, so player and source state remain safe.
`Samp::all_peds` provides the complete render-identity liveness scan required
for source retirement.

Runtime polls about every 200 ms. Optional guarded R1 SA-MP hooks request an
earlier poll after spawn or skin events; they never mutate GTA themselves.

## Verification

Run `cargo fmt --check`, `cargo test`, and strict Clippy for source changes.
Game-facing changes require in-game validation and inspection of
`wardrobe.log`. Test a profile on both the local player and several remotes,
then test skin resets, stream in/out, rule removal, a shared-asset reload, and
an incompatible clone that must remain on its server skin.
