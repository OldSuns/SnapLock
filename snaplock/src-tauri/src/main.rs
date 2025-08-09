// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod camera;
mod monitoring;
mod state;

use crate::state::{AppState, MonitoringState};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_notification::NotificationExt;

/// A Tauri command to start or stop the monitoring process from the frontend.
///
/// This command toggles the monitoring state. If the state is `Idle`, it transitions
/// to `Preparing` and then `Active`. If it's `Preparing` or `Active`, it transitions
/// back to `Idle`.
#[tauri::command]
fn start_monitoring_command(app_handle: tauri::AppHandle, camera_index: usize) {
    let state = app_handle.state::<AppState>();
    *state.camera_index.lock().unwrap() = camera_index;

    let mut status = state.status.lock().unwrap();
    match *status {
        MonitoringState::Idle => {
            *status = MonitoringState::Preparing;
            app_handle
                .emit("monitoring_status_changed", "准备中")
                .unwrap();
            drop(status);

            let handle_clone_inner = app_handle.clone();
            let monitoring_flag = app_handle.state::<Arc<AtomicBool>>().inner().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                let state = handle_clone_inner.state::<AppState>();
                let mut status_guard = state.status.lock().unwrap();
                                if *status_guard == MonitoringState::Preparing {
                                    *status_guard = MonitoringState::Active;
                                    monitoring_flag.store(true, Ordering::SeqCst);
                                    handle_clone_inner
                                        .emit("monitoring_status_changed", "警戒中")
                                        .unwrap();
                                    
                                    // 发送通知：进入警戒状态
                                    let _ = handle_clone_inner
                                        .notification()
                                        .builder()
                                        .title("SnapLock")
                                        .body("已进入警戒状态，正在监控活动")
                                        .show();
                                    
                                    monitoring::start_monitoring(camera_index, monitoring_flag);
                                }
            });
        }
        MonitoringState::Preparing | MonitoringState::Active => {
            let was_active = *status == MonitoringState::Active;
            *status = MonitoringState::Idle;
            let monitoring_flag = app_handle.state::<Arc<AtomicBool>>().clone();
            monitoring_flag.store(false, Ordering::SeqCst);
            app_handle
                .emit("monitoring_status_changed", "空闲")
                .unwrap();
            
            // 只有从Active状态退出时才发送通知
            if was_active {
                let _ = app_handle
                    .notification()
                    .builder()
                    .title("SnapLock")
                    .body("已退出警戒状态")
                    .show();
            }
        }
    }
}

fn main() {
    let app_state = AppState {
        status: Mutex::new(MonitoringState::Idle),
        camera_index: Mutex::new(0),
    };

    let last_toggle_time = Arc::new(Mutex::new(Instant::now()));
    let monitoring_flag = Arc::new(AtomicBool::new(false));

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .manage(app_state)
        .manage(monitoring_flag.clone())
        .setup(move |app| {
            let handle = app.handle().clone();
            let last_toggle_time_clone = Arc::clone(&last_toggle_time);
            let monitoring_flag_clone = Arc::clone(&monitoring_flag);

            // --- Tray Icon and Menu Setup ---
            let toggle_item = MenuItem::new(&handle, "toggle", "显示/隐藏", true, None::<&str>)?;
            let quit_item = MenuItem::new(&handle, "quit", "退出", true, None::<&str>)?;

            let menu = Menu::with_items(&handle, &[&toggle_item, &quit_item])?;

            let _tray = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("SnapLock")
                .on_menu_event(move |app_handle, event| match event.id().as_ref() {
                    "quit" => {
                        app_handle.exit(0);
                    }
                    "toggle" => {
                        if let Some(window) = app_handle.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        ..
                    } = event
                    {
                        let app_handle = tray.app_handle();
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app.handle())?;
            // --- End Tray Icon Setup ---

            // --- Shortcut Setup ---
            let shortcuts = app.global_shortcut();
            let handle_clone_for_event = handle.clone();
            shortcuts
                .on_shortcut("Alt+L", move |_app_handle, _shortcut, _event| {
                    let mut last_time = last_toggle_time_clone.lock().unwrap();
                    if last_time.elapsed() < Duration::from_millis(500) {
                        return;
                    }
                    *last_time = Instant::now();

                    let state = handle_clone_for_event.state::<AppState>();
                    let mut status = state.status.lock().unwrap();

                    match *status {
                        MonitoringState::Idle => {
                            *status = MonitoringState::Preparing;
                            // 向前端发送状态变化事件
                            handle_clone_for_event
                                .emit("monitoring_status_changed", "准备中")
                                .unwrap();

                            // 释放锁，以便其他线程可以访问状态
                            drop(status);

                            let camera_index = *state.camera_index.lock().unwrap();
                            let handle_clone_inner = handle_clone_for_event.clone();
                            let monitoring_flag_inner = Arc::clone(&monitoring_flag_clone);

                            tauri::async_runtime::spawn(async move {
                                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

                                let state = handle_clone_inner.state::<AppState>();
                                let mut status_guard = state.status.lock().unwrap();
                                // 检查当前状态是否仍然是 Preparing
                                if *status_guard == MonitoringState::Preparing {
                                    *status_guard = MonitoringState::Active;
                                    monitoring_flag_inner.store(true, Ordering::SeqCst);
                                    handle_clone_inner
                                        .emit("monitoring_status_changed", "警戒中")
                                        .unwrap();
                                    
                                    // 发送通知：进入警戒状态
                                    let _ = handle_clone_inner
                                        .notification()
                                        .builder()
                                        .title("SnapLock")
                                        .body("正在监控活动")
                                        .show();
                                    
                                    monitoring::start_monitoring(
                                        camera_index,
                                        monitoring_flag_inner,
                                    );
                                } else {
                                    // 状态已改变（被用户取消），什么都不做
                                }
                            });
                        }
                        MonitoringState::Preparing => {
                            *status = MonitoringState::Idle;
                            monitoring_flag_clone.store(false, Ordering::SeqCst);
                            handle_clone_for_event
                                .emit("monitoring_status_changed", "空闲")
                                .unwrap();
                        }
                        MonitoringState::Active => {
                            *status = MonitoringState::Idle;
                            monitoring_flag_clone.store(false, Ordering::SeqCst);
                            handle_clone_for_event
                                .emit("monitoring_status_changed", "空闲")
                                .unwrap();
                            
                            // 发送通知：手动退出警戒状态
                            let _ = handle_clone_for_event
                                .notification()
                                .builder()
                                .title("SnapLock")
                                .body("已退出监控状态")
                                .show();
                        }
                    }
                })
                .expect("Failed to register shortcut");
            // --- End Shortcut Setup ---

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            start_monitoring_command,
            camera::get_camera_list
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
