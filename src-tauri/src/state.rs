use crate::config::AppConfig;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tokio::task::JoinHandle;

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
            (MonitoringState::Idle, MonitoringState::Preparing) => Ok(next_state),
            (MonitoringState::Preparing, MonitoringState::Active) => Ok(next_state),
            (MonitoringState::Active, MonitoringState::Triggered) => Ok(next_state),
            (_, MonitoringState::Idle) => Ok(next_state),
            _ => Err("Invalid state transition"),
        }
    }
}

/// Persisted configuration has one owner; transient controls are independent.
pub struct AppState {
    pub(crate) config: Mutex<AppConfig>,
    pub(crate) settings_update: Mutex<()>,
    camera_id: AtomicU32,
    shortcuts_disabled: AtomicBool,
}

impl AppState {
    pub fn new(camera_id: u32) -> Self {
        Self {
            config: Mutex::new(AppConfig::default()),
            settings_update: Mutex::new(()),
            camera_id: AtomicU32::new(camera_id),
            shortcuts_disabled: AtomicBool::new(false),
        }
    }

    pub fn camera_id(&self) -> u32 {
        self.camera_id.load(Ordering::SeqCst)
    }

    pub fn set_camera_id(&self, id: u32) {
        self.camera_id.store(id, Ordering::SeqCst);
    }

    pub fn config(&self) -> AppConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn shortcuts_disabled(&self) -> bool {
        self.shortcuts_disabled.load(Ordering::SeqCst)
    }

    pub fn set_shortcuts_disabled(&self, disabled: bool) {
        self.shortcuts_disabled.store(disabled, Ordering::SeqCst);
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
    /// Whether the global input listener is ready to be used.
    pub(crate) listener_ready: std::sync::atomic::AtomicBool,
    /// Generation counter used to cancel stale trigger flows.
    pub(crate) action_generation: std::sync::atomic::AtomicU64,
    /// Latest listener startup/runtime error, if any.
    pub(crate) listener_error: Mutex<Option<String>>,
    /// Handle to the long-lived input listener thread.
    pub(crate) listener_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Handle to the idle check task for lifecycle management
    pub(crate) idle_check_handle: Mutex<Option<JoinHandle<()>>>,
}

impl MonitoringFlags {
    pub fn new() -> Self {
        Self {
            monitoring_active: std::sync::atomic::AtomicBool::new(false),
            shortcut_in_progress: std::sync::atomic::AtomicBool::new(false),
            last_shortcut_time: std::sync::atomic::AtomicU64::new(0),
            last_activity_time: std::sync::atomic::AtomicU64::new(0),
            listener_ready: std::sync::atomic::AtomicBool::new(false),
            action_generation: std::sync::atomic::AtomicU64::new(0),
            listener_error: Mutex::new(None),
            listener_handle: Mutex::new(None),
            idle_check_handle: Mutex::new(None),
        }
    }

    pub fn monitoring_active(&self) -> bool {
        self.monitoring_active
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn set_monitoring_active(&self, value: bool) {
        self.monitoring_active
            .store(value, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn shortcut_in_progress(&self) -> bool {
        self.shortcut_in_progress
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn set_shortcut_in_progress(&self, value: bool) {
        self.shortcut_in_progress
            .store(value, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn last_shortcut_time(&self) -> u64 {
        self.last_shortcut_time
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn set_last_shortcut_time(&self, value: u64) {
        self.last_shortcut_time
            .store(value, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn last_activity_time(&self) -> u64 {
        self.last_activity_time
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn set_last_activity_time(&self, value: u64) {
        self.last_activity_time
            .store(value, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn listener_ready(&self) -> bool {
        self.listener_ready
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn current_action_generation(&self) -> u64 {
        self.action_generation
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn invalidate_action_generation(&self) -> u64 {
        self.action_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            .saturating_add(1)
    }

    pub fn is_action_generation_current(&self, generation: u64) -> bool {
        self.current_action_generation() == generation
    }

    pub fn set_listener_ready(&self, value: bool) {
        self.listener_ready
            .store(value, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn listener_error(&self) -> Option<String> {
        self.listener_error.lock().unwrap().clone()
    }

    pub fn set_listener_error(&self, error: Option<String>) {
        *self.listener_error.lock().unwrap() = error;
    }

    pub fn clear_listener_error(&self) {
        self.set_listener_error(None);
    }

    pub fn set_listener_handle(&self, handle: std::thread::JoinHandle<()>) {
        let mut guard = self.listener_handle.lock().unwrap();
        *guard = Some(handle);
    }

    pub fn replace_idle_check_handle(&self, handle: JoinHandle<()>) {
        let mut guard = self.idle_check_handle.lock().unwrap();
        if let Some(existing) = guard.take() {
            if !existing.is_finished() {
                existing.abort();
            }
        }
        *guard = Some(handle);
    }

    pub fn stop_idle_check_thread(&self) {
        if let Ok(mut handle_guard) = self.idle_check_handle.lock() {
            if let Some(handle) = handle_guard.take() {
                if !handle.is_finished() {
                    log::info!("中止空闲检测线程");
                    handle.abort();
                }
            }
        }
    }

    pub fn is_listener_thread_alive(&self) -> bool {
        if let Ok(handle_guard) = self.listener_handle.lock() {
            if let Some(handle) = handle_guard.as_ref() {
                !handle.is_finished()
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn start_monitoring_atomic(&self) -> bool {
        if self.monitoring_active() {
            log::warn!("监控已经在运行中，忽略重复启动");
            return false;
        }

        if !self.listener_ready() {
            log::warn!("输入监听器尚未就绪，无法激活监控");
            return false;
        }

        self.stop_idle_check_thread();
        self.set_monitoring_active(true);
        true
    }

    /// Stop active monitoring state and related helper tasks.
    pub fn stop_monitoring(&self) {
        log::info!("停止监控状态...");
        self.stop_idle_check_thread();
        self.set_monitoring_active(false);
        self.invalidate_action_generation();
        log::info!("监控状态已重置为非激活");
    }

    /// 健康检查：验证监控状态与监听器状态的一致性
    pub fn health_check(&self) -> bool {
        let monitoring_active = self.monitoring_active();
        let listener_ready = self.listener_ready() && self.is_listener_thread_alive();
        let is_healthy = !monitoring_active || listener_ready;

        if !is_healthy {
            log::warn!(
                "监控健康检查失败: 监控激活={}, 监听器就绪={}",
                monitoring_active,
                listener_ready
            );
            self.set_monitoring_active(false);
            self.invalidate_action_generation();
        }

        is_healthy
    }
}

#[cfg(test)]
mod tests {
    use super::{MonitoringFlags, MonitoringState};

    #[test]
    fn monitoring_state_transitions_allow_expected_flow() {
        assert_eq!(
            MonitoringState::Idle
                .transition_to(MonitoringState::Preparing)
                .unwrap(),
            MonitoringState::Preparing
        );
        assert_eq!(
            MonitoringState::Preparing
                .transition_to(MonitoringState::Active)
                .unwrap(),
            MonitoringState::Active
        );
        assert_eq!(
            MonitoringState::Active
                .transition_to(MonitoringState::Triggered)
                .unwrap(),
            MonitoringState::Triggered
        );
        assert_eq!(
            MonitoringState::Triggered
                .transition_to(MonitoringState::Idle)
                .unwrap(),
            MonitoringState::Idle
        );
    }

    #[test]
    fn monitoring_flags_fail_health_check_when_listener_missing() {
        let flags = MonitoringFlags::new();
        flags.set_monitoring_active(true);

        assert!(!flags.health_check());
        assert!(!flags.monitoring_active());
    }

    #[test]
    fn monitoring_flags_require_listener_before_activation() {
        let flags = MonitoringFlags::new();
        assert!(!flags.start_monitoring_atomic());
        assert!(!flags.monitoring_active());
    }

    #[test]
    fn invalidating_action_generation_cancels_stale_flow() {
        let flags = MonitoringFlags::new();
        let initial_generation = flags.current_action_generation();

        let next_generation = flags.invalidate_action_generation();

        assert_eq!(initial_generation + 1, next_generation);
        assert!(!flags.is_action_generation_current(initial_generation));
        assert!(flags.is_action_generation_current(next_generation));
    }
}
