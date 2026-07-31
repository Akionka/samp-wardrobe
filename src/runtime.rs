use crate::config::{CONFIG_PATH, ConfigWatcher, SkinConfig};
use crate::gta;
use crate::samp::{PlayerId, Samp, StreamedPed};
use crate::samp_hooks;
use crate::skin_loader::SkinManager;
use retour::GenericDetour;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(200);

type GameProcessFn = unsafe extern "cdecl" fn();

#[derive(Clone, Debug)]
struct AppliedPlayer {
    skin_id: String,
    custom_model_id: i32,
    // Captured before the loader first assigns a private model and whenever
    // SA-MP later changes the ped back to an ordinary GTA model.
    last_server_model_id: Option<i16>,
}

struct Runtime {
    config: SkinConfig,
    config_watcher: ConfigWatcher,
    skins: SkinManager,
    samp: Samp,
    matched_players: HashSet<PlayerId>,
    applied_players: HashMap<PlayerId, AppliedPlayer>,
    last_poll: Option<Instant>,
}

impl Runtime {
    fn new(config: SkinConfig, samp: Samp) -> Self {
        Self {
            config,
            config_watcher: ConfigWatcher::new(),
            skins: SkinManager::default(),
            samp,
            matched_players: HashSet::new(),
            applied_players: HashMap::new(),
            last_poll: None,
        }
    }

    unsafe fn process_game_frame(&mut self) {
        // The hook runs every GTA frame, but scanning a 1004-slot SA-MP pool
        // does not need to. Five polls per second keeps skin changes
        // responsive without doing the full scan on every frame. Guarded
        // SA-MP event hooks may request an earlier pass after a remote ped
        // spawns or the server changes a skin.
        let now = Instant::now();
        let event_refresh_reason = samp_hooks::take_refresh_request();
        if let Some(reason) = event_refresh_reason {
            log::debug!("SA-MP event requested an immediate skin scan after {reason}");
        }
        if event_refresh_reason.is_none()
            && self
                .last_poll
                .is_some_and(|last_poll| now.duration_since(last_poll) < POLL_INTERVAL)
        {
            return;
        }
        self.last_poll = Some(now);

        self.reload_config_if_changed();

        if self.config.rules.is_empty() && self.applied_players.is_empty() {
            unsafe { self.cleanup_retired_skins() };
            return;
        }

        let Some(streamed_peds) = (unsafe { self.samp.streamed_peds() }) else {
            unsafe { self.cleanup_retired_skins() };
            return;
        };
        let streamed_player_ids = streamed_peds
            .iter()
            .map(|ped| ped.player_id)
            .collect::<HashSet<_>>();

        for StreamedPed {
            player_id,
            name,
            address,
        } in streamed_peds
        {
            let Some(current_model_id) = (unsafe { gta::ped_model_id(address) }) else {
                continue;
            };

            let server_model_id = if self.skins.is_private_model(current_model_id) {
                self.applied_players
                    .get(&player_id)
                    .and_then(|applied| applied.last_server_model_id)
            } else {
                Some(current_model_id)
            };
            let Some(server_model_id) = server_model_id else {
                unsafe {
                    self.restore_server_model(
                        player_id,
                        &name,
                        address,
                        "losing its remembered server model",
                    )
                };
                continue;
            };

            let Some(rule) = self.config.matching_rule(&name, server_model_id).cloned() else {
                unsafe {
                    self.restore_server_model(player_id, &name, address, "having no matching rule")
                };
                continue;
            };
            let skin_id = rule.profile_id;
            let Some(definition) = self.config.skins.get(&skin_id).cloned() else {
                unsafe {
                    self.restore_server_model(
                        player_id,
                        &name,
                        address,
                        "removing its skin profile",
                    )
                };
                continue;
            };
            if !definition.enabled {
                unsafe {
                    self.restore_server_model(
                        player_id,
                        &name,
                        address,
                        "disabling its skin profile",
                    )
                };
                continue;
            };

            if self.matched_players.insert(player_id) {
                log::info!("matched {name} with server model {server_model_id} to skin {skin_id}");
            }
            let Some(model_id) = (unsafe { self.skins.model_for(&skin_id, &definition) }) else {
                continue;
            };

            if current_model_id != model_id as i16 {
                unsafe { gta::set_ped_model_index(address, model_id) };
                self.applied_players.insert(
                    player_id,
                    AppliedPlayer {
                        skin_id,
                        custom_model_id: model_id,
                        last_server_model_id: Some(server_model_id),
                    },
                );
                log::debug!("applied custom model {model_id} to {name}");
            } else {
                let last_server_model_id = self
                    .applied_players
                    .get(&player_id)
                    .and_then(|applied| applied.last_server_model_id)
                    .or(Some(server_model_id));
                self.applied_players.insert(
                    player_id,
                    AppliedPlayer {
                        skin_id,
                        custom_model_id: model_id,
                        last_server_model_id,
                    },
                );
            }
        }

        self.prune_streamed_out_players(&streamed_player_ids);
        unsafe { self.cleanup_retired_skins() };
    }

    fn reload_config_if_changed(&mut self) {
        let Some(config) = self.config_watcher.poll_change() else {
            return;
        };

        let skin_count = config.skins.len();
        let rule_count = config.rules.len();
        self.skins.apply_config(&config);
        self.config = config;
        self.matched_players.clear();
        log::info!("reloaded {CONFIG_PATH}: {skin_count} skin(s), {rule_count} rule(s)");
    }

    unsafe fn restore_server_model(
        &mut self,
        player_id: PlayerId,
        name: &str,
        ped: *mut std::ffi::c_void,
        reason: &str,
    ) {
        let Some(current_model_id) = (unsafe { gta::ped_model_id(ped) }) else {
            return;
        };
        let Some(applied) = self.applied_players.get(&player_id).cloned() else {
            return;
        };

        let current_is_private = self.skins.is_private_model(current_model_id);
        if !current_is_private && current_model_id != applied.custom_model_id as i16 {
            // SA-MP has already supplied a normal model since the custom
            // mapping was removed. It is newer than our saved value, so leave
            // it alone.
            self.applied_players.remove(&player_id);
            self.matched_players.remove(&player_id);
            return;
        }

        if let Some(server_model_id) = applied.last_server_model_id {
            if current_model_id != server_model_id {
                unsafe { gta::set_ped_model_index(ped, server_model_id as i32) };
                log::info!(
                    "restored server model {server_model_id} for {name} after {reason} for skin {}",
                    applied.skin_id
                );
            }
        } else {
            log::warn!(
                "cannot restore {name} after {reason} for skin {}; no server model was observed",
                applied.skin_id
            );
        }

        self.applied_players.remove(&player_id);
        self.matched_players.remove(&player_id);
    }

    fn prune_streamed_out_players(&mut self, streamed_player_ids: &HashSet<PlayerId>) {
        let applied_before = self.applied_players.len();
        let matched_before = self.matched_players.len();
        self.applied_players
            .retain(|player_id, _| streamed_player_ids.contains(player_id));
        self.matched_players
            .retain(|player_id| streamed_player_ids.contains(player_id));

        let pruned_applied = applied_before - self.applied_players.len();
        let pruned_matched = matched_before - self.matched_players.len();
        if pruned_applied != 0 || pruned_matched != 0 {
            log::debug!(
                "pruned {pruned_applied} applied and {pruned_matched} matched player state entries after a complete SA-MP ped scan"
            );
        }
    }

    unsafe fn cleanup_retired_skins(&mut self) {
        let live_model_ids = (unsafe { self.samp.all_peds() }).and_then(|peds| {
            peds.into_iter()
                .map(|ped| unsafe { gta::ped_model_id(ped) })
                .collect::<Option<HashSet<_>>>()
        });
        unsafe { self.skins.cleanup_retired(live_model_ids) };
    }
}

static RUNTIME: OnceLock<Mutex<Runtime>> = OnceLock::new();
static GAME_PROCESS_HOOK: OnceLock<GenericDetour<GameProcessFn>> = OnceLock::new();
static DETOUR_ENTRY_LOGGED: AtomicBool = AtomicBool::new(false);
static DETOUR_TRAMPOLINE_LOGGED: AtomicBool = AtomicBool::new(false);

pub unsafe fn install(config: SkinConfig, samp: Samp) -> Result<(), String> {
    gta::validate_executable()?;

    let target_address = gta::cgame_process_address();
    let target: GameProcessFn = unsafe { std::mem::transmute(target_address) };
    let hook = unsafe { GenericDetour::new(target, game_process_detour as GameProcessFn) }
        .map_err(|error| format!("could not prepare CGame::Process hook: {error}"))?;

    if RUNTIME.set(Mutex::new(Runtime::new(config, samp))).is_err() {
        panic!("runtime was initialized twice");
    }

    GAME_PROCESS_HOOK
        .set(hook)
        .expect("CGame::Process hook was installed twice");

    let hook = GAME_PROCESS_HOOK.get().unwrap();
    unsafe { hook.enable() }
        .map_err(|error| format!("could not enable CGame::Process hook: {error}"))?;

    match unsafe { samp_hooks::install(&samp) } {
        Ok(()) => log::info!("installed guarded SA-MP spawn and skin-change hooks"),
        Err(error) => log::warn!("SA-MP event hooks are unavailable; using polling: {error}"),
    }

    Ok(())
}

unsafe extern "cdecl" fn game_process_detour() {
    if !DETOUR_ENTRY_LOGGED.swap(true, Ordering::Relaxed) {
        log::info!("CGame::Process detour entered; calling the GTA trampoline");
    }

    // GenericDetour::call executes the generated trampoline, never this
    // detour.
    let hook = GAME_PROCESS_HOOK
        .get()
        .expect("CGame::Process hook was enabled before it was stored");
    unsafe { hook.call() };

    if !DETOUR_TRAMPOLINE_LOGGED.swap(true, Ordering::Relaxed) {
        log::info!("CGame::Process trampoline returned; starting custom polling");
    }
    let runtime = RUNTIME
        .get()
        .expect("runtime was initialized before the CGame::Process hook");
    let mut runtime = runtime.lock().unwrap_or_else(|error| error.into_inner());
    unsafe { runtime.process_game_frame() };
}
