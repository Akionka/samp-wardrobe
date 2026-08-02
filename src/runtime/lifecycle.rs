//! Applied-ped recovery, pruning, and deferred resource retirement.

use super::*;

pub(super) fn local_instance_reset_kind(
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

impl Runtime {
    pub(super) fn restore_server_model(
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

    pub(super) fn prune_streamed_out_players(
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

    pub(super) fn live_ped_state(&self) -> Option<LivePedState> {
        let peds = self.samp.all_peds()?;
        let model_ids = peds.iter().map(gta::ped_model_id).collect();
        let render_objects = peds.iter().map(gta::ped_render_object).collect();
        Some(LivePedState {
            model_ids,
            render_objects,
        })
    }

    pub(super) fn cleanup_retired_skins(
        &mut self,
        frame: &GameFrame,
        live_peds: Option<LivePedState>,
    ) {
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
