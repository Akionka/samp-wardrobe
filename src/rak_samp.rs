//! Optional rak-samp event subscription for prompt ped reconciliation.
//!
//! The callback only coalesces work for the GTA frame thread. It must never
//! inspect or retain player payload values, block network traffic, or access
//! the runtime.

use rak_samp_plugin_api::{
    HostApi, RakSampApiV1, RakSampDirection, RakSampEventV1, RakSampHookAction, RakSampResult,
    RakSampSubscription,
    events::{RpcAction, rpc::incoming},
    wait_for_default_host,
};
use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

const HOST_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
// `WorldPlayerAdd` (RPC 32) is SA-MP's remote-player stream-in event. The
// local API's typed helper is R1-specific. Wardrobe supports every ready host,
// so it only observes the RPC identifier and deliberately never reads the
// payload.
const PLAYER_STREAM_IN_RPC_ID: u8 = 32;
const SET_PLAYER_SKIN_REFRESH: u8 = 1 << 0;
const PLAYER_STREAM_IN_REFRESH: u8 = 1 << 1;
const PLAYER_STREAM_OUT_REFRESH: u8 = 1 << 2;
const SET_PLAYER_SKIN_AND_PLAYER_STREAM_IN: u8 = SET_PLAYER_SKIN_REFRESH | PLAYER_STREAM_IN_REFRESH;
const SET_PLAYER_SKIN_AND_PLAYER_STREAM_OUT: u8 =
    SET_PLAYER_SKIN_REFRESH | PLAYER_STREAM_OUT_REFRESH;
const PLAYER_STREAM_IN_AND_PLAYER_STREAM_OUT: u8 =
    PLAYER_STREAM_IN_REFRESH | PLAYER_STREAM_OUT_REFRESH;
const ALL_REFRESH_REASONS: u8 =
    SET_PLAYER_SKIN_REFRESH | PLAYER_STREAM_IN_REFRESH | PLAYER_STREAM_OUT_REFRESH;

static LISTENER_STARTED: AtomicBool = AtomicBool::new(false);
static REFRESH_REASONS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static LISTENER: OnceLock<(HostApi, RakSampSubscription)> = OnceLock::new();

/// Starts the one process-lifetime listener after SA-MP has been discovered.
pub fn start_listener() {
    if LISTENER_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }

    if let Err(error) = thread::Builder::new()
        .name("wardrobe-rak-samp".to_owned())
        .spawn(register_listener)
    {
        log::warn!("rak-samp skin refresh unavailable ({error}); using configured complete scans");
    }
}

/// Consumes all coalesced rak-samp refresh notifications since the prior frame.
pub fn take_refresh_request() -> Option<&'static str> {
    refresh_reason(REFRESH_REASONS.swap(0, Ordering::AcqRel))
}

fn register_listener() {
    let api = match wait_for_default_host(HOST_WAIT_TIMEOUT) {
        Ok(api) => api,
        Err(error) => {
            log::warn!(
                "rak-samp skin refresh unavailable ({error}); using configured complete scans"
            );
            return;
        }
    };

    let mut subscription = RakSampSubscription::default();
    let result = unsafe {
        (api.raw().register_rpc)(
            RakSampDirection::Incoming,
            Some(on_incoming_rpc),
            (api.raw() as *const RakSampApiV1)
                .cast_mut()
                .cast::<c_void>(),
            &raw mut subscription,
        )
    };
    if result != RakSampResult::Ok {
        log::warn!(
            "rak-samp skin refresh unavailable (could not subscribe to incoming RPCs: {result:?}); using configured complete scans"
        );
        return;
    }

    if LISTENER.set((api, subscription)).is_err() {
        log::warn!(
            "rak-samp skin refresh unavailable (incoming RPC listener was already registered); using configured complete scans"
        );
        return;
    }
    log::info!(
        "rak-samp is ready; listening for SetPlayerSkin, player stream-in, and player stream-out refresh events"
    );
}

unsafe extern "system" fn on_incoming_rpc(
    user_data: *mut c_void,
    event: *mut RakSampEventV1,
) -> RakSampHookAction {
    let Ok(api) = (unsafe { HostApi::from_raw(user_data.cast::<RakSampApiV1>()) }) else {
        return RakSampHookAction::Continue;
    };

    let event_id = unsafe { (api.raw().event_id)(event.cast_const()) };
    if event_id == PLAYER_STREAM_IN_RPC_ID {
        request_refresh(PLAYER_STREAM_IN_REFRESH);
        return RakSampHookAction::Continue;
    }

    if event_id == incoming::SET_PLAYER_SKIN.id() {
        // The typed helper validates the event type. Wardrobe intentionally
        // discards the decoded values and only requests frame-thread work.
        return unsafe {
            incoming::on_set_player_skin(api, event, |_skin| {
                request_refresh(SET_PLAYER_SKIN_REFRESH);
                RpcAction::Continue
            })
        }
        .unwrap_or(RakSampHookAction::Continue);
    }

    if event_id == incoming::PLAYER_STREAM_OUT.id() {
        return unsafe {
            incoming::on_player_stream_out(api, event, |_player_id| {
                request_refresh(PLAYER_STREAM_OUT_REFRESH);
                RpcAction::Continue
            })
        }
        .unwrap_or(RakSampHookAction::Continue);
    }

    RakSampHookAction::Continue
}

fn request_refresh(reason: u8) {
    REFRESH_REASONS.fetch_or(reason, Ordering::Release);
}

fn refresh_reason(reasons: u8) -> Option<&'static str> {
    match reasons & ALL_REFRESH_REASONS {
        0 => None,
        SET_PLAYER_SKIN_REFRESH => Some("SetPlayerSkin"),
        PLAYER_STREAM_IN_REFRESH => Some("player stream-in"),
        PLAYER_STREAM_OUT_REFRESH => Some("player stream-out"),
        SET_PLAYER_SKIN_AND_PLAYER_STREAM_IN => Some("SetPlayerSkin and player stream-in"),
        SET_PLAYER_SKIN_AND_PLAYER_STREAM_OUT => Some("SetPlayerSkin and player stream-out"),
        PLAYER_STREAM_IN_AND_PLAYER_STREAM_OUT => Some("player stream-in and player stream-out"),
        _ => Some("SetPlayerSkin, player stream-in, and player stream-out"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PLAYER_STREAM_IN_REFRESH, PLAYER_STREAM_OUT_REFRESH, REFRESH_REASONS,
        SET_PLAYER_SKIN_REFRESH, take_refresh_request,
    };
    use std::sync::atomic::Ordering;

    #[test]
    fn consumes_and_describes_coalesced_refresh_requests_once() {
        REFRESH_REASONS.store(0, Ordering::Release);
        assert_eq!(take_refresh_request(), None);

        REFRESH_REASONS.store(
            SET_PLAYER_SKIN_REFRESH | PLAYER_STREAM_IN_REFRESH | PLAYER_STREAM_OUT_REFRESH,
            Ordering::Release,
        );
        let request = take_refresh_request().expect("the coalesced request should be consumed");
        assert_eq!(
            request,
            "SetPlayerSkin, player stream-in, and player stream-out"
        );
        assert_eq!(take_refresh_request(), None);
    }
}
