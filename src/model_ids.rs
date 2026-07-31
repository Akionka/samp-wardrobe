pub const MODEL_ID_LIMIT: i32 = 20_000;
pub const PRIVATE_MODEL_ID_START: i32 = 18_000;
pub const PRIVATE_MODEL_ID_END: i32 = MODEL_ID_LIMIT;

pub const fn is_valid_model_id(model_id: i32) -> bool {
    model_id >= 0 && model_id < MODEL_ID_LIMIT
}

pub const fn is_private_model_id(model_id: i32) -> bool {
    model_id >= PRIVATE_MODEL_ID_START && model_id < PRIVATE_MODEL_ID_END
}

pub const fn is_valid_donor_model_id(model_id: i32) -> bool {
    is_valid_model_id(model_id) && !is_private_model_id(model_id)
}
