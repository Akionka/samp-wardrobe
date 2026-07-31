# Wardrobe
> Wardrobe — client-side custom skins for SA-MP

Wardrobe is an experimental GTA San Andreas / SA-MP ASI plugin written in
Rust. It loads a loose `.txd` and `.dff` pair through GTA's RenderWare runtime
and applies the model locally.

This project currently targets GTA San Andreas 1.0 US (Hoodlum) and SA-MP
0.3.7-R1, both as 32-bit processes. The addresses in the source are version
specific.

## Current status

The custom TXD/DFF loading path works. RenderWare and ped-model operations run
from GTA's frame thread, avoiding crashes caused by invoking GTA engine code
from a background thread. Matching rules select skin profiles by exact SA-MP
player name, normal server model ID, or both, from `wardrobe.json`.

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

Create `wardrobe.json` in the GTA installation directory. Start by copying
`wardrobe.example.json` from this repository, then adjust the player name and
paths:

On first run, Wardrobe creates a missing `wardrobe.json` as an
empty `{}` file and remains idle until you add at least one matching rule.

```json
{
  "skins": {
    "jacob_spencer": {
      "enabled": true,
      "txd_path": "models/myskin.txd",
      "dff_path": "models/myskin.dff",
      "donor_model_id": 7
    }
  },
  "rules": [
    {
      "profile_id": "jacob_spencer",
      "player_name": "Jacob_Spencer",
      "enabled": true
    },
    {
      "profile_id": "jacob_spencer",
      "server_model_id": 67,
      "enabled": true
    }
  ]
}
```

- `skins` maps a skin ID to its asset paths and donor model ID.
- `rules` maps a profile to one or both matching conditions:
  `player_name` is an exact, case-sensitive SA-MP nickname, and
  `server_model_id` is the normal GTA model ID supplied by the server.
- Both skin profiles and rules have an `enabled` flag, which defaults to `true`
  when omitted. Disabling a rule or its profile excludes it from matching
  without removing the saved entry; the next matching rule, or the server model
  when none remains, is used instead.
- A rule must specify at least one condition. The loader resolves matches in
  this order: player name + server model, player name only, then server model
  only. Two rules with identical conditions are invalid.
- `txd_path` and `dff_path` are relative to the GTA installation directory.
- `donor_model_id` is a normal GTA ped model whose metadata is used to
  initialize a new private slot. `7` is the currently tested default.

Skins are loaded only when a matching player is streamed in. Each active skin
generation receives one private GTA model ID that every matching player shares,
so multiple rules can use one skin without duplicating it or affecting ordinary
game models.

Wardrobe notices saved changes to `wardrobe.json` within about one
second. You can add profiles, enable or disable profiles and rules, change
matching conditions, or change a profile's TXD path, DFF path, or donor while
GTA is running. It also checks the loaded TXD and DFF files about once per
second. A changed profile or asset is loaded into a fresh private model slot
and every matching streamed-in ped moves to it on the next poll.

Removing a matching rule or skin profile restores an affected streamed-in ped
to the last normal model observed from SA-MP. Invalid JSON or a failed asset
reload leaves the last working configuration/model active and reports the error
in `wardrobe.log`.

After a profile is replaced or becomes unassigned, the loader waits one second
and confirms that no live SA-MP ped still uses its old private model. It then
destroys the retired RenderWare clump and releases its TXD slot. The empty GTA
ped-model entry is retained and recycled by the loader, avoiding repeated
allocation from GTA's fixed ped-model-info array. If the safe SA-MP scan is
incomplete, cleanup is postponed rather than risking a dangling model.

## Optional MoonLoader UI

`moonloader/wardrobe_ui/wardrobe_ui.lua` is an optional
MoonLoader `mimgui` front-end for the same JSON configuration. It does not
communicate with the Rust ASI directly: it edits `wardrobe.json`, which the
ASI then reloads automatically. In-game, use `/wardrobe` to open the
editor. Profile and matching-rule changes are staged until **Save JSON** is
pressed.

Deploy the Lua script separately with:

```powershell
cargo make deploy-ui
```

The script is installed under `moonloader/scripts/wardrobe_ui/`,
where this installation's GitHelper discovers and auto-reloads it. Restart GTA
or MoonLoader once after the first deployment so GitHelper adds this new script
to its scan; subsequent Lua edits auto-reload while the game is running. Saving
uses a temporary file and an atomic Windows replacement so the ASI never
receives a partially written JSON document.

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

Wardrobe writes `wardrobe.log` in GTA's working directory. A
successful initialization includes messages like:

```text
loaded skin jacob_spencer: private model=..., donor=7, txd_slot=...
applied custom model ... to Jacob_Spencer
```

## Safety notes

Use this only where client-side cosmetic modifications are permitted. This
plugin is experimental and relies on internal game/SA-MP structures; different
executables, patches, limit adjusters, or incompatible DFFs can crash the game.

## Affiliation and assets

Wardrobe is an independent project and is not affiliated with or
endorsed by Rockstar Games, Take-Two Interactive, or the SA-MP project. It
contains no GTA San Andreas or SA-MP game assets. You are responsible for
using compatible game copies and custom assets that you have the right to use.
