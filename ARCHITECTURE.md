# Wardrobe architecture

This document maps Wardrobe's modules, data types, and function contracts.
The plugin is a Rust `cdylib` loaded into a 32-bit GTA San Andreas process;
addresses, layouts, and binary-hook signatures are intentionally isolated in
the code that owns them.

## System flow

```text
wardrobe.json / MoonLoader UI
        |                         samp.dll
        v                            |
 config::SkinConfig <--- ConfigWatcher|---> samp::Samp ---> StreamedPed
        |                                           |               |
        +--> runtime::Runtime <--- samp_hooks refresh request ------+
                   |                    |
                   v                    v
            skin_loader::SkinManager -> gta module -> GTA / RenderWare
                   ^
                   |
            model_ids and memory safety helpers
```

`Runtime` is the coordinator.  It owns the active configuration, watcher,
SA-MP scanner, skin manager, and per-player mapping state.  It is entered only
from GTA's `CGame::Process` detour after the original game function returns.

## Module responsibilities

| Module | Responsibility | Collaborates with |
| --- | --- | --- |
| [src/lib.rs](src/lib.rs) | DLL entry point and startup thread. | `logging`, `config`, `samp`, `gta`, `runtime` |
| [src/game_frame.rs](src/game_frame.rs) | Private capability proving execution is in the post-`CGame::Process` frame phase. | `runtime`, `gta`, `skin_loader` |
| [src/runtime.rs](src/runtime.rs) | Frame-thread orchestration, player mappings, restoration. | All runtime-facing modules |
| [src/config.rs](src/config.rs) | JSON types, validation, rule matching, configuration revisions. | `model_ids`, `Runtime`, `SkinManager` |
| [src/skin_loader.rs](src/skin_loader.rs) | Separate remote private-model and local instance-source caches, including reload and retirement. | `config`, `gta`, `Runtime` |
| [src/gta.rs](src/gta.rs) | Guarded GTA/RenderWare calls, ped model slots, opaque clump handles, and TXD/DFF lifetime. | `memory`, `model_ids`, `SkinManager` |
| [src/samp.rs](src/samp.rs) | SA-MP build detection and safe player/ped scanning. | `memory`, `Runtime`, `samp_hooks` |
| [src/samp_hooks.rs](src/samp_hooks.rs) | Guarded R1 event hooks that request an early scan. | `memory`, `Samp`, `Runtime` |
| [src/memory.rs](src/memory.rs) | Fallible, bit-pattern-safe process-memory reads. | `samp`, `gta`, `samp_hooks` |
| [src/model_ids.rs](src/model_ids.rs) | GTA model-ID validity and Wardrobe private range. | `config`, `gta` |
| [src/logging.rs](src/logging.rs) | `wardrobe.log` initialization. | `lib` |
| [moonloader/wardrobe_ui/wardrobe_ui.lua](moonloader/wardrobe_ui/wardrobe_ui.lua) | Staged file editor, connected-player picker, and activation-preset UI. | MoonLoader SA-MP API, `ConfigWatcher` through `wardrobe.json` |

## Structs, enums, and ownership

### Configuration

| Entity | Description | Interoperability |
| --- | --- | --- |
| [`SkinDefinition`](src/config.rs) | Enabled flag, TXD/DFF paths, and donor model ID for a profile. | Stored by `SkinConfig`; passed through both `SkinManager` cache paths to the corresponding GTA loaders. |
| [`SkinRule`](src/config.rs) | Profile ID plus optional name/model selectors. | `priority` and `matches` drive `SkinConfig::matching_rule`, which `Runtime` uses per ped. |
| [`SkinConfig`](src/config.rs) | Complete profile map and rule list. | Created by config parsing; owned by `Runtime`; reconciled by `SkinManager::apply_config`. |
| [`FileRevision`](src/config.rs) | Present (time/size), missing, or unreadable state of a file. | Used to detect JSON and asset changes without hashing files. |
| [`SkinSourceRevision`](src/config.rs) | A copied definition plus TXD/DFF revisions. | Lets `SkinManager` decide whether a loaded profile needs reload. |
| [`ConfigWatcher`](src/config.rs) | Last check time and observed config revision. | Owned by `Runtime`; yields only valid changed `SkinConfig` values. |

### SA-MP scanning

| Entity | Description | Interoperability |
| --- | --- | --- |
| [`SampVersion`](src/samp.rs) | The four recognized SA-MP revisions. | Selected from PE entry point; used by logging and optional hook installation. |
| `SampLayout` (private, [src/samp.rs](src/samp.rs)) | Version-specific offsets for net game, pools, names, and ped pointers. | Stored inside `Samp`; prevents one SA-MP version's layout from being used for another. |
| [`Samp`](src/samp.rs) | Validated `samp.dll` base address and matching layout. | Owned by `Runtime`; scanned every poll; passed to `samp_hooks::install`. |
| `PlayerId` ([src/samp.rs](src/samp.rs)) | `u16` player identity key. | Keys `Runtime`'s matched/applied maps. |
| [`StreamedPed`](src/samp.rs) | Player ID, local-player flag, optional decoded name, and GTA ped pointer. | Produced by `Samp::streamed_peds`; the local flag selects the instance path and a missing name permits only model-based matching. |

### Runtime and resources

| Entity | Description | Interoperability |
| --- | --- | --- |
| `AppliedSkin` / `AppliedPlayer` (private, [src/runtime.rs](src/runtime.rs)) | Private-model ID or installed instance-clump pointer, skin ID, and last normal server model. | Lets Runtime preserve remote behavior, detect local server resets by pointer or model-ID changes despite allocator address reuse, and restore GTA's normal representation. |
| `Runtime` (private, [src/runtime.rs](src/runtime.rs)) | Owns all active state and polling time. | Stored once in `RUNTIME`; invoked by the GTA detour. |
| `LivePedState` (private, [src/runtime.rs](src/runtime.rs)) | Model IDs and render-object pointers from a complete SA-MP ped scan. | Protects both retired private models and retired local sources during cleanup. |
| `GameFrame` (private, [src/game_frame.rs](src/game_frame.rs)) | Unforgeable proof of the GTA frame thread after the original process call. | Required by all GTA/RenderWare mutation entry points. |
| `LoadedSkin` (private, [src/skin_loader.rs](src/skin_loader.rs)) | Live `SkinResources` plus the revision that produced it. | Value in `SkinManager::loaded_models`. |
| `RetiredSkin` (private, [src/skin_loader.rs](src/skin_loader.rs)) | Old resource set and its retirement time. | Held until no ped uses it and GTA teardown succeeds. |
| `LoadedInstanceSkin` / `RetiredInstanceSkin` (private, [src/skin_loader.rs](src/skin_loader.rs)) | Revisioned source clump/TXD state for the local-only path. | Kept separate from private models and released only after the installed clone is gone. |
| [`SkinManager`](src/skin_loader.rs) | Active, failed, retired, protected, and recyclable private models plus the independent instance cache. | Owned by `Runtime`; delegates game work to `gta`. |
| [`SkinResources`](src/gta.rs) | Private GTA model ID and TXD slot. | Returned by `gta::load_skin`; held by loaded/retired skin records. |
| [`SkinLoadFailure`](src/gta.rs) | Optional private ID that can be recycled after a failed load. | Returned to `SkinManager::model_for` so it does not lose a safe slot. |
| [`InstanceSkinResources`](src/gta.rs) | TXD slot, owned raw source clump, and validated donor model-info handle; never a model ID. | Returned by `gta::load_instance_skin`; cloned only by the guarded GTA bridge. |
| [`PedRenderObject`](src/gta.rs) | Opaque comparable handle for a ped clump's address and shared geometry identity. | Stored by `AppliedSkin::InstanceClump` and used for reset/liveness checks that distinguish allocator address reuse without exposing raw pointers. |

## Startup, hooks, and thread boundary

`DllMain` invokes `plugin_thread` in [src/lib.rs](src/lib.rs).  After
configuration and dependency checks, [`runtime::install`](src/runtime.rs)
initializes two one-time globals:

- `RUNTIME: OnceLock<Mutex<Runtime>>`, the state coordinator.
- `GAME_PROCESS_HOOK: OnceLock<GenericDetour<GameProcessFn>>`, the owner of
  the `CGame::Process` detour and its trampoline.

`game_process_detour` runs the original function through the trampoline, then
constructs `GameFrame`, locks `RUNTIME`, and invokes
`Runtime::process_game_frame`.  The capability is required by GTA/RenderWare
mutation APIs, so the startup thread cannot call them by accident.
`samp_hooks::install` is
called after that frame hook is enabled; its SA-MP detours only set atomic
refresh bits and never call the GTA bridge.

## Function contracts and interactions

| Function or method | Description and callers/callees |
| --- | --- |
| `logging::init` | Creates the log file. Called first by `plugin_thread`. |
| `config::load_initial` | Parses (or creates) `wardrobe.json`; its `SkinConfig` starts Runtime. |
| `ConfigWatcher::new` / `poll_change` | Initialize and poll the one-second config watcher. `Runtime::reload_config_if_changed` accepts only its valid results. |
| `SkinConfig::matching_rule` | Finds the highest-priority enabled rule for a name and server model. Called by `Runtime::process_game_frame`. |
| `skin_source_revision` | Captures asset metadata. Called by `SkinManager::model_for` and `instance_for` before choosing reuse/reload. |
| `Samp::wait_for_load` | Detects a supported DLL and layout. Startup passes the resulting `Samp` to `runtime::install`. |
| `Samp::base` / `version` | Expose validated DLL identity to logging and `samp_hooks::install`. |
| `Samp::streamed_peds` | Produces application candidates, retaining peds with unavailable names for model-only matching; returns `None` only for an incomplete structural scan so Runtime retains state. |
| `Samp::all_peds` | Produces a complete liveness list for old-model cleanup, or `None` to defer cleanup. |
| `samp_hooks::install` | Validates R1 signatures and enables optional post-event detours. `runtime::install` logs a polling fallback on error. |
| `samp_hooks::take_refresh_request` | Atomically provides an event reason to the next frame pass. |
| `runtime::install` | Stores Runtime, creates/enables the GTA detour, and attempts optional SA-MP hooks. |
| `GameFrame::enter` | Constructs the private frame capability only in the validated `CGame::Process` detour, after its trampoline returns. |
| `Runtime::process_game_frame` | Central sequence: timing, config reload, scan, match, local/remote routing, application, restoration, pruning, and cleanup. Called only by `game_process_detour`. |
| `Runtime::reload_config_if_changed` | Reconciles `SkinManager` with the watcher replacement before storing it as active config. |
| `Runtime::restore_server_model` | Restores a remote private model or delegates a matching local clone to `gta::restore_instance_skin`; pointer or model-ID mismatches are treated as newer server resets. |
| `Runtime::prune_streamed_out_players` | Removes remote state after a complete streamed-ped scan and retains local instance state while its exact render object remains live. |
| `Runtime::live_ped_state` / `cleanup_retired_skins` | Turns `Samp::all_peds` into model-ID and render-object sets, then drives both resource cleanup paths. |
| `online_players` (MoonLoader UI) | Refreshes connected SA-MP player names at most once a second for the searchable, editable rule-editor dropdown; real players and NPCs are each ordered by player ID, with NPCs last. |
| `add_preset` / `apply_selected_preset` (MoonLoader UI) | Capture or restore only skin/rule enabled states in the UI-owned `presets` JSON field; an enabled toggle also updates the selected preset, and every change remains staged until `save_config`. |
| `SkinManager::apply_config` | Retires loaded profiles no longer referenced by an enabled rule and clears stale failure records. |
| `SkinManager::is_private_model` | Lets Runtime distinguish Wardrobe's model from an ordinary server model. |
| `SkinManager::model_for` | Reuses a current model, keeps a previous model after failure, or calls `gta::load_skin` for a replacement. |
| `SkinManager::cleanup_retired` | Waits for age/liveness conditions and delegates actual teardown to `gta::release_skin_resources`. |
| `SkinManager::instance_for` / `instance_resources` | Revision-check and expose the active local source; a changed source returns `ResetRequired` before any replacement is loaded. |
| `SkinManager::retain_instance_skins` / `cleanup_retired_instances` | Retire sources not wanted by the current local rule and release them only after no tracked live clone depends on them. |
| `gta::validate_executable` | Reads the supported GTA 1.0 US PE/code fingerprints before the TXD-pool wait and again before detouring `CGame::Process`; rejects unknown or patched fixed targets. |
| `gta::is_ready` | After executable validation, confirms GTA's TXD pool exists before runtime installation. |
| `gta::ped_model_id` / `set_ped_model_index` | Runtime's narrow read/write ped-model interface; setting is game-thread-only. |
| `gta::load_skin` | Validates assets/donor, allocates or reuses a model and TXD slot, builds the clump, and returns a resource handle or failure. |
| `gta::release_skin_resources` | Destroys a retired clump/TXD and leaves its model-info allocation inert for reuse. |
| `gta::load_instance_skin` / `release_instance_skin_resources` | Own a validated source clump and TXD for the local path without allocating or mutating a model slot. |
| `gta::apply_instance_skin` | Fully prepare a compatible animated clone, abort stale IK chains, transfer live associations, preserve entity bookkeeping, position the root through RenderWare's dirty-frame transform, update frame/HAnim state, and verify the model ID stayed unchanged. |
| `gta::restore_instance_skin` | Extracts live animations, virtually deletes the matching custom clone, rebuilds the remembered model through `CPed::SetModelIndex`, and gives the animations to the normal clump. |
| `gta::ped_render_object` | Reads an opaque ped clump handle for reset and retirement checks. |
| `memory::read`, `read_bytes` | Shared fallible memory boundary for SA-MP scanning, GTA table inspection, and signature checks. `read` accepts only sealed primitive/pointer types with no invalid bit patterns. |
| `model_ids` predicates | Shared ID rules used by config validation and GTA model allocation. |

Private helpers complete these contracts.  `Samp`'s pool, ped, and MSVC-string
helpers build `StreamedPed`s; `SkinManager`'s retirement and recycle helpers
manage private-ID and instance-source ownership; GTA's cdecl, TXD, stream,
metadata, clump, vtable, and model-table helpers perform version-specific
load/release work; and SA-MP hook helpers construct and validate
version-specific targets.

## Safety and lifecycle rules

1. GTA and RenderWare mutations require `GameFrame`, which only the frame
   detour can construct after calling GTA's original process function.
2. Every fixed GTA/RenderWare code address is signature-checked before Wardrobe
   can install the frame detour or invoke it.
3. SA-MP/GTA pointers are read through `memory` helpers; a failed scan is not
   an empty scan.
4. Raw source and installed clump pointers remain opaque GTA types; all clone,
   destroy, association, frame, and HAnim calls require `GameFrame`.
5. Invalid configuration and remote asset reloads do not replace a known-good
   private model. A changed local source first rebuilds the normal clump and is
   loaded again only after the old source retires.
6. A private model or instance source stays protected until every relevant live
   ped is known not to use it and its GTA resource teardown succeeds.
7. New SA-MP versions require a distinct entry-point/layout verification and,
   for event hooks, separately verified signatures.

See [core.md](core.md) for feature behavior and source-file references.
