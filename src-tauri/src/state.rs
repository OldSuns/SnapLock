use crate::config::{CaptureMode, PostTriggerAction};
use std::sync::Mutex;
use tokio::task::JoinHandle;

pub type MonitoringLifecycleLock = tokio::sync::Mutex<()>;

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
            (MonitoringState::Preparing, MonitoringState::Idle) => Ok(next_state),
            (MonitoringState::Active, MonitoringState::Triggered) => Ok(next_state),
            (MonitoringState::Active, MonitoringState::Idle) => Ok(next_state),
            (MonitoringState::Triggered, MonitoringState::Idle) => Ok(next_state),
            (_, MonitoringState::Idle) => Ok(next_state),
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
    /// The persisted default camera selection.
    pub(crate) default_camera_id: Mutex<Option<u32>>,
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
    /// Flag to enable/disable system notifications
    pub(crate) enable_notifications: Mutex<bool>,
    /// Post trigger action setting
    pub(crate) post_trigger_action: Mutex<PostTriggerAction>,
    /// Capture delay in seconds
    pub(crate) capture_delay_seconds: Mutex<u32>,
    /// Capture mode setting
    pub(crate) capture_mode: Mutex<CaptureMode>,
}

impl AppState {
    pub fn new(camera_id: u32) -> Self {
        Self {
            status: Mutex::new(MonitoringState::Idle),
            camera_id: Mutex::new(camera_id),
            default_camera_id: Mutex::new(None),
            save_path: Mutex::new(None),
            shortcut_key: Mutex::new("Alt+L".to_string()),
            shortcuts_disabled: Mutex::new(false),
            show_debug_logs: Mutex::new(false),
            save_logs_to_file: Mutex::new(false),
            exit_on_lock: Mutex::new(false),
            dark_mode: Mutex::new(false),
            enable_notifications: Mutex::new(true),
            post_trigger_action: Mutex::new(PostTriggerAction::CaptureAndLock),
            capture_delay_seconds: Mutex::new(0),
            capture_mode: Mutex::new(CaptureMode::Video),
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

    pub fn default_camera_id(&self) -> Option<u32> {
        *self.default_camera_id.lock().unwrap()
    }

    pub fn set_default_camera_id(&self, camera_id: Option<u32>) {
        *self.default_camera_id.lock().unwrap() = camera_id;
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

    pub fn capture_delay_seconds(&self) -> u32 {
        *self.capture_delay_seconds.lock().unwrap()
    }

    pub fn set_capture_delay_seconds(&self, delay: u32) {
        *self.capture_delay_seconds.lock().unwrap() = delay;
    }

    pub fn capture_mode(&self) -> CaptureMode {
        self.capture_mode.lock().unwrap().clone()
    }

    pub fn set_capture_mode(&self, mode: CaptureMode) {
        *self.capture_mode.lock().unwrap() = mode;
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

    /// Compatibility helper used by existing monitoring/session code paths.
    pub fn stop_monitoring_thread(&self) {
        self.stop_monitoring();
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
