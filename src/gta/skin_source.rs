//! Shared RenderWare skin-source and per-ped clone lifecycle.

use super::*;
use std::ffi::{CString, c_void};
use std::path::Path;

/// Loads a TXD and raw source clump without allocating or mutating a GTA
/// model-info slot.
pub(super) fn load(
    _frame: &GameFrame,
    skin_id: &str,
    definition: &SkinDefinition,
) -> Option<SkinSourceResources> {
    if !Path::new(&definition.txd_path).is_file() {
        log::error!(
            "skin source {skin_id}: TXD file does not exist or is not a file: {}",
            definition.txd_path
        );
        return None;
    }
    if !Path::new(&definition.dff_path).is_file() {
        log::error!(
            "skin source {skin_id}: DFF file does not exist or is not a file: {}",
            definition.dff_path
        );
        return None;
    }

    let txd_path = match CString::new(definition.txd_path.as_str()) {
        Ok(path) => path,
        Err(_) => {
            log::error!(
                "skin source {skin_id}: TXD path contains a NUL byte: {:?}",
                definition.txd_path
            );
            return None;
        }
    };
    let txd_name = CString::new(format!("csl_i_{:016x}", stable_name_hash(skin_id)))
        .expect("hexadecimal TXD name cannot contain NUL");

    log::info!(
        "loading skin source {skin_id}: txd={}, dff={}",
        definition.txd_path,
        definition.dff_path
    );

    let txd_slot: i32 = unsafe { call_cdecl_1(ADDR_CTXDSTORE_ADD_TXD_SLOT, txd_name.as_ptr()) };
    if txd_slot < 0 {
        log::error!("skin source {skin_id}: could not allocate a TXD slot");
        return None;
    }

    let loaded: u8 = unsafe { call_cdecl_2(ADDR_CTXDSTORE_LOAD_TXD, txd_slot, txd_path.as_ptr()) };
    if loaded == 0 {
        log::error!(
            "skin source {skin_id}: could not load TXD from {} into slot {txd_slot}",
            definition.txd_path
        );
        unsafe { remove_txd_slot(txd_slot, false) };
        return None;
    }
    let _: *mut c_void = unsafe { call_cdecl_1(ADDR_CTXDSTORE_ADD_REF, txd_slot) };

    let source_clump = match unsafe { load_dff_clump(txd_slot, &definition.dff_path) } {
        Some(clump) => clump,
        None => {
            unsafe { remove_txd_slot(txd_slot, true) };
            return None;
        }
    };

    if let Err(reason) = unsafe { prepare_skin_source(source_clump) } {
        log::error!("skin source {skin_id}: incompatible DFF source: {reason}");
        if unsafe { destroy_clump(source_clump) } {
            unsafe { remove_txd_slot(txd_slot, true) };
        } else {
            log::error!(
                "skin source {skin_id}: could not destroy rejected source clump; retaining TXD slot {txd_slot} to avoid dangling textures"
            );
        }
        return None;
    }
    log::info!(
        "loaded skin source {skin_id}: txd_slot={}, txd={}, dff={}",
        txd_slot,
        definition.txd_path,
        definition.dff_path
    );
    Some(SkinSourceResources {
        txd_slot,
        source_clump: SourceClump {
            address: source_clump as usize,
        },
    })
}

/// Clones and installs a ped render object while preserving its model index.
/// Failures either leave the original clump untouched or recover GTA's normal
/// clump for `server_model_id` after a swap has started.
pub(super) fn apply(
    frame: &GameFrame,
    ped: &Ped,
    server_model_id: i16,
    resources: &SkinSourceResources,
) -> Result<PedRenderObject, &'static str> {
    if ped_model_id(ped) != Some(server_model_id) {
        return Err("ped model changed before the skin-source swap");
    }
    let model_info = unsafe { verified_ped_model_info(i32::from(server_model_id)) }?;
    let old_clump = ped_render_object(ped)
        .filter(|object| !object.is_null())
        .map(|object| object.address as *mut c_void)
        .ok_or("ped has no ordinary render object to replace")?;
    let old_frame_count = unsafe { anim_blend_frame_count(old_clump) }
        .map_err(|_| "ped's ordinary clump has invalid AnimBlend frame data")?;

    let source_clump = resources.source_clump.address as *mut c_void;
    log::debug!("skin-source swap: cloning the cached custom ped source");
    let clone = unsafe { clone_clump(source_clump) };
    if clone.is_null() {
        return Err("RpClumpClone returned null");
    }
    log::debug!(
        "skin-source swap: cloned and preparing the custom ped clump for server model {server_model_id}"
    );
    let prepared = match unsafe { prepare_skin_clone(clone, model_info, ped) } {
        Ok(prepared) => prepared,
        Err(reason) => {
            if !unsafe { destroy_clump(clone) } {
                log::error!("could not destroy an unattached skin-source clone after: {reason}");
            }
            return Err(reason);
        }
    };
    let installed = match unsafe { render_object_identity(prepared.address) } {
        Some(identity) => identity,
        None => {
            if !unsafe { destroy_clump(prepared.address) } {
                log::error!(
                    "could not destroy an unattached skin-source clone after its geometry identity could not be read"
                );
            }
            return Err("could not identify the prepared skin-source clone geometry");
        }
    };
    if prepared.frame_count != old_frame_count {
        if !unsafe { destroy_clump(prepared.address) } {
            log::error!(
                "could not destroy an unattached skin-source clone after an AnimBlend frame-count mismatch"
            );
        }
        return Err("custom DFF bone count differs from the live ped skeleton");
    }
    // GTA uses this exact transfer pair while rebuilding CJ's clothes. Moving
    // the list before destruction preserves the current walk/weapon/task
    // animations instead of replacing them with a fresh idle association.
    let associations = unsafe { extract_anim_associations(old_clump) };
    if associations.is_empty() {
        if !unsafe { destroy_clump(prepared.address) } {
            log::error!(
                "could not destroy an unattached skin-source clone after finding no live animation associations"
            );
        }
        return Err("ped's ordinary clump has no live animation associations");
    }
    unsafe { give_anim_associations(prepared.address, associations) };
    if let Err(reason) = unsafe { abort_secondary_ik(ped) } {
        unsafe { return_associations(prepared.address, old_clump) };
        if !unsafe { destroy_clump(prepared.address) } {
            log::error!("could not destroy an unattached skin-source clone after: {reason}");
        }
        return Err(reason);
    }

    log::debug!("skin-source swap: prepared clone; deleting the ordinary ped render object");
    if let Err(reason) = unsafe { delete_entity_rw_object(ped) } {
        unsafe { return_associations(prepared.address, old_clump) };
        if !unsafe { destroy_clump(prepared.address) } {
            log::error!("could not destroy an unattached skin-source clone after: {reason}");
        }
        return Err(reason);
    }
    log::debug!("skin-source swap: ordinary render object deleted; restoring entity bookkeeping");

    // DeleteRwObject correctly releases the server model reference, streaming
    // link, and effects. Recreate them through CEntity, then discard only its
    // temporary ordinary clump so the custom object inherits valid entity
    // bookkeeping without ever owning or changing a model ID.
    if let Err(reason) = unsafe { create_entity_rw_object(ped) } {
        unsafe {
            recover_server_clump_after_failed_swap(
                frame,
                ped,
                server_model_id,
                prepared.address,
                reason,
            )
        };
        return Err(reason);
    }
    log::debug!("skin-source swap: entity bookkeeping restored; replacing the temporary clump");
    let temporary_clump = match ped_render_object(ped)
        .filter(|object| !object.is_null())
        .map(|object| object.address as *mut c_void)
    {
        Some(clump) => clump,
        None => {
            let reason = "CEntity::CreateRwObject did not install a temporary server clump";
            unsafe {
                recover_server_clump_after_failed_swap(
                    frame,
                    ped,
                    server_model_id,
                    prepared.address,
                    reason,
                )
            };
            return Err(reason);
        }
    };
    if let Err(reason) = unsafe { position_skin_clone(temporary_clump, prepared.address) } {
        unsafe {
            recover_server_clump_after_failed_swap(
                frame,
                ped,
                server_model_id,
                prepared.address,
                reason,
            )
        };
        return Err(reason);
    }
    if !unsafe { destroy_clump(temporary_clump) } {
        let reason = "could not destroy the temporary server clump";
        unsafe {
            recover_server_clump_after_failed_swap(
                frame,
                ped,
                server_model_id,
                prepared.address,
                reason,
            )
        };
        return Err(reason);
    }

    unsafe {
        let render_object = (ped.address as usize + ENTITY_RW_OBJECT) as *mut *mut c_void;
        *render_object = std::ptr::null_mut();
        *render_object = prepared.address;
        std::ptr::copy_nonoverlapping(
            prepared.bone_frames.as_ptr(),
            (ped.address as usize + PED_BONE_ARRAY) as *mut *mut c_void,
            PED_BONE_COUNT,
        );
    }
    if ped_render_object(ped) != Some(installed) {
        let _ = restore(frame, ped, server_model_id, installed);
        return Err("ped render-object replacement could not be verified");
    }
    if ped_model_id(ped) != Some(server_model_id) {
        let _ = restore(frame, ped, server_model_id, installed);
        return Err("skin-source replacement changed the ped model index");
    }

    unsafe {
        update_rw_frame(ped);
        update_rp_hanim(ped);
    }
    log::debug!("skin-source swap: installed and updated the custom ped clump");

    Ok(installed)
}

/// Removes a matching custom clone through the ped's virtual entity
/// lifecycle and lets CPed rebuild the remembered server model. Animation
/// associations are transferred exactly as GTA does for a clothing rebuild.
pub(super) fn restore(
    frame: &GameFrame,
    ped: &Ped,
    server_model_id: i16,
    installed: PedRenderObject,
) -> Result<(), &'static str> {
    let current = ped_render_object(ped).ok_or("could not read the ped render object")?;
    if current != installed {
        return Err("the installed skin-source clone was already replaced");
    }
    let clump = current.address as *mut c_void;
    let associations = match unsafe { anim_blend_data(clump) } {
        Ok(_) => unsafe { extract_anim_associations(clump) },
        Err(_) => AnimAssociations {
            address: std::ptr::null_mut(),
        },
    };
    if let Err(reason) = unsafe { abort_secondary_ik(ped) } {
        if !associations.is_empty() {
            unsafe { give_anim_associations(clump, associations) };
        }
        return Err(reason);
    }
    if let Err(reason) = unsafe { delete_entity_rw_object(ped) } {
        if !associations.is_empty() {
            unsafe { give_anim_associations(clump, associations) };
        }
        return Err(reason);
    }

    set_ped_model_index(frame, ped, server_model_id as i32);
    let rebuilt = ped_render_object(ped)
        .filter(|object| !object.is_null() && *object != installed)
        .ok_or("CPed::SetModelIndex did not rebuild a normal render object")?;
    if !associations.is_empty() {
        unsafe { give_anim_associations(rebuilt.address as *mut c_void, associations) };
    }
    if ped_model_id(ped) != Some(server_model_id) {
        return Err("CPed::SetModelIndex did not restore the remembered server model");
    }
    Ok(())
}

pub(super) fn release(_frame: &GameFrame, skin_id: &str, resources: &SkinSourceResources) -> bool {
    let source_clump = resources.source_clump.address as *mut c_void;
    if !unsafe { destroy_clump(source_clump) } {
        log::error!("skin source {skin_id}: could not destroy retired source clump");
        return false;
    }

    unsafe { remove_txd_slot(resources.txd_slot, true) };
    log::info!(
        "cleaned retired skin source {skin_id}: txd_slot={}",
        resources.txd_slot
    );
    true
}
