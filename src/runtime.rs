use crate::config::{CONFIG_PATH, ConfigWatcher, SkinConfig};
use crate::game_frame::GameFrame;
use crate::gta;
use crate::samp::{PlayerId, Samp, StreamedPed};
use crate::samp_hooks;
use crate::skin_loader::{SkinManager, SkinSourceLookup};
use retour::GenericDetour;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[path = "runtime/lifecycle.rs"]
mod lifecycle;

use lifecycle::skin_clone_reset_kind;

const POLL_INTERVAL: Duration = Duration::from_millis(200);

type GameProcessFn = unsafe extern "cdecl" fn();

#[derive(Clone, Debug)]
struct AppliedPlayer {
    skin_id: String,
    // Captured before Wardrobe replaces the normal server clump.
    server_model_id: i16,
    render_object: gta::PedRenderObject,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FailedApplication {
    skin_id: String,
    source_generation: u64,
    server_model_id: i16,
    render_object: gta::PedRenderObject,
}

impl FailedApplication {
    fn matches(
        &self,
        skin_id: &str,
        source_generation: u64,
        server_model_id: i16,
        render_object: gta::PedRenderObject,
    ) -> bool {
        self.skin_id == skin_id
            && self.source_generation == source_generation
            && self.server_model_id == server_model_id
            && self.render_object == render_object
    }
}

fn streamed_profile_users(
    applied_players: &HashMap<PlayerId, AppliedPlayer>,
    streamed_player_ids: impl IntoIterator<Item = PlayerId>,
    skin_id: &str,
) -> Vec<PlayerId> {
    streamed_player_ids
        .into_iter()
        .filter(|player_id| {
            applied_players
                .get(player_id)
                .is_some_and(|applied| applied.skin_id == skin_id)
        })
        .collect()
}

fn retain_applied_sources(
    desired_sources: &mut HashSet<String>,
    applied_players: &HashMap<PlayerId, AppliedPlayer>,
) {
    desired_sources.extend(
        applied_players
            .values()
            .map(|applied| applied.skin_id.clone()),
    );
}

struct LivePedState {
    render_objects: Option<HashSet<gta::PedRenderObject>>,
}

struct Runtime {
    config: SkinConfig,
    config_watcher: ConfigWatcher,
    skins: SkinManager,
    samp: Samp,
    matched_players: HashSet<PlayerId>,
    applied_players: HashMap<PlayerId, AppliedPlayer>,
    failed_applications: HashMap<PlayerId, FailedApplication>,
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
            failed_applications: HashMap::new(),
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
            self.failed_applications.clear();
            self.skins.retain_sources(&HashSet::new());
            let live_peds = self.live_ped_state();
            self.cleanup_retired_sources(frame, live_peds);
            return;
        }

        let Some(streamed_peds) = self.samp.streamed_peds() else {
            let live_peds = self.live_ped_state();
            self.cleanup_retired_sources(frame, live_peds);
            return;
        };
        let streamed_player_ids = streamed_peds
            .iter()
            .map(|ped| ped.player_id)
            .collect::<HashSet<_>>();
        let mut desired_sources = HashSet::new();
        let mut restoration_attempted_sources = HashSet::new();

        for StreamedPed {
            player_id,
            name,
            address,
        } in &streamed_peds
        {
            let name = name.as_deref();
            let Some(current_model_id) = gta::ped_model_id(address) else {
                continue;
            };
            let Some(current_render_object) = gta::ped_render_object(address) else {
                continue;
            };

            if let Some(applied) = self.applied_players.get(player_id)
                && let Some(reset_kind) = skin_clone_reset_kind(
                    applied.render_object != current_render_object,
                    applied
                        .render_object
                        .has_same_address(current_render_object),
                    applied.server_model_id,
                    current_model_id,
                )
            {
                log::info!(
                    "player {player_id} ({}) received a server reset while skin {} was applied ({reset_kind}; remembered model {}, current model {current_model_id})",
                    name.unwrap_or("unavailable name"),
                    applied.skin_id,
                    applied.server_model_id,
                );
                self.applied_players.remove(player_id);
                self.failed_applications.remove(player_id);
                self.matched_players.remove(player_id);
            }

            let server_model_id = current_model_id;
            let Some(rule) = self.config.matching_rule(name, server_model_id).cloned() else {
                self.restore_server_model(
                    frame,
                    *player_id,
                    name,
                    address,
                    "having no matching rule",
                );
                self.failed_applications.remove(player_id);
                continue;
            };
            let skin_id = rule.profile_id;
            let Some(definition) = self.config.skins.get(&skin_id).cloned() else {
                self.restore_server_model(
                    frame,
                    *player_id,
                    name,
                    address,
                    "removing its skin profile",
                );
                self.failed_applications.remove(player_id);
                continue;
            };
            if !definition.enabled {
                self.restore_server_model(
                    frame,
                    *player_id,
                    name,
                    address,
                    "disabling its skin profile",
                );
                self.failed_applications.remove(player_id);
                continue;
            }

            if self.matched_players.insert(*player_id) {
                log::info!(
                    "matched player {player_id} ({}) with server model {server_model_id} to skin {skin_id}",
                    name.unwrap_or("unavailable name")
                );
            }
            desired_sources.insert(skin_id.clone());

            let changed_skin = self
                .applied_players
                .get(player_id)
                .is_some_and(|applied| applied.skin_id != skin_id);
            if changed_skin {
                self.restore_server_model(
                    frame,
                    *player_id,
                    name,
                    address,
                    "switching its skin profile",
                );
                continue;
            }

            let already_applied = self
                .applied_players
                .get(player_id)
                .is_some_and(|applied| applied.skin_id == skin_id);
            match self.skins.source_for(frame, &skin_id, &definition) {
                SkinSourceLookup::Ready { .. } if already_applied => {}
                SkinSourceLookup::Ready { generation } => {
                    if self.application_failed(
                        *player_id,
                        &skin_id,
                        generation,
                        server_model_id,
                        current_render_object,
                    ) {
                        continue;
                    }

                    let resources = self
                        .skins
                        .source_resources(&skin_id)
                        .expect("ready skin source has no resources");
                    match gta::apply_skin_source(frame, address, server_model_id, resources) {
                        Ok(render_object) => {
                            self.skins.record_clone(&skin_id, render_object);
                            self.applied_players.insert(
                                *player_id,
                                AppliedPlayer {
                                    skin_id,
                                    server_model_id,
                                    render_object,
                                },
                            );
                            self.failed_applications.remove(player_id);
                            log::debug!(
                                "applied skin source to player {player_id} ({}); server model remains {server_model_id}",
                                name.unwrap_or("unavailable name")
                            );
                        }
                        Err(reason) => {
                            let restored_render_object =
                                gta::ped_render_object(address).unwrap_or(current_render_object);
                            self.failed_applications.insert(
                                *player_id,
                                FailedApplication {
                                    skin_id: skin_id.clone(),
                                    source_generation: generation,
                                    server_model_id,
                                    render_object: restored_render_object,
                                },
                            );
                            log::error!(
                                "could not apply skin source {skin_id} to player {player_id} ({}): {reason}; kept or recovered server model {server_model_id}",
                                name.unwrap_or("unavailable name")
                            );
                        }
                    }
                }
                SkinSourceLookup::RestoreRequired => {
                    if restoration_attempted_sources.insert(skin_id.clone()) {
                        self.restore_profile_users(
                            frame,
                            &streamed_peds,
                            &skin_id,
                            "changing its shared skin source",
                        );
                    }
                }
                SkinSourceLookup::Unavailable => {}
            }
        }

        // A transient ped-model/render-object read leaves an applied clone in
        // place. Its source must remain loaded until a healthy pass can either
        // restore that clone or observe a server reset.
        retain_applied_sources(&mut desired_sources, &self.applied_players);
        self.skins.retain_sources(&desired_sources);
        let live_peds = self.live_ped_state();
        self.prune_streamed_out_players(&streamed_player_ids);
        self.cleanup_retired_sources(frame, live_peds);
    }

    fn application_failed(
        &self,
        player_id: PlayerId,
        skin_id: &str,
        source_generation: u64,
        server_model_id: i16,
        render_object: gta::PedRenderObject,
    ) -> bool {
        self.failed_applications
            .get(&player_id)
            .is_some_and(|failed| {
                failed.matches(skin_id, source_generation, server_model_id, render_object)
            })
    }

    fn restore_profile_users(
        &mut self,
        frame: &GameFrame,
        streamed_peds: &[StreamedPed],
        skin_id: &str,
        reason: &str,
    ) {
        let player_ids = streamed_profile_users(
            &self.applied_players,
            streamed_peds.iter().map(|ped| ped.player_id),
            skin_id,
        );
        for player_id in player_ids {
            let ped = streamed_peds
                .iter()
                .find(|ped| ped.player_id == player_id)
                .expect("streamed source user disappeared during restoration");
            self.restore_server_model(
                frame,
                ped.player_id,
                ped.name.as_deref(),
                &ped.address,
                reason,
            );
        }
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
        self.failed_applications.clear();
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

#[cfg(test)]
mod tests {
    use super::{
        AppliedPlayer, FailedApplication, gta, retain_applied_sources, streamed_profile_users,
    };
    use std::collections::{HashMap, HashSet};

    #[test]
    fn a_source_generation_change_makes_a_failed_application_eligible() {
        let failed = FailedApplication {
            skin_id: "staff".to_owned(),
            source_generation: 1,
            server_model_id: 7,
            render_object: gta::PedRenderObject::for_test(0x1000, 0x2000),
        };
        assert!(failed.matches(
            "staff",
            1,
            7,
            gta::PedRenderObject::for_test(0x1000, 0x2000)
        ));
        assert!(!failed.matches(
            "staff",
            2,
            7,
            gta::PedRenderObject::for_test(0x1000, 0x2000)
        ));
    }

    #[test]
    fn a_source_change_selects_every_currently_streamed_profile_user_for_restoration() {
        let render_object = gta::PedRenderObject::for_test(0x1000, 0x2000);
        let mut applied = HashMap::new();
        applied.insert(
            1,
            AppliedPlayer {
                skin_id: "staff".to_owned(),
                server_model_id: 7,
                render_object,
            },
        );
        applied.insert(
            2,
            AppliedPlayer {
                skin_id: "staff".to_owned(),
                server_model_id: 15,
                render_object,
            },
        );
        applied.insert(
            3,
            AppliedPlayer {
                skin_id: "visitor".to_owned(),
                server_model_id: 20,
                render_object,
            },
        );

        assert_eq!(streamed_profile_users(&applied, [2, 3, 1], "staff"), [2, 1]);
    }

    #[test]
    fn retained_applied_clone_keeps_its_source_desired_after_a_ped_read_failure() {
        let mut applied = HashMap::new();
        applied.insert(
            1,
            AppliedPlayer {
                skin_id: "staff".to_owned(),
                server_model_id: 7,
                render_object: gta::PedRenderObject::for_test(0x1000, 0x2000),
            },
        );
        let mut desired_sources = HashSet::new();

        retain_applied_sources(&mut desired_sources, &applied);

        assert_eq!(desired_sources, HashSet::from(["staff".to_owned()]));
    }
}
