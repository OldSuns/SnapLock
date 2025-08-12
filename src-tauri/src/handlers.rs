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
    log::info!("重置为空闲状态: {}", reason);
    if state.set_status(MonitoringState::Idle).is_err() {
        log::error!("无法重置状态为空闲: {}", reason);
    }
    app_handle.emit("monitoring_status_changed", "空闲").unwrap();
}

/// Toggles the monitoring state.
pub async fn toggle_monitoring(app_handle: &AppHandle) {
    let state = app_handle.state::<AppState>();
    let monitoring_flags = app_handle.state::<Arc<MonitoringFlags>>().inner().clone();

    log::info!("切换监控状态请求，当前状态: {:?}", state.status());

    // 执行健康检查
    if !monitoring_flags.health_check() {
        log::warn!("健康检查失败，尝试修复状态");
    }

    // Debounce shortcut
    let mut last_toggle_time = app_handle.state::<Arc<std::sync::Mutex<Instant>>>().inner().lock().unwrap();
    if last_toggle_time.elapsed() < SHORTCUT_DEBOUNCE_TIME {
        log::debug!("快捷键防抖，忽略请求");
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

    log::debug!("设置快捷键处理标志，时间戳: {}", current_time);

    let flags_for_clear = monitoring_flags.clone();
    tokio::spawn(async move {
        tokio::time::sleep(SHORTCUT_FLAG_CLEAR_DELAY).await;
        flags_for_clear.set_shortcut_in_progress(false);
        log::debug!("清除快捷键处理标志");
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
                        log::info!("监控准备已取消，当前状态: {:?}", state.status());
                        return;
                    }

                    // Attempt to transition to Active state
                    if state.set_status(MonitoringState::Active).is_err() {
                        log::error!("无法转换到激活状态");
                        return;
                    }

                    // Try to start monitoring with proper error handling
                    log::info!("尝试启动监控线程...");
                    match monitoring::start_monitoring(app_handle_clone.clone(), monitoring_flags_clone.clone()) {
                        Ok(monitoring_handle) => {
                            log::info!("监控线程创建成功，尝试启动...");
                            // Atomically start monitoring and store handle
                            if monitoring_flags_clone.start_monitoring_atomic(monitoring_handle) {
                                // Success: emit status change and show notification
                                app_handle_clone.emit("monitoring_status_changed", "警戒中").unwrap();
                                
                                // 检查通知开关状态
                                let notifications_enabled = state.enable_notifications();
                                if notifications_enabled {
                                    log::info!("监控成功启动，发送通知...");
                                    if let Err(e) = app_handle_clone.notification()
                                        .builder()
                                        .title("SnapLock")
                                        .body("已进入警戒状态，正在监控活动")
                                        .show() {
                                        log::error!("无法显示通知: {}", e);
                                    }
                                } else {
                                    log::info!("监控成功启动，通知功能已禁用");
                                }
                                
                                if let Some(window) = app_handle_clone.get_webview_window("main") {
                                    let _ = window.hide();
                                }
                                
                                log::info!("✓ 监控启动成功");
                            } else {
                                // Failed to start monitoring atomically (already running)
                                log::error!("启动失败：监控已在运行中");
                                reset_to_idle_state(&state, &app_handle_clone, "监控已在运行中");
                            }
                        }
                        Err(e) => {
                            // Failed to create monitoring thread
                            log::error!("监控线程创建失败: {}", e);
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
                    // 检查通知开关状态
                    let notifications_enabled = state.enable_notifications();
                    if notifications_enabled {
                        // 改进的系统通知调用，添加错误处理
                        if let Err(e) = app_handle.notification()
                            .builder()
                            .title("SnapLock")
                            .body("已退出警戒状态")
                            .show() {
                            log::error!("无法显示通知: {}", e);
                        }
                    } else {
                        log::info!("监控已停止，通知功能已禁用");
                    }
                }
                log::info!("监控已成功停止");
            }
        }
    }
}

/// Sets the selected camera ID in the application state
#[tauri::command]
pub fn set_camera_id(app_handle: tauri::AppHandle, camera_id: u32) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    state.set_camera_id(camera_id);
    log::info!("摄像头ID已更新为: {}", camera_id);
    Ok(())
}

/// A Tauri command to start or stop the monitoring process from the frontend.
#[tauri::command]
pub async fn start_monitoring_command(app_handle: AppHandle, camera_id: u32) {
    let state = app_handle.state::<AppState>();
    state.set_camera_id(camera_id);
    toggle_monitoring(&app_handle).await;
}

/// Gets the current shortcut key
#[tauri::command]
pub fn get_shortcut_key(app_handle: tauri::AppHandle) -> Result<String, String> {
    let state = app_handle.state::<AppState>();
    Ok(state.shortcut_key())
}

/// Sets a new shortcut key
#[tauri::command]
pub async fn set_shortcut_key(app_handle: tauri::AppHandle, shortcut: String) -> Result<(), String> {
    // Validate the shortcut format
    if !is_valid_shortcut(&shortcut) {
        return Err("无效的快捷键格式".to_string());
    }

    let state = app_handle.state::<AppState>();
    let old_shortcut = state.shortcut_key();
    
    // Update the shortcut in state
    state.set_shortcut_key(shortcut.clone());
    
    // Re-register the global shortcut
    if let Err(e) = crate::app_setup::update_global_shortcut(&app_handle, &old_shortcut, &shortcut).await {
        // If re-registration fails, revert the state
        state.set_shortcut_key(old_shortcut);
        return Err(format!("快捷键注册失败: {}", e));
    }
    
    // 自动保存配置
    if let Err(e) = crate::config::save_config(app_handle.clone()) {
        log::warn!("保存配置失败: {}", e);
    }
    
    log::info!("快捷键已更新为: {}", shortcut);
    Ok(())
}

/// Validates if a shortcut string is in valid format
fn is_valid_shortcut(shortcut: &str) -> bool {
    // Basic validation - check if it contains valid modifier keys and a key
    let parts: Vec<&str> = shortcut.split('+').collect();
    if parts.len() < 2 {
        return false;
    }
    
    let modifiers = &parts[..parts.len()-1];
    let key = parts.last().unwrap();
    
    // Check if modifiers are valid
    for modifier in modifiers {
        if !matches!(*modifier, "Ctrl" | "Alt" | "Shift" | "Meta" | "Cmd") {
            return false;
        }
    }
    
    // Check if key is not empty and not a modifier
    !key.is_empty() && !matches!(*key, "Ctrl" | "Alt" | "Shift" | "Meta" | "Cmd")
}

/// Disables global shortcuts temporarily
#[tauri::command]
pub fn disable_shortcuts(app_handle: tauri::AppHandle) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    state.set_shortcuts_disabled(true);
    log::info!("全局快捷键已禁用");
    Ok(())
}

/// Enables global shortcuts
#[tauri::command]
pub fn enable_shortcuts(app_handle: tauri::AppHandle) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    state.set_shortcuts_disabled(false);
    log::info!("全局快捷键已启用");
    Ok(())
}

/// Gets the debug logs display setting
#[tauri::command]
pub fn get_show_debug_logs(app_handle: tauri::AppHandle) -> Result<bool, String> {
    let state = app_handle.state::<AppState>();
    Ok(state.show_debug_logs())
}

/// Sets the debug logs display setting
#[tauri::command]
pub fn set_show_debug_logs(app_handle: tauri::AppHandle, show: bool) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    state.set_show_debug_logs(show);
    
    // 自动保存配置
    if let Err(e) = crate::config::save_config(app_handle.clone()) {
        log::warn!("保存配置失败: {}", e);
    }
    
    log::info!("调试日志显示设置已更新为: {}", show);
    Ok(())
}

/// Gets the save logs to file setting
#[tauri::command]
pub fn get_save_logs_to_file(app_handle: tauri::AppHandle) -> Result<bool, String> {
    let state = app_handle.state::<AppState>();
    Ok(state.save_logs_to_file())
}

/// Sets the save logs to file setting
#[tauri::command]
pub fn set_save_logs_to_file(app_handle: tauri::AppHandle, save: bool) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    state.set_save_logs_to_file(save);
    
    // 如果启用了保存到文件，设置日志文件路径
    if save {
        let save_path = state.get_effective_save_path();
        if let Some(logger) = crate::logger::get_logger() {
            logger.set_log_file_path(Some(save_path));
        }
    }
    
    // 自动保存配置
    if let Err(e) = crate::config::save_config(app_handle.clone()) {
        log::warn!("保存配置失败: {}", e);
    }
    
    log::info!("日志保存到文件设置已更新为: {}", save);
    Ok(())
}

/// 记录保存路径更改的日志
#[tauri::command]
pub fn log_save_path_change(old_path: String, new_path: String) -> Result<(), String> {
    log::info!("保存路径已更改: '{}' -> '{}'", old_path, new_path);
    Ok(())
}

/// 获取锁定时退出设置
#[tauri::command]
pub fn get_exit_on_lock(app_handle: tauri::AppHandle) -> Result<bool, String> {
    let state = app_handle.state::<AppState>();
    Ok(state.exit_on_lock())
}

/// 设置锁定时退出选项
#[tauri::command]
pub fn set_exit_on_lock(app_handle: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    state.set_exit_on_lock(enabled);
    
    // 自动保存配置
    if let Err(e) = crate::config::save_config(app_handle.clone()) {
        log::warn!("保存配置失败: {}", e);
    }
    
    log::info!("锁定时退出设置已更新为: {}", enabled);
    Ok(())
}

/// 获取暗色模式设置
#[tauri::command]
pub fn get_dark_mode(app_handle: tauri::AppHandle) -> Result<bool, String> {
    let state = app_handle.state::<crate::state::AppState>();
    Ok(state.dark_mode())
}

/// 设置暗色模式
#[tauri::command]
pub fn set_dark_mode(app_handle: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let state = app_handle.state::<crate::state::AppState>();
    state.set_dark_mode(enabled);
    
    // 自动保存配置
    if let Err(e) = crate::config::save_config(app_handle.clone()) {
        log::warn!("保存配置失败: {}", e);
    }
    
    log::info!("暗色模式设置已更新为: {}", enabled);
    Ok(())
}

/// 获取锁屏功能启用状态
#[tauri::command]
pub fn get_enable_screen_lock(app_handle: tauri::AppHandle) -> Result<bool, String> {
    let state = app_handle.state::<crate::state::AppState>();
    Ok(state.enable_screen_lock())
}

/// 设置锁屏功能启用状态
#[tauri::command]
pub fn set_enable_screen_lock(app_handle: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let state = app_handle.state::<crate::state::AppState>();
    state.set_enable_screen_lock(enabled);
    
    // 自动保存配置
    if let Err(e) = crate::config::save_config(app_handle.clone()) {
        log::warn!("保存配置失败: {}", e);
    }
    
    log::info!("锁屏功能启用设置已更新为: {}", enabled);
    Ok(())
}

/// 获取系统通知启用状态
#[tauri::command]
pub fn get_enable_notifications(app_handle: tauri::AppHandle) -> Result<bool, String> {
    let state = app_handle.state::<crate::state::AppState>();
    Ok(state.enable_notifications())
}

/// 设置系统通知启用状态
#[tauri::command]
pub fn set_enable_notifications(app_handle: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let state = app_handle.state::<crate::state::AppState>();
    state.set_enable_notifications(enabled);
    
    // 自动保存配置
    if let Err(e) = crate::config::save_config(app_handle.clone()) {
        log::warn!("保存配置失败: {}", e);
    }
    
    log::info!("系统通知启用设置已更新为: {}", enabled);
    Ok(())
}