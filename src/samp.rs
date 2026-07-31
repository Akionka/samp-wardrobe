use crate::memory;
use std::ffi::c_void;
use std::thread;
use std::time::Duration;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;

// SA-MP 0.3.7-R1 offsets.
const SAMP_OFFSET_SAMP_INFO: usize = 0x21A0F8;
const SAMP_OFFSET_PLAYERS_POOL: usize = 0x3CD;
const SAMP_POOLS_PLAYER: usize = 0x18;
const PLAYER_POOL_REMOTE_PLAYERS: usize = 0x2E;
const PLAYER_POOL_LOCAL_PLAYER_ID: usize = 0x04;
const PLAYER_POOL_LOCAL_NAME: usize = 0x0A;
const PLAYER_POOL_LOCAL_PLAYER: usize = 0x22;
const REMOTE_PLAYER_DATA: usize = 0x00;
const REMOTE_DATA_SAMP_PED: usize = 0x00;
const SAMP_PED_GTA_PED: usize = 0x40;
const SAMP_MAX_PLAYERS: usize = 1004;
const REMOTE_PLAYER_NAME: usize = 0x0C;
const REMOTE_PLAYER_NAME_LENGTH: usize = 0x1C;
const REMOTE_PLAYER_NAME_CAPACITY: usize = 0x20;
const MSVC_STRING_SSO_CAPACITY: usize = 15;

pub struct Samp {
    base: usize,
}

pub type PlayerId = u16;

pub struct StreamedPed {
    pub player_id: PlayerId,
    pub name: String,
    pub address: *mut c_void,
}

impl Samp {
    pub fn wait_for_load() -> Self {
        let base = loop {
            let module = unsafe { GetModuleHandleA(b"samp.dll\0".as_ptr()) };
            if module != 0 {
                break module as usize;
            }
            thread::sleep(Duration::from_millis(500));
        };
        Self { base }
    }

    pub fn base(&self) -> usize {
        self.base
    }

    /// Returns every currently streamed SA-MP ped. `None` means a required
    /// player-pool entry could not be read, so callers must retain existing
    /// player state rather than treating the scan as empty.
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
        let max_player_id = unsafe { max_player_id(player_pool)? };
        let remote_players = player_pool.checked_add(PLAYER_POOL_REMOTE_PLAYERS)?;
        let remote_players_size = (max_player_id + 1).checked_mul(std::mem::size_of::<u32>())?;
        let remote_player_entries = memory::read_bytes(remote_players, remote_players_size)?;
        let mut peds = Vec::new();

        for entry in remote_player_entries.chunks_exact(std::mem::size_of::<u32>()) {
            let remote =
                u32::from_ne_bytes(entry.try_into().expect("remote player entry has 4 bytes"))
                    as usize;
            if remote == 0 {
                continue;
            }
            if let Some(ped) = unsafe { remote_gta_ped(remote)? } {
                peds.push(ped);
            }
        }

        let local_player_address = player_pool.checked_add(PLAYER_POOL_LOCAL_PLAYER)?;
        let local_player: usize = unsafe { memory::read(local_player_address)? };
        if local_player != 0 {
            let samp_ped: usize = unsafe { memory::read(local_player)? };
            if samp_ped != 0 {
                let gta_ped_address = samp_ped.checked_add(SAMP_PED_GTA_PED)?;
                let gta_ped: *mut c_void = unsafe { memory::read(gta_ped_address)? };
                if !gta_ped.is_null() {
                    peds.push(gta_ped);
                }
            }
        }

        Some(peds)
    }

    unsafe fn player_pool(&self) -> Option<usize> {
        let samp_address = self.base.checked_add(SAMP_OFFSET_SAMP_INFO)?;
        let samp: usize = unsafe { memory::read(samp_address)? };
        if samp == 0 {
            return None;
        }

        let pools_address = samp.checked_add(SAMP_OFFSET_PLAYERS_POOL)?;
        let pools: usize = unsafe { memory::read(pools_address)? };
        if pools == 0 {
            return None;
        }

        let player_pool_address = pools.checked_add(SAMP_POOLS_PLAYER)?;
        let player_pool: usize = unsafe { memory::read(player_pool_address)? };
        (player_pool != 0).then_some(player_pool)
    }

    unsafe fn streamed_local_ped(&self, player_pool: usize) -> Option<Option<StreamedPed>> {
        let local_player_address = player_pool.checked_add(PLAYER_POOL_LOCAL_PLAYER)?;
        let local_player: usize = unsafe { memory::read(local_player_address)? };
        if local_player == 0 {
            return Some(None);
        }

        let player_id_address = player_pool.checked_add(PLAYER_POOL_LOCAL_PLAYER_ID)?;
        let player_id: PlayerId = unsafe { memory::read(player_id_address)? };
        if usize::from(player_id) >= SAMP_MAX_PLAYERS {
            return None;
        }

        let samp_ped: usize = unsafe { memory::read(local_player)? };
        if samp_ped == 0 {
            return Some(None);
        }

        let gta_ped_address = samp_ped.checked_add(SAMP_PED_GTA_PED)?;
        let address: *mut c_void = unsafe { memory::read(gta_ped_address)? };
        if address.is_null() {
            return Some(None);
        }

        let name = unsafe { read_msvc_string(player_pool, PLAYER_POOL_LOCAL_NAME)? };
        Some(Some(StreamedPed {
            player_id,
            name,
            address,
        }))
    }

    unsafe fn streamed_remote_peds(&self, player_pool: usize) -> Option<Vec<StreamedPed>> {
        let max_player_id = unsafe { max_player_id(player_pool)? };
        let mut matches = Vec::new();
        let remote_players = player_pool.checked_add(PLAYER_POOL_REMOTE_PLAYERS)?;
        let remote_players_size = (max_player_id + 1).checked_mul(std::mem::size_of::<u32>())?;
        let remote_player_entries = memory::read_bytes(remote_players, remote_players_size)?;

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

            let Some(address) = (unsafe { remote_gta_ped(remote)? }) else {
                continue;
            };
            let name = unsafe { remote_player_name(remote)? };
            matches.push(StreamedPed {
                player_id: player_id as PlayerId,
                name,
                address,
            });
        }

        Some(matches)
    }
}

unsafe fn max_player_id(player_pool: usize) -> Option<usize> {
    let max_player_id: u32 = unsafe { memory::read(player_pool)? };
    let max_player_id = usize::try_from(max_player_id).ok()?;
    (max_player_id < SAMP_MAX_PLAYERS).then_some(max_player_id)
}

/// `Some(None)` means the remote player is valid but currently has no streamed
/// GTA ped. `None` means a required pointer could not be read.
unsafe fn remote_gta_ped(remote: usize) -> Option<Option<*mut c_void>> {
    let remote_data_address = remote.checked_add(REMOTE_PLAYER_DATA)?;
    let remote_data: usize = unsafe { memory::read(remote_data_address)? };
    if remote_data == 0 {
        return Some(None);
    }

    let samp_ped_address = remote_data.checked_add(REMOTE_DATA_SAMP_PED)?;
    let samp_ped: usize = unsafe { memory::read(samp_ped_address)? };
    if samp_ped == 0 {
        return Some(None);
    }

    let gta_ped_address = samp_ped.checked_add(SAMP_PED_GTA_PED)?;
    let gta_ped: *mut c_void = unsafe { memory::read(gta_ped_address)? };
    Some((!gta_ped.is_null()).then_some(gta_ped))
}

/// Reads an MSVC x86 `std::string` without treating its object storage as a
/// C string. Both stRemotePlayer and stPlayerPool use this representation.
unsafe fn read_msvc_string(object: usize, string_offset: usize) -> Option<String> {
    let name_address = object.checked_add(string_offset)?;
    let length_address =
        name_address.checked_add(REMOTE_PLAYER_NAME_LENGTH - REMOTE_PLAYER_NAME)?;
    let capacity_address =
        name_address.checked_add(REMOTE_PLAYER_NAME_CAPACITY - REMOTE_PLAYER_NAME)?;
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

unsafe fn remote_player_name(remote: usize) -> Option<String> {
    unsafe { read_msvc_string(remote, REMOTE_PLAYER_NAME) }
}
