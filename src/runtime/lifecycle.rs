//! Applied-ped recovery, pruning, and deferred skin-source retirement.

use super::*;

pub(super) fn skin_clone_reset_kind(
    render_object_changed: bool,
    render_object_address_reused: bool,
    remembered_model_id: i16,
    current_model_id: i16,
) -> Option<&'static str> {
    if render_object_changed && render_object_address_reused {
        Some("render-object address was reused with different geometry")
    } else if render_object_changed {
        Some("render-object pointer changed")
    } else if remembered_model_id != current_model_id {
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
        let Some(applied) = self.applied_players.get(&player_id).cloned() else {
            return;
        };
        let Some(current_model_id) = gta::ped_model_id(ped) else {
            return;
        };
        let Some(current_render_object) = gta::ped_render_object(ped) else {
            return;
        };

        if current_model_id != applied.server_model_id
            || current_render_object != applied.render_object
        {
            // SA-MP has already built a newer normal representation. Never
            // overwrite it with our remembered server state.
            self.applied_players.remove(&player_id);
            self.failed_applications.remove(&player_id);
            self.matched_players.remove(&player_id);
            return;
        }

        if let Err(restore_reason) =
            gta::restore_skin_source(frame, ped, applied.server_model_id, applied.render_object)
        {
            log::error!(
                "could not rebuild the normal clump for player {player_id} after {reason}: {restore_reason}; retaining skin-clone state"
            );
            return;
        }
        log::info!(
            "rebuilt server model {} for player {player_id} ({}) after {reason} for skin {}",
            applied.server_model_id,
            name.unwrap_or("unavailable name"),
            applied.skin_id
        );

        self.applied_players.remove(&player_id);
        self.failed_applications.remove(&player_id);
        self.matched_players.remove(&player_id);
    }

    pub(super) fn prune_streamed_out_players(&mut self, streamed_player_ids: &HashSet<PlayerId>) {
        let applied_before = self.applied_players.len();
        let matched_before = self.matched_players.len();
        let failed_before = self.failed_applications.len();
        self.applied_players
            .retain(|player_id, _| streamed_player_ids.contains(player_id));
        self.matched_players
            .retain(|player_id| streamed_player_ids.contains(player_id));
        self.failed_applications
            .retain(|player_id, _| streamed_player_ids.contains(player_id));

        let pruned_applied = applied_before - self.applied_players.len();
        let pruned_matched = matched_before - self.matched_players.len();
        let pruned_failed = failed_before - self.failed_applications.len();
        if pruned_applied != 0 || pruned_matched != 0 || pruned_failed != 0 {
            log::debug!(
                "pruned {pruned_applied} applied, {pruned_matched} matched, and {pruned_failed} failed player state entries after a complete SA-MP ped scan"
            );
        }
    }

    pub(super) fn live_ped_state(&self) -> Option<LivePedState> {
        let peds = self.samp.all_peds()?;
        let render_objects = peds.iter().map(gta::ped_render_object).collect();
        Some(LivePedState { render_objects })
    }

    pub(super) fn cleanup_retired_sources(
        &mut self,
        frame: &GameFrame,
        live_peds: Option<LivePedState>,
    ) {
        self.skins.cleanup_retired_sources(
            frame,
            live_peds
                .as_ref()
                .and_then(|state| state.render_objects.as_ref()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::skin_clone_reset_kind;

    #[test]
    fn detects_a_reset_when_only_the_server_model_changes() {
        assert_eq!(
            skin_clone_reset_kind(false, false, 7, 294),
            Some("model ID changed while GTA reused the render-object address")
        );
    }

    #[test]
    fn keeps_an_unchanged_skin_clone_applied() {
        assert_eq!(skin_clone_reset_kind(false, false, 7, 7), None);
    }

    #[test]
    fn a_changed_render_object_is_always_a_reset() {
        assert_eq!(
            skin_clone_reset_kind(true, false, 7, 7),
            Some("render-object pointer changed")
        );
    }

    #[test]
    fn detects_allocator_reuse_from_a_different_geometry() {
        assert_eq!(
            skin_clone_reset_kind(true, true, 7, 7),
            Some("render-object address was reused with different geometry")
        );
    }
}
