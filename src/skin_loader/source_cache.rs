//! Shared prepared skin-source cache.
//!
//! One source is loaded per profile and cloned into every matching ped. A
//! retired source owns the identities of its installed clones until a complete
//! scan proves none of them is still live.

use crate::config::{SkinDefinition, SkinSourceRevision, skin_source_revision};
use crate::game_frame::GameFrame;
use crate::gta::{self, PedRenderObject, SkinSourceResources};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

const ASSET_RELOAD_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug)]
struct LoadedSkinSource {
    resources: SkinSourceResources,
    source: SkinSourceRevision,
    generation: u64,
    installed_clones: HashSet<PedRenderObject>,
}

#[derive(Debug)]
struct RetiredSkinSource {
    skin_id: String,
    resources: SkinSourceResources,
    installed_clones: HashSet<PedRenderObject>,
}

pub enum Lookup {
    Ready { generation: u64 },
    RestoreRequired,
    Unavailable,
}

#[derive(Default)]
pub(super) struct SourceCache {
    loaded: HashMap<String, LoadedSkinSource>,
    retired: Vec<RetiredSkinSource>,
    failed_profiles: HashMap<String, SkinSourceRevision>,
    last_asset_check: HashMap<String, Instant>,
    next_generation: u64,
}

impl SourceCache {
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

        // Failed revisions stay remembered even when the source is retained;
        // `skin_source_revision` makes a corrected profile or asset eligible
        // without retrying an unchanged broken source every poll.
    }

    pub(super) fn load_or_status(
        &mut self,
        frame: &GameFrame,
        skin_id: &str,
        definition: &SkinDefinition,
    ) -> Lookup {
        // A replacement cannot be loaded while any clone may still borrow the
        // old source geometry and textures.
        if self
            .retired
            .iter()
            .any(|retired| retired.skin_id == skin_id)
        {
            // A prior restore may have failed after this source was retired.
            // Ask Runtime to retry every streamed user before permitting a
            // replacement load.
            return Lookup::RestoreRequired;
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
            return Lookup::Ready {
                generation: loaded.generation,
            };
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
            return Lookup::Ready {
                generation: loaded.generation,
            };
        }

        if self.loaded.contains_key(skin_id) {
            self.retire(skin_id);
            self.failed_profiles.remove(skin_id);
            log::info!(
                "skin source {skin_id} changed; queued its old clones for restoration before reload"
            );
            return Lookup::RestoreRequired;
        }
        if self.failed_profiles.get(skin_id) == Some(&source) {
            return Lookup::Unavailable;
        }

        match gta::load_skin_source(frame, skin_id, definition) {
            Some(resources) => {
                let generation = self.allocate_generation();
                self.loaded.insert(
                    skin_id.to_owned(),
                    LoadedSkinSource {
                        resources,
                        source,
                        generation,
                        installed_clones: HashSet::new(),
                    },
                );
                self.failed_profiles.remove(skin_id);
                Lookup::Ready { generation }
            }
            None => {
                self.failed_profiles.insert(skin_id.to_owned(), source);
                log::error!(
                    "skin source {skin_id} is unavailable until its files or profile change"
                );
                Lookup::Unavailable
            }
        }
    }

    pub(super) fn resources(&self, skin_id: &str) -> Option<&SkinSourceResources> {
        self.loaded.get(skin_id).map(|loaded| &loaded.resources)
    }

    pub(super) fn record_clone(&mut self, skin_id: &str, clone: PedRenderObject) {
        if let Some(loaded) = self.loaded.get_mut(skin_id) {
            loaded.installed_clones.insert(clone);
        }
    }

    pub(super) fn cleanup_retired(
        &mut self,
        frame: &GameFrame,
        live_render_objects: Option<&HashSet<PedRenderObject>>,
    ) {
        let Some(live_render_objects) = live_render_objects else {
            if !self.retired.is_empty() {
                log::debug!(
                    "deferred retired skin-source cleanup because ped liveness was incomplete"
                );
            }
            return;
        };

        for loaded in self.loaded.values_mut() {
            loaded
                .installed_clones
                .retain(|clone| live_render_objects.contains(clone));
        }

        if self.retired.is_empty() {
            return;
        }

        let pending = std::mem::take(&mut self.retired);
        for retired in pending {
            if retired
                .installed_clones
                .iter()
                .any(|clone| live_render_objects.contains(clone))
                || !gta::release_skin_source_resources(frame, &retired.skin_id, &retired.resources)
            {
                self.retired.push(retired);
            }
        }
    }

    fn allocate_generation(&mut self) -> u64 {
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        self.next_generation
    }

    fn retire(&mut self, skin_id: &str) {
        let Some(loaded) = self.loaded.remove(skin_id) else {
            return;
        };
        self.retired.push(RetiredSkinSource {
            skin_id: skin_id.to_owned(),
            resources: loaded.resources,
            installed_clones: loaded.installed_clones,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::SourceCache;
    use crate::config::{FileRevision, SkinDefinition, SkinSourceRevision};
    use std::collections::HashSet;

    #[test]
    fn source_generations_do_not_reuse_zero_after_wrapping() {
        let mut cache = SourceCache {
            next_generation: u64::MAX,
            ..SourceCache::default()
        };

        assert_eq!(cache.allocate_generation(), 1);
    }

    #[test]
    fn retaining_a_profile_does_not_retry_an_unchanged_failed_source() {
        let definition = SkinDefinition {
            enabled: true,
            txd_path: "skin.txd".to_owned(),
            dff_path: "skin.dff".to_owned(),
        };
        let revision = SkinSourceRevision {
            definition,
            txd: FileRevision::Missing,
            dff: FileRevision::Missing,
        };
        let mut cache = SourceCache::default();
        cache
            .failed_profiles
            .insert("staff".to_owned(), revision.clone());

        cache.retire_unreferenced(&HashSet::from(["staff".to_owned()]));

        assert_eq!(cache.failed_profiles.get("staff"), Some(&revision));
    }
}
