// snaplock/src-tauri/src/handlers.rs

use crate::{
    constants::{PREPARATION_DELAY, SHORTCUT_DEBOUNCE_TIME, SHORTCUT_FLAG_CLEAR_DELAY},
    monitoring,
    state::{AppState, MonitoringFlags, MonitoringState},
};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

/// Helper function to reset state to Idle with proper cleanup
fn reset_to_idle_state(state: &AppState, app_handle: &AppHandle, reason: &str) {
    println!("Resetting to idle state: {}", reason);
    if state.set_status(MonitoringState::Idle).is_err() {
        eprintln!("Failed to reset state to Idle: {}", reason);
    }
    app_handle.emit("monitoring_status_changed", "空闲").unwrap();
}

/// Toggles the monitoring state.
pub async fn toggle_monitoring(app_handle: &AppHandle) {
    let state = app_handle.state::<AppState>();
    let monitoring_flags = app_handle.state::<Arc<MonitoringFlags>>().inner().clone();

    println!("切换监控状态请求，当前状态: {:?}", state.status());

    // 执行健康检查
    if !monitoring_flags.health_check() {
        println!("健康检查失败，尝试修复状态");
    }

    // Debounce shortcut
    let mut last_toggle_time = app_handle.state::<Arc<std::sync::Mutex<Instant>>>().inner().lock().unwrap();
    if last_toggle_time.elapsed() < SHORTCUT_DEBOUNCE_TIME {
        println!("快捷键防抖，忽略请求");
        return;
    }
    *last_toggle_time = Instant::now();

    // Set shortcut flag
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    monitoring_flags.set_last_shortcut_time(current_time);
    monitoring_flags.set_shortcut_in_progress(true);

    println!("设置快捷键处理标志，时间戳: {}", current_time);

    let flags_for_clear = monitoring_flags.clone();
    tokio::spawn(async move {
        tokio::time::sleep(SHORTCUT_FLAG_CLEAR_DELAY).await;
        flags_for_clear.set_shortcut_in_progress(false);
        println!("清除快捷键处理标志");
    });

    match state.status() {
        MonitoringState::Idle => {
            if state.set_status(MonitoringState::Preparing).is_ok() {
                app_handle.emit("monitoring_status_changed", "准备中").unwrap();

                let app_handle_clone = app_handle.clone();
                let monitoring_flags_clone = monitoring_flags.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(PREPARATION_DELAY).await;
                    let state = app_handle_clone.state::<AppState>();
                    
                    // Double-check state is still Preparing (user might have cancelled)
                    if state.status() != MonitoringState::Preparing {
                        println!("Monitoring preparation cancelled, current state: {:?}", state.status());
                        return;
                    }

                    // Attempt to transition to Active state
                    if state.set_status(MonitoringState::Active).is_err() {
                        eprintln!("Failed to transition to Active state");
                        return;
                    }

                    // Try to start monitoring with proper error handling
                    println!("尝试启动监控线程...");
                    match monitoring::start_monitoring(app_handle_clone.clone(), monitoring_flags_clone.clone()) {
                        Ok(monitoring_handle) => {
                            println!("监控线程创建成功，尝试原子性启动...");
                            // Atomically start monitoring and store handle
                            if monitoring_flags_clone.start_monitoring_atomic(monitoring_handle) {
                                // Success: emit status change and show notification
                                app_handle_clone.emit("monitoring_status_changed", "警戒中").unwrap();
                                
                                println!("监控成功启动，发送通知...");
                                if let Err(e) = app_handle_clone.notification()
                                    .builder()
                                    .title("SnapLock")
                                    .body("已进入警戒状态，正在监控活动")
                                    .show() {
                                    eprintln!("Failed to show notification: {}", e);
                                }
                                
                                if let Some(window) = app_handle_clone.get_webview_window("main") {
                                    let _ = window.hide();
                                }
                                
                                println!("✓ 监控启动成功");
                            } else {
                                // Failed to start monitoring atomically (already running)
                                eprintln!("原子性启动失败：监控已在运行中");
                                reset_to_idle_state(&state, &app_handle_clone, "监控已在运行中");
                            }
                        }
                        Err(e) => {
                            // Failed to create monitoring thread
                            eprintln!("监控线程创建失败: {}", e);
                            reset_to_idle_state(&state, &app_handle_clone, "监控启动失败");
                        }
                    }
                });
            }
        }
        MonitoringState::Preparing | MonitoringState::Active => {
            let was_active = state.status() == MonitoringState::Active;
            
            // Stop monitoring thread and reset state
            if was_active {
                monitoring_flags.stop_monitoring_thread();
            }
            
            if state.set_status(MonitoringState::Idle).is_ok() {
                app_handle.emit("monitoring_status_changed", "空闲").unwrap();
                if was_active {
                    // 改进的系统通知调用，添加错误处理
                    if let Err(e) = app_handle.notification()
                        .builder()
                        .title("SnapLock")
                        .body("已退出警戒状态")
                        .show() {
                        eprintln!("Failed to show notification: {}", e);
                    }
                    
                }
                println!("Monitoring stopped successfully");
            }
        }
    }
}

/// Sets the selected camera ID in the application state
#[tauri::command]
pub fn set_camera_id(app_handle: tauri::AppHandle, camera_id: u32) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    state.set_camera_id(camera_id);
    println!("Camera ID updated to: {}", camera_id);
    Ok(())
}



/// Sets the custom save path for photos.
#[tauri::command]
pub fn set_save_path(app_handle: tauri::AppHandle, path: Option<String>) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    state.set_save_path(path.clone());
    println!("Save path updated to: {:?}", path);
    Ok(())
}

/// Sets whether the app should exit after locking the screen.
#[tauri::command]
pub fn set_exit_on_lock(app_handle: tauri::AppHandle, exit: bool) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    *state.exit_on_lock.lock().unwrap() = exit;
    println!("Exit on lock setting updated to: {}", exit);
    Ok(())
}

/// A Tauri command to start or stop the monitoring process from the frontend.
#[tauri::command]
pub async fn start_monitoring_command(app_handle: AppHandle, camera_id: u32) {
    let state = app_handle.state::<AppState>();
    state.set_camera_id(camera_id);
    toggle_monitoring(&app_handle).await;
}