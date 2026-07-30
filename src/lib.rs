use retour::GenericDetour;
use serde::Deserialize;
use simplelog::*;
use std::collections::{HashMap, HashSet};
use std::ffi::{CString, c_void};
use std::fs::{self, File};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

// GTA SA 1.0 US (Hoodlum), 32-bit only.
const ADDR_CMODELINFO_ADD_PED_MODEL: usize = 0x4C67A0;
const ADDR_MS_P_TXD_POOL: usize = 0xC8800C;
const ADDR_MS_MODEL_INFO_PTRS: usize = 0xA9B0C8; // CBaseModelInfo* [20000]

// RenderWare stream functions.
// 0x7EC810 is the internal five-argument _rwStreamInitialize. Calling it as
// RwStreamOpen corrupts the stack. The public three-argument wrapper is here.
const ADDR_RWSTREAMOPEN: usize = 0x7ECEF0;
const ADDR_RWSTREAMFINDCHUNK: usize = 0x7ED2D0;
const ADDR_RPCLUMPSTREAMREAD: usize = 0x74B420;
const ADDR_RWSTREAMCLOSE: usize = 0x7ECE20;

// TXD store functions.
const ADDR_CTXDSTORE_ADD_TXD_SLOT: usize = 0x731C80;
const ADDR_CTXDSTORE_LOAD_TXD: usize = 0x7320B0;
const ADDR_CTXDSTORE_ADD_REF: usize = 0x731A30;
const ADDR_CTXDSTORE_PUSHCURRENTTXD: usize = 0x7316A0;
const ADDR_CTXDSTORE_POPCURRENTTXD: usize = 0x7316B0;
const ADDR_CTXDSTORE_SETCURRENTTXD: usize = 0x7319C0;

const ADDR_CPED_SET_MODEL_INDEX: usize = 0x5E4880;
const ADDR_CPEDMODELINFO_SETCLUMP: usize = 0x4C7340;
const ADDR_CGAME_PROCESS: usize = 0x53BEE0;

// SA-MP 0.3.7-R1 offsets.
const SAMP_OFFSET_SAMP_INFO: usize = 0x21A0F8;
const SAMP_OFFSET_PLAYERS_POOL: usize = 0x3CD;
const SAMP_POOLS_PLAYER: usize = 0x18;
const PLAYER_POOL_REMOTE_PLAYERS: usize = 0x2E;
const REMOTE_PLAYER_DATA: usize = 0x00;
const REMOTE_DATA_SAMP_PED: usize = 0x00;
const SAMP_PED_GTA_PED: usize = 0x44;
const SAMP_MAX_PLAYERS: usize = 1004;
const REMOTE_PLAYER_NAME: usize = 0x0C;
const REMOTE_PLAYER_NAME_LENGTH: usize = 0x1C;
const REMOTE_PLAYER_NAME_CAPACITY: usize = 0x20;
const MSVC_STRING_SSO_CAPACITY: usize = 15;

// CBaseModelInfo and CEntity offsets in GTA SA 1.0 US.
const MODEL_INFO_TXD_INDEX: usize = 0x0A;
const MODEL_INFO_FLAGS: usize = 0x12;
const MODEL_INFO_RW_OBJECT: usize = 0x1C;
const MODEL_INFO_COPY_START: usize = 0x0C;
const PED_MODEL_INFO_SIZE: usize = 0x44;
const MODEL_FLAG_OWNS_COLLISION: u16 = 1 << 5;
const ENTITY_MODEL_INDEX: usize = 0x22;

// RenderWare enums/chunk ID.
const RWSTREAM_FILENAME: i32 = 2;
const RWSTREAM_READ: i32 = 1;
const RW_ID_CLUMP: u32 = 0x10;

const PRIVATE_MODEL_ID_START: i32 = 18_000;
const PRIVATE_MODEL_ID_END: i32 = 20_000;
const CONFIG_PATH: &str = "skins.json";
const POLL_INTERVAL: Duration = Duration::from_millis(200);

type GameProcessFn = unsafe extern "cdecl" fn();

#[derive(Debug, Deserialize)]
struct SkinDefinition {
    txd_path: String,
    dff_path: String,
    donor_model_id: i32,
}

#[derive(Debug, Default, Deserialize)]
struct SkinConfig {
    #[serde(default)]
    skins: HashMap<String, SkinDefinition>,
    #[serde(default)]
    players: HashMap<String, String>,
}

#[derive(Default)]
struct LoaderRuntime {
    loaded_models: HashMap<String, i32>,
    failed_profiles: HashSet<String>,
    last_poll: Option<Instant>,
}

// The configuration is parsed before the hook is enabled. Runtime state is
// accessed only from GTA's frame thread.
static SAMP_BASE: OnceLock<usize> = OnceLock::new();
static SKIN_CONFIG: OnceLock<SkinConfig> = OnceLock::new();
static LOADER_RUNTIME: OnceLock<Mutex<LoaderRuntime>> = OnceLock::new();
static GAME_PROCESS_HOOK: OnceLock<GenericDetour<GameProcessFn>> = OnceLock::new();

unsafe fn read_mem<T: Copy>(address: usize) -> T {
    unsafe { *(address as *const T) }
}

unsafe fn call_cdecl_0<R>(address: usize) -> R {
    let function: unsafe extern "cdecl" fn() -> R = unsafe { std::mem::transmute(address) };
    unsafe { function() }
}

unsafe fn call_cdecl_1<R, A1>(address: usize, arg1: A1) -> R {
    let function: unsafe extern "cdecl" fn(A1) -> R = unsafe { std::mem::transmute(address) };
    unsafe { function(arg1) }
}

unsafe fn call_cdecl_2<R, A1, A2>(address: usize, arg1: A1, arg2: A2) -> R {
    let function: unsafe extern "cdecl" fn(A1, A2) -> R = unsafe { std::mem::transmute(address) };
    unsafe { function(arg1, arg2) }
}

unsafe fn call_cdecl_3<R, A1, A2, A3>(address: usize, arg1: A1, arg2: A2, arg3: A3) -> R {
    let function: unsafe extern "cdecl" fn(A1, A2, A3) -> R =
        unsafe { std::mem::transmute(address) };
    unsafe { function(arg1, arg2, arg3) }
}

unsafe fn call_cdecl_4<R, A1, A2, A3, A4>(
    address: usize,
    arg1: A1,
    arg2: A2,
    arg3: A3,
    arg4: A4,
) -> R {
    let function: unsafe extern "cdecl" fn(A1, A2, A3, A4) -> R =
        unsafe { std::mem::transmute(address) };
    unsafe { function(arg1, arg2, arg3, arg4) }
}

unsafe fn set_ped_model_index(ped: *mut c_void, model_id: i32) {
    type SetModelIndex = unsafe extern "thiscall" fn(*mut c_void, i32);
    let function: SetModelIndex = unsafe { std::mem::transmute(ADDR_CPED_SET_MODEL_INDEX) };
    unsafe { function(ped, model_id) };
}

unsafe fn set_ped_model_clump(model_info: *mut c_void, clump: *mut c_void) {
    type SetClump = unsafe extern "thiscall" fn(*mut c_void, *mut c_void);
    let function: SetClump = unsafe { std::mem::transmute(ADDR_CPEDMODELINFO_SETCLUMP) };
    unsafe { function(model_info, clump) };
}

unsafe fn load_dff_clump(txd_slot: i32, dff_path: &str) -> Option<*mut c_void> {
    let dff_path = CString::new(dff_path).expect("DFF path contains a NUL byte");

    // DFF material texture names are resolved against the current TXD.
    unsafe { call_cdecl_0::<()>(ADDR_CTXDSTORE_PUSHCURRENTTXD) };
    unsafe { call_cdecl_1::<(), i32>(ADDR_CTXDSTORE_SETCURRENTTXD, txd_slot) };

    let stream: *mut c_void = unsafe {
        call_cdecl_3(
            ADDR_RWSTREAMOPEN,
            RWSTREAM_FILENAME,
            RWSTREAM_READ,
            dff_path.as_ptr(),
        )
    };

    if stream.is_null() {
        unsafe { call_cdecl_0::<()>(ADDR_CTXDSTORE_POPCURRENTTXD) };
        log::error!("could not open DFF");
        return None;
    }

    let mut length = 0_u32;
    let mut version = 0_u32;
    let has_clump: *mut c_void = unsafe {
        call_cdecl_4(
            ADDR_RWSTREAMFINDCHUNK,
            stream,
            RW_ID_CLUMP,
            &mut length as *mut u32,
            &mut version as *mut u32,
        )
    };

    let clump: *mut c_void = if has_clump.is_null() {
        log::error!("DFF does not contain a RenderWare clump");
        std::ptr::null_mut()
    } else {
        unsafe { call_cdecl_1(ADDR_RPCLUMPSTREAMREAD, stream) }
    };

    unsafe {
        let _: *mut c_void =
            call_cdecl_2(ADDR_RWSTREAMCLOSE, stream, std::ptr::null_mut::<c_void>());
        call_cdecl_0::<()>(ADDR_CTXDSTORE_POPCURRENTTXD);
    }

    (!clump.is_null()).then_some(clump)
}

/// Loads one configured TXD/DFF pair into a private ped slot cloned from its
/// configured vanilla donor model.
unsafe fn load_custom_skin(skin_id: &str, definition: &SkinDefinition) -> Option<i32> {
    let model_id = unsafe { find_free_model_id()? };
    let txd_name = CString::new(format!("csl_{model_id}")).unwrap();
    let txd_path =
        CString::new(definition.txd_path.as_str()).expect("TXD path contains a NUL byte");

    let txd_slot: i32 = unsafe { call_cdecl_1(ADDR_CTXDSTORE_ADD_TXD_SLOT, txd_name.as_ptr()) };
    if txd_slot < 0 {
        log::error!("could not allocate a TXD slot");
        return None;
    }

    let loaded: u8 = unsafe { call_cdecl_2(ADDR_CTXDSTORE_LOAD_TXD, txd_slot, txd_path.as_ptr()) };
    if loaded == 0 {
        log::error!("could not load TXD for skin {skin_id}");
        return None;
    }
    let _: *mut c_void = unsafe { call_cdecl_1(ADDR_CTXDSTORE_ADD_REF, txd_slot) };

    let donor_model_info = unsafe { get_model_info(definition.donor_model_id) };
    if donor_model_info.is_null() {
        log::error!(
            "GTA donor ped model {} is not available for skin {skin_id}",
            definition.donor_model_id
        );
        return None;
    }

    let model_info: *mut c_void = unsafe { call_cdecl_1(ADDR_CMODELINFO_ADD_PED_MODEL, model_id) };
    if model_info.is_null() {
        log::error!("CModelInfo::AddPedModel({model_id}) failed");
        return None;
    }

    unsafe { clone_ped_model_metadata(model_info, donor_model_info, txd_slot) };

    let clump = unsafe { load_dff_clump(txd_slot, &definition.dff_path)? };
    // This does ped-specific clump setup; never write m_pRwClump directly.
    unsafe { set_ped_model_clump(model_info, clump) };

    log::info!(
        "loaded skin {skin_id}: private model={model_id}, donor={}, txd_slot={txd_slot}",
        definition.donor_model_id
    );
    Some(model_id)
}

/// Copies the safe, initialized portion of a vanilla CPedModelInfo into a new
/// slot. The new entry keeps its constructor-provided vtable/key/refcount, has
/// no borrowed RenderWare clump, and never claims ownership of the donor's
/// shared collision model.
unsafe fn clone_ped_model_metadata(destination: *mut c_void, donor: *mut c_void, txd_slot: i32) {
    unsafe {
        std::ptr::copy_nonoverlapping(
            (donor as *const u8).add(MODEL_INFO_COPY_START),
            (destination as *mut u8).add(MODEL_INFO_COPY_START),
            PED_MODEL_INFO_SIZE - MODEL_INFO_COPY_START,
        );

        // The source model's clump belongs to the donor. SetClump below owns
        // the newly loaded custom clump instead.
        *((destination as usize + MODEL_INFO_RW_OBJECT) as *mut *mut c_void) = std::ptr::null_mut();

        // The copied collision model is shared with the donor; a private model
        // must not free it during GTA shutdown.
        let flags = (destination as usize + MODEL_INFO_FLAGS) as *mut u16;
        *flags &= !MODEL_FLAG_OWNS_COLLISION;

        // CBaseModelInfo::m_nTxdIndex is a signed short at +0x0A.
        *((destination as usize + MODEL_INFO_TXD_INDEX) as *mut i16) = txd_slot as i16;
    }
}

unsafe fn get_player_pool(samp_base: usize) -> Option<usize> {
    let samp: usize = unsafe { read_mem(samp_base + SAMP_OFFSET_SAMP_INFO) };
    if samp == 0 {
        return None;
    }

    let pools: usize = unsafe { read_mem(samp + SAMP_OFFSET_PLAYERS_POOL) };
    if pools == 0 {
        return None;
    }

    let player_pool: usize = unsafe { read_mem(pools + SAMP_POOLS_PLAYER) };
    if player_pool == 0 {
        return None;
    }

    Some(player_pool)
}

unsafe fn remote_gta_ped(remote: usize) -> Option<*mut c_void> {
    let remote_data: usize = unsafe { read_mem(remote + REMOTE_PLAYER_DATA) };
    if remote_data == 0 {
        return None;
    }

    let samp_ped: usize = unsafe { read_mem(remote_data + REMOTE_DATA_SAMP_PED) };
    if samp_ped == 0 {
        return None;
    }

    let gta_ped: *mut c_void = unsafe { read_mem(samp_ped + SAMP_PED_GTA_PED) };
    (!gta_ped.is_null()).then_some(gta_ped)
}

/// Reads stRemotePlayer::strPlayerName (an MSVC x86 std::string) without
/// assuming the remote-player struct itself is a C string.
unsafe fn remote_player_name(remote: usize) -> Option<String> {
    let length: usize = unsafe { read_mem(remote + REMOTE_PLAYER_NAME_LENGTH) };
    let capacity: usize = unsafe { read_mem(remote + REMOTE_PLAYER_NAME_CAPACITY) };
    if length == 0 || length > 24 || capacity < length {
        return None;
    }

    let text_ptr: *const u8 = if capacity <= MSVC_STRING_SSO_CAPACITY {
        (remote + REMOTE_PLAYER_NAME) as *const u8
    } else {
        unsafe { read_mem(remote + REMOTE_PLAYER_NAME) }
    };
    if text_ptr.is_null() {
        return None;
    }

    let bytes = unsafe { std::slice::from_raw_parts(text_ptr, length) };
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}

unsafe fn configured_remote_peds(
    samp_base: usize,
    config: &SkinConfig,
) -> Vec<(String, *mut c_void)> {
    let Some(player_pool) = (unsafe { get_player_pool(samp_base) }) else {
        return Vec::new();
    };

    let max_player_id: u32 = unsafe { read_mem(player_pool) };
    let max_player_id = (max_player_id as usize).min(SAMP_MAX_PLAYERS - 1);
    let mut matches = Vec::new();

    for player_id in 0..=max_player_id {
        let remote: usize =
            unsafe { read_mem(player_pool + PLAYER_POOL_REMOTE_PLAYERS + player_id * 4) };
        if remote == 0 {
            continue;
        }

        let Some(name) = (unsafe { remote_player_name(remote) }) else {
            continue;
        };
        if !config.players.contains_key(&name) {
            continue;
        }

        if let Some(ped) = unsafe { remote_gta_ped(remote) } {
            matches.push((name, ped));
        }
    }

    matches
}

unsafe fn ped_model_id(ped: *mut c_void) -> i16 {
    unsafe { read_mem((ped as usize) + ENTITY_MODEL_INDEX) }
}

unsafe fn is_gta_ready() -> bool {
    unsafe { read_mem::<usize>(ADDR_MS_P_TXD_POOL) != 0 }
}

unsafe fn get_model_info(model_id: i32) -> *mut c_void {
    if !(0..20_000).contains(&model_id) {
        return std::ptr::null_mut();
    }

    let model_infos = ADDR_MS_MODEL_INFO_PTRS as *const *mut c_void;
    unsafe { *model_infos.add(model_id as usize) }
}

unsafe fn find_free_model_id() -> Option<i32> {
    let model_infos = ADDR_MS_MODEL_INFO_PTRS as *const *mut c_void;
    for model_id in PRIVATE_MODEL_ID_START..PRIVATE_MODEL_ID_END {
        if unsafe { *model_infos.add(model_id as usize) }.is_null() {
            return Some(model_id);
        }
    }

    log::error!(
        "no private model ID available in {PRIVATE_MODEL_ID_START}..{PRIVATE_MODEL_ID_END}"
    );
    None
}

fn load_skin_config() -> Result<SkinConfig, String> {
    let text = match fs::read_to_string(CONFIG_PATH) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::write(CONFIG_PATH, "{}\n")
                .map_err(|error| format!("could not create {CONFIG_PATH}: {error}"))?;
            log::info!("created empty {CONFIG_PATH}");
            return Ok(SkinConfig::default());
        }
        Err(error) => return Err(format!("could not read {CONFIG_PATH}: {error}")),
    };
    let config: SkinConfig =
        serde_json::from_str(&text).map_err(|error| format!("invalid {CONFIG_PATH}: {error}"))?;

    for (skin_id, definition) in &config.skins {
        if definition.txd_path.is_empty() || definition.dff_path.is_empty() {
            return Err(format!("skin {skin_id} has an empty asset path"));
        }
        if !(0..20_000).contains(&definition.donor_model_id) {
            return Err(format!(
                "skin {skin_id} has invalid donor_model_id {}",
                definition.donor_model_id
            ));
        }
    }

    for (player_name, skin_id) in &config.players {
        if !config.skins.contains_key(skin_id) {
            return Err(format!(
                "player {player_name} references unknown skin {skin_id}"
            ));
        }
    }

    Ok(config)
}

unsafe fn model_for_skin(skin_id: &str, definition: &SkinDefinition) -> Option<i32> {
    let runtime = LOADER_RUNTIME.get_or_init(|| Mutex::new(LoaderRuntime::default()));
    {
        let state = runtime.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(model_id) = state.loaded_models.get(skin_id) {
            return Some(*model_id);
        }
        if state.failed_profiles.contains(skin_id) {
            return None;
        }
    }

    let model_id = unsafe { load_custom_skin(skin_id, definition) };
    let mut state = runtime.lock().unwrap_or_else(|error| error.into_inner());
    match model_id {
        Some(model_id) => {
            state.loaded_models.insert(skin_id.to_owned(), model_id);
            Some(model_id)
        }
        None => {
            state.failed_profiles.insert(skin_id.to_owned());
            None
        }
    }
}

unsafe fn process_skin_loader_on_game_thread() {
    // The hook runs every GTA frame, but scanning a 1004-slot SA-MP pool does
    // not need to. Five polls per second keeps skin changes responsive without
    // doing the full scan on every frame.
    let runtime = LOADER_RUNTIME.get_or_init(|| Mutex::new(LoaderRuntime::default()));
    {
        let mut state = runtime.lock().unwrap_or_else(|error| error.into_inner());
        let now = Instant::now();
        if state
            .last_poll
            .is_some_and(|last_poll| now.duration_since(last_poll) < POLL_INTERVAL)
        {
            return;
        }
        state.last_poll = Some(now);
    }

    let Some(config) = SKIN_CONFIG.get() else {
        return;
    };
    let Some(&samp_base) = SAMP_BASE.get() else {
        return;
    };

    for (name, ped) in unsafe { configured_remote_peds(samp_base, config) } {
        let skin_id = &config.players[&name];
        let definition = &config.skins[skin_id];
        let Some(model_id) = (unsafe { model_for_skin(skin_id, definition) }) else {
            continue;
        };

        // SA-MP can reset a ped while it remains streamed in. Reapply only
        // after that happens, rather than calling SetModelIndex every frame.
        if unsafe { ped_model_id(ped) } != model_id as i16 {
            unsafe { set_ped_model_index(ped, model_id) };
            log::debug!("applied custom model {model_id} to {name}");
        }
    }
}

unsafe extern "cdecl" fn game_process_detour() {
    unsafe { process_skin_loader_on_game_thread() };

    // GenericDetour::call executes the generated trampoline, never this detour.
    let hook = GAME_PROCESS_HOOK
        .get()
        .expect("CGame::Process hook was enabled before it was stored");
    unsafe { hook.call() };
}

unsafe fn install_game_process_hook() -> Result<(), retour::Error> {
    let target: GameProcessFn = unsafe { std::mem::transmute(ADDR_CGAME_PROCESS) };
    let hook = unsafe { GenericDetour::new(target, game_process_detour as GameProcessFn)? };

    GAME_PROCESS_HOOK
        .set(hook)
        .expect("CGame::Process hook was installed twice");

    let hook = GAME_PROCESS_HOOK.get().unwrap();
    unsafe { hook.enable() }
}

fn plugin_thread() {
    init_logger();

    #[cfg(debug_assertions)]
    while unsafe { winapi::um::debugapi::IsDebuggerPresent() } == 0 {
        thread::sleep(Duration::from_millis(100));
    }

    let config = match load_skin_config() {
        Ok(config) => config,
        Err(error) => {
            log::error!("{error}");
            return;
        }
    };
    let skin_count = config.skins.len();
    let player_count = config.players.len();
    if player_count == 0 {
        log::info!("{CONFIG_PATH} has no player mappings; loader is idle");
        return;
    }
    SKIN_CONFIG
        .set(config)
        .expect("skin configuration was initialized twice");

    let samp_base = loop {
        let module = unsafe { GetModuleHandleA(b"samp.dll\0".as_ptr()) };
        if module != 0 {
            break module as usize;
        }
        thread::sleep(Duration::from_millis(500));
    };
    SAMP_BASE
        .set(samp_base)
        .expect("SA-MP base was initialized twice");

    while !unsafe { is_gta_ready() } {
        thread::sleep(Duration::from_millis(100));
    }

    if let Err(error) = unsafe { install_game_process_hook() } {
        log::error!("could not install CGame::Process hook: {error}");
        return;
    }

    log::info!("watching {player_count} configured player(s) across {skin_count} skin(s)");
}

/*
The previous implementation called RenderWare and CPed::SetModelIndex directly
from this spawned thread. Both operations mutate engine state and must run from
GTA's frame thread, so they now run through game_process_detour above.
*/

#[unsafe(no_mangle)]
pub extern "system" fn DllMain(_hmodule: HMODULE, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        thread::spawn(plugin_thread);
    }
    1
}

fn init_logger() {
    if let Ok(file) = File::create("custom_skin_loader.log") {
        let config = ConfigBuilder::new()
            .set_time_level(LevelFilter::Off)
            .build();
        let _ = WriteLogger::init(LevelFilter::Debug, config, file);
        log::info!("custom_skin_loader started");
    }
}
