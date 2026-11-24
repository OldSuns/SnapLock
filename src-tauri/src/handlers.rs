use crate::{camera, monitoring_controller::MonitoringController, state::AppState};
use tauri::{AppHandle, Manager};

fn persist_state_change<Apply, Rollback>(
    app_handle: &AppHandle,
    apply: Apply,
    rollback: Rollback,
) -> Result<(), String>
where
    Apply: FnOnce(&AppState),
    Rollback: FnOnce(&AppState),
{
    let state = app_handle.state::<AppState>();
    apply(&state);

    if let Err(error) = crate::config::save_config(app_handle.clone()) {
        rollback(&state);
        return Err(format!("保存配置失败: {}", error));
    }

    Ok(())
}

pub async fn toggle_monitoring(app_handle: &AppHandle) {
    app_handle
        .state::<MonitoringController>()
        .toggle(app_handle)
        .await;
}

#[tauri::command]
pub fn set_camera_id(app_handle: tauri::AppHandle, camera_id: u32) -> Result<(), String> {
    camera::ensure_camera_available(camera_id)?;
    let state = app_handle.state::<AppState>();
    state.set_camera_id(camera_id);
    log::info!("摄像头ID已更新为: {}", camera_id);
    Ok(())
}

#[tauri::command]
pub async fn start_monitoring_command(app_handle: AppHandle, camera_id: u32) -> Result<(), String> {
    app_handle
        .state::<MonitoringController>()
        .start(&app_handle, camera_id)
        .await
}

#[tauri::command]
pub async fn stop_monitoring_command(app_handle: AppHandle) -> Result<(), String> {
    app_handle
        .state::<MonitoringController>()
        .stop(&app_handle)
        .await
}

#[tauri::command]
pub fn get_shortcut_key(app_handle: tauri::AppHandle) -> Result<String, String> {
    let state = app_handle.state::<AppState>();
    Ok(state.shortcut_key())
}

#[tauri::command]
pub async fn set_shortcut_key(
    app_handle: tauri::AppHandle,
    shortcut: String,
) -> Result<(), String> {
    if !is_valid_shortcut(&shortcut) {
        return Err("无效的快捷键格式".to_string());
    }

    let state = app_handle.state::<AppState>();
    let old_shortcut = state.shortcut_key();

    crate::app_setup::update_global_shortcut(&app_handle, &old_shortcut, &shortcut)
        .await
        .map_err(|error| format!("快捷键注册失败: {}", error))?;

    state.set_shortcut_key(shortcut.clone());

    if let Err(error) = crate::config::save_config(app_handle.clone()) {
        log::error!("保存快捷键配置失败，尝试回滚: {}", error);

        match crate::app_setup::update_global_shortcut(&app_handle, &shortcut, &old_shortcut).await
        {
            Ok(_) => {
                state.set_shortcut_key(old_shortcut);
                return Err(format!("保存配置失败，已回滚快捷键: {}", error));
            }
            Err(rollback_error) => {
                state.set_shortcut_key(shortcut.clone());
                return Err(format!(
                    "保存配置失败，且回滚快捷键失败: {}; {}",
                    error, rollback_error
                ));
            }
        }
    }

    log::info!("快捷键已更新为: {}", shortcut);
    Ok(())
}

fn is_valid_shortcut(shortcut: &str) -> bool {
    let parts: Vec<&str> = shortcut.split('+').collect();
    if parts.len() < 2 {
        return false;
    }

    let modifiers = &parts[..parts.len() - 1];
    let key = parts.last().unwrap_or(&"");

    for modifier in modifiers {
        if !matches!(*modifier, "Ctrl" | "Alt" | "Shift" | "Meta" | "Cmd") {
            return false;
        }
    }

    !key.is_empty() && !matches!(*key, "Ctrl" | "Alt" | "Shift" | "Meta" | "Cmd")
}

#[tauri::command]
pub fn disable_shortcuts(app_handle: tauri::AppHandle) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    state.set_shortcuts_disabled(true);
    log::info!("全局快捷键已禁用");
    Ok(())
}

#[tauri::command]
pub fn enable_shortcuts(app_handle: tauri::AppHandle) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    state.set_shortcuts_disabled(false);
    log::info!("全局快捷键已启用");
    Ok(())
}

#[tauri::command]
pub fn get_show_debug_logs(app_handle: tauri::AppHandle) -> Result<bool, String> {
    let state = app_handle.state::<AppState>();
    Ok(state.show_debug_logs())
}

#[tauri::command]
pub fn set_show_debug_logs(app_handle: tauri::AppHandle, show: bool) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    let old_show = state.show_debug_logs();

    persist_state_change(
        &app_handle,
        |state| state.set_show_debug_logs(show),
        |state| state.set_show_debug_logs(old_show),
    )?;

    log::info!("调试日志显示设置已更新为: {}", show);
    Ok(())
}

#[tauri::command]
pub fn get_save_logs_to_file(app_handle: tauri::AppHandle) -> Result<bool, String> {
    let state = app_handle.state::<AppState>();
    Ok(state.save_logs_to_file())
}

#[tauri::command]
pub fn set_save_logs_to_file(app_handle: tauri::AppHandle, save: bool) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    let old_save = state.save_logs_to_file();
    let old_path = state.get_effective_save_path();
    let new_path = state.get_effective_save_path();

    persist_state_change(
        &app_handle,
        |state| {
            state.set_save_logs_to_file(save);
            if let Some(logger) = crate::logger::get_logger() {
                logger.set_log_file_path(Some(new_path.clone()));
            }
        },
        |state| {
            state.set_save_logs_to_file(old_save);
            if let Some(logger) = crate::logger::get_logger() {
                logger.set_log_file_path(Some(old_path.clone()));
            }
        },
    )?;

    log::info!("日志保存到文件设置已更新为: {}", save);
    Ok(())
}

#[tauri::command]
pub fn log_save_path_change(old_path: String, new_path: String) -> Result<(), String> {
    log::info!("保存路径已更改: '{}' -> '{}'", old_path, new_path);
    Ok(())
}

#[tauri::command]
pub fn get_exit_on_lock(app_handle: tauri::AppHandle) -> Result<bool, String> {
    let state = app_handle.state::<AppState>();
    Ok(state.exit_on_lock())
}

#[tauri::command]
pub fn set_exit_on_lock(app_handle: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    let old_enabled = state.exit_on_lock();

    persist_state_change(
        &app_handle,
        |state| state.set_exit_on_lock(enabled),
        |state| state.set_exit_on_lock(old_enabled),
    )?;

    log::info!("锁定时退出设置已更新为: {}", enabled);
    Ok(())
}

#[tauri::command]
pub fn get_dark_mode(app_handle: tauri::AppHandle) -> Result<bool, String> {
    let state = app_handle.state::<AppState>();
    Ok(state.dark_mode())
}

#[tauri::command]
pub fn set_dark_mode(app_handle: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    let old_enabled = state.dark_mode();

    persist_state_change(
        &app_handle,
        |state| state.set_dark_mode(enabled),
        |state| state.set_dark_mode(old_enabled),
    )?;

    log::info!("暗色模式设置已更新为: {}", enabled);
    Ok(())
}

#[tauri::command]
pub fn get_enable_notifications(app_handle: tauri::AppHandle) -> Result<bool, String> {
    let state = app_handle.state::<AppState>();
    Ok(state.enable_notifications())
}

#[tauri::command]
pub fn set_enable_notifications(app_handle: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    let old_enabled = state.enable_notifications();

    persist_state_change(
        &app_handle,
        |state| state.set_enable_notifications(enabled),
        |state| state.set_enable_notifications(old_enabled),
    )?;

    log::info!("系统通知启用设置已更新为: {}", enabled);
    Ok(())
}

#[tauri::command]
pub fn get_post_trigger_action(
    app_handle: tauri::AppHandle,
) -> Result<crate::config::PostTriggerAction, String> {
    let state = app_handle.state::<AppState>();
    Ok(state.post_trigger_action())
}

#[tauri::command]
pub fn set_post_trigger_action(
    app_handle: tauri::AppHandle,
    action: crate::config::PostTriggerAction,
) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    let old_action = state.post_trigger_action();
    let new_action = action.clone();

    persist_state_change(
        &app_handle,
        |state| state.set_post_trigger_action(new_action.clone()),
        |state| state.set_post_trigger_action(old_action.clone()),
    )?;

    log::info!("触发后动作设置已更新为: {:?}", action);
    Ok(())
}

#[tauri::command]
pub fn get_default_camera_id(app_handle: tauri::AppHandle) -> Result<Option<u32>, String> {
    let state = app_handle.state::<AppState>();
    Ok(state.default_camera_id())
}

#[tauri::command]
pub fn set_default_camera_id(
    app_handle: tauri::AppHandle,
    camera_id: Option<u32>,
) -> Result<(), String> {
    if let Some(id) = camera_id {
        camera::ensure_camera_available(id)?;
    }

    let state = app_handle.state::<AppState>();
    let old_default_camera_id = state.default_camera_id();
    let old_camera_id = state.camera_id();

    persist_state_change(
        &app_handle,
        |state| {
            state.set_default_camera_id(camera_id);
            if let Some(id) = camera_id {
                state.set_camera_id(id);
            }
        },
        |state| {
            state.set_default_camera_id(old_default_camera_id);
            state.set_camera_id(old_camera_id);
        },
    )?;

    log::info!("默认摄像头ID设置已更新为: {:?}", camera_id);
    Ok(())
}

#[tauri::command]
pub fn get_capture_delay_seconds(app_handle: tauri::AppHandle) -> Result<u32, String> {
    let state = app_handle.state::<AppState>();
    Ok(state.capture_delay_seconds())
}

#[tauri::command]
pub fn set_capture_delay_seconds(app_handle: tauri::AppHandle, delay: u32) -> Result<(), String> {
    if delay > 60 {
        return Err("拍摄延迟必须在 0 到 60 秒之间".to_string());
    }

    let state = app_handle.state::<AppState>();
    let old_delay = state.capture_delay_seconds();

    persist_state_change(
        &app_handle,
        |state| state.set_capture_delay_seconds(delay),
        |state| state.set_capture_delay_seconds(old_delay),
    )?;

    log::info!("拍摄延迟时间设置已更新为: {}秒", delay);
    Ok(())
}

#[tauri::command]
pub fn get_capture_mode(
    app_handle: tauri::AppHandle,
) -> Result<crate::config::CaptureMode, String> {
    let state = app_handle.state::<AppState>();
    Ok(state.capture_mode())
}

#[tauri::command]
pub fn set_capture_mode(
    app_handle: tauri::AppHandle,
    mode: crate::config::CaptureMode,
) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    let old_mode = state.capture_mode();
    let new_mode = mode.clone();

    persist_state_change(
        &app_handle,
        |state| state.set_capture_mode(new_mode.clone()),
        |state| state.set_capture_mode(old_mode.clone()),
    )?;

    log::info!("拍摄模式设置已更新为: {:?}", mode);
    Ok(())
}
