use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

#[cfg(target_os = "windows")]
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;

use crate::state::{AppState, MonitoringState};

/// Windows会话监控器，用于检测系统锁定/解锁状态
pub struct SessionMonitor {
    app_handle: AppHandle,
    is_monitoring: Arc<std::sync::atomic::AtomicBool>,
    monitoring_handle: Arc<std::sync::Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl SessionMonitor {
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            is_monitoring: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            monitoring_handle: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// 启动会话监控
    pub fn start_monitoring(&self) -> Result<(), String> {
        if self.is_monitoring.load(std::sync::atomic::Ordering::SeqCst) {
            eprintln!("会话监控已在运行中");
            return Ok(());
        }

        let app_handle = self.app_handle.clone();
        let is_monitoring = self.is_monitoring.clone();
        
        let handle = std::thread::spawn(move || {
            eprintln!("启动Windows会话状态监控...");
            is_monitoring.store(true, std::sync::atomic::Ordering::SeqCst);
            
            let mut was_locked = false;
            let mut lock_detection_count = 0;
            const DETECTION_THRESHOLD: u32 = 3; // 需要连续检测到锁定状态3次才确认

            // 创建一个运行时来处理异步操作
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("无法创建Tokio运行时: {}", e);
                    is_monitoring.store(false, std::sync::atomic::Ordering::SeqCst);
                    return;
                }
            };

            loop {
                // 检查是否应该停止监控
                if !is_monitoring.load(std::sync::atomic::Ordering::SeqCst) {
                    eprintln!("停止会话监控");
                    break;
                }

                let is_currently_locked = rt.block_on(Self::is_system_locked());
                
                if is_currently_locked && !was_locked {
                    lock_detection_count += 1;
                    if lock_detection_count >= DETECTION_THRESHOLD {
                        eprintln!("检测到系统锁定");
                        was_locked = true;
                        lock_detection_count = 0;
                        
                        // 通知应用系统已锁定
                        rt.block_on(Self::handle_system_locked(&app_handle));
                    }
                } else if !is_currently_locked && was_locked {
                    eprintln!("检测到系统解锁");
                    was_locked = false;
                    lock_detection_count = 0;
                    
                    // 通知应用系统已解锁
                    rt.block_on(Self::handle_system_unlocked(&app_handle));
                } else {
                    // 重置计数器如果状态一致
                    lock_detection_count = 0;
                }

                // 等待下一次检查
                std::thread::sleep(Duration::from_millis(500));
            }
            
            eprintln!("会话监控线程已退出");
        });

        // 存储监控句柄
        if let Ok(mut guard) = self.monitoring_handle.lock() {
            *guard = Some(handle);
        }
        Ok(())
    }

    /// 停止会话监控
    pub fn stop_monitoring(&self) {
        eprintln!("停止会话监控...");
        
        self.is_monitoring.store(false, std::sync::atomic::Ordering::SeqCst);
        
        if let Ok(mut guard) = self.monitoring_handle.lock() {
            if let Some(handle) = guard.take() {
                // 对于std::thread，我们设置停止标志，线程会自然退出
                // 等待线程完成（可选，避免阻塞主线程）
                if let Err(e) = handle.join() {
                    eprintln!("等待会话监控线程退出时发生错误: {:?}", e);
                }
            }
        }
        
        eprintln!("会话监控已停止");
    }

    /// 检测系统是否处于锁定状态
    async fn is_system_locked() -> bool {
        // 方法1: 检查前台窗口
        let foreground_locked = Self::is_foreground_locked();
        
        // 方法2: 检查电源状态
        let power_status_locked = Self::is_power_status_locked();
        
        // 如果任一方法检测到锁定，则认为系统已锁定
        let is_locked = foreground_locked || power_status_locked;
        
        if is_locked {
            log::debug!("系统锁定检测: 前台窗口={}, 电源状态={}", foreground_locked, power_status_locked);
        }
        
        is_locked
    }

    /// 检查前台窗口是否为锁屏界面
    #[cfg(target_os = "windows")]
    fn is_foreground_locked() -> bool {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd == HWND(std::ptr::null_mut()) {
                return true; // 没有前台窗口可能表示锁定
            }

            // 获取窗口标题
            let mut window_title = [0u16; 256];
            let len = GetWindowTextW(hwnd, &mut window_title);
            
            if len > 0 {
                let title = String::from_utf16_lossy(&window_title[..len as usize]);
                
                // 检查是否为Windows锁屏相关窗口
                let lock_indicators = [
                    "Windows Default Lock Screen",
                    "LockApp",
                    "Windows.UI.Core.CoreWindow",
                ];
                
                for indicator in &lock_indicators {
                    if title.contains(indicator) {
                        return true;
                    }
                }
            }
        }
        
        false
    }

    /// 非Windows平台的前台窗口检查（存根实现）
    #[cfg(not(target_os = "windows"))]
    fn is_foreground_locked() -> bool {
        false // 非Windows平台暂不支持
    }

    /// 检查系统电源状态
    #[cfg(target_os = "windows")]
    fn is_power_status_locked() -> bool {
        unsafe {
            let mut power_status = SYSTEM_POWER_STATUS::default();
            if GetSystemPowerStatus(&mut power_status).is_ok() {
                // 如果系统处于待机或休眠状态，可能表示锁定
                // 注意：这个方法可能不够准确，主要用作辅助检测
                return false; // 暂时禁用此检测方法
            }
        }
        false
    }

    /// 非Windows平台的电源状态检查（存根实现）
    #[cfg(not(target_os = "windows"))]
    fn is_power_status_locked() -> bool {
        false // 非Windows平台暂不支持
    }

    /// 处理系统锁定事件
    async fn handle_system_locked(app_handle: &AppHandle) {
        log::info!("处理系统锁定事件");
        
        let state = app_handle.state::<AppState>();
        let current_status = state.status();
        
        // 只有在非锁定状态时才更新
        if current_status != MonitoringState::Idle || 
           !matches!(current_status, MonitoringState::Preparing | MonitoringState::Active) {
            log::debug!("当前状态不需要更新: {:?}", current_status);
            return;
        }
        
        log::info!("系统锁定，但应用状态无需特殊处理");
    }

    /// 处理系统解锁事件
    async fn handle_system_unlocked(app_handle: &AppHandle) {
        log::info!("处理系统解锁事件");
        
        let state = app_handle.state::<AppState>();
        let current_status = state.status();
        
        log::info!("当前应用状态: {:?}", current_status);
        
        // 停止任何可能在运行的屏幕录制
        crate::recorder::stop_screen_recording();

        // 停止监控线程
        let monitoring_flags = app_handle.state::<Arc<crate::state::MonitoringFlags>>().inner().clone();
        monitoring_flags.stop_monitoring_thread();

        // 智能状态重置：根据当前状态决定如何重置
        let reset_success = match current_status {
            MonitoringState::Idle => {
                log::info!("应用已处于空闲状态，无需重置");
                true
            },
            MonitoringState::Preparing | MonitoringState::Active | MonitoringState::Triggered => {
                // 这些状态可以正常转换到 Idle
                match state.set_status(MonitoringState::Idle) {
                    Ok(_) => {
                        log::info!("成功通过正常状态转换重置为空闲");
                        true
                    },
                    Err(e) => {
                        log::warn!("正常状态转换失败: {}, 执行强制重置", e);
                        Self::force_reset_to_idle(&state)
                    }
                }
            }
        };
        
        if reset_success {
            // 发送状态更新事件到前端
            if let Err(e) = app_handle.emit("monitoring_status_changed", "空闲") {
                log::error!("无法发送状态更新事件: {}", e);
            } else {
                log::info!("已发送状态重置事件到前端");
            }
            
            // 显示通知
            #[cfg(target_os = "windows")]
            {
                use tauri_plugin_notification::NotificationExt;
                if let Err(e) = app_handle.notification()
                    .builder()
                    .title("SnapLock")
                    .body("系统已解锁，应用状态已重置")
                    .show() {
                    log::error!("无法显示解锁通知: {}", e);
                }
            }
        }
    }
    
    /// 强制重置状态为空闲（绕过状态转换验证）
    fn force_reset_to_idle(state: &AppState) -> bool {
        if let Ok(mut status_lock) = state.status.lock() {
            *status_lock = MonitoringState::Idle;
            log::info!("强制重置状态为空闲成功");
            true
        } else {
            log::error!("无法获取状态锁进行强制重置");
            false
        }
    }
}

impl Drop for SessionMonitor {
    fn drop(&mut self) {
        self.stop_monitoring();
    }
}

/// 全局会话监控器实例
static SESSION_MONITOR: std::sync::OnceLock<Arc<std::sync::Mutex<Option<SessionMonitor>>>> = std::sync::OnceLock::new();

/// 初始化会话监控器
pub fn init_session_monitor(app_handle: AppHandle) -> Result<(), String> {
    let monitor = SessionMonitor::new(app_handle);
    
    let monitor_mutex = SESSION_MONITOR.get_or_init(|| {
        Arc::new(std::sync::Mutex::new(None))
    });
    
    if let Ok(mut guard) = monitor_mutex.lock() {
        *guard = Some(monitor);
        log::info!("会话监控器已初始化");
        Ok(())
    } else {
        Err("无法初始化会话监控器".to_string())
    }
}

/// 启动会话监控
pub fn start_session_monitoring() -> Result<(), String> {
    let monitor_mutex = SESSION_MONITOR.get()
        .ok_or("会话监控器未初始化")?;
    
    if let Ok(guard) = monitor_mutex.lock() {
        if let Some(monitor) = guard.as_ref() {
            monitor.start_monitoring()
        } else {
            Err("会话监控器实例不存在".to_string())
        }
    } else {
        Err("无法获取会话监控器锁".to_string())
    }
}

/// 停止会话监控
#[allow(dead_code)]
pub fn stop_session_monitoring() {
    if let Some(monitor_mutex) = SESSION_MONITOR.get() {
        if let Ok(guard) = monitor_mutex.lock() {
            if let Some(monitor) = guard.as_ref() {
                monitor.stop_monitoring();
            }
        }
    }
}