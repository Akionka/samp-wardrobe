# Wardrobe core features

Wardrobe is a 32-bit Rust ASI plugin for GTA San Andreas and SA-MP.  It changes
only what the local game renders: the SA-MP server remains authoritative and
other players do not receive the custom model.  A configured rule selects a
skin for a streamed player, then the plugin loads its TXD/DFF assets into a
private GTA ped-model slot and assigns that slot to the local ped.

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
function first and then runs `Runtime::process_game_frame`.  All GTA and
RenderWare changes—particularly [`gta::load_skin`](src/gta.rs),
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

The optional [MoonLoader editor](moonloader/wardrobe_ui/wardrobe_ui.lua) edits
the same JSON file and writes it atomically, so the watcher does not observe a
partial file. Its rule editor can list connected SA-MP players and fill the
player-name selector. The editor also owns an optional top-level `presets`
field: each named preset records only the enabled state of the existing
profiles and rules. Applying one stages those states until **Save JSON**; Rust
ignores the UI-only field and continues to own matching and asset loading.

## SA-MP version detection and player discovery

[`Samp::wait_for_load`](src/samp.rs) waits for `samp.dll`, reads the PE
`AddressOfEntryPoint`, and selects one verified private `SampLayout`.  It
supports SA-MP 0.3.7-R1, R3-1, R4, and 0.3.DL-R1; unknown revisions are
rejected before player-pool memory is read.  The researched entry points and
layouts are documented in [docs/samp-addresses.md](docs/samp-addresses.md).

[`Samp::streamed_peds`](src/samp.rs) returns [`StreamedPed`](src/samp.rs)
values containing a SA-MP player ID, name, and GTA ped address.  Its `None`
result means a required player-pool read was incomplete, not that no players
are streamed.  `Runtime::process_game_frame` retains all player state in that
case.  [`Samp::all_peds`](src/samp.rs) is a separate complete scan used when
deciding whether old private resources may be destroyed.

All game-owned memory is read through [`memory::read`](src/memory.rs) or
`memory::read_bytes`, which use `ReadProcessMemory` rather than directly
dereferencing a GTA/SA-MP pointer.  An unreadable or stale pointer therefore
causes a fallible scan rather than an access violation on the game thread.

## Applying and restoring a custom player model

`Runtime::process_game_frame` normally polls every 200 ms.  For each
`StreamedPed`, it reads the current model using [`gta::ped_model_id`](src/gta.rs),
finds a matching `SkinRule`, and asks `SkinManager::model_for` for the profile's
private model.  If the ped has a different model, it calls
[`gta::set_ped_model_index`](src/gta.rs) and records an `AppliedPlayer` entry.

Before Wardrobe assigns a private model, it records the ordinary server model
in `AppliedPlayer::last_server_model_id`.  When a rule or profile is disabled,
removed, or no longer matches, `Runtime::restore_server_model` restores that
saved model.  If SA-MP already set a newer normal model, Wardrobe drops its
state without replacing the newer server choice.  `prune_streamed_out_players`
discards tracking only after a successful streamed-ped scan confirms the
player has left the pool.

## TXD/DFF loading and resource retirement

[`SkinManager::model_for`](src/skin_loader.rs) owns the custom skin lifecycle.
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
