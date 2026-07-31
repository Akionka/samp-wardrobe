use crate::config::{SkinConfig, SkinDefinition, SkinSourceRevision, skin_source_revision};
use crate::gta::{self, SkinResources};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

const ASSET_RELOAD_INTERVAL: Duration = Duration::from_secs(1);
const RETIRED_MODEL_GRACE_PERIOD: Duration = Duration::from_secs(1);

#[derive(Clone, Debug)]
struct LoadedSkin {
    resources: SkinResources,
    source: SkinSourceRevision,
}

#[derive(Clone, Debug)]
struct RetiredSkin {
    skin_id: String,
    resources: SkinResources,
    retired_at: Instant,
}

#[derive(Default)]
pub struct SkinManager {
    loaded_models: HashMap<String, LoadedSkin>,
    // A private ID remains protected from server-model handling while its old
    // resources await cleanup. Once GTA/RW teardown succeeds, the inert model
    // entry can be reused by this loader.
    private_model_ids: HashSet<i32>,
    retired_skins: Vec<RetiredSkin>,
    recyclable_model_ids: HashSet<i32>,
    failed_profiles: HashMap<String, SkinSourceRevision>,
    last_asset_check: HashMap<String, Instant>,
}

impl SkinManager {
    pub fn apply_config(&mut self, config: &SkinConfig) {
        let referenced_skins = config
            .players
            .values()
            .filter(|assignment| assignment.is_enabled())
            .map(|assignment| assignment.skin_id())
            .filter(|skin_id| {
                config
                    .skins
                    .get(*skin_id)
                    .is_some_and(|definition| definition.enabled)
            })
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        let no_longer_needed = self
            .loaded_models
            .keys()
            .filter(|skin_id| !referenced_skins.contains(*skin_id))
            .cloned()
            .collect::<Vec<_>>();

        for skin_id in no_longer_needed {
            let loaded = self
                .loaded_models
                .remove(&skin_id)
                .expect("loaded skin disappeared while scheduling cleanup");
            self.retire(skin_id, loaded);
        }

        // A corrected asset path or a newly added profile should be allowed to
        // load on the next matching poll.
        self.failed_profiles.clear();
    }

    pub fn is_private_model(&self, model_id: i16) -> bool {
        self.private_model_ids.contains(&(model_id as i32))
    }

    pub unsafe fn model_for(&mut self, skin_id: &str, definition: &SkinDefinition) -> Option<i32> {
        let now = Instant::now();
        let loaded_model = self.loaded_models.get(skin_id);
        let checked_recently = self
            .last_asset_check
            .get(skin_id)
            .is_some_and(|last_check| now.duration_since(*last_check) < ASSET_RELOAD_INTERVAL);

        if let Some(loaded) = loaded_model {
            if loaded.source.definition == *definition && checked_recently {
                return Some(loaded.resources.model_id);
            }
        }

        // A failed load is retried only after the asset check interval, unless
        // the JSON profile itself changed. This keeps a bad path from filling
        // the log or consuming game-thread time every poll.
        if checked_recently
            && self
                .failed_profiles
                .get(skin_id)
                .is_some_and(|failed| failed.definition == *definition)
        {
            return loaded_model.map(|loaded| loaded.resources.model_id);
        }

        let source = skin_source_revision(definition);
        self.last_asset_check.insert(skin_id.to_owned(), now);
        if let Some(loaded) = self.loaded_models.get(skin_id) {
            if loaded.source == source {
                return Some(loaded.resources.model_id);
            }
        }
        if self.failed_profiles.get(skin_id) == Some(&source) {
            return self
                .loaded_models
                .get(skin_id)
                .map(|loaded| loaded.resources.model_id);
        }

        let recycled_model_id = self.take_recyclable_model_id();
        let loaded_skin = unsafe { gta::load_skin(skin_id, definition, recycled_model_id) };
        match loaded_skin {
            Ok(resources) => {
                let model_id = resources.model_id;
                let replaced_model = self
                    .loaded_models
                    .insert(skin_id.to_owned(), LoadedSkin { resources, source });
                self.private_model_ids.insert(model_id);
                self.failed_profiles.remove(skin_id);
                if let Some(previous) = replaced_model {
                    let previous_model_id = previous.resources.model_id;
                    self.retire(skin_id.to_owned(), previous);
                    log::info!(
                        "replaced skin {skin_id}: private model {previous_model_id} -> {model_id}; queued old resources for cleanup"
                    );
                }
                Some(model_id)
            }
            Err(failure) => {
                if let Some(model_id) = failure.recyclable_model_id {
                    self.recyclable_model_ids.insert(model_id);
                }
                self.failed_profiles.insert(skin_id.to_owned(), source);
                if let Some(loaded) = self.loaded_models.get(skin_id) {
                    log::error!(
                        "skin {skin_id} reload failed; keeping private model {} active",
                        loaded.resources.model_id
                    );
                    Some(loaded.resources.model_id)
                } else {
                    log::error!("skin {skin_id} is unavailable until its files or profile change");
                    None
                }
            }
        }
    }

    pub unsafe fn cleanup_retired(&mut self, live_model_ids: Option<HashSet<i16>>) {
        if self.retired_skins.is_empty() {
            return;
        }
        let Some(live_model_ids) = live_model_ids else {
            log::debug!("deferred retired-skin cleanup because the SA-MP ped scan was incomplete");
            return;
        };

        let now = Instant::now();
        let mut ready_for_cleanup = Vec::new();
        self.retired_skins.retain(|retired| {
            let still_in_use = live_model_ids.contains(&(retired.resources.model_id as i16));
            let old_enough = now.duration_since(retired.retired_at) >= RETIRED_MODEL_GRACE_PERIOD;
            if still_in_use || !old_enough {
                true
            } else {
                ready_for_cleanup.push(retired.clone());
                false
            }
        });

        for retired in ready_for_cleanup {
            if unsafe { gta::release_skin_resources(&retired.skin_id, retired.resources) } {
                self.private_model_ids.remove(&retired.resources.model_id);
                self.recyclable_model_ids.insert(retired.resources.model_id);
            } else {
                // Do not recycle a model whose old clump or TXD could still be
                // alive. A later game-thread pass will retry from the safe
                // state.
                self.retired_skins.push(RetiredSkin {
                    retired_at: now,
                    ..retired
                });
            }
        }
    }

    fn take_recyclable_model_id(&mut self) -> Option<i32> {
        let model_id = self.recyclable_model_ids.iter().next().copied();
        if let Some(model_id) = model_id {
            self.recyclable_model_ids.remove(&model_id);
        }
        model_id
    }

    fn retire(&mut self, skin_id: String, loaded: LoadedSkin) {
        self.retired_skins.push(RetiredSkin {
            skin_id,
            resources: loaded.resources,
            retired_at: Instant::now(),
        });
    }
}
