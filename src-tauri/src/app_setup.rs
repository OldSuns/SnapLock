// snaplock/src-tauri/src/app_setup.rs

use anyhow::Result;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
use crate::{handlers, state::AppState};

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
    let state = app.state::<AppState>();
    let shortcut_str = state.shortcut_key();
    
    let shortcut: Shortcut = shortcut_str.parse().map_err(|e| anyhow::anyhow!("Invalid shortcut format: {}", e))?;
    
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, _event| {
            let state = app.state::<AppState>();
            if state.shortcuts_disabled() {
                println!("快捷键已禁用，忽略触发");
                return;
            }
            
            let handle_clone = handle.clone();
            tauri::async_runtime::spawn(async move {
                handlers::toggle_monitoring(&handle_clone).await;
            });
        })?;
    println!("已注册全局快捷键: {}", shortcut_str);
    Ok(())
}

/// Updates the global shortcut by unregistering the old one and registering the new one
pub async fn update_global_shortcut(
    app_handle: &AppHandle<tauri::Wry>,
    old_shortcut: &str,
    new_shortcut: &str
) -> Result<()> {
    println!("更新全局快捷键: {} -> {}", old_shortcut, new_shortcut);
    
    // Unregister the old shortcut
    if let Err(e) = app_handle.global_shortcut().unregister(old_shortcut) {
        eprintln!("取消注册旧快捷键失败: {}", e);
    }
    
    // Register the new shortcut
    let shortcut: Shortcut = new_shortcut.parse().map_err(|e| anyhow::anyhow!("Invalid shortcut format: {}", e))?;
    let handle_clone = app_handle.clone();
    app_handle.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, _event| {
            let state = app.state::<AppState>();
            if state.shortcuts_disabled() {
                println!("快捷键已禁用，忽略触发");
                return;
            }
            
            let handle_clone = handle_clone.clone();
            tauri::async_runtime::spawn(async move {
                handlers::toggle_monitoring(&handle_clone).await;
            });
        })?;
        
    println!("新快捷键注册成功: {}", new_shortcut);
    Ok(())
}

pub fn setup_tauri_builder() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
}