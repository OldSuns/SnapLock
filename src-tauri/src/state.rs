use std::sync::Mutex;
use crate::config::PostTriggerAction;

/// Represents the monitoring status of the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitoringState {
    /// The application is idle and not monitoring for input.
    Idle,
    /// The application is preparing to start monitoring, with a short delay.
    Preparing,
    /// The application is actively monitoring for input.
    Active,
    /// The application has detected an event and is processing the action.
    Triggered,
}

impl MonitoringState {
    /// Transitions to a new state, enforcing valid state transitions.
    pub fn transition_to(
        &self,
        next_state: MonitoringState,
    ) -> Result<MonitoringState, &'static str> {
        match (self, next_state) {
            // Idle -> Preparing
            (MonitoringState::Idle, MonitoringState::Preparing) => Ok(next_state),
            // Preparing -> Active | Idle
            (MonitoringState::Preparing, MonitoringState::Active) => Ok(next_state),
            (MonitoringState::Preparing, MonitoringState::Idle) => Ok(next_state),
            // Active -> Triggered (the key change!) | Idle
            (MonitoringState::Active, MonitoringState::Triggered) => Ok(next_state),
            (MonitoringState::Active, MonitoringState::Idle) => Ok(next_state),
            // Triggered -> Idle
            (MonitoringState::Triggered, MonitoringState::Idle) => Ok(next_state),
            // Allow resetting from any state to Idle
            (_, MonitoringState::Idle) => Ok(next_state),
            // All other transitions are invalid
            _ => Err("Invalid state transition"),
        }
    }
}

/// Holds the shared state of the Tauri application.
pub struct AppState {
    /// The current monitoring status, protected by a Mutex.
    pub(crate) status: Mutex<MonitoringState>,
    /// The ID of the camera to be used for capturing photos.
    pub(crate) camera_id: Mutex<u32>,
    /// The custom path to save photos, protected by a Mutex.
    pub(crate) save_path: Mutex<Option<String>>,
    /// The custom shortcut key combination, protected by a Mutex.
    pub(crate) shortcut_key: Mutex<String>,
    /// Flag to temporarily disable global shortcuts (e.g., during shortcut configuration)
    pub(crate) shortcuts_disabled: Mutex<bool>,
    /// Flag to show debug logs in the UI
    pub(crate) show_debug_logs: Mutex<bool>,
    /// Flag to save logs to file
    pub(crate) save_logs_to_file: Mutex<bool>,
    /// Flag to exit application when system is locked
    pub(crate) exit_on_lock: Mutex<bool>,
    /// Flag for dark mode theme
    pub(crate) dark_mode: Mutex<bool>,
    /// Flag to enable/disable screen locking functionality
    pub(crate) enable_screen_lock: Mutex<bool>,
    /// Flag to enable/disable system notifications
    pub(crate) enable_notifications: Mutex<bool>,
    /// Post trigger action setting
    pub(crate) post_trigger_action: Mutex<PostTriggerAction>,
}

impl AppState {
    pub fn new(camera_id: u32) -> Self {
        Self {
            status: Mutex::new(MonitoringState::Idle),
            camera_id: Mutex::new(camera_id),
            save_path: Mutex::new(None),
            shortcut_key: Mutex::new("Alt+L".to_string()),
            shortcuts_disabled: Mutex::new(false),
            show_debug_logs: Mutex::new(false),
            save_logs_to_file: Mutex::new(false),
            exit_on_lock: Mutex::new(false),
            dark_mode: Mutex::new(false),
            enable_screen_lock: Mutex::new(true), // 默认启用锁屏功能
            enable_notifications: Mutex::new(true), // 默认启用系统通知
            post_trigger_action: Mutex::new(PostTriggerAction::CaptureAndLock), // 默认拍摄并锁屏
        }
    }

    pub fn status(&self) -> MonitoringState {
        *self.status.lock().unwrap()
    }

    pub fn set_status(&self, new_status: MonitoringState) -> Result<(), &'static str> {
        let mut status = self.status.lock().unwrap();
        let current_status = *status;
        match current_status.transition_to(new_status) {
            Ok(state) => {
                *status = state;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    pub fn camera_id(&self) -> u32 {
        *self.camera_id.lock().unwrap()
    }

    pub fn set_camera_id(&self, id: u32) {
        *self.camera_id.lock().unwrap() = id;
    }

    pub fn save_path(&self) -> Option<String> {
        self.save_path.lock().unwrap().clone()
    }

    /// 获取有效的保存路径，如果配置中没有设置则返回默认桌面路径
    pub fn get_effective_save_path(&self) -> String {
        match self.save_path.lock().unwrap().clone() {
            Some(path) => path,
            None => crate::config::get_default_save_path(),
        }
    }

    pub fn set_save_path(&self, path: Option<String>) {
        *self.save_path.lock().unwrap() = path;
    }

    pub fn shortcut_key(&self) -> String {
        self.shortcut_key.lock().unwrap().clone()
    }

    pub fn set_shortcut_key(&self, key: String) {
        *self.shortcut_key.lock().unwrap() = key;
    }

    pub fn shortcuts_disabled(&self) -> bool {
        *self.shortcuts_disabled.lock().unwrap()
    }

    pub fn set_shortcuts_disabled(&self, disabled: bool) {
        *self.shortcuts_disabled.lock().unwrap() = disabled;
    }

    pub fn show_debug_logs(&self) -> bool {
        *self.show_debug_logs.lock().unwrap()
    }

    pub fn set_show_debug_logs(&self, show: bool) {
        *self.show_debug_logs.lock().unwrap() = show;
    }

    pub fn save_logs_to_file(&self) -> bool {
        *self.save_logs_to_file.lock().unwrap()
    }

    pub fn set_save_logs_to_file(&self, save: bool) {
        *self.save_logs_to_file.lock().unwrap() = save;
        // 同时更新日志器设置
        if let Some(logger) = crate::logger::get_logger() {
            logger.set_log_to_file(save);
        }
    }

    pub fn exit_on_lock(&self) -> bool {
        *self.exit_on_lock.lock().unwrap()
    }

    pub fn set_exit_on_lock(&self, enabled: bool) {
        *self.exit_on_lock.lock().unwrap() = enabled;
    }

    pub fn dark_mode(&self) -> bool {
        *self.dark_mode.lock().unwrap()
    }

    pub fn set_dark_mode(&self, enabled: bool) {
        *self.dark_mode.lock().unwrap() = enabled;
    }

    pub fn set_enable_screen_lock(&self, enabled: bool) {
        *self.enable_screen_lock.lock().unwrap() = enabled;
    }

    pub fn enable_notifications(&self) -> bool {
        *self.enable_notifications.lock().unwrap()
    }

    pub fn set_enable_notifications(&self, enabled: bool) {
        *self.enable_notifications.lock().unwrap() = enabled;
    }
    
    pub fn post_trigger_action(&self) -> PostTriggerAction {
        self.post_trigger_action.lock().unwrap().clone()
    }
    
    pub fn set_post_trigger_action(&self, action: PostTriggerAction) {
        *self.post_trigger_action.lock().unwrap() = action;
    }
}


/// Holds the monitoring flags for the application.
pub struct MonitoringFlags {
    /// Flag indicating if monitoring is active
    pub(crate) monitoring_active: std::sync::atomic::AtomicBool,
    /// Flag indicating if shortcut processing is in progress
    pub(crate) shortcut_in_progress: std::sync::atomic::AtomicBool,
    /// Timestamp of last shortcut activation (in milliseconds since epoch)
    pub(crate) last_shortcut_time: std::sync::atomic::AtomicU64,
    /// Timestamp of last user activity (in milliseconds since epoch)
    pub(crate) last_activity_time: std::sync::atomic::AtomicU64,
    /// Handle to the monitoring thread for lifecycle management
    pub(crate) monitoring_handle: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Handle to the idle check thread for lifecycle management
    pub(crate) idle_check_handle: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl MonitoringFlags {
    pub fn new() -> Self {
        Self {
            monitoring_active: std::sync::atomic::AtomicBool::new(false),
            shortcut_in_progress: std::sync::atomic::AtomicBool::new(false),
            last_shortcut_time: std::sync::atomic::AtomicU64::new(0),
            last_activity_time: std::sync::atomic::AtomicU64::new(0),
            monitoring_handle: std::sync::Mutex::new(None),
            idle_check_handle: std::sync::Mutex::new(None),
        }
    }

    pub fn monitoring_active(&self) -> bool {
        self.monitoring_active.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn set_monitoring_active(&self, value: bool) {
        self.monitoring_active.store(value, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn shortcut_in_progress(&self) -> bool {
        self.shortcut_in_progress.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn set_shortcut_in_progress(&self, value: bool) {
        self.shortcut_in_progress.store(value, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn last_shortcut_time(&self) -> u64 {
        self.last_shortcut_time.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn set_last_shortcut_time(&self, value: u64) {
        self.last_shortcut_time.store(value, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn last_activity_time(&self) -> u64 {
        self.last_activity_time.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn set_last_activity_time(&self, value: u64) {
        self.last_activity_time.store(value, std::sync::atomic::Ordering::SeqCst);
    }

    /// Store the monitoring thread handle (used internally by start_monitoring_atomic)
    #[allow(dead_code)]
    pub fn set_monitoring_handle(&self, handle: tokio::task::JoinHandle<()>) {
        *self.monitoring_handle.lock().unwrap() = Some(handle);
    }

    /// Check if monitoring thread is still running
    pub fn is_monitoring_thread_alive(&self) -> bool {
        if let Ok(handle_guard) = self.monitoring_handle.lock() {
            if let Some(handle) = handle_guard.as_ref() {
                !handle.is_finished()
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Stop the monitoring thread and clean up
    pub fn stop_monitoring_thread(&self) {
        log::info!("停止监控线程...");
        if let Ok(mut handle_guard) = self.monitoring_handle.lock() {
            if let Some(handle) = handle_guard.take() {
                if !handle.is_finished() {
                    log::info!("中止监控线程");
                    handle.abort();
                } else {
                    log::info!("监控线程已经结束");
                }
            } else {
                log::warn!("没有找到监控线程句柄");
            }
        }
        // Stop the idle check thread as well
        if let Ok(mut handle_guard) = self.idle_check_handle.lock() {
            if let Some(handle) = handle_guard.take() {
                if !handle.is_finished() {
                    log::info!("中止空闲检测线程");
                    handle.abort();
                }
            }
        }
        self.set_monitoring_active(false);
        log::info!("监控状态已重置为非激活");
    }

    /// Atomically start monitoring with proper state management
    pub fn start_monitoring_atomic(&self, handle: tokio::task::JoinHandle<()>) -> bool {
        log::info!("尝试启动监控...");
        
        // First check if already monitoring
        if self.monitoring_active() {
            log::warn!("监控已经在运行中，中止新的监控线程");
            handle.abort();
            return false;
        }

        // Store handle and activate monitoring atomically
        if let Ok(mut handle_guard) = self.monitoring_handle.lock() {
            *handle_guard = Some(handle);
            self.set_monitoring_active(true);
            log::info!("监控线程已启动并激活");
            true
        } else {
            log::error!("无法获取监控句柄锁，启动失败");
            handle.abort();
            false
        }
    }

    /// 健康检查：验证监控状态的一致性
    pub fn health_check(&self) -> bool {
        let monitoring_active = self.monitoring_active();
        let thread_alive = self.is_monitoring_thread_alive();
        
        let is_healthy = monitoring_active == thread_alive;
        
        if !is_healthy {
            log::warn!("监控健康检查失败: 监控激活={}, 线程存活={}", monitoring_active, thread_alive);
            
            // 自动修复不一致的状态
            if monitoring_active && !thread_alive {
                log::warn!("检测到监控激活但线程已死，重置监控状态");
                self.set_monitoring_active(false);
            }
        }
        
        is_healthy
    }
}
