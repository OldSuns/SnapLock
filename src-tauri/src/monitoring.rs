use std::process::Command;
use std::sync::{Arc, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{
    camera,
    constants::EVENT_IGNORE_WINDOW_MS,
    state::{AppState, MonitoringFlags, MonitoringState},
};
use rdev::{Event, EventType, Key, listen};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{task, time::sleep};

fn emit_monitoring_status(app_handle: &AppHandle, status: &str) {
    if let Err(error) = app_handle.emit("monitoring_status_changed", status) {
        log::error!("无法发送监控状态事件 '{}': {}", status, error);
    }
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
            listener_flags.set_monitoring_active(false);
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

/// Starts the idle check loop for screen recording.
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
            let is_recording = crate::recorder::FFMPEG_PROCESS.lock().unwrap().is_some();

            if idle_time_ms > 20_000 && is_recording {
                log::info!("超过20秒无操作，暂停屏幕录制...");
                crate::recorder::stop_screen_recording();
            } else if idle_time_ms <= 20_000 && !is_recording {
                log::info!("检测到用户活动，恢复屏幕录制...");
                let app_handle_clone = app_handle.clone();
                tokio::spawn(async move {
                    if let Err(error) =
                        crate::recorder::start_screen_recording(app_handle_clone).await
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

    if monitoring_flags.shortcut_in_progress() {
        return;
    }

    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    if current_time.saturating_sub(monitoring_flags.last_shortcut_time()) < EVENT_IGNORE_WINDOW_MS {
        return;
    }

    if handle_key_press(&event, app_handle) {
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

    monitoring_flags.set_monitoring_active(false);

    let app_handle_clone = app_handle.clone();
    std::thread::spawn(move || match tokio::runtime::Runtime::new() {
        Ok(runtime) => {
            runtime.block_on(async move {
                trigger_lockdown(app_handle_clone).await;
            });
        }
        Err(error) => {
            log::error!("创建内部运行时失败: {}", error);
        }
    });
}

fn trigger_screen_recording_activity(app_handle: AppHandle) {
    let is_recording = crate::recorder::FFMPEG_PROCESS.lock().unwrap().is_some();
    if is_recording {
        return;
    }

    tauri::async_runtime::spawn(async move {
        if let Err(error) = crate::recorder::start_screen_recording(app_handle).await {
            log::error!("启动屏幕录制失败: {}", error);
        }
    });
}

fn handle_key_press(event: &Event, app_handle: &AppHandle) -> bool {
    match &event.event_type {
        EventType::KeyPress(key) | EventType::KeyRelease(key) => {
            let state = app_handle.state::<AppState>();
            let current_shortcut = state.shortcut_key();
            let parts: Vec<&str> = current_shortcut.split('+').collect();
            if parts.is_empty() {
                return false;
            }

            let main_key = parts.last().unwrap_or(&"");

            let should_ignore = match key {
                Key::Alt | Key::AltGr => parts.contains(&"Alt"),
                Key::ControlLeft | Key::ControlRight => parts.contains(&"Ctrl"),
                Key::ShiftLeft | Key::ShiftRight => parts.contains(&"Shift"),
                Key::MetaLeft | Key::MetaRight => parts.contains(&"Meta"),
                _ => {
                    let key_name = format!("{:?}", key);
                    key_name.contains(main_key)
                        || (*main_key == "L" && matches!(key, Key::KeyL))
                        || (*main_key == "D" && matches!(key, Key::KeyD))
                        || (*main_key == "S" && matches!(key, Key::KeyS))
                        || (*main_key == "A" && matches!(key, Key::KeyA))
                        || (*main_key == "Q" && matches!(key, Key::KeyQ))
                        || (*main_key == "W" && matches!(key, Key::KeyW))
                        || (*main_key == "E" && matches!(key, Key::KeyE))
                        || (*main_key == "R" && matches!(key, Key::KeyR))
                        || (*main_key == "T" && matches!(key, Key::KeyT))
                        || (*main_key == "Y" && matches!(key, Key::KeyY))
                        || (*main_key == "U" && matches!(key, Key::KeyU))
                        || (*main_key == "I" && matches!(key, Key::KeyI))
                        || (*main_key == "O" && matches!(key, Key::KeyO))
                        || (*main_key == "P" && matches!(key, Key::KeyP))
                        || (*main_key == "F" && matches!(key, Key::KeyF))
                        || (*main_key == "G" && matches!(key, Key::KeyG))
                        || (*main_key == "H" && matches!(key, Key::KeyH))
                        || (*main_key == "J" && matches!(key, Key::KeyJ))
                        || (*main_key == "K" && matches!(key, Key::KeyK))
                        || (*main_key == "Z" && matches!(key, Key::KeyZ))
                        || (*main_key == "X" && matches!(key, Key::KeyX))
                        || (*main_key == "C" && matches!(key, Key::KeyC))
                        || (*main_key == "V" && matches!(key, Key::KeyV))
                        || (*main_key == "B" && matches!(key, Key::KeyB))
                        || (*main_key == "N" && matches!(key, Key::KeyN))
                        || (*main_key == "M" && matches!(key, Key::KeyM))
                }
            };

            if should_ignore {
                log::debug!(
                    "过滤当前快捷键相关按键: {:?} (快捷键: {})",
                    key,
                    current_shortcut
                );
            }

            should_ignore
        }
        _ => false,
    }
}

async fn trigger_lockdown(app_handle: AppHandle) {
    log::info!("=== 开始执行锁定流程 ===");

    let state_check = app_handle.state::<AppState>();
    if state_check.status() != MonitoringState::Triggered {
        log::warn!(
            "trigger_lockdown 被调用，但当前状态不是 Triggered ({:?})，取消执行",
            state_check.status()
        );
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
        await_delayed_capture(
            app_handle.clone(),
            camera_id,
            save_path.clone(),
            capture_delay_seconds,
            capture_mode,
        )
        .await;
    } else {
        execute_capture_and_lock(
            app_handle.clone(),
            camera_id,
            save_path.clone(),
            post_trigger_action.clone(),
        )
        .await;
    }

    if notifications_enabled {
        send_security_notification(&app_handle);
    }

    if screen_lock_enabled {
        lock_screen();
        sleep(Duration::from_millis(1_000)).await;
    }

    if exit_on_lock_enabled {
        crate::recorder::stop_screen_recording();
        if let Err(error) = camera::stop_video_recording().await {
            log::error!("退出前停止摄像头录像失败: {}", error);
        }
        std::process::exit(0);
    }

    if post_trigger_action == crate::config::PostTriggerAction::CaptureOnly {
        let state = app_handle.state::<AppState>();
        if state.set_status(MonitoringState::Idle).is_ok() {
            emit_monitoring_status(&app_handle, "空闲");
        }
    }

    log::info!("=== 锁定流程执行完成 ===");
}

fn send_security_notification(app_handle: &AppHandle) {
    use tauri_plugin_notification::NotificationExt;

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
) {
    log::info!(
        "开始延迟拍摄，模式: {:?}, 延迟: {}秒",
        capture_mode,
        delay_seconds
    );

    if let Err(error) =
        camera::start_video_recording(app_handle, camera_id, save_path, Some(delay_seconds)).await
    {
        log::error!("启动录像失败: {}", error);
        return;
    }

    sleep(Duration::from_secs((delay_seconds + 2).into())).await;

    if let Err(error) = camera::stop_video_recording().await {
        log::error!("清理录像进程失败: {}", error);
    }

    sleep(Duration::from_millis(2_000)).await;
}

async fn execute_capture_and_lock(
    app_handle: AppHandle,
    camera_id: u32,
    save_path: Option<String>,
    post_trigger_action: crate::config::PostTriggerAction,
) {
    if post_trigger_action == crate::config::PostTriggerAction::ScreenRecording {
        let app_handle_clone = app_handle.clone();
        if let Err(error) = crate::recorder::start_screen_recording(app_handle_clone).await {
            log::error!("启动屏幕录制失败: {}", error);
        } else if let Err(error) = camera::take_photo(camera_id, save_path).await {
            log::error!("拍照失败: {}", error);
        }
        return;
    }

    if let Err(error) = camera::take_photo(camera_id, save_path).await {
        log::error!("拍照失败: {}", error);
    }
}
