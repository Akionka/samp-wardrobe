use crate::config::{SkinConfig, SkinDefinition};
use crate::game_frame::GameFrame;
use crate::gta::InstanceSkinResources;
use std::collections::HashSet;

#[path = "skin_loader/instance_cache.rs"]
mod instance_cache;
#[path = "skin_loader/model_cache.rs"]
mod model_cache;

pub use instance_cache::Lookup as InstanceSkinLookup;

#[derive(Default)]
pub struct SkinManager {
    models: model_cache::ModelCache,
    instances: instance_cache::InstanceCache,
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
        self.models.retire_unreferenced(&referenced_skins);
        self.instances.retire_unreferenced(&referenced_skins);
    }

    pub fn is_private_model(&self, model_id: i16) -> bool {
        self.models.is_private_model(model_id)
    }

    pub fn model_for(
        &mut self,
        frame: &GameFrame,
        skin_id: &str,
        definition: &SkinDefinition,
    ) -> Option<i32> {
        self.models.model_for(frame, skin_id, definition)
    }

    pub fn instance_for(
        &mut self,
        frame: &GameFrame,
        skin_id: &str,
        definition: &SkinDefinition,
    ) -> InstanceSkinLookup {
        self.instances.load_or_status(frame, skin_id, definition)
    }

    pub fn instance_resources(&self, skin_id: &str) -> Option<&InstanceSkinResources> {
        self.instances.resources(skin_id)
    }

    pub fn retain_instance_skins(&mut self, desired_skin_ids: &HashSet<String>) {
        self.instances.retain(desired_skin_ids);
    }

    pub fn cleanup_retired(&mut self, frame: &GameFrame, live_model_ids: Option<HashSet<i16>>) {
        self.models.cleanup_retired(frame, live_model_ids);
    }

    pub fn cleanup_retired_instances(
        &mut self,
        frame: &GameFrame,
        live_skin_ids: Option<HashSet<String>>,
    ) {
        self.instances.cleanup_retired(frame, live_skin_ids);
    }
}
