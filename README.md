# Wardrobe

Wardrobe lets you choose the GTA: San Andreas skins you see for yourself and
other SA-MP players. It is made for roleplay: the server and other players are
not changed—only your own game sees the custom model.

One custom skin profile can be used for compatible players with different
server-assigned skins. Wardrobe keeps each player's server skin ID intact, so
the server continues to treat them normally.

> Wardrobe is experimental. Use it only on servers where client-side cosmetic
> modifications are allowed.

## What you need

- GTA San Andreas 1.0 US (Hoodlum)
- SA-MP 0.3.7-R1, 0.3.7-R3-1, 0.3.7-R4, or 0.3.DL-R1
- An ASI loader (e.g. [Silent's ASI Loader](https://www.gtagarage.com/mods/show.php?id=21709))
- A compatible `.txd` and `.dff` pair for every custom skin
- Optional: `rak_samp.asi` for next-frame reconciliation after skin or player-stream events

Wardrobe is version-specific. It detects the supported SA-MP builds before
reading their player data and refuses unknown revisions. It also checks the
GTA 1.0 US code targets it uses before installing its frame hook. A different
GTA executable, or an ASI that has already patched one of those targets, is
logged as unsupported and Wardrobe stays inactive.

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

`wardrobe.json` has three parts:

- **Runtime settings** control the baseline scan interval and log verbosity.
- **Skins** describe the files Wardrobe should load.
- **Rules** decide who receives each skin.

Here is a simple starting example. It shows `Jacob_Spencer` one custom skin:

```json
{
  "poll_interval_ms": 2000,
  "log_level": "info",
  "skins": {
    "my_custom_skin": {
      "enabled": true,
      "txd_path": "models/myskin.txd",
      "dff_path": "models/myskin.dff"
    }
  },
  "rules": [
    {
      "profile_id": "my_custom_skin",
      "player_name": "Jacob_Spencer",
      "enabled": true
    }
  ],
  "presets": {
    "show_my_custom_skin": {
      "profiles": {
        "my_custom_skin": true
      },
      "rules": {
        "Jacob_Spencer\u001f": true
      }
    }
  }
}
```

Paths are relative to the GTA directory. You do not need to choose a donor
model ID: Wardrobe prepares each compatible custom skin for the player's
current server-assigned skin automatically. If an older profile still contains
`donor_model_id`, Wardrobe safely ignores it; the in-game editor no longer
shows that setting.

`poll_interval_ms` sets the delay between complete fallback scans. It defaults
to `2000` and accepts values from `100` to `60000`. A lower value finds
streamed-ped changes sooner when rak-samp is absent, at the cost of scanning more
often. `log_level` defaults to `"info"`; choose `"error"`, `"warn"`,
`"info"`, `"debug"`, or `"trace"`. Both settings take effect after Wardrobe
reloads a valid JSON edit.

In this example, `my_custom_skin` is just a name you choose for the TXD/DFF
pair. `Jacob_Spencer` is the exact in-game player name to receive it. To apply
the same skin by the player's server model instead, replace `player_name` with
`server_model_id`, for example `"server_model_id": 67`.

The `show_my_custom_skin` preset saves the enabled state of this skin and its
rule, so you can switch that setup on again with one click in `/wardrobe`.
Create and update presets with the in-game editor; it handles the rule entry
inside `presets` automatically.

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

## How Wardrobe notices changes

Wardrobe completes a streamed-player reconciliation scan at the configured
`poll_interval_ms` delay (two seconds by default). That is the reliable
baseline, including when `rak_samp.asi` is not installed.

If the optional `rak_samp.asi` host is installed and reports that it is ready,
Wardrobe listens for incoming `SetPlayerSkin`, player stream-in, and player
stream-out events and performs its next complete scan on the next GTA frame.
This applies to every supported SA-MP build that the rak-samp host marks ready.
If the host is missing, incompatible, fails to initialize, or cannot accept the
listener, Wardrobe logs one fallback message and continues with the configured
scan delay. Wardrobe never installs direct SA-MP spawn or skin-change hooks.
In every mode, model loading and model swaps stay on GTA's frame thread.

## Optional in-game editor

The MoonLoader editor is at `moonloader/wardrobe_ui/wardrobe_ui.lua`. It edits
the same `wardrobe.json`; it does not need a special bridge to the ASI. Copy it
to `GTA_FOLDER/moonloader/scripts/wardrobe_ui/wardrobe_ui.lua`, or use
`cargo make deploy-ui` when building from source.

Then open it with:

```text
/wardrobe
```

Use **Save JSON** when you are happy with your changes. The editor saves
atomically, so Wardrobe never reads a half-written file.

When editing a rule, the **Player name** dropdown lists connected SA-MP players
and also accepts any typed name for an offline player. **Activation presets**
capture the current enabled states of existing skins and rules. Click a preset
to stage its switches; later enabled/disabled changes update the selected
preset. Use **Save JSON** to send the change to Wardrobe. Presets do not
duplicate your skin paths or matching rules.

## If something does not work

Wardrobe writes `wardrobe.log` in the GTA directory. Check it first.

- A missing or invalid TXD/DFF is logged and that skin is skipped. The player
  keeps the server skin, or keeps the last successfully loaded custom skin.
- If a custom skin cannot be applied to one player, Wardrobe leaves that
  player's normal server skin in place. Other players using the same profile
  can still receive it.
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

With `cargo-make`:
```powershell
cargo make debug
```
or
```powershell
cargo make deploy
```

`cargo make debug` builds the debug ASI, copies it and its PDB to `GTA_DIR`,
and waits for a debugger to attach to `gta_sa.exe`. `cargo make deploy` builds
and copies the release ASI.

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
