use crate::{
    camera,
    constants::{PREPARATION_DELAY, SHORTCUT_DEBOUNCE_TIME, SHORTCUT_FLAG_CLEAR_DELAY},
    monitoring,
    state::{AppState, MonitoringFlags, MonitoringLifecycleLock, MonitoringState},
};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::OwnedMutexGuard;

fn emit_monitoring_status(app_handle: &AppHandle, status: &str) {
    if let Err(error) = app_handle.emit("monitoring_status_changed", status) {
        log::error!("无法发送监控状态事件 '{}': {}", status, error);
    }
}

fn show_notification(app_handle: &AppHandle, body: &str) {
    let state = app_handle.state::<AppState>();
    if !state.enable_notifications() {
        return;
    }

    if let Err(error) = app_handle
        .notification()
        .builder()
        .title("SnapLock")
        .body(body)
        .show()
    {
        log::error!("无法显示通知: {}", error);
    }
}

fn reset_to_idle_state(state: &AppState, app_handle: &AppHandle, reason: &str) {
    log::info!("重置为空闲状态: {}", reason);
    let monitoring_flags = app_handle.state::<Arc<MonitoringFlags>>().inner().clone();
    monitoring_flags.stop_monitoring_thread();

    if let Err(error) = state.set_status(MonitoringState::Idle) {
        log::error!("无法重置状态为空闲: {} ({})", reason, error);
    }

    emit_monitoring_status(app_handle, "空闲");
}

fn schedule_shortcut_flag_clear(monitoring_flags: Arc<MonitoringFlags>) {
    tokio::spawn(async move {
        tokio::time::sleep(SHORTCUT_FLAG_CLEAR_DELAY).await;
        monitoring_flags.set_shortcut_in_progress(false);
        log::debug!("清除快捷键处理标志");
    });
}

async fn lock_monitoring_lifecycle(app_handle: &AppHandle) -> OwnedMutexGuard<()> {
    app_handle
        .state::<Arc<MonitoringLifecycleLock>>()
        .inner()
        .clone()
        .lock_owned()
        .await
}

fn begin_shortcut_toggle(
    app_handle: &AppHandle,
    monitoring_flags: &Arc<MonitoringFlags>,
) -> Result<(), String> {
    let mut last_toggle_time = app_handle
        .state::<Arc<std::sync::Mutex<Instant>>>()
        .inner()
        .lock()
        .map_err(|_| "无法获取快捷键防抖锁".to_string())?;

    if last_toggle_time.elapsed() < SHORTCUT_DEBOUNCE_TIME {
        log::debug!("快捷键防抖，忽略请求");
        return Err("debounced".to_string());
    }
    *last_toggle_time = Instant::now();

    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    monitoring_flags.set_last_shortcut_time(current_time);
    monitoring_flags.set_shortcut_in_progress(true);
    log::debug!("设置快捷键处理标志，时间戳: {}", current_time);
    schedule_shortcut_flag_clear(monitoring_flags.clone());
    Ok(())
}

async fn cleanup_capture_processes() {
    crate::recorder::stop_screen_recording();
    if let Err(error) = camera::stop_video_recording().await {
        log::error!("停止摄像头录像失败: {}", error);
    }
}

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

async fn start_monitoring_locked(app_handle: &AppHandle, camera_id: u32) -> Result<(), String> {
    let monitoring_flags = app_handle.state::<Arc<MonitoringFlags>>().inner().clone();
    let state = app_handle.state::<AppState>();

    if state.status() != MonitoringState::Idle {
        log::info!("监控已处于非空闲状态，忽略重复启动");
        return Ok(());
    }

    camera::ensure_camera_available(camera_id)?;
    if !camera::check_camera_permission(camera_id).await? {
        return Err("无法访问选中的摄像头，请检查权限或设备占用".to_string());
    }

    monitoring::ensure_listener_started(app_handle.clone(), monitoring_flags.clone())?;
    if !monitoring_flags.listener_ready() {
        return Err(monitoring_flags
            .listener_error()
            .unwrap_or_else(|| "输入监听器不可用".to_string()));
    }

    state.set_camera_id(camera_id);
    if state.default_camera_id().is_none() {
        log::debug!(
            "当前未设置默认摄像头，仅更新本次运行时摄像头为 {}",
            camera_id
        );
    }

    state
        .set_status(MonitoringState::Preparing)
        .map_err(|error| format!("无法进入准备状态: {}", error))?;
    emit_monitoring_status(app_handle, "准备中");

    let app_handle_clone = app_handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(PREPARATION_DELAY).await;
        let _lifecycle_guard = lock_monitoring_lifecycle(&app_handle_clone).await;

        let state = app_handle_clone.state::<AppState>();
        let monitoring_flags = app_handle_clone
            .state::<Arc<MonitoringFlags>>()
            .inner()
            .clone();

        if state.status() != MonitoringState::Preparing {
            log::info!("监控准备已取消，当前状态: {:?}", state.status());
            return;
        }

        if !monitoring_flags.listener_ready() {
            log::error!("输入监听器不可用，无法启动监控");
            reset_to_idle_state(&state, &app_handle_clone, "输入监听器不可用");
            return;
        }

        if let Err(error) = state.set_status(MonitoringState::Active) {
            log::error!("无法转换到激活状态: {}", error);
            reset_to_idle_state(&state, &app_handle_clone, "无法进入警戒状态");
            return;
        }

        if !monitoring_flags.start_monitoring_atomic() {
            reset_to_idle_state(&state, &app_handle_clone, "监控已在运行中");
            return;
        }

        if state.post_trigger_action() == crate::config::PostTriggerAction::ScreenRecording {
            monitoring_flags.set_last_activity_time(0);
            monitoring_flags.replace_idle_check_handle(monitoring::start_idle_check_loop(
                app_handle_clone.clone(),
                monitoring_flags.clone(),
            ));
        }

        emit_monitoring_status(&app_handle_clone, "警戒中");
        show_notification(&app_handle_clone, "已进入警戒状态，正在监控活动");

        if let Some(window) = app_handle_clone.get_webview_window("main") {
            if let Err(error) = window.hide() {
                log::error!("隐藏主窗口失败: {}", error);
            }
        }

        log::info!("✓ 监控启动成功");
    });

    Ok(())
}

async fn stop_monitoring_locked(app_handle: &AppHandle) -> Result<(), String> {
    let monitoring_flags = app_handle.state::<Arc<MonitoringFlags>>().inner().clone();
    let state = app_handle.state::<AppState>();
    let current_status = state.status();
    let was_active = matches!(
        current_status,
        MonitoringState::Active | MonitoringState::Triggered
    );

    monitoring_flags.stop_monitoring_thread();
    cleanup_capture_processes().await;

    if current_status == MonitoringState::Idle {
        emit_monitoring_status(app_handle, "空闲");
        log::info!("监控已处于空闲状态，执行了幂等清理");
        return Ok(());
    }

    state
        .set_status(MonitoringState::Idle)
        .map_err(|error| format!("无法重置为空闲状态: {}", error))?;
    emit_monitoring_status(app_handle, "空闲");

    if was_active {
        show_notification(app_handle, "已退出警戒状态");
    }

    log::info!("监控已成功停止");
    Ok(())
}

pub async fn toggle_monitoring(app_handle: &AppHandle) {
    let monitoring_flags = app_handle.state::<Arc<MonitoringFlags>>().inner().clone();

    if !monitoring_flags.health_check() {
        log::warn!("健康检查失败，已尝试修复状态");
    }

    if let Err(error) = begin_shortcut_toggle(app_handle, &monitoring_flags) {
        if error != "debounced" {
            log::error!("处理快捷键切换失败: {}", error);
        }
        return;
    }

    let _lifecycle_guard = lock_monitoring_lifecycle(app_handle).await;
    let state = app_handle.state::<AppState>();
    let current_status = state.status();
    let current_camera_id = state.camera_id();

    log::info!("切换监控状态请求，当前状态: {:?}", current_status);

    let result = match current_status {
        MonitoringState::Idle => start_monitoring_locked(app_handle, current_camera_id).await,
        MonitoringState::Preparing | MonitoringState::Active | MonitoringState::Triggered => {
            stop_monitoring_locked(app_handle).await
        }
    };

    if let Err(error) = result {
        log::error!("快捷键切换监控失败: {}", error);
        reset_to_idle_state(&state, app_handle, "快捷键切换失败");
    }
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
    let _lifecycle_guard = lock_monitoring_lifecycle(&app_handle).await;
    start_monitoring_locked(&app_handle, camera_id).await
}

#[tauri::command]
pub async fn stop_monitoring_command(app_handle: AppHandle) -> Result<(), String> {
    let _lifecycle_guard = lock_monitoring_lifecycle(&app_handle).await;
    stop_monitoring_locked(&app_handle).await
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
