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
contains the enabled state and TXD/DFF paths. Legacy `donor_model_id` keys are
ignored so existing profiles remain valid. The MoonLoader UI neither displays
nor writes that key, while preserving it when another field in a legacy profile
is edited.

Rules match exact player names, server model IDs, or both. Matching priority is
name plus model, then name, then model. Disabled rules and profiles do not
match. `poll_interval_ms` controls complete-scan fallback delay (default 2000,
valid range 100–60000) and `log_level` selects error, warn, info, debug, or
trace output. [`ConfigWatcher`](src/config.rs) accepts valid changed JSON about
once per second and retains the previous configuration after invalid edits.

## Universal clump skinning

For every matching [`StreamedPed`](src/samp.rs), Runtime reads the current
server model and render-object identity. It asks `SkinManager` for the
profile's prepared source. The cache holds one TXD, prepared source clump, and
no model-info handle per profile; `gta::apply_skin_source` clones that source
for each matching ped without changing `m_nModelIndex`. Before cloning, it
validates that the ped's unchanged server model is an available `CPedModelInfo`
and uses that model info only to configure the clone's clump association,
lighting/render callbacks, and skin-atomic hierarchy.

Source loading validates the files, hierarchy, skin geometry, weights, and
bounds without depending on a GTA ped model. Before installation, the bridge
also validates the clone's live AnimBlend frame count. It prepares the clone,
transfers live animation associations, aborts secondary IK, replaces the
ordinary render object through GTA's virtual lifecycle, preserves the entity's
streaming/effect bookkeeping, copies bone pointers, and updates RenderWare and
HAnim state. The clone and ped retain the server model ID, so local and remote
peds follow the same tested path.

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

[`rak_samp::start_listener`](src/rak_samp.rs) starts after `samp.dll` is found. Its
worker waits up to 30 seconds for an ABI-v1-ready optional `rak_samp.asi` host,
then retains one incoming-RPC subscription for the process lifetime. The
callback coalesces `SetPlayerSkin`, player stream-in, and player stream-out
requests in an atomic flag and always continues traffic; it neither reads
payload data nor accesses GTA or Runtime.

Runtime invokes the throttled configuration watcher every GTA frame. It
performs a complete scan immediately after a valid configuration reload or a
consumed rak-samp request, and otherwise after the configured `poll_interval_ms`
delay. A missing,
incompatible, failed, or unregistrable rak-samp host emits one fallback message;
the configured scan remains active. Wardrobe installs no direct SA-MP event
hooks.

## Verification

Run `cargo fmt --check`, `cargo test`, and strict Clippy for source changes.
Game-facing changes require in-game validation and inspection of
`wardrobe.log`. Test a profile on both the local player and several remotes,
then test skin resets, stream in/out, rule removal, a shared-asset reload, and
an incompatible clone that must remain on its server skin.
