# TODO

## Next priorities

- [x] Validate the `skins` + `rules` JSON schema in-game with real streamed
  players. Confirm combined, player-only, and model-only rule precedence.
- [x] Throttle polling. The current game-thread pass can scan up to 1004 SA-MP
  slots per frame; run it roughly every 200 ms while still reapplying a custom
  model as soon as the next poll observes a server-side skin reset.
- [x] Add clear diagnostic logging for player-name matches, skin IDs, private
  model IDs, missing assets, invalid JSON mappings, and unavailable donor
  models.

## Configuration and lifecycle

- [x] Auto-reload `wardrobe.json` without restarting GTA. New
  profiles and matching rules become available safely.
- [x] Support live skin-profile replacement when its TXD path, DFF path, or
  donor model changes. Build a replacement into a fresh private model slot and
  move every assigned streamed-in ped to it on the game thread. Superseded
  resources deliberately remain alive until GTA exits.
- [x] Detect TXD/DFF file changes even when `wardrobe.json` is
  unchanged. Compare modification time and file length about once per second,
  then rebuild the affected skin and move configured local/remote players to
  the replacement model.
- [x] Support profile or matching-rule removal. Restore every affected streamed-in
  ped to the most recently observed normal server model. Track that model when
  SA-MP changes a ped away from a loader-owned private model.
- [x] Safely clean obsolete private skin resources. After all live SA-MP peds
  have detached, destroy the model's RenderWare object, release/remove its TXD
  slot, and recycle the inert CPedModelInfo entry without returning it to GTA's
  global model-info table.
- [ ] Stress-test repeated live reloads and profile removals, including shared
  skins and streamed-out remote players, to verify that private model/TXD
  counts remain stable.
- [x] Support toggling an individual matching rule or skin profile on and off.
- [x] Preserve and document the first-run behavior: create a missing
  `wardrobe.json` as `{}` and remain idle until a matching rule exists.
- [x] Add prioritized matching rules for player names and server model IDs:
  combined rules win over player-only rules, which win over model-only rules.

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

- [x] Add an optional MoonLoader (Lua) ImGui configuration UI after the
  configuration/reload workflow is stable. Keep it as a file-based front-end:
  Lua reads and edits `wardrobe.json`, while the Rust ASI continues
  to own GTA model loading and observes changes through its existing reload
  path. Do not introduce Lua-to-Rust FFI for this.
- [x] Make the MoonLoader UI save JSON atomically (write a temporary file, then
  rename it) so the Rust loader never observes a partially written config.
- [x] Add profile management and status feedback to the MoonLoader UI.
