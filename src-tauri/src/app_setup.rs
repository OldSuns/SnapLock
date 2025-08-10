// snaplock/src-tauri/src/app_setup.rs

use anyhow::Result;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager,
};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use crate::handlers;

pub fn setup_system_tray(app: &AppHandle<tauri::Wry>) -> Result<tauri::tray::TrayIcon<tauri::Wry>> {
    let toggle_item = MenuItem::with_id(app, "toggle", "显示/隐藏窗口", true, None::<&str>)?;
    let start_monitoring_item = MenuItem::with_id(app, "start_monitoring", "开始监控", true, None::<&str>)?;
    let stop_monitoring_item = MenuItem::with_id(app, "stop_monitoring", "停止监控", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[
        &toggle_item,
        &separator,
        &start_monitoring_item,
        &stop_monitoring_item,
        &separator,
        &quit_item
    ])?;

    let tray = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("SnapLock")
        .icon(app.default_window_icon().cloned().unwrap())
        .on_menu_event(|app_handle, event| match event.id().as_ref() {
            "quit" => app_handle.exit(0),
            "toggle" => {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = if window.is_visible().unwrap_or(false) {
                        window.hide()
                    } else {
                        window.show()
                    };
                }
            }
            "start_monitoring" => {
                let app_handle_clone = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    handlers::toggle_monitoring(&app_handle_clone).await;
                });
            }
            "stop_monitoring" => {
                let app_handle_clone = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    handlers::toggle_monitoring(&app_handle_clone).await;
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
                let app_handle = tray.app_handle();
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(tray)
}

pub fn register_global_shortcuts(app: &mut App<tauri::Wry>) -> Result<()> {
    let handle = app.handle().clone();
    app.global_shortcut()
        .on_shortcut("Alt+L", move |_app, _shortcut, _event| {
            let handle_clone = handle.clone();
            tauri::async_runtime::spawn(async move {
                handlers::toggle_monitoring(&handle_clone).await;
            });
        })?;
    Ok(())
}

pub fn setup_tauri_builder() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
}