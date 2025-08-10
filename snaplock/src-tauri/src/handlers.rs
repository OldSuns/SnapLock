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

/// Toggles the monitoring state.
pub async fn toggle_monitoring(app_handle: &AppHandle) {
    let state = app_handle.state::<AppState>();
    let monitoring_flags = app_handle.state::<Arc<MonitoringFlags>>().inner().clone();

    // Debounce shortcut
    let mut last_toggle_time = app_handle.state::<Arc<std::sync::Mutex<Instant>>>().inner().lock().unwrap();
    if last_toggle_time.elapsed() < SHORTCUT_DEBOUNCE_TIME {
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

    let flags_for_clear = monitoring_flags.clone();
    tokio::spawn(async move {
        tokio::time::sleep(SHORTCUT_FLAG_CLEAR_DELAY).await;
        flags_for_clear.set_shortcut_in_progress(false);
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
                    if state.status() == MonitoringState::Preparing {
                        if state.set_status(MonitoringState::Active).is_ok() {
                            monitoring_flags_clone.set_monitoring_active(true);
                            app_handle_clone.emit("monitoring_status_changed", "警戒中").unwrap();
                            app_handle_clone.notification().builder().title("SnapLock").body("已进入警戒状态，正在监控活动").show().unwrap();
                            if let Err(e) = monitoring::start_monitoring(app_handle_clone.clone(), monitoring_flags_clone) {
                                eprintln!("Failed to start monitoring: {}", e);
                                if state.set_status(MonitoringState::Idle).is_err() {
                                    eprintln!("Failed to reset state to Idle after monitoring start failure.");
                                }
                                app_handle_clone.emit("monitoring_status_changed", "空闲").unwrap();
                            }
                        } else {
                            // Transition to Active failed, reset to Idle
                            if state.set_status(MonitoringState::Idle).is_err() {
                                eprintln!("Failed to reset state to Idle after transition to Active failed.");
                            }
                            app_handle_clone.emit("monitoring_status_changed", "空闲").unwrap();
                        }
                    }
                });
            }
        }
        MonitoringState::Preparing | MonitoringState::Active => {
            let was_active = state.status() == MonitoringState::Active;
            if state.set_status(MonitoringState::Idle).is_ok() {
                monitoring_flags.set_monitoring_active(false);
                app_handle.emit("monitoring_status_changed", "空闲").unwrap();
                if was_active {
                    app_handle.notification().builder().title("SnapLock").body("已退出警戒状态").show().unwrap();
                }
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

/// A Tauri command to start or stop the monitoring process from the frontend.
#[tauri::command]
pub async fn start_monitoring_command(app_handle: AppHandle, camera_id: u32) {
    let state = app_handle.state::<AppState>();
    state.set_camera_id(camera_id);
    toggle_monitoring(&app_handle).await;
}