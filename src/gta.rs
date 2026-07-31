use crate::config::SkinDefinition;
use crate::memory;
use std::ffi::{CString, c_void};
use std::path::Path;

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
const ADDR_CTXDSTORE_ADD_REF: usize = 0x731A00;
const ADDR_CTXDSTORE_REMOVE_REF: usize = 0x731A30;
const ADDR_CTXDSTORE_REMOVE_TXD_SLOT: usize = 0x731CD0;
const ADDR_CTXDSTORE_PUSHCURRENTTXD: usize = 0x7316A0;
const ADDR_CTXDSTORE_POPCURRENTTXD: usize = 0x7316B0;
const ADDR_CTXDSTORE_SETCURRENTTXD: usize = 0x7319C0;

const ADDR_CPED_SET_MODEL_INDEX: usize = 0x5E4880;
const ADDR_CPEDMODELINFO_SETCLUMP: usize = 0x4C7340;

// CBaseModelInfo and CEntity offsets in GTA SA 1.0 US.
const MODEL_INFO_TXD_INDEX: usize = 0x0A;
const MODEL_INFO_FLAGS: usize = 0x12;
const MODEL_INFO_RW_OBJECT: usize = 0x1C;
const MODEL_INFO_COPY_START: usize = 0x0C;
const PED_MODEL_INFO_SIZE: usize = 0x44;
const PED_MODEL_INFO_HIT_COL_MODEL: usize = 0x34;
const MODEL_FLAG_OWNS_COLLISION: u16 = 1 << 5;
const ENTITY_MODEL_INDEX: usize = 0x22;
const VTABLE_DELETE_RW_OBJECT_OFFSET: usize = 0x20;

// RenderWare enums/chunk ID.
const RWSTREAM_FILENAME: i32 = 2;
const RWSTREAM_READ: i32 = 1;
const RW_ID_CLUMP: u32 = 0x10;

const PRIVATE_MODEL_ID_START: i32 = 18_000;
const PRIVATE_MODEL_ID_END: i32 = 20_000;

#[derive(Clone, Copy, Debug)]
pub struct SkinResources {
    pub model_id: i32,
    pub txd_slot: i32,
}

#[derive(Debug)]
pub struct SkinLoadFailure {
    pub recyclable_model_id: Option<i32>,
}

pub unsafe fn is_ready() -> bool {
    unsafe { memory::read::<usize>(ADDR_MS_P_TXD_POOL).is_some_and(|pool| pool != 0) }
}

pub unsafe fn ped_model_id(ped: *mut c_void) -> Option<i16> {
    let model_index_address = (ped as usize).checked_add(ENTITY_MODEL_INDEX)?;
    unsafe { memory::read(model_index_address) }
}

pub unsafe fn set_ped_model_index(ped: *mut c_void, model_id: i32) {
    type SetModelIndex = unsafe extern "thiscall" fn(*mut c_void, i32);
    let function: SetModelIndex = unsafe { std::mem::transmute(ADDR_CPED_SET_MODEL_INDEX) };
    unsafe { function(ped, model_id) };
}

/// Loads one configured TXD/DFF pair into a private ped slot cloned from its
/// configured vanilla donor model. A recycled slot keeps its CPedModelInfo
/// allocation but has no RenderWare object or TXD attached.
pub unsafe fn load_skin(
    skin_id: &str,
    definition: &SkinDefinition,
    recycled_model_id: Option<i32>,
) -> Result<SkinResources, SkinLoadFailure> {
    if !Path::new(&definition.txd_path).is_file() {
        log::error!(
            "skin {skin_id}: TXD file does not exist or is not a file: {}",
            definition.txd_path
        );
        return Err(SkinLoadFailure {
            recyclable_model_id: recycled_model_id,
        });
    }
    if !Path::new(&definition.dff_path).is_file() {
        log::error!(
            "skin {skin_id}: DFF file does not exist or is not a file: {}",
            definition.dff_path
        );
        return Err(SkinLoadFailure {
            recyclable_model_id: recycled_model_id,
        });
    }

    log::info!(
        "loading skin {skin_id}: donor={}, txd={}, dff={}",
        definition.donor_model_id,
        definition.txd_path,
        definition.dff_path
    );

    let model_id = match recycled_model_id {
        Some(model_id) => model_id,
        None => match unsafe { find_free_model_id() } {
            Some(model_id) => model_id,
            None => {
                return Err(SkinLoadFailure {
                    recyclable_model_id: None,
                });
            }
        },
    };
    let txd_name = CString::new(format!("csl_{model_id}")).expect("model ID cannot contain NUL");
    let txd_path = match CString::new(definition.txd_path.as_str()) {
        Ok(path) => path,
        Err(_) => {
            log::error!(
                "skin {skin_id}: TXD path contains a NUL byte: {:?}",
                definition.txd_path
            );
            return Err(SkinLoadFailure {
                recyclable_model_id: recycled_model_id,
            });
        }
    };

    let txd_slot: i32 = unsafe { call_cdecl_1(ADDR_CTXDSTORE_ADD_TXD_SLOT, txd_name.as_ptr()) };
    if txd_slot < 0 {
        log::error!("skin {skin_id}: could not allocate a TXD slot");
        return Err(SkinLoadFailure {
            recyclable_model_id: recycled_model_id,
        });
    }

    let loaded: u8 = unsafe { call_cdecl_2(ADDR_CTXDSTORE_LOAD_TXD, txd_slot, txd_path.as_ptr()) };
    if loaded == 0 {
        log::error!(
            "skin {skin_id}: could not load TXD from {} into slot {txd_slot}",
            definition.txd_path
        );
        unsafe { remove_txd_slot(txd_slot, false) };
        return Err(SkinLoadFailure {
            recyclable_model_id: recycled_model_id,
        });
    }
    let _: *mut c_void = unsafe { call_cdecl_1(ADDR_CTXDSTORE_ADD_REF, txd_slot) };

    let donor_model_info = unsafe { get_model_info(definition.donor_model_id) };
    if donor_model_info.is_null() {
        log::error!(
            "GTA donor ped model {} is not available for skin {skin_id}",
            definition.donor_model_id
        );
        unsafe { remove_txd_slot(txd_slot, true) };
        return Err(SkinLoadFailure {
            recyclable_model_id: recycled_model_id,
        });
    }

    let model_info: *mut c_void = match recycled_model_id {
        Some(_) => unsafe { get_model_info(model_id) },
        None => unsafe { call_cdecl_1(ADDR_CMODELINFO_ADD_PED_MODEL, model_id) },
    };
    if model_info.is_null() {
        log::error!("skin {skin_id}: could not prepare private model {model_id}");
        unsafe { remove_txd_slot(txd_slot, true) };
        return Err(SkinLoadFailure {
            recyclable_model_id: recycled_model_id,
        });
    }

    unsafe { clone_ped_model_metadata(model_info, donor_model_info, txd_slot) };

    let clump = match unsafe { load_dff_clump(txd_slot, &definition.dff_path) } {
        Some(clump) => clump,
        None => {
            let _ = unsafe { delete_model_rw_object(model_info) };
            unsafe { remove_txd_slot(txd_slot, true) };
            unsafe {
                *((model_info as usize + MODEL_INFO_TXD_INDEX) as *mut i16) = -1;
            }
            return Err(SkinLoadFailure {
                recyclable_model_id: Some(model_id),
            });
        }
    };
    // This does ped-specific clump setup; never write m_pRwClump directly.
    unsafe { set_ped_model_clump(model_info, clump) };

    log::info!(
        "loaded skin {skin_id}: private model={model_id}, donor={}, txd_slot={txd_slot}, txd={}, dff={}",
        definition.donor_model_id,
        definition.txd_path,
        definition.dff_path
    );
    Ok(SkinResources { model_id, txd_slot })
}

pub unsafe fn release_skin_resources(skin_id: &str, resources: SkinResources) -> bool {
    let model_info = unsafe { get_model_info(resources.model_id) };
    if model_info.is_null() {
        log::error!(
            "skin {skin_id}: private model {} disappeared before cleanup",
            resources.model_id
        );
        return false;
    }
    if !unsafe { delete_model_rw_object(model_info) } {
        log::error!(
            "skin {skin_id}: could not destroy RenderWare clump for private model {}",
            resources.model_id
        );
        return false;
    }

    unsafe { remove_txd_slot(resources.txd_slot, true) };
    // Keep the CPedModelInfo allocation valid but inert. Its ID can now be
    // reused by this loader without allocating another entry from GTA's fixed
    // ped-model-info array.
    unsafe {
        *((model_info as usize + MODEL_INFO_TXD_INDEX) as *mut i16) = -1;
    }
    log::info!(
        "cleaned retired skin {skin_id}: private model={}, txd_slot={}",
        resources.model_id,
        resources.txd_slot
    );
    true
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

unsafe fn set_ped_model_clump(model_info: *mut c_void, clump: *mut c_void) {
    type SetClump = unsafe extern "thiscall" fn(*mut c_void, *mut c_void);
    let function: SetClump = unsafe { std::mem::transmute(ADDR_CPEDMODELINFO_SETCLUMP) };
    unsafe { function(model_info, clump) };
}

/// Calls CBaseModelInfo's virtual DeleteRwObject implementation. For a ped
/// model this is CPedModelInfo::DeleteRwObject, which destroys the source
/// RenderWare clump. CStreaming::RemoveModel uses this same vtable slot.
unsafe fn delete_model_rw_object(model_info: *mut c_void) -> bool {
    if model_info.is_null() {
        return false;
    }

    let rw_object_address = model_info as usize + MODEL_INFO_RW_OBJECT;
    let Some(rw_object): Option<*mut c_void> = (unsafe { memory::read(rw_object_address) }) else {
        return false;
    };
    if rw_object.is_null() {
        return true;
    }

    let Some(vtable): Option<usize> = (unsafe { memory::read(model_info as usize) }) else {
        return false;
    };
    let Some(function_address): Option<usize> =
        (unsafe { memory::read(vtable + VTABLE_DELETE_RW_OBJECT_OFFSET) })
    else {
        return false;
    };
    if function_address == 0 {
        return false;
    }

    type DeleteRwObject = unsafe extern "thiscall" fn(*mut c_void);
    let function: DeleteRwObject = unsafe { std::mem::transmute(function_address) };
    unsafe { function(model_info) };
    true
}

unsafe fn remove_txd_slot(txd_slot: i32, has_reference: bool) {
    if has_reference {
        // CTxdStore::RemoveRef destroys the dictionary when this was the last
        // reference. RemoveTxdSlot then releases the now-empty pool entry.
        unsafe { call_cdecl_1::<(), i32>(ADDR_CTXDSTORE_REMOVE_REF, txd_slot) };
    }
    unsafe { call_cdecl_1::<(), i32>(ADDR_CTXDSTORE_REMOVE_TXD_SLOT, txd_slot) };
}

unsafe fn load_dff_clump(txd_slot: i32, dff_path: &str) -> Option<*mut c_void> {
    let dff_path_c = match CString::new(dff_path) {
        Ok(path) => path,
        Err(_) => {
            log::error!("DFF path for TXD slot {txd_slot} contains a NUL byte: {dff_path:?}");
            return None;
        }
    };

    // DFF material texture names are resolved against the current TXD.
    unsafe { call_cdecl_0::<()>(ADDR_CTXDSTORE_PUSHCURRENTTXD) };
    unsafe { call_cdecl_1::<(), i32>(ADDR_CTXDSTORE_SETCURRENTTXD, txd_slot) };

    let stream: *mut c_void = unsafe {
        call_cdecl_3(
            ADDR_RWSTREAMOPEN,
            RWSTREAM_FILENAME,
            RWSTREAM_READ,
            dff_path_c.as_ptr(),
        )
    };

    if stream.is_null() {
        unsafe { call_cdecl_0::<()>(ADDR_CTXDSTORE_POPCURRENTTXD) };
        log::error!("could not open DFF for TXD slot {txd_slot}: {dff_path}");
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
        log::error!("DFF does not contain a RenderWare clump: {dff_path}");
        std::ptr::null_mut()
    } else {
        let clump: *mut c_void = unsafe { call_cdecl_1(ADDR_RPCLUMPSTREAMREAD, stream) };
        if clump.is_null() {
            log::error!("could not read RenderWare clump from DFF: {dff_path}");
        }
        clump
    };

    unsafe {
        let _: *mut c_void =
            call_cdecl_2(ADDR_RWSTREAMCLOSE, stream, std::ptr::null_mut::<c_void>());
        call_cdecl_0::<()>(ADDR_CTXDSTORE_POPCURRENTTXD);
    }

    (!clump.is_null()).then_some(clump)
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

        // CPedModelInfo owns a separate, generated hit-collision model. Do
        // not inherit the donor's pointer: SetClump creates one for this skin
        // and DeleteRwObject later destroys only that private allocation.
        *((destination as usize + PED_MODEL_INFO_HIT_COL_MODEL) as *mut *mut c_void) =
            std::ptr::null_mut();

        // CBaseModelInfo::m_nTxdIndex is a signed short at +0x0A.
        *((destination as usize + MODEL_INFO_TXD_INDEX) as *mut i16) = txd_slot as i16;
    }
}

unsafe fn get_model_info(model_id: i32) -> *mut c_void {
    if !(0..20_000).contains(&model_id) {
        return std::ptr::null_mut();
    }

    let Some(model_info_address) =
        ADDR_MS_MODEL_INFO_PTRS.checked_add(model_id as usize * std::mem::size_of::<*mut c_void>())
    else {
        return std::ptr::null_mut();
    };
    unsafe { memory::read(model_info_address).unwrap_or_default() }
}

unsafe fn find_free_model_id() -> Option<i32> {
    for model_id in PRIVATE_MODEL_ID_START..PRIVATE_MODEL_ID_END {
        let Some(model_info_address) = ADDR_MS_MODEL_INFO_PTRS
            .checked_add(model_id as usize * std::mem::size_of::<*mut c_void>())
        else {
            log::error!("private model address calculation overflowed");
            return None;
        };
        let Some(model_info): Option<*mut c_void> = (unsafe { memory::read(model_info_address) })
        else {
            log::error!("could not read GTA's model-info table while allocating a private model");
            return None;
        };
        if model_info.is_null() {
            return Some(model_id);
        }
    }

    log::error!(
        "no private model ID available in {PRIVATE_MODEL_ID_START}..{PRIVATE_MODEL_ID_END}"
    );
    None
}
