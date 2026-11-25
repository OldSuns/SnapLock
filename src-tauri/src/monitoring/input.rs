use super::MonitoringController;
use crate::state::MonitoringFlags;
use std::sync::{Arc, mpsc};
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub(super) fn should_ignore_input_event(
    shortcut_in_progress: bool,
    within_shortcut_window: bool,
) -> bool {
    shortcut_in_progress || within_shortcut_window
}

pub(super) fn ensure_listener_started(
    app: AppHandle,
    flags: Arc<MonitoringFlags>,
) -> Result<(), String> {
    if flags.is_listener_thread_alive() {
        flags.set_listener_ready(true);
        flags.clear_listener_error();
        return Ok(());
    }
    let (sender, receiver) = mpsc::channel();
    flags.set_listener_ready(false);
    flags.clear_listener_error();
    let listener_flags = flags.clone();
    let handle = std::thread::spawn(move || {
        log::info!("启动常驻 rdev 事件监听器...");
        let callback_app = app.clone();
        if let Err(error) = rdev::listen(move |event| {
            callback_app
                .state::<MonitoringController>()
                .handle_input(event, &callback_app);
        }) {
            let error = format!("rdev 事件监听器故障: {error:?}");
            log::error!("{error}");
            listener_flags.set_listener_ready(false);
            listener_flags.set_listener_error(Some(error.clone()));
            let _ = sender.send(error);
            let generation = listener_flags.current_action_generation();
            tauri::async_runtime::spawn(async move {
                app.state::<MonitoringController>()
                    .listener_failed(&app, generation)
                    .await;
            });
        }
    });
    flags.set_listener_handle(handle);
    // rdev has no ready callback; retain the startup window for immediate failures.
    let result = match receiver.recv_timeout(Duration::from_millis(300)) {
        Ok(error) => Err(error),
        Err(_) if flags.is_listener_thread_alive() => Ok(()),
        Err(_) => Err("输入监听器启动失败".to_string()),
    };
    flags.set_listener_ready(result.is_ok());
    flags.set_listener_error(result.as_ref().err().cloned());
    result
}

#[cfg(test)]
mod tests {
    use super::should_ignore_input_event;

    #[test]
    fn ignores_event_while_shortcut_is_in_progress() {
        assert!(should_ignore_input_event(true, false));
    }
    #[test]
    fn ignores_event_inside_shortcut_window() {
        assert!(should_ignore_input_event(false, true));
    }
    #[test]
    fn does_not_ignore_normal_input() {
        assert!(!should_ignore_input_event(false, false));
    }
}
