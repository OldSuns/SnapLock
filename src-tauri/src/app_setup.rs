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
    let handle_backup = app.handle().clone();
    let state = app.state::<AppState>();
    let shortcut_str = state.shortcut_key();
    
    log::info!("尝试注册快捷键: {}", shortcut_str);
    
    // 尝试解析并注册主快捷键
    match shortcut_str.parse::<Shortcut>() {
        Ok(shortcut) => {
            // 快捷键格式有效，尝试注册
            match app.global_shortcut()
                .on_shortcut(shortcut, move |app, _shortcut, _event| {
                    // 安全获取 AppState，失败时记录错误但不阻止快捷键触发
                    match app.try_state::<AppState>() {
                        Some(state) => {
                            if state.shortcuts_disabled() {
                                log::debug!("快捷键已禁用，忽略触发");
                                return;
                            }
                        },
                        None => {
                            log::warn!("无法获取 AppState，但继续执行快捷键操作");
                        }
                    }
                    
                    let handle_clone = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        handlers::toggle_monitoring(&handle_clone).await;
                    });
                }) {
                Ok(_) => {
                    log::info!("✓ 主快捷键注册成功: {}", shortcut_str);
                    return Ok(());
                },
                Err(e) => {
                    log::warn!("主快捷键注册失败: {}，尝试备用快捷键", e);
                }
            }
        },
        Err(e) => {
            log::warn!("主快捷键格式无效: {}，尝试备用快捷键", e);
        }
    }
    
    // 主快捷键失败，尝试多个备用快捷键
    let backup_shortcuts = vec![
        "Ctrl+Alt+L",
        "Ctrl+Shift+L",
        "Alt+Shift+L",
        "Ctrl+Alt+S"
    ];
    
    for backup_shortcut in backup_shortcuts {
        log::info!("尝试注册备用快捷键: {}", backup_shortcut);
        
        match backup_shortcut.parse::<Shortcut>() {
            Ok(backup) => {
                match app.global_shortcut()
                    .on_shortcut(backup, {
                        let handle_backup = handle_backup.clone();
                        move |app, _shortcut, _event| {
                            // 安全获取 AppState，失败时记录错误但不阻止快捷键触发
                            match app.try_state::<AppState>() {
                                Some(state) => {
                                    if state.shortcuts_disabled() {
                                        log::debug!("快捷键已禁用，忽略触发");
                                        return;
                                    }
                                },
                                None => {
                                    log::warn!("无法获取 AppState，但继续执行快捷键操作");
                                }
                            }
                            
                            let handle_clone = handle_backup.clone();
                            tauri::async_runtime::spawn(async move {
                                handlers::toggle_monitoring(&handle_clone).await;
                            });
                        }
                    }) {
                    Ok(_) => {
                        // 备用快捷键注册成功，更新状态
                        state.set_shortcut_key(backup_shortcut.to_string());
                        log::info!("✓ 备用快捷键注册成功: {}", backup_shortcut);
                        
                        // 保存配置
                        if let Err(e) = crate::config::save_config(app.handle().clone()) {
                            log::warn!("保存备用快捷键配置失败: {}", e);
                        }
                        
                        return Ok(());
                    },
                    Err(e) => {
                        log::warn!("备用快捷键 {} 注册失败: {}", backup_shortcut, e);
                        continue;
                    }
                }
            },
            Err(e) => {
                log::error!("备用快捷键 {} 格式无效: {}", backup_shortcut, e);
                continue;
            }
        }
    }
    
    // 所有快捷键都失败了
    log::error!("⚠️  所有快捷键注册均失败，程序将继续运行但无法使用快捷键");
    log::info!("您仍可以通过系统托盘图标或主界面操作程序");
    
    // 返回 Ok 而不是错误，让程序继续运行
    Ok(())
}

/// Updates the global shortcut by unregistering the old one and registering the new one
pub async fn update_global_shortcut(
    app_handle: &AppHandle<tauri::Wry>,
    old_shortcut: &str,
    new_shortcut: &str
) -> Result<()> {
    log::info!("更新全局快捷键: {} -> {}", old_shortcut, new_shortcut);
    
    // Unregister the old shortcut (不用担心失败)
    if let Err(e) = app_handle.global_shortcut().unregister(old_shortcut) {
        log::warn!("取消注册旧快捷键失败 (忽略): {}", e);
    }
    
    // 验证新快捷键格式
    let shortcut = match new_shortcut.parse::<Shortcut>() {
        Ok(s) => s,
        Err(e) => {
            log::error!("新快捷键格式无效: {} - {}", new_shortcut, e);
            return Err(anyhow::anyhow!("Invalid shortcut format: {}", e));
        }
    };
    
    // Register the new shortcut
    let handle_clone = app_handle.clone();
    match app_handle.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, _event| {
            // 安全获取 AppState，失败时记录错误但不阻止快捷键触发
            match app.try_state::<AppState>() {
                Some(state) => {
                    if state.shortcuts_disabled() {
                        log::debug!("快捷键已禁用，忽略触发");
                        return;
                    }
                },
                None => {
                    log::warn!("无法获取 AppState，但继续执行快捷键操作");
                }
            }
            
            let handle_clone = handle_clone.clone();
            tauri::async_runtime::spawn(async move {
                handlers::toggle_monitoring(&handle_clone).await;
            });
        }) {
        Ok(_) => {
            log::info!("✓ 新快捷键注册成功: {}", new_shortcut);
            Ok(())
        },
        Err(e) => {
            log::error!("新快捷键注册失败: {} - {}", new_shortcut, e);
            Err(anyhow::anyhow!("Failed to register new shortcut: {}", e))
        }
    }
}

pub fn setup_tauri_builder() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
}