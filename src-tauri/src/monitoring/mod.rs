mod actions;
mod input;

use crate::{
    camera,
    config::PostTriggerAction,
    constants::{
        EVENT_IGNORE_WINDOW_MS, PREPARATION_DELAY, SHORTCUT_DEBOUNCE_TIME,
        SHORTCUT_FLAG_CLEAR_DELAY,
    },
    state::{AppState, MonitoringFlags, MonitoringState},
};
use rdev::Event;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::{MutexGuard, watch};
use tokio::task::JoinHandle;

struct MonitoringSession {
    status: Mutex<MonitoringState>,
    flags: Arc<MonitoringFlags>,
    generation_changed: watch::Sender<u64>,
}

impl MonitoringSession {
    fn new() -> Self {
        Self {
            status: Mutex::new(MonitoringState::Idle),
            flags: Arc::new(MonitoringFlags::new()),
            generation_changed: watch::channel(0).0,
        }
    }

    fn status(&self) -> MonitoringState {
        *self.status.lock().unwrap()
    }

    fn prepare(&self) -> Result<u64, &'static str> {
        let mut status = self.status.lock().unwrap();
        *status = status.transition_to(MonitoringState::Preparing)?;
        let generation = self.flags.invalidate_action_generation();
        self.generation_changed.send_replace(generation);
        Ok(generation)
    }

    fn is_current(&self, generation: u64, expected_status: MonitoringState) -> bool {
        let status = self.status.lock().unwrap();
        *status == expected_status && self.flags.is_action_generation_current(generation)
    }

    fn activate(&self, generation: u64) -> Result<bool, &'static str> {
        let mut status = self.status.lock().unwrap();
        if *status != MonitoringState::Preparing
            || !self.flags.is_action_generation_current(generation)
        {
            return Ok(false);
        }
        if !self.flags.start_monitoring_atomic() {
            return Err("输入监听器不可用或监控已在运行中");
        }
        *status = status.transition_to(MonitoringState::Active)?;
        Ok(true)
    }

    fn trigger(&self, generation: u64) -> bool {
        let mut status = self.status.lock().unwrap();
        if *status != MonitoringState::Active
            || !self.flags.is_action_generation_current(generation)
        {
            return false;
        }
        *status = MonitoringState::Triggered;
        self.flags.set_monitoring_active(false);
        true
    }

    fn cancel(&self) {
        let mut status = self.status.lock().unwrap();
        self.flags.stop_monitoring();
        *status = MonitoringState::Idle;
        self.generation_changed
            .send_replace(self.flags.current_action_generation());
    }
}

/// Owns monitoring transitions, task cancellation, and capture cleanup.
pub struct MonitoringController {
    lifecycle: tokio::sync::Mutex<()>,
    session: MonitoringSession,
    preparation_task: Mutex<Option<JoinHandle<()>>>,
    last_toggle_time: Mutex<Instant>,
}

impl MonitoringController {
    pub fn new() -> Self {
        Self {
            lifecycle: tokio::sync::Mutex::new(()),
            session: MonitoringSession::new(),
            preparation_task: Mutex::new(None),
            last_toggle_time: Mutex::new(Instant::now()),
        }
    }

    pub fn status(&self) -> MonitoringState {
        self.session.status()
    }

    pub fn is_action_current(&self, generation: u64) -> bool {
        self.session
            .is_current(generation, MonitoringState::Triggered)
    }

    /// Holds the lifecycle lock while an effect runs, so stop cannot race its creation.
    pub async fn lock_action(&self, generation: u64) -> Option<MutexGuard<'_, ()>> {
        let guard = self.lifecycle.lock().await;
        self.is_action_current(generation).then_some(guard)
    }

    pub async fn wait_action_delay(&self, generation: u64, delay: Duration) -> bool {
        let mut changed = self.session.generation_changed.subscribe();
        if !self.is_action_current(generation) {
            return false;
        }
        tokio::select! {
            _ = tokio::time::sleep(delay) => self.is_action_current(generation),
            _ = changed.changed() => false,
        }
    }

    pub async fn start(&self, app_handle: &AppHandle, camera_id: u32) -> Result<(), String> {
        let _guard = self.lifecycle.lock().await;
        self.start_locked(app_handle, camera_id).await
    }

    async fn start_locked(&self, app_handle: &AppHandle, camera_id: u32) -> Result<(), String> {
        if self.status() != MonitoringState::Idle {
            log::info!("监控已处于非空闲状态，忽略重复启动");
            return Ok(());
        }

        validate_capture_device(
            app_handle.state::<AppState>().config().post_trigger_action,
            camera_id,
        )
        .await?;

        input::ensure_listener_started(app_handle.clone(), self.session.flags.clone())?;
        if !self.session.flags.listener_ready() {
            return Err(self
                .session
                .flags
                .listener_error()
                .unwrap_or_else(|| "输入监听器不可用".to_string()));
        }

        app_handle.state::<AppState>().set_camera_id(camera_id);
        let generation = self.session.prepare().map_err(str::to_string)?;
        self.emit_status(app_handle);

        let handle = app_handle.clone();
        self.replace_preparation_task(tokio::spawn(async move {
            tokio::time::sleep(PREPARATION_DELAY).await;
            let controller = handle.state::<MonitoringController>();
            controller.finish_preparation(&handle, generation).await;
        }));
        Ok(())
    }

    async fn finish_preparation(&self, app_handle: &AppHandle, generation: u64) {
        let _guard = self.lifecycle.lock().await;
        match self.session.activate(generation) {
            Ok(false) => {
                log::debug!("监控准备已取消或已被新一轮监控替代");
                return;
            }
            Err(error) => {
                log::error!("无法进入警戒状态: {}", error);
                // This task is completing itself; only stop other helper tasks here.
                self.session.cancel();
                Self::cleanup_capture_processes(app_handle).await;
                self.emit_status(app_handle);
                return;
            }
            Ok(true) => {}
        }

        if app_handle.state::<AppState>().config().post_trigger_action
            == PostTriggerAction::ScreenRecording
        {
            self.session.flags.set_last_activity_time(0);
            self.session
                .flags
                .replace_idle_check_handle(self.start_idle_check(app_handle.clone(), generation));
        }

        self.emit_status(app_handle);
        self.show_notification(app_handle, "已进入警戒状态，正在监控活动");
        if let Some(window) = app_handle.get_webview_window("main") {
            if let Err(error) = window.hide() {
                log::error!("隐藏主窗口失败: {}", error);
            }
        }
        log::info!("✓ 监控启动成功");
    }

    pub async fn stop(&self, app_handle: &AppHandle) -> Result<(), String> {
        let _guard = self.lifecycle.lock().await;
        let notify = matches!(
            self.status(),
            MonitoringState::Active | MonitoringState::Triggered
        );
        self.stop_locked(app_handle).await;
        if notify {
            self.show_notification(app_handle, "已退出警戒状态");
        }
        Ok(())
    }

    async fn stop_locked(&self, app_handle: &AppHandle) {
        self.session.cancel();
        self.cancel_preparation_task().await;
        Self::cleanup_capture_processes(app_handle).await;
        self.emit_status(app_handle);
        log::info!("已执行监控停止与后台进程清理");
    }

    pub async fn shutdown(&self, app_handle: &AppHandle) {
        let _guard = self.lifecycle.lock().await;
        self.stop_locked(app_handle).await;
    }

    pub async fn system_unlocked(&self, app_handle: &AppHandle) {
        let _guard = self.lifecycle.lock().await;
        self.stop_locked(app_handle).await;
        self.show_notification(app_handle, "系统已解锁，应用状态已重置");
    }

    pub async fn listener_failed(&self, app_handle: &AppHandle, generation: u64) {
        let _guard = self.lifecycle.lock().await;
        if self.session.flags.is_action_generation_current(generation) {
            self.stop_locked(app_handle).await;
        }
    }

    pub async fn complete_trigger(&self, app_handle: &AppHandle, generation: u64) {
        if let Some(_guard) = self.lock_action(generation).await {
            self.stop_locked(app_handle).await;
        }
    }

    pub async fn exit_after_trigger(&self, app_handle: &AppHandle, generation: u64) {
        if let Some(_guard) = self.lock_action(generation).await {
            self.stop_locked(app_handle).await;
            app_handle.exit(0);
        }
    }

    pub async fn toggle(&self, app_handle: &AppHandle) {
        match self.begin_shortcut_toggle() {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                log::error!("处理快捷键切换失败: {}", error);
                return;
            }
        }

        let _guard = self.lifecycle.lock().await;
        if !self.session.flags.health_check() {
            log::warn!("输入监听器健康检查失败，清理当前监控");
            self.stop_locked(app_handle).await;
            return;
        }

        if self.status() == MonitoringState::Idle {
            let camera_id = app_handle.state::<AppState>().camera_id();
            if let Err(error) = self.start_locked(app_handle, camera_id).await {
                log::error!("快捷键启动监控失败: {}", error);
                self.stop_locked(app_handle).await;
            }
        } else {
            let notify = matches!(
                self.status(),
                MonitoringState::Active | MonitoringState::Triggered
            );
            self.stop_locked(app_handle).await;
            if notify {
                self.show_notification(app_handle, "已退出警戒状态");
            }
        }
    }

    fn begin_shortcut_toggle(&self) -> Result<bool, String> {
        let mut last_toggle = self
            .last_toggle_time
            .lock()
            .map_err(|_| "无法获取快捷键防抖锁".to_string())?;
        if last_toggle.elapsed() < SHORTCUT_DEBOUNCE_TIME {
            return Ok(false);
        }
        *last_toggle = Instant::now();
        self.session
            .flags
            .set_last_shortcut_time(Self::now_millis());
        self.session.flags.set_shortcut_in_progress(true);
        let flags = self.session.flags.clone();
        let shortcut_time = flags.last_shortcut_time();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(SHORTCUT_FLAG_CLEAR_DELAY).await;
            if flags.last_shortcut_time() == shortcut_time {
                flags.set_shortcut_in_progress(false);
            }
        });
        Ok(true)
    }

    pub fn handle_input(&self, event: Event, app_handle: &AppHandle) {
        let flags = &self.session.flags;
        let generation = flags.current_action_generation();
        if !flags.monitoring_active() {
            return;
        }
        let now = Self::now_millis();
        if input::should_ignore_input_event(
            flags.shortcut_in_progress(),
            now.saturating_sub(flags.last_shortcut_time()) < EVENT_IGNORE_WINDOW_MS,
        ) {
            return;
        }
        flags.set_last_activity_time(now);

        let recording = app_handle.state::<AppState>().config().post_trigger_action
            == PostTriggerAction::ScreenRecording;
        if recording
            && (app_handle
                .state::<crate::recorder::Recordings>()
                .screen_active()
                || app_handle
                    .state::<crate::recorder::Recordings>()
                    .screen_retry_remaining()
                    .is_some())
        {
            return;
        }

        let handle = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            let controller = handle.state::<MonitoringController>();
            {
                let _guard = controller.lifecycle.lock().await;
                if recording {
                    if controller
                        .session
                        .is_current(generation, MonitoringState::Active)
                    {
                        if let Err(error) =
                            crate::recorder::start_screen_recording(handle.clone()).await
                        {
                            log::error!("启动屏幕录制失败: {}", error);
                            controller.stop_locked(&handle).await;
                            controller.report_capture_error(&handle, &error);
                        }
                    }
                    return;
                }
                if !controller.session.trigger(generation) {
                    return;
                }
                log::info!("✓ 触发锁定！事件类型: {:?}", event.event_type);
                controller.emit_status(&handle);
            }
            actions::trigger_lockdown(handle.clone(), generation).await;
        });
    }

    fn start_idle_check(&self, app_handle: AppHandle, generation: u64) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let controller = app_handle.state::<MonitoringController>();
                let _guard = controller.lifecycle.lock().await;
                if !controller
                    .session
                    .is_current(generation, MonitoringState::Active)
                {
                    break;
                }
                let last_activity = controller.session.flags.last_activity_time();
                if last_activity == 0 {
                    continue;
                }
                let idle_ms = Self::now_millis().saturating_sub(last_activity);
                let result = match app_handle
                    .state::<crate::recorder::Recordings>()
                    .check_screen()
                {
                    Ok(true) if idle_ms > 20_000 => {
                        crate::recorder::stop_screen_recording(app_handle.clone()).await;
                        Ok(())
                    }
                    Ok(false)
                        if idle_ms <= 20_000
                            && app_handle
                                .state::<crate::recorder::Recordings>()
                                .screen_retry_remaining()
                                .is_none() =>
                    {
                        crate::recorder::start_screen_recording(app_handle.clone()).await
                    }
                    Ok(_) => Ok(()),
                    Err(error) => Err(error),
                };
                if let Err(error) = result {
                    // Detach this task before stop aborts helper tasks.
                    controller
                        .session
                        .flags
                        .idle_check_handle
                        .lock()
                        .unwrap()
                        .take();
                    controller.stop_locked(&app_handle).await;
                    controller.report_capture_error(&app_handle, &error);
                    break;
                }
            }
        })
    }

    fn replace_preparation_task(&self, task: JoinHandle<()>) {
        if let Some(previous) = self.preparation_task.lock().unwrap().replace(task) {
            previous.abort();
        }
    }

    async fn cancel_preparation_task(&self) {
        let task = self.preparation_task.lock().unwrap().take();
        if let Some(task) = task {
            task.abort();
            if let Err(error) = task.await {
                if !error.is_cancelled() {
                    log::error!("监控准备任务失败: {}", error);
                }
            }
        }
    }

    async fn cleanup_capture_processes(app_handle: &AppHandle) {
        let app_handle = app_handle.clone();
        if let Err(error) = tokio::task::spawn_blocking(move || {
            app_handle
                .state::<crate::recorder::Recordings>()
                .stop_matching(|_| true);
        })
        .await
        {
            log::error!("Recording cleanup failed: {error}");
        }
    }

    fn emit_status(&self, app_handle: &AppHandle) {
        let status = match self.status() {
            MonitoringState::Idle => "空闲",
            MonitoringState::Preparing => "准备中",
            MonitoringState::Active => "警戒中",
            MonitoringState::Triggered => "锁定中",
        };
        if let Err(error) = app_handle.emit("monitoring_status_changed", status) {
            log::error!("无法发送监控状态事件 '{}': {}", status, error);
        }
    }

    pub(crate) fn report_capture_error(&self, app_handle: &AppHandle, error: &str) {
        log::error!("Capture failed: {error}");
        if let Err(emit_error) = app_handle.emit("monitoring_error", error) {
            log::error!("Failed to emit capture error: {emit_error}");
        }
        if let Some(window) = app_handle.get_webview_window("main") {
            let _ = window.show();
        }
        self.show_notification(app_handle, "录像失败，请查看应用中的错误信息");
    }

    fn show_notification(&self, app_handle: &AppHandle, body: &str) {
        if app_handle.state::<AppState>().config().enable_notifications {
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
    }

    fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

async fn validate_capture_device(action: PostTriggerAction, camera_id: u32) -> Result<(), String> {
    if action == PostTriggerAction::ScreenRecording {
        return Ok(());
    }
    camera::ensure_camera_available(camera_id)?;
    if !camera::check_camera_permission(camera_id).await? {
        return Err("无法访问选中的摄像头，请检查权限或设备占用".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MonitoringController, MonitoringSession};
    use crate::state::MonitoringState;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn screen_recording_does_not_require_a_valid_camera() {
        use super::validate_capture_device;
        use crate::config::PostTriggerAction;
        assert!(
            validate_capture_device(PostTriggerAction::ScreenRecording, u32::MAX)
                .await
                .is_ok()
        );
        for action in [
            PostTriggerAction::CaptureOnly,
            PostTriggerAction::CaptureAndLock,
        ] {
            assert!(validate_capture_device(action, u32::MAX).await.is_err());
        }
    }

    #[test]
    fn cancelled_preparation_cannot_activate_a_restarted_session() {
        let session = MonitoringSession::new();
        let old_generation = session.prepare().unwrap();
        session.cancel();
        let new_generation = session.prepare().unwrap();
        session.flags.set_listener_ready(true);

        assert!(!session.activate(old_generation).unwrap());
        assert_eq!(session.status(), MonitoringState::Preparing);
        assert!(!session.flags.monitoring_active());
        assert!(session.activate(new_generation).unwrap());
        assert_eq!(session.status(), MonitoringState::Active);
    }

    #[test]
    fn stop_invalidates_triggered_actions_and_is_idempotent() {
        let session = MonitoringSession::new();
        let old_generation = session.prepare().unwrap();
        session.flags.set_listener_ready(true);
        assert!(session.activate(old_generation).unwrap());
        assert!(session.trigger(old_generation));
        assert!(!session.trigger(old_generation));

        session.cancel();
        session.cancel();
        assert_eq!(session.status(), MonitoringState::Idle);
        assert!(!session.flags.monitoring_active());
        let new_generation = session.prepare().unwrap();
        assert!(session.activate(new_generation).unwrap());
        assert!(!session.trigger(old_generation));
        assert!(session.trigger(new_generation));
        assert!(!session.is_current(old_generation, MonitoringState::Triggered));
        assert!(session.is_current(new_generation, MonitoringState::Triggered));
    }

    #[test]
    fn missing_listener_cannot_activate_preparation() {
        let session = MonitoringSession::new();
        let generation = session.prepare().unwrap();
        assert!(session.activate(generation).is_err());
        assert_eq!(session.status(), MonitoringState::Preparing);
        assert!(!session.flags.monitoring_active());
    }

    #[tokio::test]
    async fn cancelling_preparation_joins_the_task_and_allows_replacement() {
        let controller = MonitoringController::new();
        let (started_tx, started_rx) = oneshot::channel();
        let (resume_tx, resume_rx) = oneshot::channel::<()>();
        controller.replace_preparation_task(tokio::spawn(async move {
            started_tx.send(()).unwrap();
            let _ = resume_rx.await;
        }));
        started_rx.await.unwrap();
        controller.cancel_preparation_task().await;
        assert!(resume_tx.send(()).is_err());
        assert!(controller.preparation_task.lock().unwrap().is_none());

        let (completed_tx, completed_rx) = oneshot::channel();
        controller.replace_preparation_task(tokio::spawn(async move {
            completed_tx.send(()).unwrap();
        }));
        completed_rx.await.unwrap();
        controller.cancel_preparation_task().await;
    }

    #[tokio::test]
    async fn old_action_waiting_for_the_lock_cannot_affect_a_new_session() {
        let controller = Arc::new(MonitoringController::new());
        let old_generation = controller.session.prepare().unwrap();
        controller.session.flags.set_listener_ready(true);
        controller.session.activate(old_generation).unwrap();
        assert!(controller.session.trigger(old_generation));

        let lifecycle_guard = controller.lifecycle.lock().await;
        let pending_controller = controller.clone();
        let (started_tx, started_rx) = oneshot::channel();
        let pending_action = tokio::spawn(async move {
            started_tx.send(()).unwrap();
            pending_controller
                .lock_action(old_generation)
                .await
                .is_some()
        });
        started_rx.await.unwrap();
        assert!(!pending_action.is_finished());

        controller.session.cancel();
        let new_generation = controller.session.prepare().unwrap();
        controller.session.activate(new_generation).unwrap();
        assert!(controller.session.trigger(new_generation));
        drop(lifecycle_guard);

        assert!(!pending_action.await.unwrap());
        assert!(controller.lock_action(new_generation).await.is_some());
    }

    #[tokio::test]
    async fn stopping_monitoring_wakes_a_delayed_action() {
        let controller = MonitoringController::new();
        let generation = controller.session.prepare().unwrap();
        controller.session.flags.set_listener_ready(true);
        controller.session.activate(generation).unwrap();
        assert!(controller.session.trigger(generation));

        let delay = controller.wait_action_delay(generation, Duration::from_secs(60));
        tokio::pin!(delay);
        tokio::select! {
            biased;
            _ = &mut delay => panic!("action delay completed before cancellation"),
            _ = std::future::ready(()) => {}
        }
        controller.session.cancel();
        assert!(
            !tokio::time::timeout(Duration::from_secs(1), delay)
                .await
                .unwrap()
        );
    }
}
