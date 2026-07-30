# Custom Skin Loader

An experimental GTA San Andreas / SA-MP ASI plugin written in Rust. It loads a
loose `.txd` and `.dff` pair through GTA's RenderWare runtime and applies the
model locally.

This project currently targets GTA San Andreas 1.0 US (Hoodlum) and SA-MP
0.3.7-R1, both as 32-bit processes. The addresses in the source are version
specific.

## Current status

The custom TXD/DFF loading path works. RenderWare and ped-model operations run
from GTA's frame thread, avoiding crashes caused by invoking GTA engine code
from a background thread.

The loader creates a private, unused GTA model ID and initializes it from a
vanilla ped-model definition before attaching the custom clump. This avoids
globally replacing the donor skin and lets the plugin assign the custom model
only to selected peds on this client. It does not alter server state or other
clients.

## Requirements

- GTA San Andreas 1.0 US / Hoodlum, 32-bit
- SA-MP 0.3.7-R1 for remote-player support
- An ASI loader
- Rust with the `i686-pc-windows-msvc` target
- `cargo-make`

## Assets

Place the skin files in the GTA installation directory:

```text
models/myskin.txd
models/myskin.dff
```

`myskin.dff` must be a GTA SA ped-compatible RenderWare clump with the expected
ped skeleton/frame hierarchy. Its material texture names must exist in
`myskin.txd`.

## Build and deploy

The project is configured to build for 32-bit Windows in
`.cargo/config.toml`.

```powershell
cargo make debug
```

The `debug` task copies the resulting ASI and PDB to the GTA directory configured
in `Makefile.toml` (`C:/Games/GTASA` by default). Close GTA before deploying;
Windows cannot overwrite an ASI that the running game has loaded.

Debug builds wait until a debugger is attached to `gta_sa.exe`.

## Configuration

The main settings are near the top of `src/lib.rs`:

- `APPLY_TO_LOCAL_PLAYER`: keep `true` while testing the local player.
- `TARGET_PLAYER_ID`: SA-MP player ID used when local-player mode is `false`.
- `DONOR_PED_MODEL_ID`: the initialized GTA ped model whose metadata is cloned
  for each private custom-model slot (currently `7`).
- `TXD_PATH` and `DFF_PATH`: relative paths to the skin assets.

## Logs

The plugin writes `custom_skin_loader.log` in GTA's working directory. A
successful initialization includes messages like:

```text
custom skin loaded: private model=..., donor=7, txd_slot=...
reapplied custom model ...
```

## Safety notes

Use this only where client-side cosmetic modifications are permitted. This
plugin is experimental and relies on internal game/SA-MP structures; different
executables, patches, limit adjusters, or incompatible DFFs can crash the game.
