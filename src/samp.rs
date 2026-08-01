use crate::memory;
use std::ffi::c_void;
use std::thread;
use std::time::Duration;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;

const SAMP_MAX_PLAYERS: usize = 1004;
const SAMP_PED_GTA_PED: usize = 0x40;
const MSVC_STRING_LENGTH: usize = 0x10;
const MSVC_STRING_CAPACITY: usize = 0x14;
const MSVC_STRING_SSO_CAPACITY: usize = 15;

const DOS_SIGNATURE: u16 = 0x5A4D;
const PE_SIGNATURE: u32 = 0x0000_4550;
const PE_LFANEW_OFFSET: usize = 0x3C;
const PE_ENTRY_POINT_OFFSET: usize = 0x28;
const MAX_PE_HEADER_OFFSET: usize = 0x10_000;

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
struct SampLayout {
    version: SampVersion,
    samp_info_offset: usize,
    net_game_pools_offset: usize,
    pools_player_offset: usize,
    player_pool_largest_id_offset: usize,
    player_pool_remote_players_offset: usize,
    player_pool_local_player_id_offset: usize,
    player_pool_local_name_offset: usize,
    player_pool_local_player_offset: usize,
    player_info_remote_player_offset: usize,
    remote_player_samp_ped_offset: usize,
    remote_player_name_offset: usize,
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

#[derive(Clone, Copy)]
pub struct Samp {
    base: usize,
    layout: SampLayout,
}

pub type PlayerId = u16;

pub struct StreamedPed {
    pub player_id: PlayerId,
    /// `None` when the player name is empty, malformed, or unreadable. The
    /// ped remains usable for server-model-only rules.
    pub name: Option<String>,
    pub address: *mut c_void,
}

impl Samp {
    pub fn wait_for_load() -> Result<Self, String> {
        let base = loop {
            let module = unsafe { GetModuleHandleA(c"samp.dll".as_ptr().cast()) };
            if module != 0 {
                break module as usize;
            }
            thread::sleep(Duration::from_millis(500));
        };

        let entry_point = unsafe { pe_entry_point(base)? };
        let layout = layout_for_entry_point(entry_point).ok_or_else(|| {
            format!(
                "unsupported samp.dll build (PE entry point 0x{entry_point:06X}); supported: 0.3.7-R1, 0.3.7-R3-1, 0.3.7-R4, and 0.3.DL-R1"
            )
        })?;
        Ok(Self { base, layout })
    }

    pub const fn base(&self) -> usize {
        self.base
    }

    pub const fn version(&self) -> SampVersion {
        self.layout.version
    }

    /// Returns every currently streamed SA-MP ped. A missing player name is
    /// represented on its `StreamedPed`; `None` means a required player-pool
    /// or ped entry could not be read, so callers must retain existing player
    /// state rather than treating the scan as empty.
    pub unsafe fn streamed_peds(&self) -> Option<Vec<StreamedPed>> {
        let player_pool = unsafe { self.player_pool()? };
        let mut peds = unsafe { self.streamed_remote_peds(player_pool)? };
        if let Some(local_player) = unsafe { self.streamed_local_ped(player_pool)? } {
            peds.push(local_player);
        }
        Some(peds)
    }

    /// A successful result means all readable SA-MP ped entries have been
    /// enumerated. Callers should postpone destructive cleanup on failure.
    pub unsafe fn all_peds(&self) -> Option<Vec<*mut c_void>> {
        let player_pool = unsafe { self.player_pool()? };
        let max_player_id = unsafe { self.max_player_id(player_pool)? };
        let remote_players =
            player_pool.checked_add(self.layout.player_pool_remote_players_offset)?;
        let remote_player_entries = unsafe { read_player_entries(remote_players, max_player_id)? };
        let mut peds = Vec::new();

        for entry in remote_player_entries.chunks_exact(std::mem::size_of::<u32>()) {
            let remote =
                u32::from_ne_bytes(entry.try_into().expect("remote player entry has 4 bytes"))
                    as usize;
            if remote == 0 {
                continue;
            }
            if let Some(ped) = unsafe { self.remote_gta_ped(remote)? } {
                peds.push(ped);
            }
        }

        if let Some(ped) = unsafe { self.local_gta_ped(player_pool)? } {
            peds.push(ped);
        }

        Some(peds)
    }

    unsafe fn player_pool(&self) -> Option<usize> {
        let samp_address = self.base.checked_add(self.layout.samp_info_offset)?;
        let samp: usize = unsafe { memory::read(samp_address)? };
        if samp == 0 {
            return None;
        }

        let pools_address = samp.checked_add(self.layout.net_game_pools_offset)?;
        let pools: usize = unsafe { memory::read(pools_address)? };
        if pools == 0 {
            return None;
        }

        let player_pool_address = pools.checked_add(self.layout.pools_player_offset)?;
        let player_pool: usize = unsafe { memory::read(player_pool_address)? };
        (player_pool != 0).then_some(player_pool)
    }

    unsafe fn streamed_local_ped(&self, player_pool: usize) -> Option<Option<StreamedPed>> {
        let local_player_address =
            player_pool.checked_add(self.layout.player_pool_local_player_offset)?;
        let local_player: usize = unsafe { memory::read(local_player_address)? };
        if local_player == 0 {
            return Some(None);
        }

        let player_id_address =
            player_pool.checked_add(self.layout.player_pool_local_player_id_offset)?;
        let player_id: PlayerId = unsafe { memory::read(player_id_address)? };
        if usize::from(player_id) >= SAMP_MAX_PLAYERS {
            return None;
        }

        let Some(address) = (unsafe { self.gta_ped_from_local_player(local_player)? }) else {
            return Some(None);
        };
        let name =
            unsafe { read_msvc_string(player_pool, self.layout.player_pool_local_name_offset) };
        Some(Some(StreamedPed {
            player_id,
            name,
            address,
        }))
    }

    unsafe fn streamed_remote_peds(&self, player_pool: usize) -> Option<Vec<StreamedPed>> {
        let max_player_id = unsafe { self.max_player_id(player_pool)? };
        let remote_players =
            player_pool.checked_add(self.layout.player_pool_remote_players_offset)?;
        let remote_player_entries = unsafe { read_player_entries(remote_players, max_player_id)? };
        let mut matches = Vec::new();

        for (player_id, entry) in remote_player_entries
            .chunks_exact(std::mem::size_of::<u32>())
            .enumerate()
        {
            let remote =
                u32::from_ne_bytes(entry.try_into().expect("remote player entry has 4 bytes"))
                    as usize;
            if remote == 0 {
                continue;
            }

            let Some(address) = (unsafe { self.remote_gta_ped(remote)? }) else {
                continue;
            };
            let name = unsafe { self.remote_player_name(remote) };
            matches.push(StreamedPed {
                player_id: player_id as PlayerId,
                name,
                address,
            });
        }

        Some(matches)
    }

    unsafe fn max_player_id(&self, player_pool: usize) -> Option<usize> {
        let max_player_id_address =
            player_pool.checked_add(self.layout.player_pool_largest_id_offset)?;
        let max_player_id: u32 = unsafe { memory::read(max_player_id_address)? };
        let max_player_id = usize::try_from(max_player_id).ok()?;
        (max_player_id < SAMP_MAX_PLAYERS).then_some(max_player_id)
    }

    /// `Some(None)` means the player is valid but currently has no streamed
    /// GTA ped. `None` means a required pointer could not be read.
    unsafe fn remote_gta_ped(&self, remote: usize) -> Option<Option<*mut c_void>> {
        let remote_player_address =
            remote.checked_add(self.layout.player_info_remote_player_offset)?;
        let remote_player: usize = unsafe { memory::read(remote_player_address)? };
        if remote_player == 0 {
            return Some(None);
        }

        let samp_ped_address =
            remote_player.checked_add(self.layout.remote_player_samp_ped_offset)?;
        let samp_ped: usize = unsafe { memory::read(samp_ped_address)? };
        unsafe { gta_ped_from_samp_ped(samp_ped) }
    }

    unsafe fn local_gta_ped(&self, player_pool: usize) -> Option<Option<*mut c_void>> {
        let local_player_address =
            player_pool.checked_add(self.layout.player_pool_local_player_offset)?;
        let local_player: usize = unsafe { memory::read(local_player_address)? };
        unsafe { self.gta_ped_from_local_player(local_player) }
    }

    unsafe fn gta_ped_from_local_player(&self, local_player: usize) -> Option<Option<*mut c_void>> {
        if local_player == 0 {
            return Some(None);
        }
        let samp_ped: usize = unsafe { memory::read(local_player)? };
        unsafe { gta_ped_from_samp_ped(samp_ped) }
    }

    unsafe fn remote_player_name(&self, remote: usize) -> Option<String> {
        unsafe { read_msvc_string(remote, self.layout.remote_player_name_offset) }
    }
}

fn layout_for_entry_point(entry_point: u32) -> Option<SampLayout> {
    match entry_point {
        R1_ENTRY_POINT => Some(R1_LAYOUT),
        R3_ENTRY_POINT => Some(R3_LAYOUT),
        R4_ENTRY_POINT => Some(R4_LAYOUT),
        DL_R1_ENTRY_POINT => Some(DL_R1_LAYOUT),
        _ => None,
    }
}

unsafe fn pe_entry_point(base: usize) -> Result<u32, String> {
    let signature: u16 =
        unsafe { memory::read(base) }.ok_or("could not read samp.dll DOS header")?;
    if signature != DOS_SIGNATURE {
        return Err("samp.dll has an invalid DOS header".to_owned());
    }

    let lfanew_address = base
        .checked_add(PE_LFANEW_OFFSET)
        .ok_or("samp.dll DOS-header address overflowed")?;
    let lfanew: u32 = unsafe { memory::read(lfanew_address) }
        .ok_or("could not read samp.dll PE header offset")?;
    let lfanew = usize::try_from(lfanew).map_err(|_| "samp.dll PE header offset is invalid")?;
    if lfanew > MAX_PE_HEADER_OFFSET {
        return Err(format!(
            "samp.dll PE header offset 0x{lfanew:X} is implausible"
        ));
    }

    let pe_header = base
        .checked_add(lfanew)
        .ok_or("samp.dll PE header address overflowed")?;
    let pe_signature: u32 =
        unsafe { memory::read(pe_header) }.ok_or("could not read samp.dll PE signature")?;
    if pe_signature != PE_SIGNATURE {
        return Err("samp.dll has an invalid PE signature".to_owned());
    }

    let entry_point_address = pe_header
        .checked_add(PE_ENTRY_POINT_OFFSET)
        .ok_or("samp.dll PE entry-point address overflowed")?;
    unsafe { memory::read(entry_point_address) }
        .ok_or_else(|| "could not read samp.dll PE entry point".to_owned())
}

unsafe fn read_player_entries(remote_players: usize, max_player_id: usize) -> Option<Vec<u8>> {
    let remote_players_size = (max_player_id + 1).checked_mul(std::mem::size_of::<u32>())?;
    memory::read_bytes(remote_players, remote_players_size)
}

unsafe fn gta_ped_from_samp_ped(samp_ped: usize) -> Option<Option<*mut c_void>> {
    if samp_ped == 0 {
        return Some(None);
    }

    let gta_ped_address = samp_ped.checked_add(SAMP_PED_GTA_PED)?;
    let gta_ped: *mut c_void = unsafe { memory::read(gta_ped_address)? };
    Some((!gta_ped.is_null()).then_some(gta_ped))
}

/// Reads an MSVC x86 `std::string` without treating its object storage as a
/// C string. Both player-pool and remote-player entries use this representation.
unsafe fn read_msvc_string(object: usize, string_offset: usize) -> Option<String> {
    let name_address = object.checked_add(string_offset)?;
    let length_address = name_address.checked_add(MSVC_STRING_LENGTH)?;
    let capacity_address = name_address.checked_add(MSVC_STRING_CAPACITY)?;
    let length: usize = unsafe { memory::read(length_address)? };
    let capacity: usize = unsafe { memory::read(capacity_address)? };
    if length == 0 || length > 24 || capacity < length {
        return None;
    }

    let text_address: usize = if capacity <= MSVC_STRING_SSO_CAPACITY {
        name_address
    } else {
        unsafe { memory::read(name_address)? }
    };
    let bytes = memory::read_bytes(text_address, length)?;
    std::str::from_utf8(&bytes).ok().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::{
        DL_R1_ENTRY_POINT, DL_R1_LAYOUT, R1_ENTRY_POINT, R1_LAYOUT, R3_ENTRY_POINT, R3_LAYOUT,
        R4_ENTRY_POINT, R4_LAYOUT, SampVersion, layout_for_entry_point,
    };

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
