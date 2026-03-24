use std::process::Command;
use std::sync::{mpsc, Arc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{
    camera,
    constants::EVENT_IGNORE_WINDOW_MS,
    state::{AppState, MonitoringFlags, MonitoringState},
};
use rdev::{listen, Event};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{task, time::sleep};

fn emit_monitoring_status(app_handle: &AppHandle, status: &str) {
    if let Err(error) = app_handle.emit("monitoring_status_changed", status) {
        log::error!("无法发送监控状态事件 '{}': {}", status, error);
    }
}

fn should_ignore_input_event(shortcut_in_progress: bool, within_shortcut_window: bool) -> bool {
    shortcut_in_progress || within_shortcut_window
}

fn is_action_still_current(app_handle: &AppHandle, action_generation: u64) -> bool {
    let monitoring_flags = app_handle.state::<Arc<MonitoringFlags>>().inner().clone();
    monitoring_flags.is_action_generation_current(action_generation)
        && app_handle.state::<AppState>().status() == MonitoringState::Triggered
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
        let callback_flags = listener_flags.clone();

        if let Err(error) = listen(move |event| {
            callback(event, &callback_handle, &callback_flags);
        }) {
            let error_message = format!("rdev 事件监听器故障: {:?}", error);
            log::error!("{}", error_message);
            let _ = tx.send(error_message.clone());
            listener_flags.set_listener_ready(false);
            listener_flags.set_listener_error(Some(error_message));
            listener_flags.stop_monitoring_thread();
            crate::recorder::stop_screen_recording();

            if let Ok(runtime) = tokio::runtime::Runtime::new() {
                runtime.block_on(async {
                    if let Err(error) = camera::stop_video_recording().await {
                        log::error!("监听器故障后停止摄像头录像失败: {}", error);
                    }
                });
            }

            let state = listener_app_handle.state::<AppState>();
            if state.set_status(MonitoringState::Idle).is_ok() {
                emit_monitoring_status(&listener_app_handle, "空闲");
            }
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

pub fn start_idle_check_loop(
    app_handle: AppHandle,
    monitoring_flags: Arc<MonitoringFlags>,
) -> task::JoinHandle<()> {
    log::info!("启动空闲检测循环...");
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(2)).await;

            if !monitoring_flags.monitoring_active() {
                log::debug!("监控非激活状态，空闲检测循环终止");
                break;
            }

            let last_activity = monitoring_flags.last_activity_time();
            if last_activity == 0 {
                continue;
            }

            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let idle_time_ms = current_time.saturating_sub(last_activity);
            let is_recording = crate::recorder::is_screen_recording_running();

            if idle_time_ms > 20_000 && is_recording {
                log::info!("超过20秒无操作，暂停屏幕录制...");
                crate::recorder::stop_screen_recording();
            } else if idle_time_ms <= 20_000 && !is_recording {
                if let Some(remaining_ms) = crate::recorder::screen_recording_retry_remaining_ms() {
                    log::debug!("屏幕录制处于冷却中，剩余 {} ms", remaining_ms);
                    continue;
                }

                log::info!("检测到用户活动，恢复屏幕录制...");
                let app_handle_clone = app_handle.clone();
                tokio::spawn(async move {
                    if let Err(error) = crate::recorder::start_screen_recording_with_options(
                        app_handle_clone,
                        false,
                    )
                    .await
                    {
                        log::error!("无法恢复屏幕录制: {}", error);
                    }
                });
            }
        }
    })
}

fn callback(event: Event, app_handle: &AppHandle, monitoring_flags: &Arc<MonitoringFlags>) {
    if !monitoring_flags.monitoring_active() {
        return;
    }

    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let within_shortcut_window =
        current_time.saturating_sub(monitoring_flags.last_shortcut_time()) < EVENT_IGNORE_WINDOW_MS;

    if should_ignore_input_event(
        monitoring_flags.shortcut_in_progress(),
        within_shortcut_window,
    ) {
        return;
    }

    monitoring_flags.set_last_activity_time(current_time);

    let state = app_handle.state::<AppState>();
    if state.post_trigger_action() == crate::config::PostTriggerAction::ScreenRecording {
        log::debug!("屏幕录制模式下检测到真实活动");
        trigger_screen_recording_activity(app_handle.clone());
        return;
    }

    log::info!("✓ 触发锁定！事件类型: {:?}", event.event_type);

    if state.set_status(MonitoringState::Triggered).is_err() {
        log::warn!("状态转换到 Triggered 失败，忽略本次事件");
        return;
    }

    let action_generation = monitoring_flags.current_action_generation();
    monitoring_flags.set_monitoring_active(false);

    let app_handle_clone = app_handle.clone();
    std::thread::spawn(move || match tokio::runtime::Runtime::new() {
        Ok(runtime) => {
            runtime.block_on(async move {
                trigger_lockdown(app_handle_clone, action_generation).await;
            });
        }
        Err(error) => {
            log::error!("创建内部运行时失败: {}", error);
        }
    });
}

fn trigger_screen_recording_activity(app_handle: AppHandle) {
    if crate::recorder::is_screen_recording_running() {
        return;
    }

    if let Some(remaining_ms) = crate::recorder::screen_recording_retry_remaining_ms() {
        log::debug!("屏幕录制启动冷却中，剩余 {} ms", remaining_ms);
        return;
    }

    tauri::async_runtime::spawn(async move {
        if let Err(error) =
            crate::recorder::start_screen_recording_with_options(app_handle, false).await
        {
            log::error!("启动屏幕录制失败: {}", error);
        }
    });
}

async fn trigger_lockdown(app_handle: AppHandle, action_generation: u64) {
    log::info!("=== 开始执行锁定流程 ===");

    if !is_action_still_current(&app_handle, action_generation) {
        log::info!("锁定流程已失效，取消执行");
        return;
    }

    emit_monitoring_status(&app_handle, "锁定中");

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

    if notifications_enabled && is_action_still_current(&app_handle, action_generation) {
        send_security_notification(&app_handle);
    }

    if screen_lock_enabled && is_action_still_current(&app_handle, action_generation) {
        lock_screen();
        sleep(Duration::from_millis(1_000)).await;
    }

    if exit_on_lock_enabled && is_action_still_current(&app_handle, action_generation) {
        crate::recorder::stop_screen_recording();
        if let Err(error) = camera::stop_video_recording().await {
            log::error!("退出前停止摄像头录像失败: {}", error);
        }
        std::process::exit(0);
    }

    if post_trigger_action == crate::config::PostTriggerAction::CaptureOnly
        && is_action_still_current(&app_handle, action_generation)
    {
        let state = app_handle.state::<AppState>();
        if state.set_status(MonitoringState::Idle).is_ok() {
            emit_monitoring_status(&app_handle, "空闲");
        }
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

    if !is_action_still_current(&app_handle, action_generation) {
        log::info!("延迟拍摄前流程已取消");
        return false;
    }

    if let Err(error) =
        camera::start_video_recording(app_handle.clone(), camera_id, save_path, Some(delay_seconds))
            .await
    {
        log::error!("启动录像失败: {}", error);
        return is_action_still_current(&app_handle, action_generation);
    }

    sleep(Duration::from_secs((delay_seconds + 2).into())).await;

    if let Err(error) = camera::stop_video_recording().await {
        log::error!("清理录像进程失败: {}", error);
    }

    if !is_action_still_current(&app_handle, action_generation) {
        log::info!("延迟拍摄完成后流程已取消");
        return false;
    }

    sleep(Duration::from_millis(2_000)).await;
    is_action_still_current(&app_handle, action_generation)
}

async fn execute_capture_and_lock(
    app_handle: AppHandle,
    camera_id: u32,
    save_path: Option<String>,
    post_trigger_action: crate::config::PostTriggerAction,
    action_generation: u64,
) -> bool {
    if !is_action_still_current(&app_handle, action_generation) {
        log::info!("执行触发动作前流程已取消");
        return false;
    }

    if post_trigger_action == crate::config::PostTriggerAction::ScreenRecording {
        if let Err(error) = crate::recorder::start_screen_recording(app_handle).await {
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
