# Wardrobe

Wardrobe lets you choose the GTA: San Andreas skins you see for yourself and
other SA-MP players. It is made for roleplay: the server and other players are
not changed—only your own game sees the custom model.

> Wardrobe is experimental. Use it only on servers where client-side cosmetic
> modifications are allowed.

## What you need

- GTA San Andreas 1.0 US (Hoodlum)
- SA-MP 0.3.7-R1
- An ASI loader (e.g. [Silent's ASI Loader](https://www.gtagarage.com/mods/show.php?id=21709))
- A compatible `.txd` and `.dff` pair for every custom skin

Wardrobe is version-specific. Do not use it with a different GTA executable,
SA-MP revision, or unknown limit adjusters unless you are comfortable testing
for crashes yourself.

## Getting started

1. Put `wardrobe.asi` in your GTA directory.
2. Put your `.txd` and `.dff` files somewhere under that directory, for
   example `models/myskin.txd` and `models/myskin.dff`.
3. Start GTA once. Wardrobe creates an empty `wardrobe.json` beside the ASI.
4. Either edit that JSON yourself or install the optional MoonLoader UI and
   use `/wardrobe` in-game.

The skin files must be GTA SA ped-compatible. Their textures must be present
in the TXD, and the DFF needs the expected ped skeleton/frame hierarchy.

## Configuring skins

`wardrobe.json` has two parts:

- **Skins** describe the files Wardrobe should load.
- **Rules** decide who receives each skin.

Here is a complete small example:

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

Paths are relative to the GTA directory. `donor_model_id` is a normal GTA ped
model used as a template for the custom one. Use the ID of the model the skin
was originally created for. Wardrobe reserves IDs `18000` through `19999`, so
they cannot be donors; it also rejects vehicles and objects when loading.

A rule can match by player name, by the server-set model ID, or by both. Player
names are exact and case-sensitive. When multiple rules apply, Wardrobe picks
the most specific one:

1. Player name and server model ID
2. Player name only
3. Server model ID only

Set `enabled` to `false` on a skin or rule to keep it saved without using it.
Removing or disabling a rule returns affected streamed-in players to the last
normal skin Wardrobe saw from the server.

You can edit and save the JSON while GTA is running. Wardrobe notices config
and skin-file changes within about a second and updates matching streamed-in
players automatically.

## Optional in-game editor

The MoonLoader editor is at `moonloader/wardrobe_ui/wardrobe_ui.lua`. It edits
the same `wardrobe.json`; it does not need a special bridge to the ASI.

Copy it to `GTA_FOLDER/moonloader/`, then open it with:

```text
/wardrobe
```

Use **Save JSON** when you are happy with your changes. The editor saves
atomically, so Wardrobe never reads a half-written file.

## If something does not work

Wardrobe writes `wardrobe.log` in the GTA directory. Check it first.

- A missing or invalid TXD/DFF is logged and that skin is skipped. The player
  keeps the server skin, or keeps the last successfully loaded custom skin.
- Invalid JSON leaves the last working configuration active.
- If no rule matches, Wardrobe leaves the player on the server-provided skin.

## Building from source

You need Rust with the `i686-pc-windows-msvc` target and `cargo-make` (optional).
For the `cargo-make` deploy commands, set `GTA_DIR` once per PowerShell
session:

```powershell
$env:GTA_DIR = 'D:\Games\GTASA'
```

Regular:
```powershell
cargo build --release
```

With `cargo-make` :
```powershell
cargo make debug
```
or
```powershell
cargo make deploy
```

This builds a debug Wardrobe ASI and copies it, together with its PDB, to the
GTA directory stored in `GTA_DIR`.
Debug builds wait for a debugger to attach to `gta_sa.exe`.

To deploy the optional editor:

```powershell
cargo make deploy-ui
```

## License and affiliation

Wardrobe is released under the [MIT License](LICENSE). It is an independent
project, is not affiliated with Rockstar Games, Take-Two Interactive, or the
SA-MP project, and contains no GTA San Andreas or SA-MP assets. You are
responsible for using game copies and custom assets that you have the right to
use.
