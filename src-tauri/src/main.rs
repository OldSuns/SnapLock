// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_setup;
mod camera;
mod config;
mod constants;
mod handlers;
mod logger;
mod media_files;
mod monitoring;
mod process_utils;
mod recorder;
mod state;

#[cfg(target_os = "windows")]
mod session_monitor;

use crate::{monitoring::MonitoringController, state::AppState};
use tauri::Manager;
use tauri_plugin_notification::NotificationExt;

fn main() {
    let app_state = AppState::new(0);

    let builder = app_setup::setup_tauri_builder();

    builder
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .manage(MonitoringController::new())
        .manage(recorder::Recordings::default())
        .setup(|app| {
            let handle = app.handle().clone();

            // 初始化日志系统
            if let Err(e) = logger::init_logger(handle.clone()) {
                eprintln!("Failed to initialize logger: {}", e);
            }

            // 加载配置
            if let Err(error) = config::initialize_config(handle.clone()) {
                log::error!("应用配置加载失败: {}", error);
            } else {
                log::info!("应用配置已加载");
            }

            // 初始化会话监控器 (仅Windows)
            #[cfg(target_os = "windows")]
            {
                if let Err(e) = session_monitor::start_session_monitoring(handle.clone()) {
                    log::error!("启动会话监控失败: {}", e);
                } else {
                    log::info!("会话监控已启动");
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

            // Register global shortcuts - 不要让快捷键注册失败导致程序崩溃
            if let Err(e) = app_setup::register_global_shortcuts(app) {
                log::error!("快捷键注册失败: {}", e);
                // 程序继续运行，用户可以通过系统托盘或界面操作
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    if let Err(error) = window.hide() {
                        log::error!("隐藏窗口失败: {}", error);
                    }
                }
                tauri::WindowEvent::Destroyed => {
                    // 应用退出时，确保停止所有后台进程
                    log::info!("窗口已销毁，正在停止后台进程...");
                    let handle = window.app_handle().clone();
                    tauri::async_runtime::spawn(async move {
                        handle
                            .state::<MonitoringController>()
                            .shutdown(&handle)
                            .await;
                    });
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            handlers::start_monitoring_command,
            handlers::stop_monitoring_command,
            camera::get_camera_list,
            camera::check_camera_permission,
            camera::get_camera_preview,
            handlers::set_camera_id,
            handlers::set_shortcuts_disabled,
            config::get_config,
            config::update_config,
            logger::get_debug_logs,
            logger::clear_debug_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
