pub const MODEL_ID_LIMIT: i32 = 20_000;

pub const fn is_valid_model_id(model_id: i32) -> bool {
    model_id >= 0 && model_id < MODEL_ID_LIMIT
}
