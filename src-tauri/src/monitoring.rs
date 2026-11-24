use std::process::Command;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use crate::{
    camera,
    monitoring_controller::MonitoringController,
    state::{AppState, MonitoringFlags},
};
use rdev::listen;
use tauri::{AppHandle, Manager};

pub(crate) fn should_ignore_input_event(
    shortcut_in_progress: bool,
    within_shortcut_window: bool,
) -> bool {
    shortcut_in_progress || within_shortcut_window
}

fn is_action_still_current(app_handle: &AppHandle, action_generation: u64) -> bool {
    app_handle
        .state::<MonitoringController>()
        .is_action_current(action_generation)
}

pub fn lock_screen() {
    log::info!("执行锁屏命令...");
    match Command::new("rundll32.exe")
        .args(["user32.dll,LockWorkStation"])
        .spawn()
    {
        Ok(mut child) => {
            log::info!("锁屏命令已启动，进程ID: {:?}", child.id());
            match child.wait() {
                Ok(status) if status.success() => log::info!("锁屏命令执行成功"),
                Ok(status) => log::error!("锁屏命令执行失败，退出码: {:?}", status.code()),
                Err(error) => log::error!("等待锁屏命令完成时发生错误: {}", error),
            }
        }
        Err(error) => {
            log::error!("启动锁屏命令失败: {}", error);
        }
    }
}

pub fn ensure_listener_started(
    app_handle: AppHandle,
    monitoring_flags: Arc<MonitoringFlags>,
) -> Result<(), String> {
    if monitoring_flags.listener_ready() && monitoring_flags.is_listener_thread_alive() {
        monitoring_flags.clear_listener_error();
        return Ok(());
    }

    if let Ok(handle_guard) = monitoring_flags.listener_handle.lock() {
        if let Some(handle) = handle_guard.as_ref() {
            if !handle.is_finished() {
                monitoring_flags.set_listener_ready(true);
                monitoring_flags.clear_listener_error();
                return Ok(());
            }
        }
    }

    let (tx, rx) = mpsc::channel::<String>();
    let listener_app_handle = app_handle.clone();
    let listener_flags = monitoring_flags.clone();
    monitoring_flags.set_listener_ready(false);
    monitoring_flags.clear_listener_error();

    let handle = std::thread::spawn(move || {
        log::info!("启动常驻 rdev 事件监听器...");

        let callback_handle = listener_app_handle.clone();
        if let Err(error) = listen(move |event| {
            callback_handle
                .state::<MonitoringController>()
                .handle_input(event, &callback_handle);
        }) {
            let error_message = format!("rdev 事件监听器故障: {:?}", error);
            log::error!("{}", error_message);
            let _ = tx.send(error_message.clone());
            listener_flags.set_listener_ready(false);
            listener_flags.set_listener_error(Some(error_message));
            let generation = listener_flags.current_action_generation();
            tauri::async_runtime::spawn(async move {
                listener_app_handle
                    .state::<MonitoringController>()
                    .listener_failed(&listener_app_handle, generation)
                    .await;
            });
        }

        log::info!("rdev 事件监听器线程退出");
    });

    monitoring_flags.set_listener_handle(handle);

    match rx.recv_timeout(Duration::from_millis(300)) {
        Ok(error) => {
            monitoring_flags.set_listener_ready(false);
            monitoring_flags.set_listener_error(Some(error.clone()));
            Err(error)
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            monitoring_flags.set_listener_ready(true);
            monitoring_flags.clear_listener_error();
            Ok(())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            if monitoring_flags.is_listener_thread_alive() {
                monitoring_flags.set_listener_ready(true);
                monitoring_flags.clear_listener_error();
                Ok(())
            } else {
                monitoring_flags.set_listener_ready(false);
                monitoring_flags.set_listener_error(Some("输入监听器启动失败".to_string()));
                Err("输入监听器启动失败".to_string())
            }
        }
    }
}

pub(crate) async fn trigger_lockdown(app_handle: AppHandle, action_generation: u64) {
    log::info!("=== 开始执行锁定流程 ===");

    if !is_action_still_current(&app_handle, action_generation) {
        log::info!("锁定流程已失效，取消执行");
        return;
    }

    let (
        camera_id,
        save_path,
        exit_on_lock_enabled,
        post_trigger_action,
        notifications_enabled,
        capture_delay_seconds,
        capture_mode,
    ) = {
        let state = app_handle.state::<AppState>();
        (
            state.camera_id(),
            state.save_path(),
            state.exit_on_lock(),
            state.post_trigger_action(),
            state.enable_notifications(),
            state.capture_delay_seconds(),
            state.capture_mode(),
        )
    };

    let screen_lock_enabled = matches!(
        post_trigger_action,
        crate::config::PostTriggerAction::CaptureAndLock
    );

    log::info!(
        "监控触发，使用摄像头ID: {}, 触发后动作: {:?}, 通知功能: {}, 锁定时退出: {}, 拍摄延迟: {}秒, 拍摄模式: {:?}",
        camera_id,
        post_trigger_action,
        notifications_enabled,
        exit_on_lock_enabled,
        capture_delay_seconds,
        capture_mode
    );

    if capture_delay_seconds > 0 {
        if !await_delayed_capture(
            app_handle.clone(),
            camera_id,
            save_path.clone(),
            capture_delay_seconds,
            capture_mode,
            action_generation,
        )
        .await
        {
            return;
        }
    } else if !execute_capture_and_lock(
        app_handle.clone(),
        camera_id,
        save_path.clone(),
        post_trigger_action.clone(),
        action_generation,
    )
    .await
    {
        return;
    }

    {
        let controller = app_handle.state::<MonitoringController>();
        let Some(_guard) = controller.lock_action(action_generation).await else {
            return;
        };
        if notifications_enabled {
            send_security_notification(&app_handle);
        }
        if screen_lock_enabled {
            if let Err(error) = tokio::task::spawn_blocking(lock_screen).await {
                log::error!("锁屏任务失败: {}", error);
            }
        }
    }
    let controller = app_handle.state::<MonitoringController>();
    if screen_lock_enabled
        && !controller
            .wait_action_delay(action_generation, Duration::from_millis(1_000))
            .await
    {
        return;
    }
    if exit_on_lock_enabled {
        controller
            .exit_after_trigger(&app_handle, action_generation)
            .await;
    } else if post_trigger_action == crate::config::PostTriggerAction::CaptureOnly {
        controller
            .complete_trigger(&app_handle, action_generation)
            .await;
    }

    log::info!("=== 锁定流程执行完成 ===");
}

fn send_security_notification(app_handle: &AppHandle) {
    use tauri_plugin_notification::NotificationExt;

    let state = app_handle.state::<AppState>();
    if !state.enable_notifications() {
        return;
    }

    match app_handle
        .notification()
        .builder()
        .title("SnapLock 安全警报")
        .body("检测到未授权访问")
        .icon("📷")
        .show()
    {
        Ok(_) => log::info!("安全通知发送成功"),
        Err(error) => log::error!("发送安全通知失败: {}", error),
    }
}

async fn await_delayed_capture(
    app_handle: AppHandle,
    camera_id: u32,
    save_path: Option<String>,
    delay_seconds: u32,
    capture_mode: crate::config::CaptureMode,
    action_generation: u64,
) -> bool {
    log::info!(
        "开始延迟拍摄，模式: {:?}, 延迟: {}秒",
        capture_mode,
        delay_seconds
    );

    {
        let controller = app_handle.state::<MonitoringController>();
        let Some(_guard) = controller.lock_action(action_generation).await else {
            return false;
        };
        if let Err(error) = camera::start_video_recording(
            app_handle.clone(),
            camera_id,
            save_path,
            Some(delay_seconds),
        )
        .await
        {
            log::error!("启动录像失败: {}", error);
            return true;
        }
    }

    let controller = app_handle.state::<MonitoringController>();
    if !controller
        .wait_action_delay(
            action_generation,
            Duration::from_secs((delay_seconds + 2).into()),
        )
        .await
    {
        return false;
    }

    {
        let controller = app_handle.state::<MonitoringController>();
        let Some(_guard) = controller.lock_action(action_generation).await else {
            log::info!("延迟拍摄已取消，跳过旧流程的进程清理");
            return false;
        };
        if let Err(error) = camera::stop_video_recording().await {
            log::error!("清理录像进程失败: {}", error);
        }
    }

    controller
        .wait_action_delay(action_generation, Duration::from_millis(2_000))
        .await
}

async fn execute_capture_and_lock(
    app_handle: AppHandle,
    camera_id: u32,
    save_path: Option<String>,
    post_trigger_action: crate::config::PostTriggerAction,
    action_generation: u64,
) -> bool {
    let controller = app_handle.state::<MonitoringController>();
    let Some(_guard) = controller.lock_action(action_generation).await else {
        return false;
    };

    if post_trigger_action == crate::config::PostTriggerAction::ScreenRecording {
        if let Err(error) = crate::recorder::start_screen_recording(app_handle.clone()).await {
            log::error!("启动屏幕录制失败: {}", error);
        }
        return true;
    }

    if let Err(error) = camera::take_photo(camera_id, save_path).await {
        log::error!("拍照失败: {}", error);
    }

    is_action_still_current(&app_handle, action_generation)
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
