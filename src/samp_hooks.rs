use crate::memory;
use crate::samp::Samp;
use retour::{GenericDetour, RawDetour};
use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

// SA-MP 0.3.7-R1. The version marker is checked before either hook target.
const R1_VERSION_MARKER_OFFSET: usize = 0xBABE;
const R1_VERSION_MARKER: [u8; 10] = [0xF8, 0x03, 0x6A, 0x00, 0x40, 0x50, 0x51, 0x8D, 0x4C, 0x24];

// CRemotePlayer::Spawn returns after SA-MP has created the remote GTA ped.
const REMOTE_PLAYER_SPAWN_OFFSET: usize = 0x13890;
const REMOTE_PLAYER_SPAWN_SIGNATURE: [u8; 16] = [
    0x56, 0x8B, 0xF1, 0x8B, 0x0D, 0x0C, 0xA1, 0x21, 0x10, 0xE8, 0xD2, 0x86, 0x08, 0x00, 0x85, 0xC0,
];
const REMOTE_PLAYER_SPAWN_GAME_OFFSET: usize = 0x21A10C;
const REMOTE_PLAYER_SPAWN_GAME_POINTER: std::ops::Range<usize> = 5..9;

// The RPC handler applies a server-issued skin change before returning.
const SET_PLAYER_SKIN_OFFSET: usize = 0x15860;
const SET_PLAYER_SKIN_SIGNATURE: [u8; 16] = [
    0xE9, 0xA2, 0xB6, 0x27, 0x00, 0x0F, 0x90, 0xC5, 0x52, 0xC7, 0x44, 0x24, 0x08, 0x40, 0x00, 0x00,
];

type RemotePlayerSpawnFn =
    unsafe extern "thiscall" fn(*mut c_void, i32, i32, *mut c_void, f32, u32, i32) -> i32;
type SetPlayerSkinFn = unsafe extern "cdecl" fn(*mut c_void);

static REMOTE_PLAYER_SPAWN_HOOK: OnceLock<RawDetour> = OnceLock::new();
static SET_PLAYER_SKIN_HOOK: OnceLock<GenericDetour<SetPlayerSkinFn>> = OnceLock::new();
const REMOTE_PLAYER_SPAWN_REFRESH: u8 = 1 << 0;
const SET_PLAYER_SKIN_REFRESH: u8 = 1 << 1;

static REFRESH_REASONS: AtomicU8 = AtomicU8::new(0);

pub unsafe fn install(samp: &Samp) -> Result<(), String> {
    let version_marker_address = samp
        .base()
        .checked_add(R1_VERSION_MARKER_OFFSET)
        .ok_or("SA-MP version marker address overflowed")?;
    validate_bytes(
        version_marker_address,
        &R1_VERSION_MARKER,
        "0.3.7-R1 version marker",
    )?;

    let remote_player_spawn_signature = remote_player_spawn_signature(samp.base())?;
    let remote_player_spawn = target_with_signature(
        samp,
        REMOTE_PLAYER_SPAWN_OFFSET,
        &remote_player_spawn_signature,
        "CRemotePlayer::Spawn",
    )?;
    let set_player_skin = target_with_signature(
        samp,
        SET_PLAYER_SKIN_OFFSET,
        &SET_PLAYER_SKIN_SIGNATURE,
        "ScrSetPlayerSkin",
    )?;

    let set_player_skin: SetPlayerSkinFn = unsafe { std::mem::transmute(set_player_skin) };
    let remote_player_spawn_hook = unsafe {
        RawDetour::new(
            remote_player_spawn as *const (),
            remote_player_spawn_detour as *const (),
        )
    }
    .map_err(|error| format!("could not prepare CRemotePlayer::Spawn hook: {error}"))?;
    let set_player_skin_hook =
        unsafe { GenericDetour::new(set_player_skin, set_player_skin_detour as SetPlayerSkinFn) }
            .map_err(|error| format!("could not prepare ScrSetPlayerSkin hook: {error}"))?;

    REMOTE_PLAYER_SPAWN_HOOK
        .set(remote_player_spawn_hook)
        .map_err(|_| "CRemotePlayer::Spawn hook was installed twice")?;
    SET_PLAYER_SKIN_HOOK
        .set(set_player_skin_hook)
        .map_err(|_| "ScrSetPlayerSkin hook was installed twice")?;

    let remote_player_spawn_hook = REMOTE_PLAYER_SPAWN_HOOK
        .get()
        .expect("CRemotePlayer::Spawn hook was stored");
    unsafe { remote_player_spawn_hook.enable() }
        .map_err(|error| format!("could not enable CRemotePlayer::Spawn hook: {error}"))?;

    let set_player_skin_hook = SET_PLAYER_SKIN_HOOK
        .get()
        .expect("ScrSetPlayerSkin hook was stored");
    if let Err(error) = unsafe { set_player_skin_hook.enable() } {
        let _ = unsafe { remote_player_spawn_hook.disable() };
        return Err(format!("could not enable ScrSetPlayerSkin hook: {error}"));
    }

    Ok(())
}

pub fn take_refresh_request() -> Option<&'static str> {
    refresh_reason(REFRESH_REASONS.swap(0, Ordering::AcqRel))
}

unsafe extern "thiscall" fn remote_player_spawn_detour(
    remote_player: *mut c_void,
    unused: i32,
    model_id: i32,
    position: *mut c_void,
    rotation: f32,
    color: u32,
    fighting_style: i32,
) -> i32 {
    let hook = REMOTE_PLAYER_SPAWN_HOOK
        .get()
        .expect("CRemotePlayer::Spawn hook was enabled before it was stored");
    let original: RemotePlayerSpawnFn = unsafe { std::mem::transmute(hook.trampoline()) };
    let result = unsafe {
        original(
            remote_player,
            unused,
            model_id,
            position,
            rotation,
            color,
            fighting_style,
        )
    };
    if result != 0 {
        REFRESH_REASONS.fetch_or(REMOTE_PLAYER_SPAWN_REFRESH, Ordering::Release);
    }
    result
}

unsafe extern "cdecl" fn set_player_skin_detour(parameters: *mut c_void) {
    let hook = SET_PLAYER_SKIN_HOOK
        .get()
        .expect("ScrSetPlayerSkin hook was enabled before it was stored");
    unsafe { hook.call(parameters) };
    REFRESH_REASONS.fetch_or(SET_PLAYER_SKIN_REFRESH, Ordering::Release);
}

fn target_with_signature(
    samp: &Samp,
    offset: usize,
    signature: &[u8],
    name: &str,
) -> Result<usize, String> {
    let address = samp
        .base()
        .checked_add(offset)
        .ok_or_else(|| format!("{name} address overflowed"))?;
    validate_bytes(address, signature, name)?;
    Ok(address)
}

fn remote_player_spawn_signature(samp_base: usize) -> Result<[u8; 16], String> {
    // The original R1 prologue loads the SA-MP CGame pointer from
    // `samp.dll + 0x21A10C`. The loader may relocate samp.dll, so validate the
    // relocated absolute pointer rather than the preferred-image bytes.
    let game_pointer = samp_base
        .checked_add(REMOTE_PLAYER_SPAWN_GAME_OFFSET)
        .ok_or("CRemotePlayer::Spawn CGame pointer address overflowed")?;
    let game_pointer = u32::try_from(game_pointer)
        .map_err(|_| "CRemotePlayer::Spawn CGame pointer does not fit in x86 address space")?;

    let mut signature = REMOTE_PLAYER_SPAWN_SIGNATURE;
    signature[REMOTE_PLAYER_SPAWN_GAME_POINTER].copy_from_slice(&game_pointer.to_le_bytes());
    Ok(signature)
}

fn validate_bytes(address: usize, expected: &[u8], name: &str) -> Result<(), String> {
    let actual = memory::read_bytes(address, expected.len())
        .ok_or_else(|| format!("could not read {name} at 0x{address:08X}"))?;
    if actual == expected {
        return Ok(());
    }

    Err(format!(
        "{name} bytes differ at 0x{address:08X} (expected {}, found {})",
        hex(expected),
        hex(&actual)
    ))
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn refresh_reason(reasons: u8) -> Option<&'static str> {
    match reasons & (REMOTE_PLAYER_SPAWN_REFRESH | SET_PLAYER_SKIN_REFRESH) {
        0 => None,
        REMOTE_PLAYER_SPAWN_REFRESH => Some("a remote player spawn"),
        SET_PLAYER_SKIN_REFRESH => Some("a server skin change"),
        _ => Some("a remote player spawn and a server skin change"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        REMOTE_PLAYER_SPAWN_REFRESH, SET_PLAYER_SKIN_REFRESH, hex, refresh_reason,
        remote_player_spawn_signature,
    };

    #[test]
    fn formats_target_bytes_for_diagnostics() {
        assert_eq!(hex(&[0xE9, 0xA2, 0x00]), "E9 A2 00");
    }

    #[test]
    fn describes_coalesced_refresh_requests() {
        assert_eq!(refresh_reason(0), None);
        assert_eq!(
            refresh_reason(REMOTE_PLAYER_SPAWN_REFRESH),
            Some("a remote player spawn")
        );
        assert_eq!(
            refresh_reason(SET_PLAYER_SKIN_REFRESH),
            Some("a server skin change")
        );
        assert_eq!(
            refresh_reason(REMOTE_PLAYER_SPAWN_REFRESH | SET_PLAYER_SKIN_REFRESH),
            Some("a remote player spawn and a server skin change")
        );
    }

    #[test]
    fn validates_the_spawn_prologue_after_samp_relocation() {
        let signature = remote_player_spawn_signature(0x0405_0000).unwrap();
        assert_eq!(&signature[5..9], &[0x0C, 0xA1, 0x26, 0x04]);
    }
}
