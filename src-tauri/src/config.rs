use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const MAX_CAPTURE_DELAY_SECONDS: u32 = 60;

/// 触发后动作选项
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PostTriggerAction {
    /// 拍摄并锁屏
    CaptureAndLock,
    /// 只拍摄
    CaptureOnly,
    /// 屏幕录制
    ScreenRecording,
}

impl Default for PostTriggerAction {
    fn default() -> Self {
        PostTriggerAction::CaptureAndLock
    }
}

/// 拍摄模式选项
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CaptureMode {
    /// 录像模式
    Video,
}

impl Default for CaptureMode {
    fn default() -> Self {
        CaptureMode::Video
    }
}

/// 为启用系统通知提供默认值
fn default_enable_notifications() -> bool {
    true
}

/// 为拍摄延迟时间提供默认值
fn default_capture_delay_seconds() -> u32 {
    0
}

fn normalize_capture_delay(delay: u32) -> u32 {
    delay.min(MAX_CAPTURE_DELAY_SECONDS)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub shortcut_key: String,
    pub save_path: Option<String>,
    pub show_debug_logs: bool,
    pub save_logs_to_file: bool,
    pub dark_mode: bool,
    pub exit_on_lock: bool,
    #[serde(default)]
    pub post_trigger_action: PostTriggerAction,
    #[serde(default = "default_enable_notifications")]
    pub enable_notifications: bool,
    #[serde(default)]
    pub default_camera_id: Option<u32>,
    #[serde(default = "default_capture_delay_seconds")]
    pub capture_delay_seconds: u32,
    #[serde(default)]
    pub capture_mode: CaptureMode,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            shortcut_key: "Alt+L".to_string(),
            save_path: None,
            show_debug_logs: false,
            save_logs_to_file: false,
            dark_mode: false,
            exit_on_lock: false,
            post_trigger_action: PostTriggerAction::CaptureAndLock,
            enable_notifications: true,
            default_camera_id: None,
            capture_delay_seconds: 0,
            capture_mode: CaptureMode::Video,
        }
    }
}

impl AppConfig {
    /// 获取配置文件路径（保存在系统临时目录）
    fn get_config_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let temp_dir = std::env::temp_dir();
        let config_path = temp_dir.join("snaplock_config.json");
        Ok(config_path)
    }

    fn sanitize(mut self) -> Self {
        self.capture_delay_seconds = normalize_capture_delay(self.capture_delay_seconds);
        self
    }

    /// 从文件加载配置
    pub fn load() -> Self {
        match Self::get_config_path() {
            Ok(config_path) => {
                if config_path.exists() {
                    match fs::read_to_string(&config_path) {
                        Ok(content) => match serde_json::from_str::<AppConfig>(&content) {
                            Ok(config) => {
                                println!("配置文件加载成功: {:?}", config_path);
                                return config.sanitize();
                            }
                            Err(e) => {
                                log::error!("配置文件解析失败: {}", e);
                            }
                        },
                        Err(e) => {
                            log::error!("读取配置文件失败: {}", e);
                        }
                    }
                } else {
                    log::info!("配置文件不存在，使用默认配置");
                }
            }
            Err(e) => {
                log::error!("获取配置文件路径失败: {}", e);
            }
        }

        Self::default()
    }

    /// 保存配置到文件
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = Self::get_config_path()?;
        let content = serde_json::to_string_pretty(&self.clone().sanitize())
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        fs::write(&config_path, content)
            .map_err(|e| format!("Failed to write config file: {}", e))?;

        println!("配置文件已保存: {:?}", config_path);
        Ok(())
    }

    /// 从应用状态更新配置
    pub fn update_from_state(&mut self, state: &crate::state::AppState) {
        self.shortcut_key = state.shortcut_key();
        self.save_path = state.save_path();
        self.show_debug_logs = state.show_debug_logs();
        self.save_logs_to_file = state.save_logs_to_file();
        self.exit_on_lock = state.exit_on_lock();
        self.dark_mode = state.dark_mode();
        self.post_trigger_action = state.post_trigger_action();
        self.enable_notifications = state.enable_notifications();
        self.default_camera_id = state.default_camera_id();
        self.capture_delay_seconds = normalize_capture_delay(state.capture_delay_seconds());
        self.capture_mode = state.capture_mode();
    }

    /// 将配置应用到应用状态
    pub fn apply_to_state(&self, state: &crate::state::AppState) {
        state.set_shortcut_key(self.shortcut_key.clone());
        state.set_save_path(self.save_path.clone());
        state.set_show_debug_logs(self.show_debug_logs);
        state.set_save_logs_to_file(self.save_logs_to_file);
        state.set_exit_on_lock(self.exit_on_lock);
        state.set_dark_mode(self.dark_mode);
        state.set_post_trigger_action(self.post_trigger_action.clone());
        state.set_enable_notifications(self.enable_notifications);
        state.set_default_camera_id(self.default_camera_id);

        if let Some(camera_id) = self.default_camera_id {
            state.set_camera_id(camera_id);
        }

        state.set_capture_delay_seconds(normalize_capture_delay(self.capture_delay_seconds));
        state.set_capture_mode(self.capture_mode.clone());

        if self.save_logs_to_file {
            if let Some(logger) = crate::logger::get_logger() {
                logger.set_log_file_path(Some(state.get_effective_save_path()));
            }
        }
    }
}

/// 获取默认保存路径（桌面）
pub fn get_default_save_path() -> String {
    match dirs::desktop_dir() {
        Some(desktop) => desktop.to_string_lossy().to_string(),
        None => match std::env::current_dir() {
            Ok(current) => current.to_string_lossy().to_string(),
            Err(_) => ".".to_string(),
        },
    }
}

/// 自动保存配置的命令
#[tauri::command]
pub fn save_config(app_handle: AppHandle) -> Result<(), String> {
    let state = app_handle.state::<crate::state::AppState>();
    let mut config = AppConfig::default();
    config.update_from_state(&state);

    config.save().map_err(|e| e.to_string())?;
    Ok(())
}

/// 加载配置的命令
#[tauri::command]
pub fn load_config(app_handle: AppHandle) -> Result<AppConfig, String> {
    let config = AppConfig::load();
    let state = app_handle.state::<crate::state::AppState>();
    config.apply_to_state(&state);
    Ok(config)
}

/// 保存暗色模式设置的命令
#[tauri::command]
pub fn save_dark_mode_setting(app_handle: AppHandle, enabled: bool) -> Result<(), String> {
    let state = app_handle.state::<crate::state::AppState>();
    state.set_dark_mode(enabled);

    let mut config = AppConfig::load();
    config.update_from_state(&state);
    config.save().map_err(|e| e.to_string())?;

    log::info!("暗色模式设置已保存: {}", enabled);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, CaptureMode, PostTriggerAction};
    use crate::state::AppState;

    #[test]
    fn config_roundtrip_preserves_security_related_fields() {
        let state = AppState::new(7);
        state.set_save_path(Some("D:/captures".to_string()));
        state.set_post_trigger_action(PostTriggerAction::ScreenRecording);
        state.set_default_camera_id(Some(42));
        state.set_capture_delay_seconds(15);
        state.set_capture_mode(CaptureMode::Video);

        let mut config = AppConfig::default();
        config.update_from_state(&state);

        assert_eq!(config.save_path.as_deref(), Some("D:/captures"));
        assert_eq!(
            config.post_trigger_action,
            PostTriggerAction::ScreenRecording
        );
        assert_eq!(config.default_camera_id, Some(42));
        assert_eq!(config.capture_delay_seconds, 15);

        let restored_state = AppState::new(0);
        config.apply_to_state(&restored_state);

        assert_eq!(restored_state.save_path().as_deref(), Some("D:/captures"));
        assert_eq!(
            restored_state.post_trigger_action(),
            PostTriggerAction::ScreenRecording
        );
        assert_eq!(restored_state.default_camera_id(), Some(42));
        assert_eq!(restored_state.camera_id(), 42);
    }

    #[test]
    fn config_sanitizes_capture_delay() {
        let config = AppConfig {
            capture_delay_seconds: 999,
            ..AppConfig::default()
        }
        .sanitize();

        assert_eq!(config.capture_delay_seconds, 60);
    }
}
