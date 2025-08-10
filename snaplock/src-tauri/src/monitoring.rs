use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{camera, constants::EVENT_IGNORE_WINDOW_MS, state::{AppState, MonitoringFlags}};
use rdev::{listen, Event, EventType, Key};
use tauri::{AppHandle, Manager};
use tokio::{runtime::Runtime as TokioRuntime, task, time::sleep};

pub fn lock_screen() {
    if let Err(e) = Command::new("rundll32.exe")
        .args(["user32.dll,LockWorkStation"])
        .spawn()
    {
        eprintln!("Failed to execute screen lock command: {}", e);
    }
}

/// Starts the global input monitor on a blocking-safe Tokio thread.
pub fn start_monitoring(
    app_handle: AppHandle,
    monitoring_flags: Arc<MonitoringFlags>,
) -> Result<task::JoinHandle<()>, String> {
    let rt = Arc::new(TokioRuntime::new().map_err(|e| format!("Failed to create Tokio runtime: {}", e))?);
    let rt_clone = rt.clone();

    let handle = rt.spawn_blocking(move || {
        let callback_handle = app_handle.clone();
        let flags_handle = monitoring_flags.clone();
        if let Err(error) = listen(move |event| {
            callback(event, &callback_handle, &flags_handle, &rt_clone);
        }) {
            eprintln!("Error listening to input events: {:?}", error);
        }
    });

    Ok(handle)
}

/// The primary callback for `rdev` events.
fn callback(event: Event, app_handle: &AppHandle, monitoring_flags: &Arc<MonitoringFlags>, rt: &TokioRuntime) {
    // --- 状态检查 ---
    let monitoring_active = monitoring_flags.monitoring_active();
    let shortcut_in_progress = monitoring_flags.shortcut_in_progress();
    let last_shortcut_time = monitoring_flags.last_shortcut_time();
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    // 1. 必须激活监控
    // 2. 快捷键不能正在处理中
    // 3. 必须在快捷键触发的忽略窗口之外
    if !monitoring_active || shortcut_in_progress || current_time.saturating_sub(last_shortcut_time) < EVENT_IGNORE_WINDOW_MS {
        return;
    }

    // --- 事件过滤 ---
    if handle_key_press(&event) {
        return; // 如果是快捷键相关按键，则忽略
    }

    // --- 触发核心逻辑 ---
    let app_handle_clone = app_handle.clone();
    rt.spawn(async move {
        trigger_lockdown(app_handle_clone).await;
    });

    // 禁用监控以防止重复触发
    monitoring_flags.set_monitoring_active(false);
}

/// Handles key press events to filter out shortcut-related keys.
/// Returns `true` if the event should be ignored.
fn handle_key_press(event: &Event) -> bool {
    match &event.event_type {
        EventType::KeyPress(key) | EventType::KeyRelease(key) => matches!(key, Key::KeyL | Key::Alt | Key::AltGr),
        _ => false,
    }
}

/// Asynchronously triggers photo capture, screen lock, and application exit.
async fn trigger_lockdown(app_handle: AppHandle) {
    // --- 动态获取摄像头ID和保存路径 ---
    let (camera_id, save_path) = {
        let state = app_handle.state::<AppState>();
        let camera_id = state.camera_id();
        let save_path = state.save_path();
        (camera_id, save_path)
    };

    println!("Monitoring triggered, using camera ID: {}", camera_id);

    // --- 异步执行拍照 ---
    if let Err(e) = camera::take_photo(camera_id, save_path).await {
        eprintln!("Failed to capture photo: {}", e);
    }

    // --- 锁屏并退出 ---
    lock_screen();
    sleep(Duration::from_millis(500)).await; // 确保锁屏命令已发出
    std::process::exit(0);
}
