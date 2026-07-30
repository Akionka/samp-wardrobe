# TODO

## Next priorities

- [ ] Validate the `skins` + `players` JSON schema in-game with a real streamed
  player. Confirm one skin can be assigned to multiple player names.
- [x] Throttle polling. The current game-thread pass can scan up to 1004 SA-MP
  slots per frame; run it roughly every 200 ms while still reapplying a custom
  model as soon as the next poll observes a server-side skin reset.
- [ ] Add clear diagnostic logging for player-name matches, skin IDs, private
  model IDs, missing assets, invalid JSON mappings, and unavailable donor
  models.

## Configuration and lifecycle

- [ ] Reload `skins.json` without restarting GTA. New profiles should become
  available safely; define and implement safe cleanup for obsolete private
  model slots, TXD references, and clumps before unloading old profiles.
- [ ] Add a user-facing reload control, such as a chat command or hotkey.
- [ ] Support toggling an individual player or skin profile on and off.
- [ ] Preserve and document the first-run behavior: create a missing
  `skins.json` as `{}` and remain idle until a player mapping exists.

## Compatibility and safety

- [ ] Verify the GTA executable and SA-MP client revision before installing
  hooks or reading SA-MP structures. Log a clear unsupported-version error
  instead of risking an invalid memory access.
- [ ] Review compatibility with Fastman92 Limit Adjuster and other common ASI
  plugins, especially model-info pointer handling and private model ID limits.
- [ ] Define cleanup behavior for game shutdown/restart and streamed-out
  players.

## Future event-driven detection

- [ ] Keep polling as the default, reliable detection path.
- [ ] Evaluate a pure-Rust `samp.dll` hook for remote-player stream-in and
  model-change handling. Choose hook targets that do not conflict with
  SAMPFUNCS or other common mods, validate target bytes before patching, and
  retain polling as a fallback.
- [ ] Optionally add a SAMPFUNCS integration through a thin C++ bridge if its
  callback API proves more stable than maintaining direct hooks. The bridge
  should enqueue events only; GTA/RenderWare work must remain on the existing
  `CGame::Process` path.

## Later quality of life

- [ ] Consider an ImGui interface after the configuration/reload workflow is
  stable.
- [ ] Add profile management and status feedback to the UI, if implemented.
