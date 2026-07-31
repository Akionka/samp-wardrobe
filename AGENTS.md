# Repository Guidelines

## Project Structure & Module Organization

Wardrobe is a Rust `cdylib` ASI plugin for 32-bit GTA San Andreas/SA-MP.
Production code lives in `src/`: `lib.rs` starts the plugin, `runtime.rs`
coordinates game-thread work, `samp.rs` reads SA-MP state, and `gta.rs` owns
GTA/RenderWare calls. Keep configuration, logging, memory access, model IDs,
and skin loading in their focused modules. The optional MoonLoader editor is
`moonloader/wardrobe_ui/wardrobe_ui.lua`; researched version details belong in
`docs/`. Keep user-facing setup information in `README.md` and planned work in
`TODO.md`.

## Project References

- [`core.md`](core.md) explains the implemented features, their runtime behavior,
  and the responsible code entities and files.
- [`architecture.md`](architecture.md) maps modules, structs, functions, ownership,
  and the interactions between the loader, SA-MP scanner, runtime, and GTA bridge.

## Documentation Maintenance

When a new feature or module is complete, update `core.md` and
`architecture.md` when the change affects their documented behavior or
structure. Do this once the implementation is complete, not after every
intermediate step.

`README.md` is user-facing documentation, not a change log. Do not mirror every
internal change there. Update it only when the user-facing behavior, setup, or
configuration would otherwise be inconsistent, or when a README update is
intentionally required. Make README updates only after the feature has been
merged into the `master` branch.

## Build, Test, and Development Commands

The configured Cargo target is `i686-pc-windows-msvc`.

- `cargo test` runs the Rust unit tests.
- `cargo fmt --check` verifies formatting; use `cargo fmt` to apply it.
- `cargo clippy --all-targets --all-features -- -D warnings` runs Clippy for
  every target and feature, treating warnings as errors.
- `cargo make debug` builds the debug ASI and copies `wardrobe.asi` plus its
  PDB to `$env:GTA_DIR`; debug builds wait for a debugger.
- `cargo make deploy` builds and copies the release ASI.
- `cargo make deploy-ui` copies the Lua editor to the GTA MoonLoader scripts
  directory.

Set `$env:GTA_DIR = 'D:\Games\GTASA'` before any deploy command. Do not deploy
or alter a game installation unless the task explicitly calls for it.

## Coding Style & Safety

Follow `rustfmt`; use `snake_case` for functions/modules and `CamelCase` for
types. Prefer small, focused modules and descriptive constants for addresses.
Treat SA-MP and GTA pointers as untrusted: use `memory::read`/
`ReadProcessMemory` for game-owned memory and validate version-specific bytes
before patching hooks. RenderWare and ped-model mutations must remain on the
`CGame::Process` game thread—background threads may only wait or prepare data.

## Testing Guidelines

Add unit tests beside the module under `#[cfg(test)]`. Name tests after the
observable behavior, e.g. `detects_each_supported_samp_build_from_its_entry_point`.
Run formatting and `cargo test` before handing work off. For game-facing
changes, state what needs in-game validation and inspect `wardrobe.log` after
the user tests it.

## Commit & Pull Request Guidelines

Use short, imperative commit subjects consistent with history: `Add guarded
SA-MP refresh hooks` or `Prune streamed-out player state`. Keep a commit focused
on one feature or fix. PRs should explain player-visible behavior, compatibility
impact, test commands, and any required in-game checks; include log excerpts or
screenshots for UI/debugging changes when useful.
