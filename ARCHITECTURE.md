# Wardrobe architecture

```text
wardrobe.json / MoonLoader UI        samp.dll       rak_samp.asi (optional)
             |                          |                    |
             v                          v                    v
      ConfigWatcher ------------> Runtime <----- atomic refresh request
                                      |
                                      v
                         SkinManager / SourceCache
                                      |
                                      v
                              GTA / RenderWare
```

`Runtime` owns configuration, the SA-MP scanner, source cache, and per-player
state. It runs only after the original `CGame::Process` returns.

## Components

| Area | Files | Responsibility |
| --- | --- | --- |
| Startup | `lib.rs`, `game_frame.rs` | Start safely and establish game-frame execution. |
| Configuration | `config.rs`, `logging.rs` | Parse/reload JSON and set runtime logging. |
| Scheduling | `runtime.rs`, `rak_samp.rs` | Decide when to scan; coalesce optional host events. |
| SA-MP access | `samp.rs`, `samp/layout.rs`, `memory.rs` | Validate supported layouts and read streamed peds safely. |
| Skin resources | `skin_loader.rs`, `skin_loader/source_cache.rs` | Load, share, retire, and release profile sources. |
| GTA bridge | `gta.rs`, `gta/skin_source.rs` | Validate targets and perform clump load, apply, restore, and cleanup. |

## Data ownership

- `SkinConfig` owns rules, profiles, scan interval, and log level.
- `Runtime` tracks applied clones and failed attempts by player ID.
- `SourceCache` owns loaded and retired TXD/source resources and their clone
  identities.
- GTA handles are opaque outside the GTA bridge; SA-MP and GTA pointers are
  treated as untrusted reads.

## Invariants

1. Only code holding `GameFrame` mutates GTA or RenderWare.
2. A partial SA-MP scan never authorizes destructive cleanup.
3. Wardrobe does not change a custom-skinned ped's server model ID.
4. A retired source is released only after every recorded clone identity is
   absent from a complete liveness scan.
5. rak-samp callbacks only set atomic refresh bits and return `Continue`.
