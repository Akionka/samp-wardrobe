//! Raw local-player instance-source cache.
//!
//! Unlike the remote cache, this owns no GTA model IDs. Sources retire only
//! after runtime confirms no local ped is still using a clone of the source.

use crate::config::{SkinDefinition, SkinSourceRevision, skin_source_revision};
use crate::game_frame::GameFrame;
use crate::gta::{self, InstanceSkinResources};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

const ASSET_RELOAD_INTERVAL: Duration = Duration::from_secs(1);

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

pub enum Lookup {
    Ready,
    ResetRequired,
    Unavailable,
}

#[derive(Default)]
pub(super) struct InstanceCache {
    loaded: HashMap<String, LoadedInstanceSkin>,
    retired: Vec<RetiredInstanceSkin>,
    failed_profiles: HashMap<String, SkinSourceRevision>,
    last_asset_check: HashMap<String, Instant>,
}

impl InstanceCache {
    pub(super) fn retire_unreferenced(&mut self, referenced_skin_ids: &HashSet<String>) {
        let unneeded = self
            .loaded
            .keys()
            .filter(|skin_id| !referenced_skin_ids.contains(*skin_id))
            .cloned()
            .collect::<Vec<_>>();
        for skin_id in unneeded {
            self.retire(&skin_id);
        }

        // A corrected asset path or a newly added profile should be allowed to
        // load on the next matching poll.
        self.failed_profiles.clear();
    }

    pub(super) fn load_or_status(
        &mut self,
        frame: &GameFrame,
        skin_id: &str,
        definition: &SkinDefinition,
    ) -> Lookup {
        if self
            .retired
            .iter()
            .any(|retired| retired.skin_id == skin_id)
        {
            return Lookup::Unavailable;
        }

        let now = Instant::now();
        let checked_recently = self
            .last_asset_check
            .get(skin_id)
            .is_some_and(|last_check| now.duration_since(*last_check) < ASSET_RELOAD_INTERVAL);

        if let Some(loaded) = self.loaded.get(skin_id)
            && loaded.source.definition == *definition
            && checked_recently
        {
            return Lookup::Ready;
        }
        if checked_recently
            && self
                .failed_profiles
                .get(skin_id)
                .is_some_and(|failed| failed.definition == *definition)
        {
            return Lookup::Unavailable;
        }

        let source = skin_source_revision(definition);
        self.last_asset_check.insert(skin_id.to_owned(), now);
        if let Some(loaded) = self.loaded.get(skin_id)
            && loaded.source == source
        {
            return Lookup::Ready;
        }

        if self.loaded.contains_key(skin_id) {
            self.retire(skin_id);
            self.failed_profiles.remove(skin_id);
            log::info!(
                "local instance skin {skin_id} changed; queued its old source for cleanup before reload"
            );
            return Lookup::ResetRequired;
        }
        if self.failed_profiles.get(skin_id) == Some(&source) {
            return Lookup::Unavailable;
        }

        match gta::load_instance_skin(frame, skin_id, definition) {
            Some(resources) => {
                self.loaded
                    .insert(skin_id.to_owned(), LoadedInstanceSkin { resources, source });
                self.failed_profiles.remove(skin_id);
                Lookup::Ready
            }
            None => {
                self.failed_profiles.insert(skin_id.to_owned(), source);
                log::error!(
                    "local instance skin {skin_id} is unavailable until its files or profile change"
                );
                Lookup::Unavailable
            }
        }
    }

    pub(super) fn resources(&self, skin_id: &str) -> Option<&InstanceSkinResources> {
        self.loaded.get(skin_id).map(|loaded| &loaded.resources)
    }

    pub(super) fn retain(&mut self, desired_skin_ids: &HashSet<String>) {
        self.retire_unreferenced(desired_skin_ids);
    }

    pub(super) fn cleanup_retired(
        &mut self,
        frame: &GameFrame,
        live_skin_ids: Option<HashSet<String>>,
    ) {
        if self.retired.is_empty() {
            return;
        }
        let Some(live_skin_ids) = live_skin_ids else {
            log::debug!("deferred local instance-skin cleanup because ped liveness was incomplete");
            return;
        };

        let pending = std::mem::take(&mut self.retired);
        for retired in pending {
            if live_skin_ids.contains(&retired.skin_id)
                || !gta::release_instance_skin_resources(
                    frame,
                    &retired.skin_id,
                    &retired.resources,
                )
            {
                self.retired.push(retired);
            }
        }
    }

    fn retire(&mut self, skin_id: &str) {
        let Some(loaded) = self.loaded.remove(skin_id) else {
            return;
        };
        self.retired.push(RetiredInstanceSkin {
            skin_id: skin_id.to_owned(),
            resources: loaded.resources,
        });
    }
}
