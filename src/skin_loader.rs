use crate::config::{SkinConfig, SkinDefinition, SkinSourceRevision, skin_source_revision};
use crate::game_frame::GameFrame;
use crate::gta::{self, InstanceSkinResources, SkinResources};
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

#[derive(Debug)]
struct LoadedInstanceSkin {
    resources: InstanceSkinResources,
    source: SkinSourceRevision,
}

#[derive(Debug)]
struct RetiredInstanceSkin {
    skin_id: String,
    resources: InstanceSkinResources,
}

pub enum InstanceSkinLookup {
    Ready,
    ResetRequired,
    Unavailable,
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
    loaded_instances: HashMap<String, LoadedInstanceSkin>,
    retired_instances: Vec<RetiredInstanceSkin>,
    failed_instance_profiles: HashMap<String, SkinSourceRevision>,
    last_instance_asset_check: HashMap<String, Instant>,
}

impl SkinManager {
    pub fn apply_config(&mut self, config: &SkinConfig) {
        let referenced_skins = config
            .rules
            .iter()
            .filter(|rule| rule.enabled)
            .map(|rule| rule.profile_id.as_str())
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

        let unneeded_instances = self
            .loaded_instances
            .keys()
            .filter(|skin_id| !referenced_skins.contains(*skin_id))
            .cloned()
            .collect::<Vec<_>>();
        for skin_id in unneeded_instances {
            self.retire_instance(&skin_id);
        }

        // A corrected asset path or a newly added profile should be allowed to
        // load on the next matching poll.
        self.failed_profiles.clear();
        self.failed_instance_profiles.clear();
    }

    pub fn is_private_model(&self, model_id: i16) -> bool {
        self.private_model_ids.contains(&(model_id as i32))
    }

    pub fn model_for(
        &mut self,
        frame: &GameFrame,
        skin_id: &str,
        definition: &SkinDefinition,
    ) -> Option<i32> {
        let now = Instant::now();
        let loaded_model = self.loaded_models.get(skin_id);
        let checked_recently = self
            .last_asset_check
            .get(skin_id)
            .is_some_and(|last_check| now.duration_since(*last_check) < ASSET_RELOAD_INTERVAL);

        if let Some(loaded) = loaded_model
            && loaded.source.definition == *definition
            && checked_recently
        {
            return Some(loaded.resources.model_id);
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
        if let Some(loaded) = self.loaded_models.get(skin_id)
            && loaded.source == source
        {
            return Some(loaded.resources.model_id);
        }
        if self.failed_profiles.get(skin_id) == Some(&source) {
            return self
                .loaded_models
                .get(skin_id)
                .map(|loaded| loaded.resources.model_id);
        }

        let recycled_model_id = self.take_recyclable_model_id();
        let loaded_skin = gta::load_skin(frame, skin_id, definition, recycled_model_id);
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

    pub fn instance_for(
        &mut self,
        frame: &GameFrame,
        skin_id: &str,
        definition: &SkinDefinition,
    ) -> InstanceSkinLookup {
        if self
            .retired_instances
            .iter()
            .any(|retired| retired.skin_id == skin_id)
        {
            return InstanceSkinLookup::Unavailable;
        }

        let now = Instant::now();
        let checked_recently = self
            .last_instance_asset_check
            .get(skin_id)
            .is_some_and(|last_check| now.duration_since(*last_check) < ASSET_RELOAD_INTERVAL);

        if let Some(loaded) = self.loaded_instances.get(skin_id)
            && loaded.source.definition == *definition
            && checked_recently
        {
            return InstanceSkinLookup::Ready;
        }
        if checked_recently
            && self
                .failed_instance_profiles
                .get(skin_id)
                .is_some_and(|failed| failed.definition == *definition)
        {
            return InstanceSkinLookup::Unavailable;
        }

        let source = skin_source_revision(definition);
        self.last_instance_asset_check
            .insert(skin_id.to_owned(), now);
        if let Some(loaded) = self.loaded_instances.get(skin_id)
            && loaded.source == source
        {
            return InstanceSkinLookup::Ready;
        }

        if self.loaded_instances.contains_key(skin_id) {
            self.retire_instance(skin_id);
            self.failed_instance_profiles.remove(skin_id);
            log::info!(
                "local instance skin {skin_id} changed; queued its old source for cleanup before reload"
            );
            return InstanceSkinLookup::ResetRequired;
        }
        if self.failed_instance_profiles.get(skin_id) == Some(&source) {
            return InstanceSkinLookup::Unavailable;
        }

        match gta::load_instance_skin(frame, skin_id, definition) {
            Some(resources) => {
                self.loaded_instances
                    .insert(skin_id.to_owned(), LoadedInstanceSkin { resources, source });
                self.failed_instance_profiles.remove(skin_id);
                InstanceSkinLookup::Ready
            }
            None => {
                self.failed_instance_profiles
                    .insert(skin_id.to_owned(), source);
                log::error!(
                    "local instance skin {skin_id} is unavailable until its files or profile change"
                );
                InstanceSkinLookup::Unavailable
            }
        }
    }

    pub fn instance_resources(&self, skin_id: &str) -> Option<&InstanceSkinResources> {
        self.loaded_instances
            .get(skin_id)
            .map(|loaded| &loaded.resources)
    }

    pub fn retain_instance_skins(&mut self, desired_skin_ids: &HashSet<String>) {
        let unneeded = self
            .loaded_instances
            .keys()
            .filter(|skin_id| !desired_skin_ids.contains(*skin_id))
            .cloned()
            .collect::<Vec<_>>();
        for skin_id in unneeded {
            self.retire_instance(&skin_id);
        }
    }

    pub fn cleanup_retired(&mut self, frame: &GameFrame, live_model_ids: Option<HashSet<i16>>) {
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
            if gta::release_skin_resources(frame, &retired.skin_id, retired.resources) {
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

    pub fn cleanup_retired_instances(
        &mut self,
        frame: &GameFrame,
        live_skin_ids: Option<HashSet<String>>,
    ) {
        if self.retired_instances.is_empty() {
            return;
        }
        let Some(live_skin_ids) = live_skin_ids else {
            log::debug!("deferred local instance-skin cleanup because ped liveness was incomplete");
            return;
        };

        let pending = std::mem::take(&mut self.retired_instances);
        for retired in pending {
            if live_skin_ids.contains(&retired.skin_id)
                || !gta::release_instance_skin_resources(
                    frame,
                    &retired.skin_id,
                    &retired.resources,
                )
            {
                self.retired_instances.push(retired);
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

    fn retire_instance(&mut self, skin_id: &str) {
        let Some(loaded) = self.loaded_instances.remove(skin_id) else {
            return;
        };
        self.retired_instances.push(RetiredInstanceSkin {
            skin_id: skin_id.to_owned(),
            resources: loaded.resources,
        });
    }
}
