# Wardrobe core features

Wardrobe is a 32-bit Rust ASI plugin for GTA San Andreas and SA-MP.  It changes
only what the local game renders: the SA-MP server remains authoritative and
other players do not receive the custom model.  A configured rule selects a
skin for a streamed player. Remote players use a private GTA ped-model slot;
the local player instead receives a cloned RenderWare clump while its
server-supplied model ID remains unchanged.

This document describes the implemented behavior and points to the code that
owns it.  The public installation and JSON guide is [README.md](README.md);
the detailed type and function relationships are in
[architecture.md](architecture.md).

## Startup and safe execution context

[`DllMain`](src/lib.rs) starts `plugin_thread` only on
`DLL_PROCESS_ATTACH`.  The background thread initializes logging through
[`logging::init`](src/logging.rs), loads the initial configuration, waits for a
supported `samp.dll`, verifies the supported GTA 1.0 US PE/code fingerprints,
waits for GTA's TXD/model system, and installs the runtime hook with
[`runtime::install`](src/runtime.rs). An unknown executable or an ASI-patched
fixed GTA target is logged and leaves Wardrobe inactive.

The background thread never loads a model or changes a ped.  `runtime::install`
detours GTA's `CGame::Process`; `game_process_detour` calls GTA's original
function first, creates a [`GameFrame`](src/game_frame.rs) capability, and then
runs `Runtime::process_game_frame`.  Only that detour can construct the
capability, and GTA/RenderWare mutation APIs require it.  All GTA and
RenderWare changes—particularly [`gta::load_skin`](src/gta.rs),
[`gta::load_instance_skin`](src/gta.rs),
[`gta::release_skin_resources`](src/gta.rs), and
[`gta::set_ped_model_index`](src/gta.rs)—therefore happen on GTA's frame
thread.

## Configuration and matching

The editable `wardrobe.json` is represented by three types in
[src/config.rs](src/config.rs):

- [`SkinDefinition`](src/config.rs) identifies one profile's enabled state,
  TXD/DFF paths, and vanilla donor model ID.
- [`SkinRule`](src/config.rs) links a profile to an exact player name, server
  model ID, or both.
- [`SkinConfig`](src/config.rs) contains the profile map and ordered rule list.

`load_initial` creates an empty configuration file if it does not exist.
Otherwise, the private `parse` function validates it before activation:
enabled profiles need both asset paths, donors must be ordinary GTA model IDs,
rules must point to known profiles, selectors cannot be empty, and duplicate
selectors are rejected.  The shared range checks live in
[src/model_ids.rs](src/model_ids.rs), where Wardrobe reserves `18000..20000`
for private models.

[`SkinConfig::matching_rule`](src/config.rs) applies a deterministic priority:
name plus server model, name only, then server model only.  Matching is exact
and case-sensitive for player names.  Disabled rules and disabled profiles do
not match.  See [wardrobe.example.json](wardrobe.example.json) for the
supported file format.

## Live configuration and asset reloads

[`ConfigWatcher::poll_change`](src/config.rs) checks `wardrobe.json` at most
once per second.  It returns a replacement only when its file revision changes
and the new JSON validates.  Missing, unreadable, or invalid edits are logged;
the currently active configuration is kept.

`Runtime::reload_config_if_changed` in [src/runtime.rs](src/runtime.rs) applies
each valid replacement to [`SkinManager`](src/skin_loader.rs) and clears the
one-time matching log state.  `SkinManager::model_for` independently tracks a
[`SkinSourceRevision`](src/config.rs) (definition plus TXD/DFF metadata).  It
reuses an unchanged private model, notices an edited asset or definition within
about a second, and attempts a controlled replacement.  A failed reload keeps
the prior working model when one exists and throttles repeated failures.
`SkinManager::instance_for` performs the same revision check for its separate
local-instance source cache. It first retires a changed source so Runtime can
rebuild the normal local-player clump; the replacement source is loaded and
applied on a later scan.

The optional [MoonLoader editor](moonloader/wardrobe_ui/wardrobe_ui.lua) edits
the same JSON file and writes it atomically, so the watcher does not observe a
partial file. Its rule editor can list connected SA-MP players and fill the
player-name selector. The editor also owns an optional top-level `presets`
field: each named preset records only the enabled state of the existing skins
and rules. Creating one captures the current state; clicking one applies it,
and later toggle changes update the selected preset until **Save JSON**. Rust
ignores the UI-only field and continues to own matching and asset loading.

## SA-MP version detection and player discovery

[`Samp::wait_for_load`](src/samp.rs) waits for `samp.dll`, reads the PE
`AddressOfEntryPoint`, and selects one verified private `SampLayout`.  It
supports SA-MP 0.3.7-R1, R3-1, R4, and 0.3.DL-R1; unknown revisions are
rejected before player-pool memory is read.  The researched entry points and
layouts are documented in [docs/samp-addresses.md](docs/samp-addresses.md).

[`Samp::streamed_peds`](src/samp.rs) returns [`StreamedPed`](src/samp.rs)
values containing a SA-MP player ID, local-player flag, optional decoded name,
and GTA ped address. An unreadable or malformed name is isolated to that
player: only name-based matching is skipped, while server-model rules continue
to work.
Its `None` result means a required player-pool or ped read was incomplete, not
that no players are streamed. `Runtime::process_game_frame` retains all player
state in that case. [`Samp::all_peds`](src/samp.rs) is a separate complete scan
used when deciding whether old private resources may be destroyed.

All game-owned memory is read through [`memory::read`](src/memory.rs) or
`memory::read_bytes`, which use `ReadProcessMemory` rather than directly
dereferencing a GTA/SA-MP pointer. `memory::read` accepts only a sealed set of
primitive and raw-pointer types whose every bit pattern is valid. An unreadable
or stale pointer therefore causes a fallible scan rather than an access
violation on the game thread.

## Applying and restoring a custom player model

`Runtime::process_game_frame` normally polls every 200 ms. For each
`StreamedPed`, it reads the current model using [`gta::ped_model_id`](src/gta.rs)
and finds a matching `SkinRule`. Remote peds keep the original path: Runtime
asks `SkinManager::model_for` for the profile's private model and assigns it
with [`gta::set_ped_model_index`](src/gta.rs).

The local ped uses `SkinManager::instance_for` instead. The source loader
normalizes its skin weights and expands its render bounds exactly as GTA does
for a streamed ped model. Before touching the ped, the GTA bridge clones that
cached source, verifies that all 18 CPed bone tags are present and that the
hierarchy, skin, and live ped have the same frame count, attaches the hierarchy
to the skin atomic, creates its initial RenderWare animation, initializes
AnimBlend, fills a temporary bone array, and associates the clump with the
configured donor `CPedModelInfo`. It transfers the live animation-association
list using the same extract/give pair GTA uses for CJ clothing rebuilds,
including immediately aborting the secondary IK manager so no IK chain retains
pointers into the old bone data.

The bridge then destroys the ordinary render object through the ped's virtual
`CEntity::DeleteRwObject`. It briefly invokes `CEntity::CreateRwObject` and
destroys only that temporary clump so the entity retains a balanced model
reference, streaming link, and effects while the prepared clone is installed
at `CEntity::m_pRwClump`. Finally it copies the prepared bone pointers and calls
`CEntity::UpdateRwFrame` and `UpdateRpHAnim`; the clone receives the already
positioned temporary clump's root transform through `RwFrameTransform` before
that temporary object is destroyed, which dirties the RenderWare hierarchy and
updates its world transform. This path never calls `CModelInfo::AddPedModel` or
`CPedModelInfo::SetClump`, and never changes the local ped's `m_nModelIndex`.

Before Wardrobe changes either representation, it records the ordinary server
model in `AppliedPlayer::last_server_model_id`. When a rule or profile is
disabled, removed, changed, or no longer matches,
`Runtime::restore_server_model` restores that model. For the local path the
custom clone is first destroyed through the virtual lifecycle, then
`SetModelIndex` rebuilds GTA's normal clump even though the saved index is
already correct. Current animations are transferred to the rebuilt clump. The
applied local state records the exact installed render-object pointer, its
shared geometry identity, and the server model ID present at installation. A
clump-identity or model-ID mismatch means SA-MP already reset the ped. The
geometry check distinguishes a normal server clump even when GTA's allocator
reuses the same clump address. Wardrobe drops stale state and can reapply the
current rule normally. `prune_streamed_out_players` retains
orphaned local instance state until a complete ped scan proves its render
object is gone.

## TXD/DFF loading and resource retirement

[`SkinManager::model_for`](src/skin_loader.rs) owns the remote private-model
lifecycle.
It requests [`gta::load_skin`](src/gta.rs) when no current resource can be
reused.  The GTA bridge validates the TXD and DFF paths, verifies that the
configured donor is a `CPedModelInfo` (not a vehicle or object), allocates or
reuses a private model ID, creates a TXD slot, clones the safe donor metadata,
loads the DFF clump under that TXD, and performs GTA's ped-specific clump
setup.  The returned [`SkinResources`](src/gta.rs) stores the private model ID
and TXD slot.

Replacing a profile or making it no longer referenced moves its old resources
to `RetiredSkin`.  `SkinManager::cleanup_retired` keeps each retired resource
for at least one second and waits for `Samp::all_peds` to prove no live ped uses
the old model ID.  Only then does it call
[`gta::release_skin_resources`](src/gta.rs), which destroys the RenderWare
clump, removes the TXD reference and slot, and leaves the model-info entry
inert.  The ID can then enter the manager's recyclable pool.  If cleanup fails
or the player scan is incomplete, the resources stay protected for a later
frame-thread retry.

The separate instance cache owns an [`InstanceSkinResources`](src/gta.rs)
value containing a TXD slot, raw source clump, and validated donor pointer, but
no GTA model ID. `gta::load_instance_skin` streams the source DFF under its TXD,
requires a skinned hierarchy with GTA's complete CPed bone set, and configures
the source's ped render callback, skin hierarchy, and donor model metadata. A
retired instance source is released only after a complete ped scan and the
applied-pointer state prove its local clone is gone. Failures before mutation
destroy the unattached clone and leave the ordinary ped untouched; failures
after virtual destruction recover the server clump and transfer the saved
animation associations before returning without applied local state.

## Responsive updates with safe polling fallback

Polling is the baseline: it detects server skin changes on every supported
SA-MP revision.  [src/samp_hooks.rs](src/samp_hooks.rs) adds optional immediate
refreshes for an unmodified 0.3.7-R1 client.  `samp_hooks::install` confirms a
version marker and exact target signatures before enabling hooks for remote
player spawn and the skin RPC.

Each SA-MP detour calls the original handler, then sets an atomic refresh bit.
[`samp_hooks::take_refresh_request`](src/samp_hooks.rs) lets the next
`Runtime::process_game_frame` skip its normal poll delay.  The hook never
loads a skin or mutates GTA itself.  If the version or byte signatures differ,
installation logs the reason and Wardrobe continues with normal polling.

## Failure behavior and verification

The code favors continuity and safe cleanup: unsupported GTA executables or
patched fixed targets stop before any detour or GTA call; invalid JSON preserves
the active configuration, bad assets preserve a previous skin where possible,
incomplete SA-MP scans preserve player and resource state, and failed GTA
teardown is retried. Diagnostic output goes to `wardrobe.log` via
[src/logging.rs](src/logging.rs).

Unit tests next to [src/config.rs](src/config.rs), [src/samp.rs](src/samp.rs),
and [src/samp_hooks.rs](src/samp_hooks.rs) cover rule selection, model-ID
validation, supported SA-MP fingerprints/layouts, and hook-signature helpers.
Use `cargo fmt --check` and `cargo test` for source changes; validate any
GTA-facing change in a supported game client and inspect `wardrobe.log`.
