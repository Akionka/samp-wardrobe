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
from a background thread. Skin profiles are selected by exact SA-MP player name
from `skins.json`.

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

## Skin profiles

Create `skins.json` in the GTA installation directory. Start by copying
`skins.example.json` from this repository, then adjust the player name and
paths:

On first run, the loader creates a missing `skins.json` as an empty `{}` file
and remains idle until you add at least one profile.

```json
{
  "skins": {
    "jacob_spencer": {
      "txd_path": "models/myskin.txd",
      "dff_path": "models/myskin.dff",
      "donor_model_id": 7
    }
  },
  "players": {
    "Jacob_Spencer": "jacob_spencer",
    "Jacob_Alt": "jacob_spencer"
  }
}
```

- `skins` maps a skin ID to its asset paths and donor model ID.
- `players` maps an exact, case-sensitive SA-MP nickname to a skin ID.
- `txd_path` and `dff_path` are relative to the GTA installation directory.
- `donor_model_id` is a normal GTA ped model whose metadata is used to
  initialize a new private slot. `7` is the currently tested default.

Skins are loaded only when an assigned player is streamed in. Each skin receives
one private GTA model ID that every mapped player shares, so multiple configured
players can use one skin without duplicating it or affecting ordinary game models.

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

## Logs

The plugin writes `custom_skin_loader.log` in GTA's working directory. A
successful initialization includes messages like:

```text
loaded skin jacob_spencer: private model=..., donor=7, txd_slot=...
applied custom model ... to Jacob_Spencer
```

## Safety notes

Use this only where client-side cosmetic modifications are permitted. This
plugin is experimental and relies on internal game/SA-MP structures; different
executables, patches, limit adjusters, or incompatible DFFs can crash the game.
