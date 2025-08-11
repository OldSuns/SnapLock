// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod camera;
mod monitoring;
mod state;
mod app_setup;
mod constants;
mod handlers;
mod logger;
mod config;

#[cfg(target_os = "windows")]
mod session_monitor;

use crate::state::{AppState, MonitoringFlags};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{Manager};
use tauri_plugin_notification::NotificationExt;

fn main() {
    let app_state = AppState::new(0);

    let last_toggle_time = Arc::new(Mutex::new(Instant::now()));
    let monitoring_flags = Arc::new(MonitoringFlags::new());

    let builder = app_setup::setup_tauri_builder();

    builder
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .manage(monitoring_flags)
        .manage(last_toggle_time)
        .setup(|app| {
            let handle = app.handle().clone();
            
            // 初始化日志系统
            if let Err(e) = logger::init_logger(handle.clone()) {
                eprintln!("Failed to initialize logger: {}", e);
            }
            
            // 加载配置
            let config = config::AppConfig::load();
            let state = app.state::<AppState>();
            config.apply_to_state(&state);
            log::info!("应用配置已加载");
            
            // 初始化会话监控器 (仅Windows)
            #[cfg(target_os = "windows")]
            {
                if let Err(e) = session_monitor::init_session_monitor(handle.clone()) {
                    log::error!("初始化会话监控器失败: {}", e);
                } else {
                    // 启动会话监控
                    if let Err(e) = session_monitor::start_session_monitoring() {
                        log::error!("启动会话监控失败: {}", e);
                    } else {
                        log::info!("会话监控已启动");
                    }
                }
            }
            
            // 请求通知权限
            #[cfg(target_os = "windows")]
            {
                if let Err(e) = app.notification().request_permission() {
                    eprintln!("Failed to request notification permission: {}", e);
                }
            }
            
            // Setup tray icon
            let _tray = app_setup::setup_system_tray(&handle)?;

            // Register global shortcuts
            app_setup::register_global_shortcuts(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                window.hide().unwrap();
            }
        })
        .invoke_handler(tauri::generate_handler![
            handlers::start_monitoring_command,
            camera::get_camera_list,
            camera::check_camera_permission,
            camera::get_camera_preview,
            handlers::set_camera_id,
            camera::set_save_path,
            handlers::get_shortcut_key,
            handlers::set_shortcut_key,
            handlers::disable_shortcuts,
            handlers::enable_shortcuts,
            handlers::get_show_debug_logs,
            handlers::set_show_debug_logs,
            handlers::get_save_logs_to_file,
            handlers::set_save_logs_to_file,
            handlers::get_exit_on_lock,
            handlers::set_exit_on_lock,
            handlers::get_dark_mode,
            handlers::set_dark_mode,
            handlers::log_save_path_change,
            config::save_config,
            config::load_config,
            config::save_dark_mode_setting,
            logger::get_debug_logs,
            logger::clear_debug_logs,
            logger::set_log_to_file,
            logger::set_log_file_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
