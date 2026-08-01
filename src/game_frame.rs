/// Proof that execution is currently on GTA's frame thread, immediately after
/// the original `CGame::Process` call. GTA and RenderWare mutation APIs require
/// this capability so the startup thread cannot call them by accident.
pub(crate) struct GameFrame {
    _private: (),
}

impl GameFrame {
    /// # Safety
    ///
    /// Call only from Wardrobe's `CGame::Process` detour after its trampoline
    /// has returned. Runtime installation verifies the supported GTA code
    /// targets before that detour can run.
    pub(crate) unsafe fn enter() -> Self {
        Self { _private: () }
    }
}
