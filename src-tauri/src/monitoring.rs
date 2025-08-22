use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{camera, constants::EVENT_IGNORE_WINDOW_MS, state::{AppState, MonitoringFlags, MonitoringState}};
use rdev::{listen, Event, EventType, Key};
use tauri::{AppHandle, Manager, Emitter};
use tokio::{runtime::Runtime as TokioRuntime, task, time::sleep};

pub fn lock_screen() {
    log::info!("执行锁屏命令...");
    match Command::new("rundll32.exe")
        .args(["user32.dll,LockWorkStation"])
        .spawn()
    {
        Ok(mut child) => {
            log::info!("锁屏命令已启动，进程ID: {:?}", child.id());
            match child.wait() {
                Ok(status) => {
                    if status.success() {
                        log::info!("锁屏命令执行成功");
                    } else {
                        log::error!("锁屏命令执行失败，退出码: {:?}", status.code());
                    }
                }
                Err(e) => {
                    log::error!("等待锁屏命令完成时发生错误: {}", e);
                }
            }
        }
        Err(e) => {
            log::error!("启动锁屏命令失败: {}", e);
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
            sleep(Duration::from_secs(5)).await;

            if !monitoring_flags.monitoring_active() {
                log::debug!("监控非激活状态，空闲检测循环终止");
                break; // Exit the loop if monitoring is no longer active
            }

            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            
            let last_activity = monitoring_flags.last_activity_time();
            let idle_time_ms = current_time.saturating_sub(last_activity);

            let is_recording = crate::recorder::FFMPEG_PROCESS.lock().unwrap().is_some();

            if idle_time_ms > 20000 && is_recording {
                log::info!("超过20秒无操作，暂停屏幕录制...");
                crate::recorder::stop_screen_recording();
            }
            else if idle_time_ms <= 20000 && !is_recording {
                log::info!("检测到用户活动，恢复屏幕录制...");
                let app_handle_clone = app_handle.clone();
                // We need to spawn a new task for the async function call
                tokio::spawn(async move {
                    if let Err(e) = crate::recorder::start_screen_recording(app_handle_clone).await {
                        log::error!("无法恢复屏幕录制: {}", e);
                    }
                });
            }
        }
    })
}


/// Starts the global input monitor on a blocking-safe Tokio thread.
pub fn start_monitoring(
    app_handle: AppHandle,
    monitoring_flags: Arc<MonitoringFlags>,
) -> Result<task::JoinHandle<()>, String> {
    let rt = Arc::new(TokioRuntime::new().map_err(|e| format!("Failed to create Tokio runtime: {}", e))?);
    let rt_clone = rt.clone();

    let handle = rt.spawn_blocking(move || {
        let callback_handle = app_handle.clone();
        let flags_handle = monitoring_flags.clone();
        
        log::info!("启动rdev事件监听器...");
        
        let error_callback_handle = callback_handle.clone();
        let error_flags_handle = flags_handle.clone();
        
        if let Err(error) = listen(move |event| {
            callback(event, &callback_handle, &flags_handle, &rt_clone);
        }) {
            log::error!("rdev事件监听器严重错误: {:?}", error);
            log::error!("由于监听器故障停止监控");
            
            error_flags_handle.set_monitoring_active(false);
            
            let state = error_callback_handle.state::<AppState>();
            if state.set_status(MonitoringState::Idle).is_ok() {
                error_callback_handle.emit("monitoring_status_changed", "空闲").unwrap_or_else(|e| {
                    log::error!("无法发送状态变化事件: {}", e);
                });
            }
        }
        
        log::info!("rdev事件监听器线程退出");
    });

    Ok(handle)
}

/// The primary callback for `rdev` events.
fn callback(event: Event, app_handle: &AppHandle, monitoring_flags: &Arc<MonitoringFlags>, _rt: &TokioRuntime) {
    let monitoring_active = monitoring_flags.monitoring_active();
    let shortcut_in_progress = monitoring_flags.shortcut_in_progress();
    let last_shortcut_time = monitoring_flags.last_shortcut_time();
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    // Update last activity time on any valid input event
    if monitoring_active {
        monitoring_flags.set_last_activity_time(current_time);
    }

    if !monitoring_active {
        return;
    }

    if !monitoring_flags.is_monitoring_thread_alive() {
        log::error!("监控线程已终止，停止监控");
        monitoring_flags.set_monitoring_active(false);
        let state = app_handle.state::<AppState>();
        if state.set_status(MonitoringState::Idle).is_ok() {
            app_handle.emit("monitoring_status_changed", "空闲").unwrap_or_else(|e| {
                log::error!("无法发送状态变化事件: {}", e);
            });
        }
        return;
    }

    if shortcut_in_progress {
        return;
    }

    if current_time.saturating_sub(last_shortcut_time) < EVENT_IGNORE_WINDOW_MS {
        return;
    }

    if handle_key_press(&event, app_handle) {
        return;
    }

    let state = app_handle.state::<AppState>();
    
    // 如果是屏幕录制模式，回调只负责更新活动时间，不触发锁定流程
    if state.post_trigger_action() == crate::config::PostTriggerAction::ScreenRecording {
        log::debug!("屏幕录制模式下检测到活动");
        // The idle check loop will handle starting/stopping the recording.
        return;
    }

    log::info!("✓ 触发锁定！事件类型: {:?}", event.event_type);

    if state.set_status(MonitoringState::Triggered).is_err() {
        log::warn!("状态转换到Triggered失败，可能已被其他线程处理。忽略此事件。");
        return;
    }
    
    monitoring_flags.set_monitoring_active(false);
    
    let app_handle_clone = app_handle.clone();
    
    std::thread::spawn(move || {
        log::info!("锁定任务已启动...");
        
        match tokio::runtime::Runtime::new() {
            Ok(rt_inner) => {
                log::info!("创建内部运行时成功");
                rt_inner.block_on(async move {
                    log::info!("开始执行锁定流程...");
                    trigger_lockdown(app_handle_clone).await;
                });
                log::info!("锁定流程执行完成");
            }
            Err(e) => {
                log::error!("创建内部运行时失败: {}", e);
            }
        }
    });
    
    log::info!("锁定任务句柄创建成功");
}

/// Handles key press events to filter out shortcut-related keys.
fn handle_key_press(event: &Event, app_handle: &AppHandle) -> bool {
    match &event.event_type {
        EventType::KeyPress(key) | EventType::KeyRelease(key) => {
            let state = app_handle.state::<AppState>();
            let current_shortcut = state.shortcut_key();
            
            let parts: Vec<&str> = current_shortcut.split('+').collect();
            if parts.is_empty() {
                return false;
            }
            
            let main_key = parts.last().unwrap();
            
            let should_ignore = match key {
                Key::Alt | Key::AltGr => parts.contains(&"Alt"),
                Key::ControlLeft | Key::ControlRight => parts.contains(&"Ctrl"),
                Key::ShiftLeft | Key::ShiftRight => parts.contains(&"Shift"),
                Key::MetaLeft | Key::MetaRight => parts.contains(&"Meta"),
                _ => {
                    let key_name = format!("{:?}", key);
                    key_name.contains(main_key) ||
                    (*main_key == "L" && matches!(key, Key::KeyL)) ||
                    (*main_key == "D" && matches!(key, Key::KeyD)) ||
                    (*main_key == "S" && matches!(key, Key::KeyS)) ||
                    (*main_key == "A" && matches!(key, Key::KeyA)) ||
                    (*main_key == "Q" && matches!(key, Key::KeyQ)) ||
                    (*main_key == "W" && matches!(key, Key::KeyW)) ||
                    (*main_key == "E" && matches!(key, Key::KeyE)) ||
                    (*main_key == "R" && matches!(key, Key::KeyR)) ||
                    (*main_key == "T" && matches!(key, Key::KeyT)) ||
                    (*main_key == "Y" && matches!(key, Key::KeyY)) ||
                    (*main_key == "U" && matches!(key, Key::KeyU)) ||
                    (*main_key == "I" && matches!(key, Key::KeyI)) ||
                    (*main_key == "O" && matches!(key, Key::KeyO)) ||
                    (*main_key == "P" && matches!(key, Key::KeyP)) ||
                    (*main_key == "F" && matches!(key, Key::KeyF)) ||
                    (*main_key == "G" && matches!(key, Key::KeyG)) ||
                    (*main_key == "H" && matches!(key, Key::KeyH)) ||
                    (*main_key == "J" && matches!(key, Key::KeyJ)) ||
                    (*main_key == "K" && matches!(key, Key::KeyK)) ||
                    (*main_key == "Z" && matches!(key, Key::KeyZ)) ||
                    (*main_key == "X" && matches!(key, Key::KeyX)) ||
                    (*main_key == "C" && matches!(key, Key::KeyC)) ||
                    (*main_key == "V" && matches!(key, Key::KeyV)) ||
                    (*main_key == "B" && matches!(key, Key::KeyB)) ||
                    (*main_key == "N" && matches!(key, Key::KeyN)) ||
                    (*main_key == "M" && matches!(key, Key::KeyM))
                }
            };
            
            if should_ignore {
                log::debug!("过滤当前快捷键相关按键: {:?} (快捷键: {})", key, current_shortcut);
            }
            should_ignore
        },
        _ => false,
    }
}

/// Asynchronously triggers photo capture, screen lock, and application exit.
async fn trigger_lockdown(app_handle: AppHandle) {
    log::info!("=== 开始执行锁定流程 ===");

    let state_check = app_handle.state::<AppState>();
    if state_check.status() != crate::state::MonitoringState::Triggered {
        log::warn!("trigger_lockdown被调用，但当前状态不是Triggered ({:?})。取消执行。", state_check.status());
        return;
    }
    
    app_handle.emit("monitoring_status_changed", "锁定中").unwrap_or_else(|e| {
        log::error!("无法发送锁定状态事件: {}", e);
    });
    
    let (camera_id, save_path, exit_on_lock_enabled, post_trigger_action, notifications_enabled) = {
        let state = app_handle.state::<AppState>();
        let camera_id = state.camera_id();
        let save_path = state.save_path();
        let exit_on_lock = state.exit_on_lock();
        let post_trigger_action = state.post_trigger_action();
        let notifications = state.enable_notifications();
        (camera_id, save_path, exit_on_lock, post_trigger_action, notifications)
    };
    
    let screen_lock_enabled = match post_trigger_action {
        crate::config::PostTriggerAction::CaptureAndLock => true,
        crate::config::PostTriggerAction::CaptureOnly => false,
        crate::config::PostTriggerAction::ScreenRecording => false,
    };

    log::info!("监控触发，使用摄像头ID: {}, 触发后动作: {:?}, 通知功能: {}, 锁定时退出: {}",
        camera_id, post_trigger_action, notifications_enabled, exit_on_lock_enabled);

    if post_trigger_action == crate::config::PostTriggerAction::ScreenRecording {
        log::info!("开始屏幕录制...");
        // This block is now primarily for legacy or non-idle-check scenarios.
        // The main logic is in the idle_check_loop.
        let app_handle_clone = app_handle.clone();
        if let Err(e) = crate::recorder::start_screen_recording(app_handle_clone).await {
            log::error!("启动屏幕录制失败: {}", e);
        } else {
            log::info!("屏幕录制已启动");
            log::info!("开始拍照...");
            if let Err(e) = camera::take_photo(camera_id, save_path).await {
                log::error!("拍照失败: {}", e);
            } else {
                log::info!("拍照完成");
            }
        }
    } else {
        log::info!("开始拍照...");
        if let Err(e) = camera::take_photo(camera_id, save_path).await {
            log::error!("拍照失败: {}", e);
        } else {
            log::info!("拍照完成");
        }
    }

    if notifications_enabled {
        log::info!("通知功能已启用，发送系统通知...");
        send_security_notification(&app_handle);
    } else {
        log::info!("通知功能已禁用，跳过通知步骤");
    }

    if screen_lock_enabled {
        log::info!("锁屏功能已启用，准备执行锁屏...");
        lock_screen();
        
        log::info!("等待锁屏命令完成...");
        sleep(Duration::from_millis(1000)).await;
    } else {
        log::info!("锁屏功能已禁用，跳过锁屏步骤");
    }
    
    if exit_on_lock_enabled {
        log::info!("锁定时退出已启用，准备退出程序...");
        std::process::exit(0);
    } else {
        log::info!("锁定时退出已禁用，程序继续运行");
        
        if post_trigger_action == crate::config::PostTriggerAction::CaptureOnly {
            log::info!("“只拍摄”模式完成，主动重置应用状态为空闲");
            let state = app_handle.state::<AppState>();
            
            let reset_success = if state.set_status(MonitoringState::Idle).is_ok() {
                log::info!("成功重置状态为空闲");
                true
            } else {
                log::warn!("重置状态到Idle失败，可能状态已被改变");
                false
            };
            
            if reset_success {
                app_handle.emit("monitoring_status_changed", "空闲").unwrap_or_else(|e| {
                    log::error!("无法发送状态重置事件: {}", e);
                });
                log::info!("已发送状态重置事件到前端");
            }
        } else {
            log::info!("动作 {:?} 已启动，等待用户或系统事件来重置状态", post_trigger_action);
        }
    }
    
    log::info!("=== 锁定流程执行完成 ===");
}

/// 发送安全通知
fn send_security_notification(app_handle: &AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    
    let notification_result = app_handle
        .notification()
        .builder()
        .title("SnapLock 安全警报")
        .body("检测到未授权访问")
        .icon("📷")
        .show();
    
    match notification_result {
        Ok(_) => {
            log::info!("安全通知发送成功");
        }
        Err(e) => {
            log::error!("发送安全通知失败: {}", e);
        }
    }
}
