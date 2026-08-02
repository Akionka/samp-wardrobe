//! Version fingerprints and packed player-pool layouts for supported SA-MP DLLs.

// These entry points identify the known official 32-bit samp.dll revisions.
// They are preferred over string/byte searches because the executable loader
// can relocate a DLL while its PE header still describes the exact build.
const R1_ENTRY_POINT: u32 = 0x31DF13;
const R3_ENTRY_POINT: u32 = 0x0CC4D0;
const R4_ENTRY_POINT: u32 = 0x0CBCB0;
const DL_R1_ENTRY_POINT: u32 = 0x0FDB60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampVersion {
    V037R1,
    V037R3,
    V037R4,
    V03DlR1,
}

impl SampVersion {
    pub const fn name(self) -> &'static str {
        match self {
            Self::V037R1 => "SA-MP 0.3.7-R1",
            Self::V037R3 => "SA-MP 0.3.7-R3-1",
            Self::V037R4 => "SA-MP 0.3.7-R4",
            Self::V03DlR1 => "SA-MP 0.3.DL-R1",
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct SampLayout {
    pub(super) version: SampVersion,
    pub(super) samp_info_offset: usize,
    pub(super) net_game_pools_offset: usize,
    pub(super) pools_player_offset: usize,
    pub(super) player_pool_largest_id_offset: usize,
    pub(super) player_pool_remote_players_offset: usize,
    pub(super) player_pool_local_player_id_offset: usize,
    pub(super) player_pool_local_name_offset: usize,
    pub(super) player_pool_local_player_offset: usize,
    pub(super) player_info_remote_player_offset: usize,
    pub(super) remote_player_samp_ped_offset: usize,
    pub(super) remote_player_name_offset: usize,
}

// SA-MP's structures are packed. Keep each layout here rather than trying to
// share a C representation: R1, R3/R4, and DL order their player-pool fields
// differently. R4 shares R3's player-pool layout; its separate PE entry point
// still prevents an R3 DLL from being mistaken for R4 or vice versa.
const R1_LAYOUT: SampLayout = SampLayout {
    version: SampVersion::V037R1,
    samp_info_offset: 0x21A0F8,
    net_game_pools_offset: 0x3CD,
    pools_player_offset: 0x18,
    player_pool_largest_id_offset: 0x00,
    player_pool_remote_players_offset: 0x2E,
    player_pool_local_player_id_offset: 0x04,
    player_pool_local_name_offset: 0x0A,
    player_pool_local_player_offset: 0x22,
    player_info_remote_player_offset: 0x00,
    remote_player_samp_ped_offset: 0x00,
    remote_player_name_offset: 0x0C,
};

const R3_LAYOUT: SampLayout = SampLayout {
    version: SampVersion::V037R3,
    samp_info_offset: 0x26E8DC,
    net_game_pools_offset: 0x3DE,
    pools_player_offset: 0x08,
    player_pool_largest_id_offset: 0x00,
    player_pool_remote_players_offset: 0x04,
    player_pool_local_player_id_offset: 0x2F1C,
    player_pool_local_name_offset: 0x2F22,
    player_pool_local_player_offset: 0x2F3A,
    player_info_remote_player_offset: 0x00,
    remote_player_samp_ped_offset: 0x00,
    remote_player_name_offset: 0x0C,
};

const R4_LAYOUT: SampLayout = SampLayout {
    version: SampVersion::V037R4,
    samp_info_offset: 0x26EA0C,
    ..R3_LAYOUT
};

const DL_R1_LAYOUT: SampLayout = SampLayout {
    version: SampVersion::V03DlR1,
    samp_info_offset: 0x2ACA24,
    net_game_pools_offset: 0x3DE,
    pools_player_offset: 0x08,
    player_pool_largest_id_offset: 0x22,
    player_pool_remote_players_offset: 0x26,
    player_pool_local_player_id_offset: 0x00,
    player_pool_local_name_offset: 0x06,
    player_pool_local_player_offset: 0x1E,
    player_info_remote_player_offset: 0x08,
    remote_player_samp_ped_offset: 0x04,
    remote_player_name_offset: 0x14,
};

pub(super) fn layout_for_entry_point(entry_point: u32) -> Option<SampLayout> {
    match entry_point {
        R1_ENTRY_POINT => Some(R1_LAYOUT),
        R3_ENTRY_POINT => Some(R3_LAYOUT),
        R4_ENTRY_POINT => Some(R4_LAYOUT),
        DL_R1_ENTRY_POINT => Some(DL_R1_LAYOUT),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_each_supported_samp_build_from_its_entry_point() {
        assert_eq!(
            layout_for_entry_point(R1_ENTRY_POINT).unwrap().version,
            SampVersion::V037R1
        );
        assert_eq!(
            layout_for_entry_point(R3_ENTRY_POINT).unwrap().version,
            SampVersion::V037R3
        );
        assert_eq!(
            layout_for_entry_point(R4_ENTRY_POINT).unwrap().version,
            SampVersion::V037R4
        );
        assert_eq!(
            layout_for_entry_point(DL_R1_ENTRY_POINT).unwrap().version,
            SampVersion::V03DlR1
        );
        assert!(layout_for_entry_point(0xDEAD_BEEF).is_none());
    }

    #[test]
    fn keeps_r1_r3_and_dl_player_pools_separate() {
        assert_eq!(R1_LAYOUT.player_pool_remote_players_offset, 0x2E);
        assert_eq!(R3_LAYOUT.player_pool_remote_players_offset, 0x04);
        assert_eq!(DL_R1_LAYOUT.player_pool_remote_players_offset, 0x26);
        assert_eq!(R3_LAYOUT.player_pool_local_player_offset, 0x2F3A);
        assert_eq!(DL_R1_LAYOUT.player_pool_local_player_offset, 0x1E);
    }

    #[test]
    fn r4_reuses_the_r3_player_pool_layout_with_its_own_global() {
        assert_eq!(
            R4_LAYOUT.net_game_pools_offset,
            R3_LAYOUT.net_game_pools_offset
        );
        assert_eq!(R4_LAYOUT.pools_player_offset, R3_LAYOUT.pools_player_offset);
        assert_eq!(
            R4_LAYOUT.player_pool_remote_players_offset,
            R3_LAYOUT.player_pool_remote_players_offset
        );
        assert_ne!(R4_LAYOUT.samp_info_offset, R3_LAYOUT.samp_info_offset);
    }
}
