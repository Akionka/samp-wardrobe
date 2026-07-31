mod config;
mod gta;
mod logging;
mod memory;
mod runtime;
mod samp;
mod skin_loader;

use std::ffi::c_void;
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

fn plugin_thread() {
    logging::init();

    #[cfg(debug_assertions)]
    while unsafe { winapi::um::debugapi::IsDebuggerPresent() } == 0 {
        thread::sleep(Duration::from_millis(100));
    }

    let config = match config::load_initial() {
        Ok(config) => config,
        Err(error) => {
            log::error!("{error}");
            return;
        }
    };
    let skin_count = config.skins.len();
    let rule_count = config.rules.len();
    if rule_count == 0 {
        log::info!(
            "{} has no rules; waiting for a configuration change",
            config::CONFIG_PATH
        );
    }
    log::info!(
        "loaded {}: {skin_count} skin(s), {rule_count} rule(s)",
        config::CONFIG_PATH
    );

    let samp = samp::Samp::wait_for_load();
    log::info!("found samp.dll at 0x{:08X}", samp.base());

    while !unsafe { gta::is_ready() } {
        thread::sleep(Duration::from_millis(100));
    }
    log::info!("GTA model system is ready");

    if let Err(error) = unsafe { runtime::install(config, samp) } {
        log::error!("could not install CGame::Process hook: {error}");
        return;
    }
    log::info!("installed CGame::Process hook");
    log::info!("watching {rule_count} configured rule(s) across {skin_count} skin(s)");
}

/*
RenderWare and CPed::SetModelIndex mutate GTA engine state, so they run only
from the CGame::Process detour on GTA's frame thread. The loader thread above
only waits for dependencies and installs that detour.
*/

#[unsafe(no_mangle)]
pub extern "system" fn DllMain(_hmodule: HMODULE, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        thread::spawn(plugin_thread);
    }
    1
}
