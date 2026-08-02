use crate::config::SkinDefinition;
use crate::game_frame::GameFrame;
use crate::memory;
use crate::model_ids::is_valid_model_id;
use std::ffi::{CString, c_void};

#[path = "gta/skin_source.rs"]
mod skin_source;

// GTA SA 1.0 US (Hoodlum), 32-bit only.
const ADDR_CGAME_PROCESS: usize = 0x53BEE0;
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
const ADDR_CPED_DELETE_RW_OBJECT: usize = 0x5DEBF0;
const ADDR_CENTITY_CREATE_RW_OBJECT: usize = 0x533D30;
const ADDR_CTASKSIMPLEIKMANAGER_MAKE_ABORTABLE: usize = 0x6338A0;
const ADDR_CENTITY_UPDATE_RW_FRAME: usize = 0x532B00;
const ADDR_CENTITY_UPDATE_RP_HANIM: usize = 0x532B20;
const ADDR_CCLUMPMODELINFO_SET_HIERARCHY_FOR_SKIN_ATOMIC: usize = 0x4C4EF0;
const ADDR_CCLUMPMODELINFO_ATOMIC_SETUP_LIGHTING_CB: usize = 0x4C4F30;
const ADDR_CCLUMPMODELINFO_SET_ATOMIC_RENDERER_CB: usize = 0x4C5280;
const ADDR_CVISIBILITYPLUGINS_RENDER_PED_CB: usize = 0x7335B0;
const ADDR_CVISIBILITYPLUGINS_SET_CLUMP_MODEL_INFO: usize = 0x733750;
const ADDR_IS_CLUMP_SKINNED: usize = 0x4C4DC0;
const ADDR_GET_ANIM_HIERARCHY_FROM_CLUMP: usize = 0x734B10;
const ADDR_GET_ANIM_HIERARCHY_FROM_SKIN_CLUMP: usize = 0x734A40;
const ADDR_RPANIMBLEND_CREATE_ANIMATION_FOR_HIERARCHY: usize = 0x4D60E0;
const ADDR_RPANIMBLEND_CLUMP_INIT: usize = 0x4D6720;
const ADDR_RPANIMBLEND_CLUMP_FILL_FRAME_ARRAY: usize = 0x4D64A0;
const ADDR_RPANIMBLEND_CLUMP_EXTRACT_ASSOCIATIONS: usize = 0x4D6BE0;
const ADDR_RPANIMBLEND_CLUMP_GIVE_ASSOCIATIONS: usize = 0x4D6C30;
const ADDR_RPCLUMP_CLONE: usize = 0x749F70;
const ADDR_RPCLUMP_DESTROY: usize = 0x74A310;
const ADDR_RPCLUMP_FOR_ALL_ATOMICS: usize = 0x749B70;
const ADDR_GET_FIRST_ATOMIC: usize = 0x734820;
const ADDR_RPSKIN_GEOMETRY_GET_SKIN: usize = 0x7C7550;
const ADDR_RPSKIN_GET_VERTEX_BONE_WEIGHTS: usize = 0x7C77F0;
const ADDR_RPHANIM_ID_GET_INDEX: usize = 0x7C51A0;
const ADDR_RTANIM_INTERPOLATOR_SET_CURRENT_ANIM: usize = 0x7CD5A0;
const ADDR_RTANIM_ANIMATION_DESTROY: usize = 0x7CCF10;
const ADDR_RWFRAME_TRANSFORM: usize = 0x7F0F70;

// Set by RpAnimBlendPluginAttach during GTA startup. The value is the runtime
// extension offset of CAnimBlendClumpData* inside an RpClump.
const ADDR_RPANIMBLEND_CLUMP_OFFSET: usize = 0xB5F878;

// CBaseModelInfo and CEntity offsets in GTA SA 1.0 US.
const ENTITY_MODEL_INDEX: usize = 0x22;
const ENTITY_RW_OBJECT: usize = 0x18;
const VTABLE_DELETE_RW_OBJECT_OFFSET: usize = 0x20;
const PED_INTELLIGENCE: usize = 0x47C;
const PED_INTELLIGENCE_SECONDARY_IK_TASK: usize = 0x2C;
const CTASKSIMPLEIKMANAGER_VTABLE: usize = 0x86E358;
const ABORT_PRIORITY_IMMEDIATE: i32 = 2;
const RW_OBJECT_PARENT: usize = 0x04;
const RW_FRAME_MODELLING_MATRIX: usize = 0x10;
const RW_MATRIX_SIZE: usize = 0x40;
const RW_COMBINE_REPLACE: i32 = 0;
const RP_ATOMIC_GEOMETRY: usize = 0x18;
const RP_GEOMETRY_NUM_VERTICES: usize = 0x14;
const RP_GEOMETRY_MORPH_TARGET: usize = 0x5C;
const RP_MORPH_TARGET_BOUNDING_SPHERE_RADIUS: usize = 0x10;
const RW_MATRIX_WEIGHTS_SIZE: usize = 0x10;
const MAX_PED_GEOMETRY_VERTICES: i32 = 100_000;
const PED_BONE_ARRAY: usize = 0x488;
const PED_ANIM_MOVING_SHIFT_LOCAL: usize = 0x4D8;
const PED_BONE_COUNT: usize = 19;
const HANIM_HIERARCHY_FLAGS: usize = 0x00;
const HANIM_HIERARCHY_NODE_COUNT: usize = 0x04;
const HANIM_HIERARCHY_CURRENT_ANIM: usize = 0x20;
const ANIM_BLEND_DATA_FRAME_COUNT: usize = 0x08;
const ANIM_BLEND_DATA_PED_POSITION: usize = 0x0C;
const HANIM_UPDATE_BOTH_MATRICES: i32 = 0x3000;

// ConvertPedNode2BoneTag for CPed::m_apBones[1..19]. Validating these tags
// before RpAnimBlendClumpFillFrameArray prevents its unchecked index writes
// from accepting a merely-present but incompatible hierarchy.
const REQUIRED_PED_BONE_TAGS: [i32; PED_BONE_COUNT - 1] = [
    3, 5, 32, 22, 34, 24, 41, 51, 43, 53, 52, 42, 33, 23, 31, 21, 4, 8,
];

// RenderWare enums/chunk ID.
const RWSTREAM_FILENAME: i32 = 2;
const RWSTREAM_READ: i32 = 1;
const RW_ID_CLUMP: u32 = 0x10;

// Model 7 is a vanilla CPedModelInfo in the supported GTA SA 1.0 US build.
// Its vtable is used to distinguish ped model infos from vehicles and objects.
const KNOWN_PED_MODEL_ID: i32 = 7;

struct ExecutableSignature {
    name: &'static str,
    address: usize,
    expected: &'static [u8],
}

// These signatures cover the PE header and every fixed code target used by
// this module or Runtime. They identify the GTA SA 1.0 US executable Wardrobe
// was written for and also reject targets another ASI has already patched.
const EXECUTABLE_SIGNATURES: &[ExecutableSignature] = &[
    ExecutableSignature {
        name: "GTA SA 1.0 US PE header",
        address: 0x0040_0080,
        expected: &[
            0x50, 0x45, 0x00, 0x00, 0x4C, 0x01, 0x0B, 0x00, 0xCA, 0x01, 0x71, 0x42,
        ],
    },
    ExecutableSignature {
        name: "CGame::Process",
        address: ADDR_CGAME_PROCESS,
        expected: &[
            0x83, 0xEC, 0x0C, 0x53, 0x56, 0x57, 0xE8, 0xE5, 0x5E, 0x00, 0x00, 0xB9, 0x78, 0x29,
            0xB7, 0x00,
        ],
    },
    ExecutableSignature {
        name: "CPed::SetModelIndex",
        address: ADDR_CPED_SET_MODEL_INDEX,
        expected: &[
            0x56, 0x8B, 0xF1, 0x81, 0x4E, 0x1C, 0x80, 0x00, 0x00, 0x00, 0x8B, 0x44, 0x24, 0x08,
            0x57, 0x50,
        ],
    },
    ExecutableSignature {
        name: "CPed::DeleteRwObject",
        address: ADDR_CPED_DELETE_RW_OBJECT,
        expected: &[
            0xE9, 0x3B, 0x54, 0xF5, 0xFF, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
            0x90, 0x90,
        ],
    },
    ExecutableSignature {
        name: "CEntity::CreateRwObject",
        address: ADDR_CENTITY_CREATE_RW_OBJECT,
        expected: &[
            0x56, 0x8B, 0xF1, 0x8B, 0x46, 0x1C, 0x84, 0xC0, 0x0F, 0x89, 0x8E, 0x01, 0x00, 0x00,
            0xF6, 0xC4,
        ],
    },
    ExecutableSignature {
        name: "CTaskSimpleIKManager::MakeAbortable",
        address: ADDR_CTASKSIMPLEIKMANAGER_MAKE_ABORTABLE,
        expected: &[
            0x83, 0x7C, 0x24, 0x08, 0x02, 0x75, 0x29, 0x56, 0x57, 0x8D, 0x71, 0x08, 0xBF, 0x04,
            0x00, 0x00,
        ],
    },
    ExecutableSignature {
        name: "CEntity::UpdateRwFrame",
        address: ADDR_CENTITY_UPDATE_RW_FRAME,
        expected: &[
            0x8B, 0x41, 0x18, 0x85, 0xC0, 0x74, 0x0A, 0x8B, 0x40, 0x04, 0x50, 0xE8, 0x00, 0xDE,
            0x2B, 0x00,
        ],
    },
    ExecutableSignature {
        name: "CEntity::UpdateRpHAnim",
        address: ADDR_CENTITY_UPDATE_RP_HANIM,
        expected: &[
            0x56, 0x8B, 0xF1, 0x8B, 0x46, 0x18, 0x50, 0xE8, 0xF4, 0x1C, 0x20, 0x00, 0x83, 0xC4,
            0x04, 0x85,
        ],
    },
    ExecutableSignature {
        name: "CVisibilityPlugins::SetClumpModelInfo",
        address: ADDR_CVISIBILITYPLUGINS_SET_CLUMP_MODEL_INFO,
        expected: &[
            0x8B, 0x0D, 0x90, 0x60, 0x8D, 0x00, 0x56, 0x8B, 0x74, 0x24, 0x0C, 0x57, 0x8B, 0x7C,
            0x24, 0x0C,
        ],
    },
    ExecutableSignature {
        name: "CClumpModelInfo::SetHierarchyForSkinAtomic",
        address: ADDR_CCLUMPMODELINFO_SET_HIERARCHY_FOR_SKIN_ATOMIC,
        expected: &[
            0x8B, 0x44, 0x24, 0x08, 0x85, 0xC0, 0x74, 0x11, 0x50, 0x8B, 0x44, 0x24, 0x08, 0x50,
            0xE8, 0x1D,
        ],
    },
    ExecutableSignature {
        name: "CClumpModelInfo::AtomicSetupLightingCB",
        address: ADDR_CCLUMPMODELINFO_ATOMIC_SETUP_LIGHTING_CB,
        expected: &[
            0x56, 0x8B, 0x74, 0x24, 0x08, 0x56, 0xE8, 0x05, 0x30, 0x11, 0x00, 0x83, 0xC4, 0x04,
            0x85, 0xC0,
        ],
    },
    ExecutableSignature {
        name: "CClumpModelInfo::SetAtomicRendererCB",
        address: ADDR_CCLUMPMODELINFO_SET_ATOMIC_RENDERER_CB,
        expected: &[
            0x8B, 0x44, 0x24, 0x08, 0x56, 0x8B, 0x74, 0x24, 0x08, 0x50, 0x56, 0xE8, 0x10, 0xD6,
            0x26, 0x00,
        ],
    },
    ExecutableSignature {
        name: "CVisibilityPlugins::RenderPedCB",
        address: ADDR_CVISIBILITYPLUGINS_RENDER_PED_CB,
        expected: &[
            0x56, 0x57, 0x8B, 0x7C, 0x24, 0x0C, 0x8B, 0x77, 0x3C, 0x8B, 0x46, 0x04, 0x50, 0xE8,
            0xCE, 0xD3,
        ],
    },
    ExecutableSignature {
        name: "IsClumpSkinned",
        address: ADDR_IS_CLUMP_SKINNED,
        expected: &[
            0x8B, 0x44, 0x24, 0x04, 0x50, 0xE8, 0x56, 0xFA, 0x26, 0x00, 0x83, 0xC4, 0x04, 0x85,
            0xC0, 0x74,
        ],
    },
    ExecutableSignature {
        name: "GetAnimHierarchyFromClump",
        address: ADDR_GET_ANIM_HIERARCHY_FROM_CLUMP,
        expected: &[
            0x8B, 0x44, 0x24, 0x04, 0x8B, 0x48, 0x04, 0x89, 0x4C, 0x24, 0x04, 0xE9, 0x90, 0xFF,
            0xFF, 0xFF,
        ],
    },
    ExecutableSignature {
        name: "GetAnimHierarchyFromSkinClump",
        address: ADDR_GET_ANIM_HIERARCHY_FROM_SKIN_CLUMP,
        expected: &[
            0x51, 0x8B, 0x4C, 0x24, 0x08, 0x8D, 0x44, 0x24, 0x00, 0x50, 0x68, 0x20, 0x4A, 0x73,
            0x00, 0x51,
        ],
    },
    ExecutableSignature {
        name: "RpAnimBlendCreateAnimationForHierarchy",
        address: ADDR_RPANIMBLEND_CREATE_ANIMATION_FOR_HIERARCHY,
        expected: &[
            0x8B, 0x44, 0x24, 0x04, 0x85, 0xC0, 0x75, 0x01, 0xC3, 0x56, 0x8B, 0x70, 0x04, 0x6A,
            0x00, 0x6A,
        ],
    },
    ExecutableSignature {
        name: "RpAnimBlendClumpInit",
        address: ADDR_RPANIMBLEND_CLUMP_INIT,
        expected: &[
            0x56, 0x8B, 0x74, 0x24, 0x08, 0x56, 0xE8, 0xF5, 0xE0, 0x25, 0x00, 0x83, 0xC4, 0x04,
            0x85, 0xC0,
        ],
    },
    ExecutableSignature {
        name: "RpAnimBlendClumpFillFrameArray",
        address: ADDR_RPANIMBLEND_CLUMP_FILL_FRAME_ARRAY,
        expected: &[
            0xA1, 0x78, 0xF8, 0xB5, 0x00, 0x56, 0x8B, 0x74, 0x24, 0x08, 0x57, 0x8B, 0x3C, 0x30,
            0x56, 0xE8,
        ],
    },
    ExecutableSignature {
        name: "RpAnimBlendClumpExtractAssociations",
        address: ADDR_RPANIMBLEND_CLUMP_EXTRACT_ASSOCIATIONS,
        expected: &[
            0x8B, 0x0D, 0x78, 0xF8, 0xB5, 0x00, 0x8B, 0x44, 0x24, 0x04, 0x8B, 0x04, 0x01, 0x8B,
            0x08, 0xC7,
        ],
    },
    ExecutableSignature {
        name: "RpAnimBlendClumpGiveAssociations",
        address: ADDR_RPANIMBLEND_CLUMP_GIVE_ASSOCIATIONS,
        expected: &[
            0x8B, 0x44, 0x24, 0x04, 0x8B, 0x0D, 0x78, 0xF8, 0xB5, 0x00, 0x57, 0x8B, 0x3C, 0x01,
            0x8B, 0x07,
        ],
    },
    ExecutableSignature {
        name: "RpClumpClone",
        address: ADDR_RPCLUMP_CLONE,
        expected: &[
            0x83, 0xEC, 0x14, 0x53, 0x55, 0x56, 0x57, 0xE8, 0x14, 0x03, 0x00, 0x00, 0x8B, 0xE8,
            0x85, 0xED,
        ],
    },
    ExecutableSignature {
        name: "RpClumpDestroy",
        address: ADDR_RPCLUMP_DESTROY,
        expected: &[
            0x53, 0x55, 0x56, 0x8B, 0x74, 0x24, 0x10, 0x57, 0x56, 0x68, 0x64, 0x62, 0x8D, 0x00,
            0xE8, 0x1D,
        ],
    },
    ExecutableSignature {
        name: "RpClumpForAllAtomics",
        address: ADDR_RPCLUMP_FOR_ALL_ATOMICS,
        expected: &[
            0x8B, 0x44, 0x24, 0x04, 0x53, 0x55, 0x56, 0x57, 0x8D, 0x78, 0x08, 0x8B, 0x40, 0x08,
            0x3B, 0xC7,
        ],
    },
    ExecutableSignature {
        name: "GetFirstAtomic",
        address: ADDR_GET_FIRST_ATOMIC,
        expected: &[
            0x51, 0x8B, 0x4C, 0x24, 0x08, 0x8D, 0x44, 0x24, 0x00, 0x50, 0x68, 0x10, 0x48, 0x73,
            0x00, 0x51,
        ],
    },
    ExecutableSignature {
        name: "RpSkinGeometryGetSkin",
        address: ADDR_RPSKIN_GEOMETRY_GET_SKIN,
        expected: &[
            0x8B, 0x44, 0x24, 0x04, 0x8B, 0x0D, 0xA8, 0x78, 0xC9, 0x00, 0x8B, 0x04, 0x01, 0xC3,
            0x90, 0x90,
        ],
    },
    ExecutableSignature {
        name: "RpSkinGetVertexBoneWeights",
        address: ADDR_RPSKIN_GET_VERTEX_BONE_WEIGHTS,
        expected: &[
            0x8B, 0x44, 0x24, 0x04, 0x8B, 0x40, 0x18, 0xC3, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
            0x90, 0x90,
        ],
    },
    ExecutableSignature {
        name: "RpHAnimIDGetIndex",
        address: ADDR_RPHANIM_ID_GET_INDEX,
        expected: &[
            0x8B, 0x54, 0x24, 0x04, 0x56, 0x83, 0xC8, 0xFF, 0x8B, 0x72, 0x10, 0x8B, 0x52, 0x04,
            0x33, 0xC9,
        ],
    },
    ExecutableSignature {
        name: "RtAnimInterpolatorSetCurrentAnim",
        address: ADDR_RTANIM_INTERPOLATOR_SET_CURRENT_ANIM,
        expected: &[
            0x53, 0x8B, 0x5C, 0x24, 0x0C, 0x55, 0x56, 0x8B, 0x74, 0x24, 0x10, 0x57, 0x33, 0xFF,
            0x89, 0x1E,
        ],
    },
    ExecutableSignature {
        name: "RtAnimAnimationDestroy",
        address: ADDR_RTANIM_ANIMATION_DESTROY,
        expected: &[
            0x8B, 0x44, 0x24, 0x04, 0x8B, 0x0D, 0x24, 0x7B, 0xC9, 0x00, 0x50, 0xFF, 0x91, 0x38,
            0x01, 0x00,
        ],
    },
    ExecutableSignature {
        name: "RwFrameTransform",
        address: ADDR_RWFRAME_TRANSFORM,
        expected: &[
            0x8B, 0x44, 0x24, 0x0C, 0x8B, 0x4C, 0x24, 0x08, 0x56, 0x8B, 0x74, 0x24, 0x08, 0x50,
            0x51, 0x8D,
        ],
    },
    ExecutableSignature {
        name: "RwStreamOpen",
        address: ADDR_RWSTREAMOPEN,
        expected: &[
            0xA1, 0x24, 0x7B, 0xC9, 0x00, 0x8B, 0x0D, 0x2C, 0x79, 0xC9, 0x00, 0x83, 0xEC, 0x20,
            0x8B, 0x14,
        ],
    },
    ExecutableSignature {
        name: "RwStreamFindChunk",
        address: ADDR_RWSTREAMFINDCHUNK,
        expected: &[
            0x83, 0xEC, 0x0C, 0x8D, 0x44, 0x24, 0x04, 0x8D, 0x4C, 0x24, 0x10, 0x8D, 0x54, 0x24,
            0x00, 0x56,
        ],
    },
    ExecutableSignature {
        name: "RpClumpStreamRead",
        address: ADDR_RPCLUMPSTREAMREAD,
        expected: &[
            0x83, 0xEC, 0x44, 0x8D, 0x44, 0x24, 0x04, 0x8D, 0x4C, 0x24, 0x0C, 0x53, 0x55, 0x56,
            0x57, 0x8B,
        ],
    },
    ExecutableSignature {
        name: "RwStreamClose",
        address: ADDR_RWSTREAMCLOSE,
        expected: &[
            0x83, 0xEC, 0x08, 0x56, 0x8B, 0x74, 0x24, 0x10, 0x57, 0x8B, 0x06, 0x48, 0x83, 0xF8,
            0x03, 0x0F,
        ],
    },
    ExecutableSignature {
        name: "CTxdStore::AddTxdSlot",
        address: ADDR_CTXDSTORE_ADD_TXD_SLOT,
        expected: &[
            0x8B, 0x0D, 0x0C, 0x80, 0xC8, 0x00, 0x56, 0xE8, 0xF4, 0xFE, 0xFF, 0xFF, 0x8B, 0xF0,
            0x8B, 0x44,
        ],
    },
    ExecutableSignature {
        name: "CTxdStore::LoadTxd",
        address: ADDR_CTXDSTORE_LOAD_TXD,
        expected: &[
            0x8B, 0x44, 0x24, 0x08, 0x90, 0xE9, 0xA7, 0x00, 0xCD, 0xFF, 0x50, 0x8D, 0x4C, 0x24,
            0x04, 0x68,
        ],
    },
    ExecutableSignature {
        name: "CTxdStore::AddRef",
        address: ADDR_CTXDSTORE_ADD_REF,
        expected: &[
            0x8B, 0x0D, 0x0C, 0x80, 0xC8, 0x00, 0x8B, 0x51, 0x04, 0x8B, 0x44, 0x24, 0x04, 0x80,
            0x3C, 0x10,
        ],
    },
    ExecutableSignature {
        name: "CTxdStore::RemoveRef",
        address: ADDR_CTXDSTORE_REMOVE_REF,
        expected: &[
            0xA1, 0x0C, 0x80, 0xC8, 0x00, 0x8B, 0x50, 0x04, 0x8B, 0x4C, 0x24, 0x04, 0x80, 0x3C,
            0x11, 0x00,
        ],
    },
    ExecutableSignature {
        name: "CTxdStore::RemoveTxdSlot",
        address: ADDR_CTXDSTORE_REMOVE_TXD_SLOT,
        expected: &[
            0x8B, 0x0D, 0x0C, 0x80, 0xC8, 0x00, 0x8B, 0x41, 0x04, 0x53, 0x56, 0x8B, 0x74, 0x24,
            0x0C, 0x80,
        ],
    },
    ExecutableSignature {
        name: "CTxdStore::PushCurrentTxd",
        address: ADDR_CTXDSTORE_PUSHCURRENTTXD,
        expected: &[
            0xE8, 0xEB, 0x23, 0x0C, 0x00, 0xE9, 0xCE, 0x04, 0xCD, 0xFF, 0xC3, 0x90, 0x90, 0x90,
            0x90, 0x90,
        ],
    },
    ExecutableSignature {
        name: "CTxdStore::PopCurrentTxd",
        address: ADDR_CTXDSTORE_POPCURRENTTXD,
        expected: &[
            0xA1, 0x10, 0x80, 0xC8, 0x00, 0x50, 0xE8, 0xB5, 0x23, 0x0C, 0x00, 0x83, 0xC4, 0x04,
            0xC7, 0x05,
        ],
    },
    ExecutableSignature {
        name: "CTxdStore::SetCurrentTxd",
        address: ADDR_CTXDSTORE_SETCURRENTTXD,
        expected: &[
            0x8B, 0x0D, 0x0C, 0x80, 0xC8, 0x00, 0x8B, 0x51, 0x04, 0x8B, 0x44, 0x24, 0x04, 0x80,
            0x3C, 0x10,
        ],
    },
];

/// Prepared TXD/source-clump resources shared by every ped using a profile.
/// The raw RenderWare objects remain opaque outside this module.
#[derive(Debug)]
pub struct SkinSourceResources {
    txd_slot: i32,
    source_clump: SourceClump,
}

#[derive(Debug)]
struct SourceClump {
    address: usize,
}

struct PreparedSkinClump {
    address: *mut c_void,
    bone_frames: [*mut c_void; PED_BONE_COUNT],
    frame_count: u32,
}

#[derive(Clone, Copy)]
struct AnimAssociations {
    address: *mut c_void,
}

impl AnimAssociations {
    const fn is_empty(self) -> bool {
        self.address.is_null()
    }
}

/// The exact RenderWare object Wardrobe installed on a ped. Runtime may compare
/// handles for reset detection, but cannot dereference or destroy them.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PedRenderObject {
    address: usize,
    geometry: usize,
}

impl PedRenderObject {
    pub(crate) const fn is_null(self) -> bool {
        self.address == 0
    }

    pub(crate) const fn has_same_address(self, other: Self) -> bool {
        self.address == other.address
    }

    #[cfg(test)]
    pub(crate) const fn for_test(address: usize, geometry: usize) -> Self {
        Self { address, geometry }
    }
}

/// A GTA `CPed` pointer obtained from a successful SA-MP scan. It is opaque so
/// only the scanner can introduce a raw game pointer into the safe runtime API.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ped {
    address: *mut c_void,
}

impl Ped {
    /// # Safety
    ///
    /// `address` must point to a live GTA `CPed` for the current game frame.
    /// SA-MP's ped scanner establishes this after reading the active player
    /// structures through `ReadProcessMemory`.
    pub(crate) unsafe fn from_samp(address: *mut c_void) -> Self {
        Self { address }
    }
}

/// Rejects every GTA executable or code-patched installation that does not
/// match the exact 1.0 US code addresses used below. This performs only safe
/// process-memory reads; callers must run it before creating the frame detour
/// or invoking a fixed GTA/RenderWare address.
pub fn validate_executable() -> Result<(), String> {
    for signature in EXECUTABLE_SIGNATURES {
        let actual =
            memory::read_bytes(signature.address, signature.expected.len()).ok_or_else(|| {
                format!(
                    "unsupported GTA executable: could not read {} at 0x{:08X}",
                    signature.name, signature.address
                )
            })?;
        if actual != signature.expected {
            return Err(format!(
                "unsupported GTA executable or modified target: {} differs at 0x{:08X} (expected {}, found {})",
                signature.name,
                signature.address,
                hex(signature.expected),
                hex(&actual)
            ));
        }
    }
    Ok(())
}

pub const fn cgame_process_address() -> usize {
    ADDR_CGAME_PROCESS
}

pub fn is_ready() -> bool {
    memory::read::<usize>(ADDR_MS_P_TXD_POOL).is_some_and(|pool| pool != 0)
}

pub fn ped_model_id(ped: &Ped) -> Option<i16> {
    let model_index_address = (ped.address as usize).checked_add(ENTITY_MODEL_INDEX)?;
    memory::read(model_index_address)
}

pub fn ped_render_object(ped: &Ped) -> Option<PedRenderObject> {
    let render_object_address = (ped.address as usize).checked_add(ENTITY_RW_OBJECT)?;
    let render_object: *mut c_void = memory::read(render_object_address)?;
    unsafe { render_object_identity(render_object) }
}

pub fn set_ped_model_index(_frame: &GameFrame, ped: &Ped, model_id: i32) {
    type SetModelIndex = unsafe extern "thiscall" fn(*mut c_void, i32);
    let function: SetModelIndex = unsafe { std::mem::transmute(ADDR_CPED_SET_MODEL_INDEX) };
    unsafe { function(ped.address, model_id) };
}

/// Loads one TXD and prepared source clump without allocating or mutating a
/// GTA model-info slot.
pub fn load_skin_source(
    frame: &GameFrame,
    skin_id: &str,
    definition: &SkinDefinition,
) -> Option<SkinSourceResources> {
    skin_source::load(frame, skin_id, definition)
}

/// Clones and installs a custom render object while preserving the ped's model
/// index. Failures either leave the original clump untouched or recover GTA's
/// ordinary clump for `server_model_id` after a swap has started.
pub fn apply_skin_source(
    frame: &GameFrame,
    ped: &Ped,
    server_model_id: i16,
    resources: &SkinSourceResources,
) -> Result<PedRenderObject, &'static str> {
    skin_source::apply(frame, ped, server_model_id, resources)
}

/// Removes a matching custom clump through the ped's virtual entity lifecycle
/// and lets CPed rebuild the remembered server model. Animation associations
/// are transferred exactly as GTA does for a clothing rebuild.
pub fn restore_skin_source(
    frame: &GameFrame,
    ped: &Ped,
    server_model_id: i16,
    installed: PedRenderObject,
) -> Result<(), &'static str> {
    skin_source::restore(frame, ped, server_model_id, installed)
}

pub fn release_skin_source_resources(
    frame: &GameFrame,
    skin_id: &str,
    resources: &SkinSourceResources,
) -> bool {
    skin_source::release(frame, skin_id, resources)
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

unsafe fn clone_clump(clump: *mut c_void) -> *mut c_void {
    unsafe { call_cdecl_1(ADDR_RPCLUMP_CLONE, clump) }
}

unsafe fn render_object_identity(clump: *mut c_void) -> Option<PedRenderObject> {
    if clump.is_null() {
        return Some(PedRenderObject {
            address: 0,
            geometry: 0,
        });
    }
    let atomic: *mut c_void = unsafe { call_cdecl_1(ADDR_GET_FIRST_ATOMIC, clump) };
    if atomic.is_null() {
        return None;
    }
    let geometry: *mut c_void = memory::read(atomic as usize + RP_ATOMIC_GEOMETRY)?;
    if geometry.is_null() {
        return None;
    }
    Some(PedRenderObject {
        address: clump as usize,
        geometry: geometry as usize,
    })
}

unsafe fn destroy_clump(clump: *mut c_void) -> bool {
    let destroyed: i32 = unsafe { call_cdecl_1(ADDR_RPCLUMP_DESTROY, clump) };
    destroyed != 0
}

unsafe fn prepare_skin_source(clump: *mut c_void) -> Result<(), &'static str> {
    unsafe { validated_ped_hierarchy(clump) }?;
    unsafe { prepare_skin_geometry(clump) }?;
    Ok(())
}

unsafe fn prepare_skin_clone(
    clump: *mut c_void,
    model_info: *mut c_void,
    ped: &Ped,
) -> Result<PreparedSkinClump, &'static str> {
    let hierarchy = unsafe { validated_ped_hierarchy(clump) }?;
    let hierarchy_node_count = unsafe { hierarchy_node_count(hierarchy) }?;
    unsafe { setup_skin_atomics(clump, hierarchy, model_info) }?;

    let animation: *mut c_void =
        unsafe { call_cdecl_1(ADDR_RPANIMBLEND_CREATE_ANIMATION_FOR_HIERARCHY, hierarchy) };
    if animation.is_null() {
        return Err("could not create the hierarchy's initial RenderWare animation");
    }
    let Some(interpolator): Option<*mut c_void> =
        memory::read(hierarchy as usize + HANIM_HIERARCHY_CURRENT_ANIM)
    else {
        unsafe { destroy_animation(animation) };
        return Err("could not read the hierarchy animation interpolator");
    };
    if interpolator.is_null() {
        unsafe { destroy_animation(animation) };
        return Err("ped hierarchy has no animation interpolator");
    }
    let current_set: i32 = unsafe {
        call_cdecl_2(
            ADDR_RTANIM_INTERPOLATOR_SET_CURRENT_ANIM,
            interpolator,
            animation,
        )
    };
    if current_set == 0 {
        unsafe { destroy_animation(animation) };
        return Err("RenderWare rejected the hierarchy's initial animation");
    }
    unsafe {
        *(hierarchy.cast::<i32>().byte_add(HANIM_HIERARCHY_FLAGS)) = HANIM_UPDATE_BOTH_MATRICES;
        call_cdecl_1::<(), _>(ADDR_RPANIMBLEND_CLUMP_INIT, clump);
    }

    let anim_data = unsafe { anim_blend_data(clump) }?;
    let frame_count = unsafe { anim_blend_frame_count_from_data(anim_data) }?;
    if frame_count != hierarchy_node_count as u32 {
        return Err("skin bone count differs from its animation hierarchy node count");
    }
    let mut bone_frames: [*mut c_void; PED_BONE_COUNT] = [std::ptr::null_mut(); PED_BONE_COUNT];
    unsafe {
        call_cdecl_2::<(), _, _>(
            ADDR_RPANIMBLEND_CLUMP_FILL_FRAME_ARRAY,
            clump,
            bone_frames.as_mut_ptr(),
        );
    }
    if bone_frames[1..].iter().any(|frame| frame.is_null()) {
        return Err("AnimBlend did not resolve every required CPed bone frame");
    }
    unsafe {
        *((anim_data as usize + ANIM_BLEND_DATA_PED_POSITION) as *mut *mut c_void) =
            (ped.address as usize + PED_ANIM_MOVING_SHIFT_LOCAL) as *mut c_void;
    }

    Ok(PreparedSkinClump {
        address: clump,
        bone_frames,
        frame_count,
    })
}

unsafe fn validated_ped_hierarchy(clump: *mut c_void) -> Result<*mut c_void, &'static str> {
    let skinned: u8 = unsafe { call_cdecl_1(ADDR_IS_CLUMP_SKINNED, clump) };
    if skinned == 0 {
        return Err("DFF clump is not skinned");
    }
    let hierarchy = unsafe { anim_hierarchy_from_clump(clump) };
    if hierarchy.is_null() {
        return Err("DFF clump has no animation hierarchy");
    }
    let node_count = unsafe { hierarchy_node_count(hierarchy) }?;
    for bone_tag in REQUIRED_PED_BONE_TAGS {
        let index: i32 = unsafe { call_cdecl_2(ADDR_RPHANIM_ID_GET_INDEX, hierarchy, bone_tag) };
        if index < 0 || index >= node_count {
            return Err("DFF hierarchy is missing a required GTA ped bone");
        }
    }
    Ok(hierarchy)
}

/// Validates and normalizes intrinsic skinned-geometry data shared by every
/// clone. Model-info-dependent RenderWare callbacks are applied per clone.
unsafe fn prepare_skin_geometry(clump: *mut c_void) -> Result<(), &'static str> {
    let atomic: *mut c_void = unsafe { call_cdecl_1(ADDR_GET_FIRST_ATOMIC, clump) };
    if atomic.is_null() {
        return Err("DFF clump has no atomic");
    }
    let Some(geometry): Option<*mut c_void> = memory::read(atomic as usize + RP_ATOMIC_GEOMETRY)
    else {
        return Err("could not read the DFF atomic geometry");
    };
    if geometry.is_null() {
        return Err("DFF atomic has no geometry");
    }

    let skin: *mut c_void = unsafe { call_cdecl_1(ADDR_RPSKIN_GEOMETRY_GET_SKIN, geometry) };
    if skin.is_null() {
        return Err("DFF geometry has no skin");
    }
    let Some(vertex_count): Option<i32> =
        memory::read(geometry as usize + RP_GEOMETRY_NUM_VERTICES)
    else {
        return Err("could not read the DFF geometry vertex count");
    };
    if !(1..=MAX_PED_GEOMETRY_VERTICES).contains(&vertex_count) {
        return Err("DFF geometry vertex count is outside the supported range");
    }

    let weights: *mut c_void = unsafe { call_cdecl_1(ADDR_RPSKIN_GET_VERTEX_BONE_WEIGHTS, skin) };
    if weights.is_null() || !(weights as usize).is_multiple_of(std::mem::align_of::<f32>()) {
        return Err("DFF skin has an invalid vertex-weight array");
    }
    let weights_size = usize::try_from(vertex_count)
        .ok()
        .and_then(|count| count.checked_mul(RW_MATRIX_WEIGHTS_SIZE))
        .ok_or("DFF skin vertex-weight size overflowed")?;
    let weight_bytes = memory::read_bytes(weights as usize, weights_size)
        .ok_or("could not read the DFF skin vertex weights")?;
    let mut reciprocals = Vec::with_capacity(vertex_count as usize);
    let mut minimum_sum = f32::INFINITY;
    let mut maximum_sum = f32::NEG_INFINITY;
    for bytes in weight_bytes.chunks_exact(RW_MATRIX_WEIGHTS_SIZE) {
        let sum = bytes
            .chunks_exact(std::mem::size_of::<f32>())
            .map(|component| {
                f32::from_ne_bytes(
                    component
                        .try_into()
                        .expect("a weight component has exactly four bytes"),
                )
            })
            .try_fold(0.0_f32, |sum, component| {
                component.is_finite().then_some(sum + component)
            })
            .ok_or("DFF skin contains a non-finite vertex weight")?;
        if !sum.is_finite() || sum.abs() <= f32::EPSILON {
            return Err("DFF skin contains a vertex with no usable bone weight");
        }
        minimum_sum = minimum_sum.min(sum);
        maximum_sum = maximum_sum.max(sum);
        reciprocals.push(sum.recip());
    }
    for (vertex, reciprocal) in reciprocals.into_iter().enumerate() {
        let weights = (weights as *mut f32).wrapping_add(vertex * 4);
        for component in 0..4 {
            unsafe { *weights.add(component) *= reciprocal };
        }
    }

    let Some(morph_target): Option<*mut c_void> =
        memory::read(geometry as usize + RP_GEOMETRY_MORPH_TARGET)
    else {
        return Err("could not read the DFF geometry morph target");
    };
    if morph_target.is_null() {
        return Err("DFF geometry has no morph target");
    }
    let radius_address = morph_target as usize + RP_MORPH_TARGET_BOUNDING_SPHERE_RADIUS;
    let Some(radius): Option<f32> = memory::read(radius_address) else {
        return Err("could not read the DFF geometry bounding sphere");
    };
    let expanded_radius = radius * 1.2;
    if !radius.is_finite() || radius <= 0.0 || !expanded_radius.is_finite() {
        return Err("DFF geometry has an invalid bounding sphere");
    }
    unsafe { *(radius_address as *mut f32) = expanded_radius };

    log::debug!(
        "skin source: normalized {vertex_count} skin weights (sums {minimum_sum:.4}..{maximum_sum:.4}) and expanded its render bounds"
    );
    Ok(())
}

unsafe fn hierarchy_node_count(hierarchy: *mut c_void) -> Result<i32, &'static str> {
    let Some(node_count): Option<i32> =
        memory::read(hierarchy as usize + HANIM_HIERARCHY_NODE_COUNT)
    else {
        return Err("could not read the DFF hierarchy node count");
    };
    if !(1..=64).contains(&node_count) {
        return Err("DFF hierarchy node count is outside GTA's ped limit");
    }
    Ok(node_count)
}

unsafe fn setup_skin_atomics(
    clump: *mut c_void,
    hierarchy: *mut c_void,
    model_info: *mut c_void,
) -> Result<(), &'static str> {
    unsafe {
        set_clump_model_info(clump, model_info);
        for_all_atomics(
            clump,
            ADDR_CCLUMPMODELINFO_ATOMIC_SETUP_LIGHTING_CB,
            model_info,
        );
        for_all_atomics(
            clump,
            ADDR_CCLUMPMODELINFO_SET_ATOMIC_RENDERER_CB,
            ADDR_CVISIBILITYPLUGINS_RENDER_PED_CB as *mut c_void,
        );
        for_all_atomics(
            clump,
            ADDR_CCLUMPMODELINFO_SET_HIERARCHY_FOR_SKIN_ATOMIC,
            hierarchy,
        );
        *(hierarchy.cast::<i32>().byte_add(HANIM_HIERARCHY_FLAGS)) = HANIM_UPDATE_BOTH_MATRICES;
    }
    if unsafe { anim_hierarchy_from_skin_clump(clump) } != hierarchy {
        return Err("could not attach the DFF hierarchy to its skinned atomic");
    }
    Ok(())
}

unsafe fn anim_hierarchy_from_clump(clump: *mut c_void) -> *mut c_void {
    unsafe { call_cdecl_1(ADDR_GET_ANIM_HIERARCHY_FROM_CLUMP, clump) }
}

unsafe fn anim_hierarchy_from_skin_clump(clump: *mut c_void) -> *mut c_void {
    unsafe { call_cdecl_1(ADDR_GET_ANIM_HIERARCHY_FROM_SKIN_CLUMP, clump) }
}

unsafe fn for_all_atomics(clump: *mut c_void, callback: usize, data: *mut c_void) {
    let _: *mut c_void = unsafe {
        call_cdecl_3(
            ADDR_RPCLUMP_FOR_ALL_ATOMICS,
            clump,
            callback as *mut c_void,
            data,
        )
    };
}

unsafe fn set_clump_model_info(clump: *mut c_void, model_info: *mut c_void) {
    unsafe {
        call_cdecl_2::<(), _, _>(
            ADDR_CVISIBILITYPLUGINS_SET_CLUMP_MODEL_INFO,
            clump,
            model_info,
        )
    };
}

unsafe fn anim_blend_data(clump: *mut c_void) -> Result<*mut c_void, &'static str> {
    let Some(offset): Option<u32> = memory::read(ADDR_RPANIMBLEND_CLUMP_OFFSET) else {
        return Err("could not read the AnimBlend clump-plugin offset");
    };
    if !(0x20..=0x400).contains(&offset) {
        return Err("AnimBlend clump-plugin offset is outside the expected range");
    }
    let Some(data): Option<*mut c_void> = memory::read(clump as usize + offset as usize) else {
        return Err("could not read the clump's AnimBlend data pointer");
    };
    if data.is_null() {
        return Err("clump has no initialized AnimBlend data");
    }
    Ok(data)
}

unsafe fn anim_blend_frame_count(clump: *mut c_void) -> Result<u32, &'static str> {
    let data = unsafe { anim_blend_data(clump) }?;
    unsafe { anim_blend_frame_count_from_data(data) }
}

unsafe fn anim_blend_frame_count_from_data(data: *mut c_void) -> Result<u32, &'static str> {
    let Some(frame_count): Option<u32> = memory::read(data as usize + ANIM_BLEND_DATA_FRAME_COUNT)
    else {
        return Err("could not read the clump's AnimBlend frame count");
    };
    if !(1..=64).contains(&frame_count) {
        return Err("clump AnimBlend frame count is outside GTA's ped limit");
    }
    Ok(frame_count)
}

unsafe fn extract_anim_associations(clump: *mut c_void) -> AnimAssociations {
    AnimAssociations {
        address: unsafe { call_cdecl_1(ADDR_RPANIMBLEND_CLUMP_EXTRACT_ASSOCIATIONS, clump) },
    }
}

unsafe fn give_anim_associations(clump: *mut c_void, associations: AnimAssociations) {
    debug_assert!(!associations.is_empty());
    unsafe {
        call_cdecl_2::<(), _, _>(
            ADDR_RPANIMBLEND_CLUMP_GIVE_ASSOCIATIONS,
            clump,
            associations.address,
        )
    };
}

unsafe fn return_associations(from: *mut c_void, to: *mut c_void) {
    let associations = unsafe { extract_anim_associations(from) };
    if !associations.is_empty() {
        unsafe { give_anim_associations(to, associations) };
    }
}

unsafe fn destroy_animation(animation: *mut c_void) {
    let _: i32 = unsafe { call_cdecl_1(ADDR_RTANIM_ANIMATION_DESTROY, animation) };
}

/// Applies the temporary ordinary clump's world transform through RenderWare's
/// frame API. A raw matrix copy leaves the destination hierarchy clean, so
/// `RwFrameUpdateObjects` may retain the source DFF's stale LTM and render the
/// replacement away from the ped.
unsafe fn position_skin_clone(
    source: *mut c_void,
    destination: *mut c_void,
) -> Result<(), &'static str> {
    let Some(source_frame): Option<*mut c_void> = memory::read(source as usize + RW_OBJECT_PARENT)
    else {
        return Err("could not read the temporary server clump's root frame");
    };
    let Some(destination_frame): Option<*mut c_void> =
        memory::read(destination as usize + RW_OBJECT_PARENT)
    else {
        return Err("could not read the skin-source clone's root frame");
    };
    if source_frame.is_null() || destination_frame.is_null() {
        return Err("a replacement clump has no root frame");
    }
    let source_matrix = source_frame as usize + RW_FRAME_MODELLING_MATRIX;
    if memory::read_bytes(source_matrix, RW_MATRIX_SIZE).is_none() {
        return Err("could not read the temporary server clump's modeling matrix");
    }

    let transformed: *mut c_void = unsafe {
        call_cdecl_3(
            ADDR_RWFRAME_TRANSFORM,
            destination_frame,
            source_matrix as *const c_void,
            RW_COMBINE_REPLACE,
        )
    };
    if transformed != destination_frame {
        return Err("RwFrameTransform could not position the skin-source clone");
    }
    Ok(())
}

unsafe fn abort_secondary_ik(ped: &Ped) -> Result<(), &'static str> {
    let Some(intelligence): Option<*mut c_void> =
        memory::read(ped.address as usize + PED_INTELLIGENCE)
    else {
        return Err("could not read the ped intelligence pointer");
    };
    if intelligence.is_null() {
        return Err("ped has no intelligence object");
    }
    let Some(task): Option<*mut c_void> =
        memory::read(intelligence as usize + PED_INTELLIGENCE_SECONDARY_IK_TASK)
    else {
        return Err("could not read the ped's secondary IK task");
    };
    if task.is_null() {
        return Ok(());
    }
    if memory::read::<usize>(task as usize) != Some(CTASKSIMPLEIKMANAGER_VTABLE) {
        return Err("ped has an unsupported task in the secondary IK slot");
    }

    type MakeAbortable =
        unsafe extern "thiscall" fn(*mut c_void, *mut c_void, i32, *mut c_void) -> u8;
    let function: MakeAbortable =
        unsafe { std::mem::transmute(ADDR_CTASKSIMPLEIKMANAGER_MAKE_ABORTABLE) };
    let aborted = unsafe {
        function(
            task,
            ped.address,
            ABORT_PRIORITY_IMMEDIATE,
            std::ptr::null_mut(),
        )
    };
    if aborted == 0 {
        return Err("GTA refused to abort the secondary IK task");
    }
    log::debug!("skin-source swap: aborted secondary IK before replacing ped bones");
    Ok(())
}

unsafe fn update_rw_frame(ped: &Ped) {
    type UpdateRwFrame = unsafe extern "thiscall" fn(*mut c_void);
    let function: UpdateRwFrame = unsafe { std::mem::transmute(ADDR_CENTITY_UPDATE_RW_FRAME) };
    unsafe { function(ped.address) };
}

unsafe fn update_rp_hanim(ped: &Ped) {
    type UpdateRpHAnim = unsafe extern "thiscall" fn(*mut c_void);
    let function: UpdateRpHAnim = unsafe { std::mem::transmute(ADDR_CENTITY_UPDATE_RP_HANIM) };
    unsafe { function(ped.address) };
}

unsafe fn recover_server_clump_after_failed_swap(
    frame: &GameFrame,
    ped: &Ped,
    server_model_id: i16,
    prepared_clump: *mut c_void,
    reason: &'static str,
) {
    let associations = unsafe { extract_anim_associations(prepared_clump) };
    if ped_render_object(ped).is_some_and(|object| !object.is_null())
        && let Err(delete_reason) = unsafe { delete_entity_rw_object(ped) }
    {
        if !associations.is_empty() {
            unsafe { give_anim_associations(prepared_clump, associations) };
        }
        log::error!(
            "could not clean the temporary server clump while recovering from {reason}: {delete_reason}; retaining the unattached prepared clone to avoid destroying its live animations"
        );
        return;
    }

    set_ped_model_index(frame, ped, server_model_id as i32);
    let rebuilt = ped_render_object(ped).filter(|object| !object.is_null());
    if let Some(rebuilt) = rebuilt {
        if !associations.is_empty() {
            unsafe {
                give_anim_associations(rebuilt.address as *mut c_void, associations);
            }
        }
        if !unsafe { destroy_clump(prepared_clump) } {
            log::error!("could not destroy the prepared clone while recovering from {reason}");
        }
    } else {
        if !associations.is_empty() {
            unsafe { give_anim_associations(prepared_clump, associations) };
        }
        log::error!(
            "could not rebuild server model {server_model_id} while recovering from {reason}; retaining the prepared clone to avoid destroying its live animations"
        );
    }
}

unsafe fn create_entity_rw_object(ped: &Ped) -> Result<(), &'static str> {
    if ped_render_object(ped).is_some_and(|object| !object.is_null()) {
        return Err("CEntity::CreateRwObject called while the ped still had a render object");
    }
    type CreateRwObject = unsafe extern "thiscall" fn(*mut c_void);
    let function: CreateRwObject = unsafe { std::mem::transmute(ADDR_CENTITY_CREATE_RW_OBJECT) };
    unsafe { function(ped.address) };
    if ped_render_object(ped).is_some_and(|object| !object.is_null()) {
        Ok(())
    } else {
        Err("CEntity::CreateRwObject did not rebuild the temporary server clump")
    }
}

/// Calls the ped's virtual CEntity::DeleteRwObject implementation. A successful
/// call must clear m_pRwClump before the replacement can be installed.
unsafe fn delete_entity_rw_object(ped: &Ped) -> Result<(), &'static str> {
    let Some(current): Option<*mut c_void> = memory::read(ped.address as usize + ENTITY_RW_OBJECT)
    else {
        return Err("could not read the ped's current render object");
    };
    if current.is_null() {
        return Ok(());
    }

    let Some(vtable): Option<usize> = memory::read(ped.address as usize) else {
        return Err("could not read the ped vtable");
    };
    let Some(function_address): Option<usize> =
        memory::read(vtable + VTABLE_DELETE_RW_OBJECT_OFFSET)
    else {
        return Err("could not read CEntity::DeleteRwObject from the ped vtable");
    };
    if function_address == 0 {
        return Err("CEntity::DeleteRwObject is null in the ped vtable");
    }
    if function_address != ADDR_CPED_DELETE_RW_OBJECT {
        return Err("ped vtable has an unsupported CEntity::DeleteRwObject target");
    }

    type DeleteRwObject = unsafe extern "thiscall" fn(*mut c_void);
    let function: DeleteRwObject = unsafe { std::mem::transmute(function_address) };
    unsafe { function(ped.address) };

    match memory::read::<*mut c_void>(ped.address as usize + ENTITY_RW_OBJECT) {
        Some(render_object) if render_object.is_null() => Ok(()),
        Some(_) => Err("CEntity::DeleteRwObject did not clear the ped render object"),
        None => Err("could not verify CEntity::DeleteRwObject completion"),
    }
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

unsafe fn get_model_info(model_id: i32) -> *mut c_void {
    if !is_valid_model_id(model_id) {
        return std::ptr::null_mut();
    }

    let Some(model_info_address) =
        ADDR_MS_MODEL_INFO_PTRS.checked_add(model_id as usize * std::mem::size_of::<*mut c_void>())
    else {
        return std::ptr::null_mut();
    };
    memory::read(model_info_address).unwrap_or_default()
}

/// Returns a ped model-info only after proving it has CPedModelInfo's vtable.
/// GTA's model-info table is game-owned and must only be inspected from the
/// game thread.
unsafe fn verified_ped_model_info(model_id: i32) -> Result<*mut c_void, &'static str> {
    if !is_valid_model_id(model_id) {
        return Err("is outside the valid GTA model range");
    }

    let model_info = unsafe { get_model_info(model_id) };
    if model_info.is_null() {
        return Err("is not available in GTA's model-info table");
    }

    let known_ped = unsafe { get_model_info(KNOWN_PED_MODEL_ID) };
    if known_ped.is_null() {
        return Err("cannot be type-checked because GTA's known ped model is unavailable");
    }

    let Some(model_vtable): Option<usize> = memory::read(model_info as usize) else {
        return Err("has an unreadable model-info vtable");
    };
    let Some(ped_vtable): Option<usize> = memory::read(known_ped as usize) else {
        return Err("cannot be type-checked because GTA's known ped vtable is unreadable");
    };
    if model_vtable != ped_vtable {
        return Err("is not a CPedModelInfo ped model");
    }

    Ok(model_info)
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn stable_name_hash(text: &str) -> u64 {
    text.as_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01B3)
        })
}

#[cfg(test)]
mod tests {
    use super::{
        ADDR_CCLUMPMODELINFO_ATOMIC_SETUP_LIGHTING_CB, ADDR_CCLUMPMODELINFO_SET_ATOMIC_RENDERER_CB,
        ADDR_CCLUMPMODELINFO_SET_HIERARCHY_FOR_SKIN_ATOMIC, ADDR_CENTITY_CREATE_RW_OBJECT,
        ADDR_CENTITY_UPDATE_RP_HANIM, ADDR_CENTITY_UPDATE_RW_FRAME, ADDR_CGAME_PROCESS,
        ADDR_CPED_DELETE_RW_OBJECT, ADDR_CPED_SET_MODEL_INDEX,
        ADDR_CTASKSIMPLEIKMANAGER_MAKE_ABORTABLE, ADDR_CTXDSTORE_ADD_REF,
        ADDR_CTXDSTORE_ADD_TXD_SLOT, ADDR_CTXDSTORE_LOAD_TXD, ADDR_CTXDSTORE_POPCURRENTTXD,
        ADDR_CTXDSTORE_PUSHCURRENTTXD, ADDR_CTXDSTORE_REMOVE_REF, ADDR_CTXDSTORE_REMOVE_TXD_SLOT,
        ADDR_CTXDSTORE_SETCURRENTTXD, ADDR_CVISIBILITYPLUGINS_RENDER_PED_CB,
        ADDR_CVISIBILITYPLUGINS_SET_CLUMP_MODEL_INFO, ADDR_GET_ANIM_HIERARCHY_FROM_CLUMP,
        ADDR_GET_ANIM_HIERARCHY_FROM_SKIN_CLUMP, ADDR_GET_FIRST_ATOMIC, ADDR_IS_CLUMP_SKINNED,
        ADDR_RPANIMBLEND_CLUMP_EXTRACT_ASSOCIATIONS, ADDR_RPANIMBLEND_CLUMP_FILL_FRAME_ARRAY,
        ADDR_RPANIMBLEND_CLUMP_GIVE_ASSOCIATIONS, ADDR_RPANIMBLEND_CLUMP_INIT,
        ADDR_RPANIMBLEND_CREATE_ANIMATION_FOR_HIERARCHY, ADDR_RPCLUMP_CLONE, ADDR_RPCLUMP_DESTROY,
        ADDR_RPCLUMP_FOR_ALL_ATOMICS, ADDR_RPCLUMPSTREAMREAD, ADDR_RPHANIM_ID_GET_INDEX,
        ADDR_RPSKIN_GEOMETRY_GET_SKIN, ADDR_RPSKIN_GET_VERTEX_BONE_WEIGHTS,
        ADDR_RTANIM_ANIMATION_DESTROY, ADDR_RTANIM_INTERPOLATOR_SET_CURRENT_ANIM,
        ADDR_RWFRAME_TRANSFORM, ADDR_RWSTREAMCLOSE, ADDR_RWSTREAMFINDCHUNK, ADDR_RWSTREAMOPEN,
        EXECUTABLE_SIGNATURES, hex,
    };

    #[test]
    fn validates_every_fixed_gta_code_target() {
        let targets = [
            ADDR_CGAME_PROCESS,
            ADDR_CPED_SET_MODEL_INDEX,
            ADDR_CPED_DELETE_RW_OBJECT,
            ADDR_CENTITY_CREATE_RW_OBJECT,
            ADDR_CTASKSIMPLEIKMANAGER_MAKE_ABORTABLE,
            ADDR_CENTITY_UPDATE_RW_FRAME,
            ADDR_CENTITY_UPDATE_RP_HANIM,
            ADDR_CCLUMPMODELINFO_SET_HIERARCHY_FOR_SKIN_ATOMIC,
            ADDR_CCLUMPMODELINFO_ATOMIC_SETUP_LIGHTING_CB,
            ADDR_CCLUMPMODELINFO_SET_ATOMIC_RENDERER_CB,
            ADDR_CVISIBILITYPLUGINS_RENDER_PED_CB,
            ADDR_CVISIBILITYPLUGINS_SET_CLUMP_MODEL_INFO,
            ADDR_IS_CLUMP_SKINNED,
            ADDR_GET_ANIM_HIERARCHY_FROM_CLUMP,
            ADDR_GET_ANIM_HIERARCHY_FROM_SKIN_CLUMP,
            ADDR_RPANIMBLEND_CREATE_ANIMATION_FOR_HIERARCHY,
            ADDR_RPANIMBLEND_CLUMP_INIT,
            ADDR_RPANIMBLEND_CLUMP_FILL_FRAME_ARRAY,
            ADDR_RPANIMBLEND_CLUMP_EXTRACT_ASSOCIATIONS,
            ADDR_RPANIMBLEND_CLUMP_GIVE_ASSOCIATIONS,
            ADDR_RPCLUMP_CLONE,
            ADDR_RPCLUMP_DESTROY,
            ADDR_RPCLUMP_FOR_ALL_ATOMICS,
            ADDR_GET_FIRST_ATOMIC,
            ADDR_RPSKIN_GEOMETRY_GET_SKIN,
            ADDR_RPSKIN_GET_VERTEX_BONE_WEIGHTS,
            ADDR_RPHANIM_ID_GET_INDEX,
            ADDR_RTANIM_INTERPOLATOR_SET_CURRENT_ANIM,
            ADDR_RTANIM_ANIMATION_DESTROY,
            ADDR_RWFRAME_TRANSFORM,
            ADDR_RWSTREAMOPEN,
            ADDR_RWSTREAMFINDCHUNK,
            ADDR_RPCLUMPSTREAMREAD,
            ADDR_RWSTREAMCLOSE,
            ADDR_CTXDSTORE_ADD_TXD_SLOT,
            ADDR_CTXDSTORE_LOAD_TXD,
            ADDR_CTXDSTORE_ADD_REF,
            ADDR_CTXDSTORE_REMOVE_REF,
            ADDR_CTXDSTORE_REMOVE_TXD_SLOT,
            ADDR_CTXDSTORE_PUSHCURRENTTXD,
            ADDR_CTXDSTORE_POPCURRENTTXD,
            ADDR_CTXDSTORE_SETCURRENTTXD,
        ];

        for target in targets {
            assert!(
                EXECUTABLE_SIGNATURES
                    .iter()
                    .any(|signature| signature.address == target),
                "missing validation signature for fixed GTA target 0x{target:08X}"
            );
        }
    }

    #[test]
    fn formats_executable_mismatch_bytes_for_logs() {
        assert_eq!(hex(&[0x90, 0xE9, 0x00]), "90 E9 00");
    }
}
