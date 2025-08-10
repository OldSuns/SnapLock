use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{camera, constants::EVENT_IGNORE_WINDOW_MS, state::{AppState, MonitoringFlags, MonitoringState}};
use rdev::{listen, Event, EventType, Key};
use tauri::{AppHandle, Manager, Emitter};
use tokio::{runtime::Runtime as TokioRuntime, task, time::sleep};

pub fn lock_screen() {
    println!("执行锁屏命令...");
    match Command::new("rundll32.exe")
        .args(["user32.dll,LockWorkStation"])
        .spawn()
    {
        Ok(mut child) => {
            println!("锁屏命令已启动，进程ID: {:?}", child.id());
            // 等待命令完成
            match child.wait() {
                Ok(status) => {
                    if status.success() {
                        println!("锁屏命令执行成功");
                    } else {
                        eprintln!("锁屏命令执行失败，退出码: {:?}", status.code());
                    }
                }
                Err(e) => {
                    eprintln!("等待锁屏命令完成时发生错误: {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("启动锁屏命令失败: {}", e);
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
        
        println!("Starting rdev event listener...");
        
        // 为错误处理创建额外的克隆
        let error_callback_handle = callback_handle.clone();
        let error_flags_handle = flags_handle.clone();
        
        // 改进错误处理：当rdev监听失败时，立即停止监控
        if let Err(error) = listen(move |event| {
            callback(event, &callback_handle, &flags_handle, &rt_clone);
        }) {
            eprintln!("Critical error in rdev event listener: {:?}", error);
            eprintln!("Stopping monitoring due to listener failure");
            
            // 立即停止监控状态
            error_flags_handle.set_monitoring_active(false);
            
            // 通知前端状态变化
            let state = error_callback_handle.state::<AppState>();
            if state.set_status(MonitoringState::Idle).is_ok() {
                error_callback_handle.emit("monitoring_status_changed", "空闲").unwrap_or_else(|e| {
                    eprintln!("Failed to emit status change: {}", e);
                });
            }
        }
        
        println!("rdev event listener thread exiting");
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
        println!("收到事件: {:?}, 监控激活: {}, 快捷键处理中: {}, 时间差: {}ms",
                event.event_type, monitoring_active, shortcut_in_progress,
                current_time.saturating_sub(last_shortcut_time));
    }

    // 1. 必须激活监控
    if !monitoring_active {
        return;
    }

    // 2. 检查监控线程是否仍然存活
    if !monitoring_flags.is_monitoring_thread_alive() {
        eprintln!("监控线程已终止，停止监控");
        monitoring_flags.set_monitoring_active(false);
        // 通知前端状态变化
        let state = app_handle.state::<AppState>();
        if state.set_status(MonitoringState::Idle).is_ok() {
            app_handle.emit("monitoring_status_changed", "空闲").unwrap_or_else(|e| {
                eprintln!("Failed to emit status change: {}", e);
            });
        }
        return;
    }

    // 3. 快捷键不能正在处理中
    if shortcut_in_progress {
        println!("忽略事件：快捷键处理中 (剩余时间: {}ms)",
                EVENT_IGNORE_WINDOW_MS.saturating_sub(current_time.saturating_sub(last_shortcut_time)));
        return;
    }

    // 4. 必须在快捷键触发的忽略窗口之外
    if current_time.saturating_sub(last_shortcut_time) < EVENT_IGNORE_WINDOW_MS {
        println!("忽略事件：在忽略窗口内 (剩余: {}ms)",
                EVENT_IGNORE_WINDOW_MS - current_time.saturating_sub(last_shortcut_time));
        return;
    }

    // --- 事件过滤 ---
    if handle_key_press(&event) {
        println!("忽略事件：快捷键相关按键 ({:?})", event.event_type);
        return;
    }

    // --- 触发核心逻辑 ---
    println!("✓ 触发锁定！事件类型: {:?}", event.event_type);
    
    // 立即停止监控以防止重复触发
    monitoring_flags.set_monitoring_active(false);
    
    let app_handle_clone = app_handle.clone();
    
    // 使用标准线程确保任务稳定执行，避免Tokio运行时的复杂性
    std::thread::spawn(move || {
        println!("锁定任务已启动...");
        
        // 创建新的Tokio运行时来执行异步操作
        match tokio::runtime::Runtime::new() {
            Ok(rt_inner) => {
                println!("创建内部运行时成功");
                rt_inner.block_on(async move {
                    println!("开始执行锁定流程...");
                    trigger_lockdown(app_handle_clone).await;
                });
                println!("锁定流程执行完成");
            }
            Err(e) => {
                eprintln!("创建内部运行时失败: {}", e);
            }
        }
    });
    
    println!("锁定任务句柄创建成功");
}

/// Handles key press events to filter out shortcut-related keys.
/// Returns `true` if the event should be ignored.
fn handle_key_press(event: &Event) -> bool {
    match &event.event_type {
        // 过滤Alt+L组合键相关的按键，防止快捷键触发熄屏
        EventType::KeyPress(key) | EventType::KeyRelease(key) => {
            let should_ignore = matches!(key, Key::KeyL | Key::Alt | Key::AltGr);
            if should_ignore {
                println!("过滤快捷键相关按键: {:?}", key);
            }
            should_ignore
        },
        _ => false,
    }
}

/// Asynchronously triggers photo capture, screen lock, and application exit.
async fn trigger_lockdown(app_handle: AppHandle) {
    println!("=== 开始执行锁定流程 ===");
    
    // 通知前端状态变化
    let state = app_handle.state::<AppState>();
    if state.set_status(MonitoringState::Idle).is_ok() {
        app_handle.emit("monitoring_status_changed", "锁定中").unwrap_or_else(|e| {
            eprintln!("Failed to emit lockdown status: {}", e);
        });
    }
    
    // --- 动态获取摄像头ID和保存路径 ---
    let (camera_id, save_path) = {
        let state = app_handle.state::<AppState>();
        let camera_id = state.camera_id();
        let save_path = state.save_path();
        (camera_id, save_path)
    };

    println!("监控触发，使用摄像头ID: {}", camera_id);

    // --- 异步执行拍照 ---
    println!("开始拍照...");
    if let Err(e) = camera::take_photo(camera_id, save_path).await {
        eprintln!("拍照失败: {}", e);
    } else {
        println!("拍照完成");
    }

    // --- 锁屏并退出 ---
    println!("准备执行锁屏...");
    lock_screen();
    
    println!("等待锁屏命令完成...");
    sleep(Duration::from_millis(1000)).await; // 增加等待时间确保锁屏命令完成
    
    println!("准备退出程序...");
    std::process::exit(0);
}
