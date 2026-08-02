use crate::config::{SkinConfig, SkinDefinition};
use crate::game_frame::GameFrame;
use crate::gta::{PedRenderObject, SkinSourceResources};
use std::collections::HashSet;

#[path = "skin_loader/source_cache.rs"]
mod source_cache;

pub use source_cache::Lookup as SkinSourceLookup;

#[derive(Default)]
pub struct SkinManager {
    sources: source_cache::SourceCache,
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
        self.sources.retire_unreferenced(&referenced_skins);
    }

    pub fn source_for(
        &mut self,
        frame: &GameFrame,
        skin_id: &str,
        definition: &SkinDefinition,
    ) -> SkinSourceLookup {
        self.sources.load_or_status(frame, skin_id, definition)
    }

    pub fn source_resources(&self, skin_id: &str) -> Option<&SkinSourceResources> {
        self.sources.resources(skin_id)
    }

    pub fn record_clone(&mut self, skin_id: &str, clone: PedRenderObject) {
        self.sources.record_clone(skin_id, clone);
    }

    pub fn retain_sources(&mut self, desired_skin_ids: &HashSet<String>) {
        self.sources.retire_unreferenced(desired_skin_ids);
    }

    pub fn cleanup_retired_sources(
        &mut self,
        frame: &GameFrame,
        live_render_objects: Option<&HashSet<PedRenderObject>>,
    ) {
        self.sources.cleanup_retired(frame, live_render_objects);
    }
}
