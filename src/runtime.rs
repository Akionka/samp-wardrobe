use crate::config::{CONFIG_PATH, ConfigWatcher, SkinConfig};
use crate::game_frame::GameFrame;
use crate::gta;
use crate::samp::{PlayerId, Samp, StreamedPed};
use crate::samp_hooks;
use crate::skin_loader::{InstanceSkinLookup, SkinManager};
use retour::GenericDetour;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[path = "runtime/lifecycle.rs"]
mod lifecycle;

use lifecycle::local_instance_reset_kind;

const POLL_INTERVAL: Duration = Duration::from_millis(200);

type GameProcessFn = unsafe extern "cdecl" fn();

#[derive(Clone, Copy, Debug)]
enum AppliedSkin {
    PrivateModel { model_id: i32 },
    InstanceClump { render_object: gta::PedRenderObject },
}

#[derive(Clone, Debug)]
struct AppliedPlayer {
    skin_id: String,
    // Captured before Wardrobe first changes the ped's model or render object.
    last_server_model_id: Option<i16>,
    skin: AppliedSkin,
}

struct LivePedState {
    model_ids: Option<HashSet<i16>>,
    render_objects: Option<HashSet<gta::PedRenderObject>>,
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

    fn process_game_frame(&mut self, frame: &GameFrame) {
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
            self.skins.retain_instance_skins(&HashSet::new());
            let live_peds = self.live_ped_state();
            self.cleanup_retired_skins(frame, live_peds);
            return;
        }

        let Some(streamed_peds) = self.samp.streamed_peds() else {
            let live_peds = self.live_ped_state();
            self.cleanup_retired_skins(frame, live_peds);
            return;
        };
        let streamed_player_ids = streamed_peds
            .iter()
            .map(|ped| ped.player_id)
            .collect::<HashSet<_>>();
        let mut desired_instance_skins = HashSet::new();

        for StreamedPed {
            player_id,
            is_local,
            name,
            address,
        } in streamed_peds
        {
            let name = name.as_deref();
            let Some(current_model_id) = gta::ped_model_id(&address) else {
                continue;
            };

            let role_changed = self.applied_players.get(&player_id).is_some_and(|applied| {
                is_local != matches!(applied.skin, AppliedSkin::InstanceClump { .. })
            });
            if role_changed {
                if is_local {
                    self.restore_server_model(
                        frame,
                        player_id,
                        name,
                        &address,
                        "changing between local and remote SA-MP roles",
                    );
                } else {
                    self.applied_players.remove(&player_id);
                    self.matched_players.remove(&player_id);
                }
                continue;
            }

            if is_local {
                let Some(current_render_object) = gta::ped_render_object(&address) else {
                    continue;
                };
                if let Some(AppliedPlayer {
                    skin_id,
                    last_server_model_id,
                    skin: AppliedSkin::InstanceClump { render_object },
                    ..
                }) = self.applied_players.get(&player_id)
                    && let Some(reset_kind) = local_instance_reset_kind(
                        *render_object != current_render_object,
                        *render_object != current_render_object
                            && render_object.has_same_address(current_render_object),
                        *last_server_model_id,
                        current_model_id,
                    )
                {
                    log::info!(
                        "local player {player_id} ({}) received a server reset while instance skin {skin_id} was applied ({reset_kind}; remembered model {:?}, current model {current_model_id})",
                        name.unwrap_or("unavailable name"),
                        last_server_model_id
                    );
                    self.applied_players.remove(&player_id);
                    self.matched_players.remove(&player_id);
                }
            }

            let server_model_id = if !is_local && self.skins.is_private_model(current_model_id) {
                self.applied_players
                    .get(&player_id)
                    .and_then(|applied| applied.last_server_model_id)
            } else {
                Some(current_model_id)
            };
            let Some(server_model_id) = server_model_id else {
                self.restore_server_model(
                    frame,
                    player_id,
                    name,
                    &address,
                    "losing its remembered server model",
                );
                continue;
            };

            let Some(rule) = self.config.matching_rule(name, server_model_id).cloned() else {
                self.restore_server_model(
                    frame,
                    player_id,
                    name,
                    &address,
                    "having no matching rule",
                );
                continue;
            };
            let skin_id = rule.profile_id;
            let Some(definition) = self.config.skins.get(&skin_id).cloned() else {
                self.restore_server_model(
                    frame,
                    player_id,
                    name,
                    &address,
                    "removing its skin profile",
                );
                continue;
            };
            if !definition.enabled {
                self.restore_server_model(
                    frame,
                    player_id,
                    name,
                    &address,
                    "disabling its skin profile",
                );
                continue;
            };

            if self.matched_players.insert(player_id) {
                log::info!(
                    "matched player {player_id} ({}) with server model {server_model_id} to skin {skin_id}",
                    name.unwrap_or("unavailable name")
                );
            }

            if is_local {
                desired_instance_skins.insert(skin_id.clone());
                let changed_skin = self.applied_players.get(&player_id).is_some_and(|applied| {
                    matches!(applied.skin, AppliedSkin::InstanceClump { .. })
                        && applied.skin_id != skin_id
                });
                if changed_skin {
                    self.restore_server_model(
                        frame,
                        player_id,
                        name,
                        &address,
                        "switching its local instance skin",
                    );
                    continue;
                }

                let already_applied = self.applied_players.get(&player_id).is_some_and(|applied| {
                    matches!(applied.skin, AppliedSkin::InstanceClump { .. })
                        && applied.skin_id == skin_id
                });
                match self.skins.instance_for(frame, &skin_id, &definition) {
                    InstanceSkinLookup::Ready if already_applied => {}
                    InstanceSkinLookup::Ready => {
                        let resources = self
                            .skins
                            .instance_resources(&skin_id)
                            .expect("ready instance skin has no resources");
                        match gta::apply_instance_skin(frame, &address, server_model_id, resources)
                        {
                            Ok(render_object) => {
                                self.applied_players.insert(
                                    player_id,
                                    AppliedPlayer {
                                        skin_id,
                                        last_server_model_id: Some(server_model_id),
                                        skin: AppliedSkin::InstanceClump { render_object },
                                    },
                                );
                                log::debug!(
                                    "applied local instance skin to player {player_id} ({}); server model remains {server_model_id}",
                                    name.unwrap_or("unavailable name")
                                );
                            }
                            Err(reason) => {
                                log::error!(
                                    "could not apply local instance skin {skin_id} to player {player_id} ({}): {reason}; kept or recovered server model {server_model_id}",
                                    name.unwrap_or("unavailable name")
                                );
                            }
                        }
                    }
                    InstanceSkinLookup::ResetRequired => {
                        self.restore_server_model(
                            frame,
                            player_id,
                            name,
                            &address,
                            "changing its local instance source",
                        );
                    }
                    InstanceSkinLookup::Unavailable => {}
                }
                continue;
            }

            let Some(model_id) = self.skins.model_for(frame, &skin_id, &definition) else {
                continue;
            };

            if current_model_id != model_id as i16 {
                gta::set_ped_model_index(frame, &address, model_id);
                self.applied_players.insert(
                    player_id,
                    AppliedPlayer {
                        skin_id,
                        last_server_model_id: Some(server_model_id),
                        skin: AppliedSkin::PrivateModel { model_id },
                    },
                );
                log::debug!(
                    "applied custom model {model_id} to player {player_id} ({})",
                    name.unwrap_or("unavailable name")
                );
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
                        last_server_model_id,
                        skin: AppliedSkin::PrivateModel { model_id },
                    },
                );
            }
        }

        self.skins.retain_instance_skins(&desired_instance_skins);
        let live_peds = self.live_ped_state();
        self.prune_streamed_out_players(
            &streamed_player_ids,
            live_peds
                .as_ref()
                .and_then(|state| state.render_objects.as_ref()),
        );
        self.cleanup_retired_skins(frame, live_peds);
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
}

static RUNTIME: OnceLock<Mutex<Runtime>> = OnceLock::new();
static GAME_PROCESS_HOOK: OnceLock<GenericDetour<GameProcessFn>> = OnceLock::new();
static DETOUR_ENTRY_LOGGED: AtomicBool = AtomicBool::new(false);
static DETOUR_TRAMPOLINE_LOGGED: AtomicBool = AtomicBool::new(false);

/// Installs Wardrobe's frame detour only after verifying every fixed GTA
/// target used by the runtime. The raw detour setup remains local to this
/// function so startup cannot bypass the executable guard.
pub fn install(config: SkinConfig, samp: Samp) -> Result<(), String> {
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

    match samp_hooks::install(&samp) {
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
    let frame = unsafe { GameFrame::enter() };
    runtime.process_game_frame(&frame);
}
