use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{camera, constants::EVENT_IGNORE_WINDOW_MS, state::{AppState, MonitoringFlags, MonitoringState}};
use rdev::{listen, Event, EventType, Key};
use tauri::{AppHandle, Manager, Emitter};
use tokio::{runtime::Runtime as TokioRuntime, task, time::sleep};

pub fn lock_screen() {
    log::info!("执行锁屏命令...");
    match Command::new("rundll32.exe")
        .args(["user32.dll,LockWorkStation"])
        .spawn()
    {
        Ok(mut child) => {
            log::info!("锁屏命令已启动，进程ID: {:?}", child.id());
            // 等待命令完成
            match child.wait() {
                Ok(status) => {
                    if status.success() {
                        log::info!("锁屏命令执行成功");
                    } else {
                        log::error!("锁屏命令执行失败，退出码: {:?}", status.code());
                    }
                }
                Err(e) => {
                    log::error!("等待锁屏命令完成时发生错误: {}", e);
                }
            }
        }
        Err(e) => {
            log::error!("启动锁屏命令失败: {}", e);
        }
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
        
        log::info!("启动rdev事件监听器...");
        
        // 为错误处理创建额外的克隆
        let error_callback_handle = callback_handle.clone();
        let error_flags_handle = flags_handle.clone();
        
        // 改进错误处理：当rdev监听失败时，立即停止监控
        if let Err(error) = listen(move |event| {
            callback(event, &callback_handle, &flags_handle, &rt_clone);
        }) {
            log::error!("rdev事件监听器严重错误: {:?}", error);
            log::error!("由于监听器故障停止监控");
            
            // 立即停止监控状态
            error_flags_handle.set_monitoring_active(false);
            
            // 通知前端状态变化
            let state = error_callback_handle.state::<AppState>();
            if state.set_status(MonitoringState::Idle).is_ok() {
                error_callback_handle.emit("monitoring_status_changed", "空闲").unwrap_or_else(|e| {
                    log::error!("无法发送状态变化事件: {}", e);
                });
            }
        }
        
        log::info!("rdev事件监听器线程退出");
    });

    Ok(handle)
}

/// The primary callback for `rdev` events.
fn callback(event: Event, app_handle: &AppHandle, monitoring_flags: &Arc<MonitoringFlags>, _rt: &TokioRuntime) {
    // --- 状态检查 ---
    let monitoring_active = monitoring_flags.monitoring_active();
    let shortcut_in_progress = monitoring_flags.shortcut_in_progress();
    let last_shortcut_time = monitoring_flags.last_shortcut_time();
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    // 只在监控激活时记录事件日志
    if monitoring_active {
        log::debug!("收到事件: {:?}, 监控激活: {}, 快捷键处理中: {}, 时间差: {}ms",
                event.event_type, monitoring_active, shortcut_in_progress,
                current_time.saturating_sub(last_shortcut_time));
    }

    // 1. 必须激活监控
    if !monitoring_active {
        return;
    }

    // 2. 检查监控线程是否仍然存活
    if !monitoring_flags.is_monitoring_thread_alive() {
        log::error!("监控线程已终止，停止监控");
        monitoring_flags.set_monitoring_active(false);
        // 通知前端状态变化
        let state = app_handle.state::<AppState>();
        if state.set_status(MonitoringState::Idle).is_ok() {
            app_handle.emit("monitoring_status_changed", "空闲").unwrap_or_else(|e| {
                log::error!("无法发送状态变化事件: {}", e);
            });
        }
        return;
    }

    // 3. 快捷键不能正在处理中
    if shortcut_in_progress {
        log::debug!("忽略事件：快捷键处理中 (剩余时间: {}ms)",
                EVENT_IGNORE_WINDOW_MS.saturating_sub(current_time.saturating_sub(last_shortcut_time)));
        return;
    }

    // 4. 必须在快捷键触发的忽略窗口之外
    if current_time.saturating_sub(last_shortcut_time) < EVENT_IGNORE_WINDOW_MS {
        log::debug!("忽略事件：在忽略窗口内 (剩余: {}ms)",
                EVENT_IGNORE_WINDOW_MS - current_time.saturating_sub(last_shortcut_time));
        return;
    }

    // --- 事件过滤 ---
    if handle_key_press(&event, app_handle) {
        log::debug!("忽略事件：快捷键相关按键 ({:?})", event.event_type);
        return;
    }

    // --- 触发核心逻辑 ---
    log::info!("✓ 触发锁定！事件类型: {:?}", event.event_type);
    
    // 立即停止监控以防止重复触发
    monitoring_flags.set_monitoring_active(false);
    
    let app_handle_clone = app_handle.clone();
    
    // 使用标准线程确保任务稳定执行，避免Tokio运行时的复杂性
    std::thread::spawn(move || {
        log::info!("锁定任务已启动...");
        
        // 创建新的Tokio运行时来执行异步操作
        match tokio::runtime::Runtime::new() {
            Ok(rt_inner) => {
                log::info!("创建内部运行时成功");
                rt_inner.block_on(async move {
                    log::info!("开始执行锁定流程...");
                    trigger_lockdown(app_handle_clone).await;
                });
                log::info!("锁定流程执行完成");
            }
            Err(e) => {
                log::error!("创建内部运行时失败: {}", e);
            }
        }
    });
    
    log::info!("锁定任务句柄创建成功");
}

/// Handles key press events to filter out shortcut-related keys.
/// Returns `true` if the event should be ignored.
fn handle_key_press(event: &Event, app_handle: &AppHandle) -> bool {
    match &event.event_type {
        EventType::KeyPress(key) | EventType::KeyRelease(key) => {
            // 动态获取当前快捷键设置
            let state = app_handle.state::<AppState>();
            let current_shortcut = state.shortcut_key();
            
            // 解析快捷键组合
            let parts: Vec<&str> = current_shortcut.split('+').collect();
            if parts.is_empty() {
                return false;
            }
            
            // 获取主键（最后一个部分）
            let main_key = parts.last().unwrap();
            
            // 检查是否是当前快捷键相关的按键
            let should_ignore = match key {
                Key::Alt | Key::AltGr => parts.contains(&"Alt"),
                Key::ControlLeft | Key::ControlRight => parts.contains(&"Ctrl"),
                Key::ShiftLeft | Key::ShiftRight => parts.contains(&"Shift"),
                Key::MetaLeft | Key::MetaRight => parts.contains(&"Meta"),
                _ => {
                    // 检查主键
                    let key_name = format!("{:?}", key);
                    key_name.contains(main_key) ||
                    (*main_key == "L" && matches!(key, Key::KeyL)) ||
                    (*main_key == "D" && matches!(key, Key::KeyD)) ||
                    (*main_key == "S" && matches!(key, Key::KeyS)) ||
                    (*main_key == "A" && matches!(key, Key::KeyA)) ||
                    (*main_key == "Q" && matches!(key, Key::KeyQ)) ||
                    (*main_key == "W" && matches!(key, Key::KeyW)) ||
                    (*main_key == "E" && matches!(key, Key::KeyE)) ||
                    (*main_key == "R" && matches!(key, Key::KeyR)) ||
                    (*main_key == "T" && matches!(key, Key::KeyT)) ||
                    (*main_key == "Y" && matches!(key, Key::KeyY)) ||
                    (*main_key == "U" && matches!(key, Key::KeyU)) ||
                    (*main_key == "I" && matches!(key, Key::KeyI)) ||
                    (*main_key == "O" && matches!(key, Key::KeyO)) ||
                    (*main_key == "P" && matches!(key, Key::KeyP)) ||
                    (*main_key == "F" && matches!(key, Key::KeyF)) ||
                    (*main_key == "G" && matches!(key, Key::KeyG)) ||
                    (*main_key == "H" && matches!(key, Key::KeyH)) ||
                    (*main_key == "J" && matches!(key, Key::KeyJ)) ||
                    (*main_key == "K" && matches!(key, Key::KeyK)) ||
                    (*main_key == "Z" && matches!(key, Key::KeyZ)) ||
                    (*main_key == "X" && matches!(key, Key::KeyX)) ||
                    (*main_key == "C" && matches!(key, Key::KeyC)) ||
                    (*main_key == "V" && matches!(key, Key::KeyV)) ||
                    (*main_key == "B" && matches!(key, Key::KeyB)) ||
                    (*main_key == "N" && matches!(key, Key::KeyN)) ||
                    (*main_key == "M" && matches!(key, Key::KeyM))
                }
            };
            
            if should_ignore {
                log::debug!("过滤当前快捷键相关按键: {:?} (快捷键: {})", key, current_shortcut);
            }
            should_ignore
        },
        _ => false,
    }
}

/// Asynchronously triggers photo capture, screen lock, and application exit.
async fn trigger_lockdown(app_handle: AppHandle) {
    log::info!("=== 开始执行锁定流程 ===");
    
    // 首先将状态设置为锁定中，但不改变内部状态机状态
    // 这样可以避免状态转换验证的问题
    app_handle.emit("monitoring_status_changed", "锁定中").unwrap_or_else(|e| {
        log::error!("无法发送锁定状态事件: {}", e);
    });
    
    // --- 动态获取摄像头ID和保存路径 ---
    let (camera_id, save_path, exit_on_lock_enabled) = {
        let state = app_handle.state::<AppState>();
        let camera_id = state.camera_id();
        let save_path = state.save_path();
        let exit_on_lock = state.exit_on_lock();
        (camera_id, save_path, exit_on_lock)
    };

    log::info!("监控触发，使用摄像头ID: {}, 锁定时退出: {}", camera_id, exit_on_lock_enabled);

    // --- 异步执行拍照 ---
    log::info!("开始拍照...");
    if let Err(e) = camera::take_photo(camera_id, save_path).await {
        log::error!("拍照失败: {}", e);
    } else {
        log::info!("拍照完成");
    }

    // --- 锁屏 ---
    log::info!("准备执行锁屏...");
    lock_screen();
    
    log::info!("等待锁屏命令完成...");
    sleep(Duration::from_millis(1000)).await; // 等待锁屏命令完成
    
    // 检查是否启用了锁定时退出功能
    if exit_on_lock_enabled {
        log::info!("锁定时退出已启用，准备退出程序...");
        std::process::exit(0);
    } else {
        log::info!("锁定时退出已禁用，程序继续运行");
        log::info!("状态将由会话监控器在系统解锁时自动重置");
        // 注意：此时不立即重置状态，而是依赖会话监控器在系统解锁时重置状态
        // 这样可以确保状态同步的准确性
    }
    
    log::info!("=== 锁定流程执行完成 ===");
}
