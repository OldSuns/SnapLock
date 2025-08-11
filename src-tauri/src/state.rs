use std::sync::Mutex;

/// Represents the monitoring status of the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitoringState {
    /// The application is idle and not monitoring for input.
    Idle,
    /// The application is preparing to start monitoring, with a short delay.
    Preparing,
    /// The application is actively monitoring for input.
    Active,
}

impl MonitoringState {
    /// Transitions to a new state, enforcing valid state transitions.
    ///
    /// # Rules:
    /// - `Idle` -> `Preparing`
    /// - `Preparing` -> `Active` | `Idle`
    /// - `Active` -> `Idle`
    pub fn transition_to(
        &self,
        next_state: MonitoringState,
    ) -> Result<MonitoringState, &'static str> {
        match (self, next_state) {
            // Idle can only transition to Preparing
            (MonitoringState::Idle, MonitoringState::Preparing) => Ok(next_state),
            // Preparing can transition to Active (success) or Idle (cancelled)
            (MonitoringState::Preparing, MonitoringState::Active) => Ok(next_state),
            (MonitoringState::Preparing, MonitoringState::Idle) => Ok(next_state),
            // Active can only transition back to Idle
            (MonitoringState::Active, MonitoringState::Idle) => Ok(next_state),
            // All other transitions are invalid
            _ => Err("Invalid state transition"),
        }
    }
}

pub struct AppState {
    /// The current monitoring status, protected by a Mutex.
    pub(crate) status: Mutex<MonitoringState>,
    /// The ID of the camera to be used for capturing photos.
    pub(crate) camera_id: Mutex<u32>,
    /// The custom path to save photos, protected by a Mutex.
    pub(crate) save_path: Mutex<Option<String>>,
    /// Determines whether the app should exit after locking the screen.
    pub(crate) exit_on_lock: Mutex<bool>,
}

impl AppState {
    pub fn new(camera_id: u32) -> Self {
        Self {
            status: Mutex::new(MonitoringState::Idle),
            camera_id: Mutex::new(camera_id),
            save_path: Mutex::new(None),
            exit_on_lock: Mutex::new(true), // Default to true to exit on lock
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

    pub fn set_save_path(&self, path: Option<String>) {
        *self.save_path.lock().unwrap() = path;
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
    /// Handle to the monitoring thread for lifecycle management
    pub(crate) monitoring_handle: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl MonitoringFlags {
    pub fn new() -> Self {
        Self {
            monitoring_active: std::sync::atomic::AtomicBool::new(false),
            shortcut_in_progress: std::sync::atomic::AtomicBool::new(false),
            last_shortcut_time: std::sync::atomic::AtomicU64::new(0),
            monitoring_handle: std::sync::Mutex::new(None),
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
        println!("停止监控线程...");
        if let Ok(mut handle_guard) = self.monitoring_handle.lock() {
            if let Some(handle) = handle_guard.take() {
                if !handle.is_finished() {
                    println!("中止监控线程");
                    handle.abort();
                } else {
                    println!("监控线程已经结束");
                }
            } else {
                println!("没有找到监控线程句柄");
            }
        }
        self.set_monitoring_active(false);
        println!("监控状态已重置为非激活");
    }

    /// Atomically start monitoring with proper state management
    pub fn start_monitoring_atomic(&self, handle: tokio::task::JoinHandle<()>) -> bool {
        println!("尝试原子性启动监控...");
        
        // First check if already monitoring
        if self.monitoring_active() {
            println!("监控已经在运行中，中止新的监控线程");
            handle.abort();
            return false;
        }

        // Store handle and activate monitoring atomically
        if let Ok(mut handle_guard) = self.monitoring_handle.lock() {
            *handle_guard = Some(handle);
            self.set_monitoring_active(true);
            println!("监控线程已启动并激活");
            true
        } else {
            println!("无法获取监控句柄锁，启动失败");
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
            println!("监控健康检查失败: 监控激活={}, 线程存活={}", monitoring_active, thread_alive);
            
            // 自动修复不一致的状态
            if monitoring_active && !thread_alive {
                println!("检测到监控激活但线程已死，重置监控状态");
                self.set_monitoring_active(false);
            }
        }
        
        is_healthy
    }
}
