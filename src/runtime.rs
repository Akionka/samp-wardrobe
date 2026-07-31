use crate::config::{CONFIG_PATH, ConfigWatcher, SkinConfig};
use crate::gta;
use crate::samp::{Samp, StreamedPed};
use crate::skin_loader::SkinManager;
use retour::GenericDetour;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const ADDR_CGAME_PROCESS: usize = 0x53BEE0;
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
    matched_players: HashSet<String>,
    applied_players: HashMap<String, AppliedPlayer>,
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
        // responsive without doing the full scan on every frame.
        let now = Instant::now();
        if self
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

        let streamed_peds = unsafe { self.samp.streamed_peds() };
        for StreamedPed { name, address } in streamed_peds {
            let Some(current_model_id) = (unsafe { gta::ped_model_id(address) }) else {
                continue;
            };

            let server_model_id = if self.skins.is_private_model(current_model_id) {
                self.applied_players
                    .get(&name)
                    .and_then(|applied| applied.last_server_model_id)
            } else {
                Some(current_model_id)
            };
            let Some(server_model_id) = server_model_id else {
                unsafe {
                    self.restore_server_model(&name, address, "losing its remembered server model")
                };
                continue;
            };

            let Some(rule) = self.config.matching_rule(&name, server_model_id).cloned() else {
                unsafe { self.restore_server_model(&name, address, "having no matching rule") };
                continue;
            };
            let skin_id = rule.profile_id;
            let Some(definition) = self.config.skins.get(&skin_id).cloned() else {
                unsafe { self.restore_server_model(&name, address, "removing its skin profile") };
                continue;
            };
            if !definition.enabled {
                unsafe { self.restore_server_model(&name, address, "disabling its skin profile") };
                continue;
            };

            if self.matched_players.insert(name.clone()) {
                log::info!("matched {name} with server model {server_model_id} to skin {skin_id}");
            }
            let Some(model_id) = (unsafe { self.skins.model_for(&skin_id, &definition) }) else {
                continue;
            };

            if current_model_id != model_id as i16 {
                unsafe { gta::set_ped_model_index(address, model_id) };
                self.applied_players.insert(
                    name.clone(),
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
                    .get(&name)
                    .and_then(|applied| applied.last_server_model_id)
                    .or(Some(server_model_id));
                self.applied_players.insert(
                    name.clone(),
                    AppliedPlayer {
                        skin_id,
                        custom_model_id: model_id,
                        last_server_model_id,
                    },
                );
            }
        }

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
        name: &str,
        ped: *mut std::ffi::c_void,
        reason: &str,
    ) {
        let Some(current_model_id) = (unsafe { gta::ped_model_id(ped) }) else {
            return;
        };
        let Some(applied) = self.applied_players.get(name).cloned() else {
            return;
        };

        let current_is_private = self.skins.is_private_model(current_model_id);
        if !current_is_private && current_model_id != applied.custom_model_id as i16 {
            // SA-MP has already supplied a normal model since the custom
            // mapping was removed. It is newer than our saved value, so leave
            // it alone.
            self.applied_players.remove(name);
            self.matched_players.remove(name);
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

        self.applied_players.remove(name);
        self.matched_players.remove(name);
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

pub unsafe fn install(config: SkinConfig, samp: Samp) -> Result<(), retour::Error> {
    if RUNTIME.set(Mutex::new(Runtime::new(config, samp))).is_err() {
        panic!("runtime was initialized twice");
    }

    let target: GameProcessFn = unsafe { std::mem::transmute(ADDR_CGAME_PROCESS) };
    let hook = unsafe { GenericDetour::new(target, game_process_detour as GameProcessFn)? };
    GAME_PROCESS_HOOK
        .set(hook)
        .expect("CGame::Process hook was installed twice");

    let hook = GAME_PROCESS_HOOK.get().unwrap();
    unsafe { hook.enable() }
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
