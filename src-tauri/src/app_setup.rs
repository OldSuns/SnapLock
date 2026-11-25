use crate::{config, monitoring::MonitoringController, state::AppState};
use anyhow::Result;
use tauri::{
    App, AppHandle, Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

pub fn setup_system_tray(app: &AppHandle) -> Result<tauri::tray::TrayIcon> {
    let toggle = MenuItem::with_id(app, "toggle", "显示/隐藏窗口", true, None::<&str>)?;
    let start = MenuItem::with_id(app, "start_monitoring", "开始监控", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop_monitoring", "停止监控", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&toggle, &separator, &start, &stop, &separator, &quit],
    )?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("缺少默认窗口图标"))?;
    Ok(TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("SnapLock")
        .icon(icon)
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "toggle" {
                if let Some(window) = app.get_webview_window("main") {
                    let result = if window.is_visible().unwrap_or(false) {
                        window.hide()
                    } else {
                        window.show()
                    };
                    if let Err(error) = result {
                        log::error!("切换窗口显示失败: {error}");
                    }
                }
                return;
            }
            let app = app.clone();
            let action = event.id().as_ref().to_string();
            tauri::async_runtime::spawn(async move {
                let controller = app.state::<MonitoringController>();
                let result = match action.as_str() {
                    "start_monitoring" => {
                        controller
                            .start(&app, app.state::<AppState>().camera_id())
                            .await
                    }
                    "stop_monitoring" => controller.stop(&app).await,
                    "quit" => {
                        controller.shutdown(&app).await;
                        app.exit(0);
                        Ok(())
                    }
                    _ => Ok(()),
                };
                if let Err(error) = result {
                    controller.report_capture_error(&app, &error);
                }
            });
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                if let Some(window) = tray.app_handle().get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?)
}

fn register_shortcut(app: &AppHandle, shortcut: &str) -> Result<()> {
    app.global_shortcut()
        .on_shortcut(shortcut.parse::<Shortcut>()?, |app, _, _| {
            if app.state::<AppState>().shortcuts_disabled() {
                return;
            }
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                app.state::<MonitoringController>().toggle(&app).await;
            });
        })?;
    Ok(())
}

pub fn register_global_shortcuts(app: &mut App) -> Result<()> {
    let state = app.state::<AppState>();
    let configured = state.config().shortcut_key;
    for shortcut in std::iter::once(configured.as_str()).chain([
        "Ctrl+Alt+L",
        "Ctrl+Shift+L",
        "Alt+Shift+L",
        "Ctrl+Alt+S",
    ]) {
        match register_shortcut(app.handle(), shortcut) {
            Ok(()) => {
                if shortcut != configured {
                    state.config.lock().unwrap().shortcut_key = shortcut.into();
                    if let Err(error) = config::save_config(app.handle().clone()) {
                        log::warn!("保存备用快捷键失败: {error}");
                    }
                }
                log::info!("快捷键已注册: {shortcut}");
                return Ok(());
            }
            Err(error) => log::warn!("快捷键 {shortcut} 注册失败: {error}"),
        }
    }
    log::error!("所有快捷键注册失败，请通过主界面或托盘操作");
    Ok(())
}

/// Keep the old registration until the replacement succeeds.
pub fn update_global_shortcut(app: &AppHandle, old: &str, new: &str) -> Result<()> {
    if old == new {
        return Ok(());
    }
    register_shortcut(app, new)?;
    if let Err(error) = app.global_shortcut().unregister(old) {
        // A failed removal must not leave two live registrations behind.
        let rollback = app.global_shortcut().unregister(new);
        return Err(anyhow::anyhow!(
            "取消注册旧快捷键失败: {error}; 回滚: {rollback:?}"
        ));
    }
    Ok(())
}

pub fn setup_tauri_builder() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
}
