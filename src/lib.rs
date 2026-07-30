use retour::GenericDetour;
use simplelog::*;
use std::ffi::{CString, c_void};
use std::fs::File;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;
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
const ADDR_FIND_PLAYER_PED: usize = 0x56E210;
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

// Keep this true while testing the DFF/TXD pipeline on your own ped. Set it to
// false to target TARGET_PLAYER_ID instead.
const APPLY_TO_LOCAL_PLAYER: bool = true;

// Metadata is cloned from this initialized vanilla ped. The resulting custom
// model uses a private, currently unused model ID and does not replace slot 7.
const DONOR_PED_MODEL_ID: i32 = 7;
const PRIVATE_MODEL_ID_START: i32 = 18_000;
const PRIVATE_MODEL_ID_END: i32 = 20_000;

// Change this to the SA-MP ID of the player whose skin you want to override.
// ID-based lookup is intentional for now: stRemotePlayer::strPlayerName is a
// C++ std::string, so treating the remote-player address as a C string was UB.
const TARGET_PLAYER_ID: usize = 0;
const TXD_PATH: &str = "models/myskin.txd";
const DFF_PATH: &str = "models/myskin.dff";

type GameProcessFn = unsafe extern "cdecl" fn();

#[derive(Clone, Copy)]
enum LoaderState {
    WaitingForAssets,
    Ready(i32),
    Failed,
}

// These are written before the hook is enabled. The hook then owns all
// interaction with GTA/RenderWare objects on GTA's game thread.
static SAMP_BASE: AtomicUsize = AtomicUsize::new(0);
static GAME_PROCESS_HOOK: OnceLock<GenericDetour<GameProcessFn>> = OnceLock::new();
static mut LOADER_STATE: LoaderState = LoaderState::WaitingForAssets;

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

unsafe fn load_dff_clump(txd_slot: i32) -> Option<*mut c_void> {
    let dff_path = CString::new(DFF_PATH).expect("DFF path contains a NUL byte");

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
        log::error!("could not open DFF: {DFF_PATH}");
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
        log::error!("DFF does not contain a RenderWare clump: {DFF_PATH}");
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

/// Loads a loose TXD/DFF pair into a private ped slot cloned from a vanilla ped.
unsafe fn load_custom_skin() -> Option<i32> {
    let txd_name = CString::new("custom_skin_loader_txd").unwrap();
    let txd_path = CString::new(TXD_PATH).expect("TXD path contains a NUL byte");

    let txd_slot: i32 = unsafe { call_cdecl_1(ADDR_CTXDSTORE_ADD_TXD_SLOT, txd_name.as_ptr()) };
    if txd_slot < 0 {
        log::error!("could not allocate a TXD slot");
        return None;
    }

    let loaded: u8 = unsafe { call_cdecl_2(ADDR_CTXDSTORE_LOAD_TXD, txd_slot, txd_path.as_ptr()) };
    if loaded == 0 {
        log::error!("could not load TXD: {TXD_PATH}");
        return None;
    }
    let _: *mut c_void = unsafe { call_cdecl_1(ADDR_CTXDSTORE_ADD_REF, txd_slot) };

    let donor_model_info = unsafe { get_model_info(DONOR_PED_MODEL_ID) };
    if donor_model_info.is_null() {
        log::error!("GTA donor ped model {DONOR_PED_MODEL_ID} is not available");
        return None;
    }

    let model_id = unsafe { find_free_model_id()? };
    let model_info: *mut c_void = unsafe { call_cdecl_1(ADDR_CMODELINFO_ADD_PED_MODEL, model_id) };
    if model_info.is_null() {
        log::error!("CModelInfo::AddPedModel({model_id}) failed");
        return None;
    }

    unsafe { clone_ped_model_metadata(model_info, donor_model_info, txd_slot) };

    let clump = unsafe { load_dff_clump(txd_slot)? };
    // This does ped-specific clump setup; never write m_pRwClump directly.
    unsafe { set_ped_model_clump(model_info, clump) };

    log::info!(
        "custom skin loaded: private model={model_id}, donor={DONOR_PED_MODEL_ID}, txd_slot={txd_slot}",
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

unsafe fn find_remote_gta_ped(samp_base: usize, player_id: usize) -> Option<*mut c_void> {
    if player_id >= SAMP_MAX_PLAYERS {
        return None;
    }

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

    let remote: usize =
        unsafe { read_mem(player_pool + PLAYER_POOL_REMOTE_PLAYERS + player_id * 4) };
    if remote == 0 {
        return None;
    }

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

unsafe fn find_local_gta_ped() -> Option<*mut c_void> {
    // FindPlayerPed(-1) selects GTA's active local player.
    let ped: *mut c_void = unsafe { call_cdecl_1(ADDR_FIND_PLAYER_PED, -1_i32) };
    (!ped.is_null()).then_some(ped)
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

unsafe fn process_skin_loader_on_game_thread() {
    let model_id = unsafe {
        match LOADER_STATE {
            LoaderState::WaitingForAssets => match load_custom_skin() {
                Some(model_id) => {
                    LOADER_STATE = LoaderState::Ready(model_id);
                    model_id
                }
                None => {
                    LOADER_STATE = LoaderState::Failed;
                    log::error!("custom skin initialization failed; not retrying this session");
                    return;
                }
            },
            LoaderState::Ready(model_id) => model_id,
            LoaderState::Failed => return,
        }
    };

    let target_ped = if APPLY_TO_LOCAL_PLAYER {
        unsafe { find_local_gta_ped() }
    } else {
        let samp_base = SAMP_BASE.load(Ordering::Acquire);
        if samp_base == 0 {
            None
        } else {
            unsafe { find_remote_gta_ped(samp_base, TARGET_PLAYER_ID) }
        }
    };

    if let Some(ped) = target_ped {
        // SA-MP can reset a ped while it remains streamed in. Reapply only
        // after that happens, rather than calling SetModelIndex every frame.
        if unsafe { ped_model_id(ped) } != model_id as i16 {
            unsafe { set_ped_model_index(ped, model_id) };
            log::debug!("reapplied custom model {model_id}");
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

    if !APPLY_TO_LOCAL_PLAYER {
        let samp_base = loop {
            let module = unsafe { GetModuleHandleA(b"samp.dll\0".as_ptr()) };
            if module != 0 {
                break module as usize;
            }
            thread::sleep(Duration::from_millis(500));
        };
        SAMP_BASE.store(samp_base, Ordering::Release);
    }

    while !unsafe { is_gta_ready() } {
        thread::sleep(Duration::from_millis(100));
    }

    if let Err(error) = unsafe { install_game_process_hook() } {
        log::error!("could not install CGame::Process hook: {error}");
        return;
    }

    if APPLY_TO_LOCAL_PLAYER {
        log::info!("testing custom skin on the local player");
    } else {
        log::info!("watching SA-MP player ID {TARGET_PLAYER_ID}");
    }
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
