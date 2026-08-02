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

fn local_instance_reset_kind(
    render_object_changed: bool,
    render_object_address_reused: bool,
    remembered_model_id: Option<i16>,
    current_model_id: i16,
) -> Option<&'static str> {
    if render_object_changed && render_object_address_reused {
        Some("render-object address was reused with different geometry")
    } else if render_object_changed {
        Some("render-object pointer changed")
    } else if remembered_model_id.is_some_and(|model_id| model_id != current_model_id) {
        Some("model ID changed while GTA reused the render-object address")
    } else {
        None
    }
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

    fn restore_server_model(
        &mut self,
        frame: &GameFrame,
        player_id: PlayerId,
        name: Option<&str>,
        ped: &gta::Ped,
        reason: &str,
    ) {
        let Some(current_model_id) = gta::ped_model_id(ped) else {
            return;
        };
        let Some(applied) = self.applied_players.get(&player_id).cloned() else {
            return;
        };

        match applied.skin {
            AppliedSkin::PrivateModel { model_id } => {
                let current_is_private = self.skins.is_private_model(current_model_id);
                if !current_is_private && current_model_id != model_id as i16 {
                    // SA-MP has already supplied a normal model since the
                    // custom mapping was removed. It is newer than our saved
                    // value, so leave it alone.
                    self.applied_players.remove(&player_id);
                    self.matched_players.remove(&player_id);
                    return;
                }

                if let Some(server_model_id) = applied.last_server_model_id
                    && current_model_id != server_model_id
                {
                    gta::set_ped_model_index(frame, ped, server_model_id as i32);
                    log::info!(
                        "restored server model {server_model_id} for player {player_id} ({}) after {reason} for skin {}",
                        name.unwrap_or("unavailable name"),
                        applied.skin_id
                    );
                }
            }
            AppliedSkin::InstanceClump { render_object } => {
                let Some(current_render_object) = gta::ped_render_object(ped) else {
                    return;
                };
                if current_render_object != render_object {
                    // GTA/SA-MP already destroyed the custom clone and built a
                    // newer render object. Never overwrite that server reset.
                    self.applied_players.remove(&player_id);
                    self.matched_players.remove(&player_id);
                    return;
                }

                if let Some(server_model_id) = applied.last_server_model_id {
                    if let Err(restore_reason) =
                        gta::restore_instance_skin(frame, ped, server_model_id, render_object)
                    {
                        log::error!(
                            "could not rebuild the normal clump for local player {player_id} after {reason}: {restore_reason}; retaining instance state"
                        );
                        return;
                    }
                    log::info!(
                        "rebuilt server model {server_model_id} for local player {player_id} ({}) after {reason} for instance skin {}",
                        name.unwrap_or("unavailable name"),
                        applied.skin_id
                    );
                }
            }
        }

        if applied.last_server_model_id.is_none() {
            log::warn!(
                "cannot restore player {player_id} ({}) after {reason} for skin {}; no server model was observed",
                name.unwrap_or("unavailable name"),
                applied.skin_id
            );
        }

        self.applied_players.remove(&player_id);
        self.matched_players.remove(&player_id);
    }

    fn prune_streamed_out_players(
        &mut self,
        streamed_player_ids: &HashSet<PlayerId>,
        live_render_objects: Option<&HashSet<gta::PedRenderObject>>,
    ) {
        let applied_before = self.applied_players.len();
        let matched_before = self.matched_players.len();
        self.applied_players.retain(|player_id, applied| {
            if streamed_player_ids.contains(player_id) {
                return true;
            }
            match applied.skin {
                AppliedSkin::PrivateModel { .. } => false,
                AppliedSkin::InstanceClump { render_object } => live_render_objects
                    .is_none_or(|render_objects| render_objects.contains(&render_object)),
            }
        });
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

    fn live_ped_state(&self) -> Option<LivePedState> {
        let peds = self.samp.all_peds()?;
        let model_ids = peds.iter().map(gta::ped_model_id).collect();
        let render_objects = peds.iter().map(gta::ped_render_object).collect();
        Some(LivePedState {
            model_ids,
            render_objects,
        })
    }

    fn cleanup_retired_skins(&mut self, frame: &GameFrame, live_peds: Option<LivePedState>) {
        let live_instance_skin_ids = live_peds.as_ref().and_then(|state| {
            state.render_objects.as_ref().map(|render_objects| {
                self.applied_players
                    .values()
                    .filter_map(|applied| match applied.skin {
                        AppliedSkin::InstanceClump { render_object }
                            if render_objects.contains(&render_object) =>
                        {
                            Some(applied.skin_id.clone())
                        }
                        _ => None,
                    })
                    .collect::<HashSet<_>>()
            })
        });
        self.skins
            .cleanup_retired_instances(frame, live_instance_skin_ids);
        self.skins
            .cleanup_retired(frame, live_peds.and_then(|state| state.model_ids));
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

#[cfg(test)]
mod tests {
    use super::local_instance_reset_kind;

    #[test]
    fn detects_a_local_reset_when_only_the_server_model_changes() {
        assert_eq!(
            local_instance_reset_kind(false, false, Some(7), 294),
            Some("model ID changed while GTA reused the render-object address")
        );
    }

    #[test]
    fn keeps_an_unchanged_local_instance_applied() {
        assert_eq!(local_instance_reset_kind(false, false, Some(7), 7), None);
    }

    #[test]
    fn a_changed_render_object_is_always_a_local_reset() {
        assert_eq!(
            local_instance_reset_kind(true, false, Some(7), 7),
            Some("render-object pointer changed")
        );
    }

    #[test]
    fn detects_allocator_reuse_from_a_different_geometry() {
        assert_eq!(
            local_instance_reset_kind(true, true, Some(7), 7),
            Some("render-object address was reused with different geometry")
        );
    }
}
