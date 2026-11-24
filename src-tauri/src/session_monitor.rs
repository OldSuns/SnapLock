use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    WM_NCDESTROY, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};

use crate::monitoring_controller::MonitoringController;

const SESSION_SUBCLASS_ID: usize = 1;

#[derive(Debug, PartialEq)]
enum SessionEvent {
    Locked,
    Unlocked,
}

fn session_event(message: u32, reason: usize) -> Option<SessionEvent> {
    if message != WM_WTSSESSION_CHANGE {
        return None;
    }
    match reason as u32 {
        WTS_SESSION_LOCK => Some(SessionEvent::Locked),
        WTS_SESSION_UNLOCK => Some(SessionEvent::Unlocked),
        _ => None,
    }
}

struct SessionContext {
    events: mpsc::UnboundedSender<SessionEvent>,
}

// The context belongs to this window subclass and is released on WM_NCDESTROY.
unsafe extern "system" fn session_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    context: usize,
) -> LRESULT {
    if let Some(event) = session_event(message, wparam.0) {
        let context = unsafe { &*(context as *const SessionContext) };
        let _ = context.events.send(event);
    }
    if message == WM_NCDESTROY {
        unsafe {
            let _ = WTSUnRegisterSessionNotification(hwnd);
            let _ = RemoveWindowSubclass(hwnd, Some(session_window_proc), subclass_id);
            drop(Box::from_raw(context as *mut SessionContext));
        }
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

/// Registers notifications for the main window's Windows session, including while hidden.
/// Must run on the window thread during Tauri setup.
pub fn start_session_monitoring(app_handle: AppHandle) -> Result<(), String> {
    let window = app_handle
        .get_webview_window("main")
        .ok_or("Main window is missing")?;
    let hwnd = window.hwnd().map_err(|error| error.to_string())?;
    let (events, mut receiver) = mpsc::unbounded_channel();
    let context = Box::into_raw(Box::new(SessionContext { events }));
    unsafe {
        if !SetWindowSubclass(
            hwnd,
            Some(session_window_proc),
            SESSION_SUBCLASS_ID,
            context as usize,
        )
        .as_bool()
        {
            drop(Box::from_raw(context));
            return Err("Failed to install Windows session notification handler".to_string());
        }
        if let Err(error) = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) {
            if RemoveWindowSubclass(hwnd, Some(session_window_proc), SESSION_SUBCLASS_ID).as_bool()
            {
                drop(Box::from_raw(context));
            }
            return Err(format!(
                "Failed to register Windows session notifications: {error}"
            ));
        }
    }
    // One consumer preserves the OS event order instead of racing lock/unlock tasks.
    tauri::async_runtime::spawn(async move {
        while let Some(event) = receiver.recv().await {
            match event {
                SessionEvent::Locked => log::info!("Windows session locked"),
                SessionEvent::Unlocked => {
                    log::info!("Windows session unlocked");
                    app_handle
                        .state::<MonitoringController>()
                        .system_unlocked(&app_handle)
                        .await;
                }
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_session_lock_and_unlock_notifications_change_state() {
        assert_eq!(
            session_event(WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK as usize),
            Some(SessionEvent::Locked)
        );
        assert_eq!(
            session_event(WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK as usize),
            Some(SessionEvent::Unlocked)
        );
        for message in [0, 6, 7, 8, WM_NCDESTROY] {
            assert_eq!(session_event(message, WTS_SESSION_UNLOCK as usize), None);
        }
        for reason in [0, 1, 2, 3, 4, 5, 6, 9, 10] {
            assert_eq!(session_event(WM_WTSSESSION_CHANGE, reason), None);
        }
    }
}
