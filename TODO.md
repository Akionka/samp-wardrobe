# TODO

## Next priorities

- [x] Validate the `skins` + `rules` JSON schema in-game with real streamed
  players. Confirm combined, player-only, and model-only rule precedence.
- [x] Configure complete SA-MP scan delay. The game thread scans up to 1004
  slots at the configured interval, except after a valid config reload or an
  optional rak-samp skin/stream refresh request.
- [x] Add clear diagnostic logging for player-name matches, skin profiles,
  source changes, missing assets, invalid JSON mappings, and unavailable server
  ped models.

## Configuration and lifecycle

- [x] Auto-reload `wardrobe.json` without restarting GTA. New
  profiles and matching rules become available safely.
- [x] Support live skin-profile replacement when its TXD or DFF path changes.
  Restore every streamed clone using the old source, release it only after
  clone-identity liveness permits it, then load and apply the replacement on
  the game thread.
- [x] Detect TXD/DFF file changes even when `wardrobe.json` is unchanged.
  Compare modification time and file length about once per second, then rebuild
  the affected shared source for all matching local and remote players.
- [x] Support profile or matching-rule removal. Restore every affected
  streamed-in ped to the most recently observed normal server model and retain
  newer server resets.
- [x] Safely clean obsolete shared skin sources. Release their source clump and
  TXD only after a complete scan finds no recorded installed clone identity.
- [ ] Stress-test repeated live reloads and profile removals, including shared
  sources and streamed-out remote players, to verify that source/TXD counts
  remain stable.
- [x] Support toggling an individual matching rule or skin profile on and off.
- [x] Preserve and document the first-run behavior: create a missing
  `wardrobe.json` as `{}` and remain idle until a matching rule exists.
- [x] Add prioritized matching rules for player names and server model IDs:
  combined rules win over player-only rules, which win over model-only rules.

## Compatibility and safety

- [x] Detect the supported SA-MP client revision before reading its player
  structures. Select the matching layout for 0.3.7-R1, 0.3.7-R3-1, 0.3.7-R4,
  or 0.3.DL-R1, and log a clear unsupported-version error for every other
  build.
- [x] Verify the GTA executable before waiting on GTA structures or installing
  hooks. Require the supported 1.0 US PE fingerprint and every fixed
  GTA/RenderWare call target to match exact bytes; log a clear error and remain
  inactive when a target is unknown or already patched.
- [x] Smoke-test compatibility with Fastman92 Limit Adjuster both enabled and
  disabled. Wardrobe uses no private GTA model IDs and continues to reject any
  setup that patches a required entry point.
- [x] Prune applied, matched, and failed-application state after a complete
  SA-MP scan when a player streams out.

## Future event-driven detection

- [x] Keep polling as the default, reliable detection path.
- [x] Integrate the optional rak-samp host for incoming `SetPlayerSkin`, player
  stream-in, and player stream-out requests. Its callback only coalesces a
  frame-thread scan; if the host is unavailable, the configured complete-scan
  fallback remains active.
- [ ] Smoke-test ready rak-samp hosts on R1, R3-1, R4, and DL: confirm local and
  remote skin changes reconcile on the next frame, and repeat with the host
  absent, failed, and ABI-incompatible.

## Later quality of life

- [x] Add an optional MoonLoader (Lua) ImGui configuration UI after the
  configuration/reload workflow is stable. Keep it as a file-based front-end:
  Lua reads and edits `wardrobe.json`, while the Rust ASI continues
  to own GTA skin-source loading and observes changes through its existing reload
  path. Do not introduce Lua-to-Rust FFI for this.
- [x] Make the MoonLoader UI save JSON atomically (write a temporary file, then
  rename it) so the Rust loader never observes a partially written config.
- [x] Add profile management and status feedback to the MoonLoader UI.
- [x] Add an online-player picker to the MoonLoader rule editor. It should
  enumerate connected SA-MP players, search by nickname, and fill the selected
  rule's player-name selector without a Rust/Lua bridge.
- [x] Add named MoonLoader presets. A preset should capture the enabled state
  of the current profiles and rules, then apply that activation set later for
  a different character or server without duplicating skin asset definitions.
