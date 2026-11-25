//! Takes a photo when a Windows Hello PIN is rejected while the session is locked.

use std::ffi::c_void;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;
use windows::Win32::System::EventLog::{
    EVT_HANDLE, EVT_SUBSCRIBE_NOTIFY_ACTION, EvtSubscribe, EvtSubscribeActionDeliver,
    EvtSubscribeToFutureEvents,
};
use windows::core::{PCWSTR, w};

use crate::{camera, monitoring::MonitoringController, session_monitor, state::AppState};

const CHANNEL: PCWSTR = w!("Microsoft-Windows-HelloForBusiness/Operational");
// Event 7001 is a failed Windows Hello sign-in; sub-status 0xC0000380
// (STATUS_SMARTCARD_WRONG_PIN) narrows it to a rejected PIN.
const WRONG_PIN_QUERY: PCWSTR = w!(
    "*[System[(EventID=7001)]] and *[EventData[Data[@Name='AuthenticationErrorSubStatus']='0xc0000380']]"
);

static STARTED: Mutex<bool> = Mutex::new(false);

enum Signal {
    WrongPin,
    /// Win32 error code reported by the subscription itself.
    SubscriptionError(u32),
}

// The context is the sender leaked by `subscribe`, so it is valid whenever Windows calls back.
unsafe extern "system" fn on_event(
    action: EVT_SUBSCRIBE_NOTIFY_ACTION,
    context: *const c_void,
    event: EVT_HANDLE,
) -> u32 {
    let signals = unsafe { &*context.cast::<mpsc::UnboundedSender<Signal>>() };
    let signal = if action == EvtSubscribeActionDeliver {
        Signal::WrongPin
    } else {
        // Error notifications carry a Win32 error code in place of the event handle.
        Signal::SubscriptionError(event.0 as u32)
    };
    let _ = signals.send(signal);
    0
}

/// Subscribes for the rest of the process lifetime. The subscription is never cancelled,
/// so its callback context is leaked rather than freed while Windows may still use it.
fn subscribe() -> windows::core::Result<mpsc::UnboundedReceiver<Signal>> {
    let (signals, receiver) = mpsc::unbounded_channel();
    let context = Box::into_raw(Box::new(signals));
    let subscription = unsafe {
        EvtSubscribe(
            None,
            None,
            CHANNEL,
            WRONG_PIN_QUERY,
            None,
            Some(context as *const c_void),
            Some(on_event),
            EvtSubscribeToFutureEvents.0,
        )
    };
    if let Err(error) = subscription {
        // No subscription exists, so nothing else can reach the context.
        drop(unsafe { Box::from_raw(context) });
        return Err(error);
    }
    Ok(receiver)
}

/// Starts watching for rejected PINs once; later calls do nothing.
/// The watcher outlives the setting and checks it again for every event.
pub fn ensure_started(app_handle: &AppHandle) -> windows::core::Result<()> {
    let mut started = STARTED.lock().unwrap();
    if *started {
        return Ok(());
    }
    let mut signals = subscribe()?;
    let app_handle = app_handle.clone();
    // One consumer takes the photos in order instead of opening the camera concurrently.
    tauri::async_runtime::spawn(async move {
        while let Some(signal) = signals.recv().await {
            match signal {
                Signal::WrongPin => capture_if_locked(&app_handle).await,
                Signal::SubscriptionError(code) => {
                    log::warn!("Windows Hello event subscription reported error {code}")
                }
            }
        }
    });
    *started = true;
    log::info!("Watching for failed PIN unlocks");
    Ok(())
}

async fn capture_if_locked(app_handle: &AppHandle) {
    let state = app_handle.state::<AppState>();
    let config = state.config();
    if !config.capture_on_failed_unlock {
        return;
    }
    match session_monitor::is_session_unlocked() {
        // A PIN rejected by a prompt inside the unlocked session is not an unlock attempt.
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => log::warn!("Session lock state is unavailable, capturing anyway: {error}"),
    }
    log::info!("PIN unlock failed, taking a photo");
    match camera::take_photo(state.camera_id(), config.save_path).await {
        Ok(path) => log::info!("Failed unlock photo saved: {path}"),
        Err(error) => app_handle
            .state::<MonitoringController>()
            .report_capture_error(app_handle, &format!("解锁失败时拍照出错: {error}")),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_accepts_the_wrong_pin_subscription() {
        assert!(super::subscribe().is_ok());
    }
}
