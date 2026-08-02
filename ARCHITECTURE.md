# Wardrobe architecture

Wardrobe is a Rust `cdylib` loaded into a 32-bit GTA San Andreas process. Code
that knows executable addresses, layouts, and hooks remains isolated behind
fallible APIs and the frame-thread capability.

## System flow

```text
wardrobe.json / MoonLoader UI                 samp.dll
        |                                        |
        v                                        v
config::SkinConfig <--- ConfigWatcher ---> samp::Samp ---> StreamedPed
        |                                             |
        +--------------> runtime::Runtime <----------+
                              |
                              v
                  skin_loader::SkinManager
                              |
                              v
                    gta::skin_source -> GTA / RenderWare
```

`Runtime` owns the configuration, scanner, source manager, and per-player
state. It runs only after GTA's original `CGame::Process` returns.

## Modules

| Module | Responsibility |
| --- | --- |
| [src/lib.rs](src/lib.rs) | DLL entry point and startup thread. |
| [src/game_frame.rs](src/game_frame.rs) | Unforgeable proof of post-`CGame::Process` execution. |
| [src/runtime.rs](src/runtime.rs) | Polling, matching, application failure suppression, and GTA hook installation. |
| [src/runtime/lifecycle.rs](src/runtime/lifecycle.rs) | Clone restoration, pruning, reset detection, and source cleanup dispatch. |
| [src/config.rs](src/config.rs) | JSON schema, matching, configuration watching, and source revisions. |
| [src/skin_loader.rs](src/skin_loader.rs) | `SkinManager` façade over the sole shared source cache. |
| [src/skin_loader/source_cache.rs](src/skin_loader/source_cache.rs) | Per-profile source loading, generations, clone identity tracking, and liveness-gated release. |
| [src/gta.rs](src/gta.rs) | Guarded GTA bridge, opaque ped/render handles, executable checks, and RenderWare primitives. |
| [src/gta/skin_source.rs](src/gta/skin_source.rs) | Shared source preparation plus compatible clone apply, recovery, restore, and release. |
| [src/samp.rs](src/samp.rs) | Validated SA-MP player/ped scans. |
| [src/samp/layout.rs](src/samp/layout.rs) | Supported SA-MP layouts selected by PE entry point. |
| [src/samp_hooks.rs](src/samp_hooks.rs) | Guarded R1 refresh-event hooks. |
| [src/model_ids.rs](src/model_ids.rs) | GTA model-ID range validation. |
| [src/memory.rs](src/memory.rs) | Fallible, bit-pattern-safe process-memory reads. |

## State and ownership

| Entity | Owner and purpose |
| --- | --- |
| `SkinDefinition` / `SkinRule` / `SkinConfig` | Parsed configuration owned by `Runtime`. Definitions are passed to `SkinManager`; rules select a profile per ped. |
| `SkinSourceRevision` | Definition plus TXD/DFF metadata used by `SourceCache` to detect reloads. |
| `StreamedPed` | SA-MP player ID, optional name, and opaque GTA ped address. There is no local/remote routing state. |
| `AppliedPlayer` | Profile ID, original server model ID, and the exact installed clone identity. |
| `FailedApplication` | Profile ID, source generation, server model ID, and normal render identity that suppress repeated failed clone installations. |
| `SkinSourceResources` | Opaque TXD slot and source clump. One donor-free value is shared per profile. |
| `PedRenderObject` | Comparable clump address plus geometry identity. Runtime never dereferences it. |
| `LoadedSkinSource` | Active source revision, generation, resources, and all clone identities installed from it. |
| `RetiredSkinSource` | Old resources and clone identities retained until a complete liveness scan finds no matching identity. |

## Runtime sequence

1. `Runtime::process_game_frame` receives `GameFrame`, reloads valid JSON, and obtains a complete streamed-ped scan.
2. For each ped it reads its model ID and render identity. An installed clone whose identity or model differs from `AppliedPlayer` is stale server-reset state and is discarded.
3. Runtime resolves the highest-priority rule. Rule/profile removal restores the matching clone to the stored server model; profile changes restore before applying the new profile.
4. `SkinManager::source_for` returns a ready source and generation, a restore request for a changed or still-retiring source, or unavailable. Runtime retries the restore request for every streamed user until the old clones are gone; only then can the cache load a replacement.
5. `gta::apply_skin_source` first validates the unchanged server model as an available `CPedModelInfo`, then clones the ready source and applies that model info only to the new clone. Success records both `AppliedPlayer` and the clone identity in `SourceCache`. Failure records its attempt fingerprint after the bridge has left or recovered the normal server clump.
6. Runtime retires sources no longer desired, prunes state after a complete scan, then gives exact live render identities to `SourceCache` for cleanup.

## Critical contracts

| Function or method | Contract |
| --- | --- |
| `SkinManager::source_for` | Checks revision state, loads one source per profile, returns its generation, and returns `RestoreRequired` after retiring a changed source or while an earlier restore remains pending. |
| `SkinManager::record_clone` | Associates every successful installation, including remote peds, with the active source. |
| `SkinManager::cleanup_retired_sources` | Releases a retired TXD/source only when its recorded clone identities are absent from a complete live-ped scan. |
| `Runtime::restore_profile_users` | Restores all currently streamed clones that use a changed source before any replacement can load. |
| `Runtime::restore_server_model` | Restores only when both the exact clone and remembered model are still current; otherwise it preserves the newer server representation. |
| `gta::load_skin_source` | Validates intrinsic files, hierarchy, skin geometry, weights, and bounds; then loads a donor-free TXD/DFF source without model-slot allocation. |
| `gta::apply_skin_source` | Validates the unchanged server model as `CPedModelInfo`, configures an animated clone for that model, transfers associations, replaces the entity render object, and verifies the model ID did not change. |
| `gta::restore_skin_source` | Rebuilds the remembered server clump after safely deleting the matching clone and transfers animations back. |

## Safety and lifecycle rules

1. GTA/RenderWare mutation requires `GameFrame`; the background thread only waits and prepares startup state.
2. All fixed GTA/RenderWare targets are signature-checked before calling or detouring them.
3. SA-MP and GTA pointers are read through `memory`; an incomplete scan never authorizes destructive cleanup.
4. Source clumps and installed clones are opaque outside the GTA bridge.
5. Wardrobe never allocates GTA model slots or changes a custom-skinned ped's model ID.
6. A shared source is retired by clone identity only, never by model ID or local/remote role. If teardown fails, it remains queued for a later frame-thread retry.
7. Per-ped clone-install failures are retried only when their profile, source generation, server model, or render identity changes.
