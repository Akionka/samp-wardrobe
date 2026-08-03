# Wardrobe

Wardrobe is a client-side GTA: San Andreas skin loader for SA-MP. It changes
only what you see; servers and other players never receive the custom model.

> Experimental software: use it only where client-side cosmetic modifications
> are allowed.

## Requirements

- GTA San Andreas 1.0 US (Hoodlum)
- SA-MP 0.3.7-R1, R3-1, R4, or DL-R1
- An ASI loader
- A compatible ped `.txd` and `.dff` pair for each custom skin
- Optional: `rak_samp.asi` for faster reactions to skin and stream events

Wardrobe refuses unsupported SA-MP or GTA executable revisions rather than
reading unknown memory layouts.

## Install and configure

1. Copy `wardrobe.asi` to the GTA directory.
2. Put skin files under that directory, for example `models/myskin.txd` and
   `models/myskin.dff`.
3. Start GTA once to create `wardrobe.json`, then edit it or use the optional
   MoonLoader editor.

Example configuration:

```json
{
  "poll_interval_ms": 2000,
  "log_level": "info",
  "skins": {
    "my_skin": {
      "enabled": true,
      "txd_path": "models/myskin.txd",
      "dff_path": "models/myskin.dff"
    }
  },
  "rules": [
    {
      "profile_id": "my_skin",
      "player_name": "Jacob_Spencer",
      "enabled": true
    }
  ]
}
```

Paths are relative to the GTA directory. A profile needs no donor model ID:
Wardrobe adapts a prepared clone to the player's current server model while
leaving that server model unchanged.

Rules match an exact, case-sensitive player name, a `server_model_id`, or both.
When several rules match, priority is: name and model (`3`), name (`2`), then
model (`1`). Disabled profiles and rules are retained but not applied.

`poll_interval_ms` is the complete-scan fallback delay (100–60000 ms; default
2000). `log_level` is one of `error`, `warn`, `info`, `debug`, or `trace`.
Valid configuration and skin-file changes are picked up while GTA is running;
invalid JSON leaves the last working configuration active.

## Refresh behavior

Wardrobe reconciles all streamed players at the configured interval. With an
optional ready `rak_samp.asi`, incoming `SetPlayerSkin`, player stream-in, and
player stream-out events request a scan on the next GTA frame. Without it,
Wardrobe continues using the configured fallback. Model loading and swaps always
remain on the game thread.

## MoonLoader editor

Copy `moonloader/wardrobe_ui/wardrobe_ui.lua` to
`GTA_FOLDER/moonloader/scripts/wardrobe_ui/`, then use `/wardrobe`. The editor
has **Skins & rules** and **Runtime** tabs; save changes to apply them. It also
provides player selection and activation presets.

## Troubleshooting

Check `wardrobe.log` in the GTA directory first. Invalid assets or JSON are
logged and skipped; affected players keep their server skin. If no enabled rule
matches, Wardrobe does nothing.

## Build from source

Install Rust with the `i686-pc-windows-msvc` target. For deployment, set
`GTA_DIR` and run:

```powershell
$env:GTA_DIR = 'D:\Games\GTASA'
cargo make deploy
cargo make deploy-ui
```

## License

Wardrobe is released under the [MIT License](LICENSE). It is independent of
Rockstar Games, Take-Two Interactive, and SA-MP, and includes no game assets.
