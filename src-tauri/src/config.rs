use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const MAX_CAPTURE_DELAY_SECONDS: u32 = 60;

/// 触发后动作选项
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
    fn legacy_config_path() -> PathBuf {
        std::env::temp_dir().join("snaplock_config.json")
    }

    /// 获取配置文件路径（保存在用户配置目录）
    fn get_config_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let base_dir = dirs::config_dir().unwrap_or_else(std::env::temp_dir);
        let config_dir = base_dir.join("SnapLock");
        fs::create_dir_all(&config_dir)?;
        Ok(config_dir.join("snaplock_config.json"))
    }

    fn migrate_legacy_config_if_needed(
        config_path: &PathBuf,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let legacy_path = Self::legacy_config_path();

        if config_path.exists() || !legacy_path.exists() || *config_path == legacy_path {
            return Ok(());
        }

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::copy(&legacy_path, config_path)?;
        if let Err(error) = fs::remove_file(&legacy_path) {
            log::warn!(
                "旧配置文件迁移后删除失败 ({}): {}",
                legacy_path.display(),
                error
            );
        }

        log::info!(
            "已将旧配置从 '{}' 迁移到 '{}'",
            legacy_path.display(),
            config_path.display()
        );
        Ok(())
    }

    pub fn sanitize(mut self) -> Self {
        self.capture_delay_seconds = normalize_capture_delay(self.capture_delay_seconds);
        self
    }

    pub fn prepare_for_runtime(mut self) -> (Self, Option<u32>) {
        self = self.sanitize();

        match crate::camera::resolve_camera_selection(self.default_camera_id) {
            Ok(selection) => {
                self.default_camera_id = selection.persisted_default_camera_id;
                (self, selection.runtime_camera_id)
            }
            Err(error) => {
                log::warn!("运行时校验默认摄像头失败，保留当前配置: {}", error);
                let runtime_camera_id = self.default_camera_id;
                (self, runtime_camera_id)
            }
        }
    }

    /// 从文件加载配置
    pub fn load() -> Self {
        match Self::get_config_path() {
            Ok(config_path) => {
                if let Err(error) = Self::migrate_legacy_config_if_needed(&config_path) {
                    log::warn!("迁移旧配置失败: {}", error);
                }

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
        self.save_to(&config_path)
    }

    fn save_to(&self, config_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_json::to_string_pretty(&self.clone().sanitize())
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        fs::write(&config_path, content)
            .map_err(|e| format!("Failed to write config file: {}", e))?;

        println!("配置文件已保存: {:?}", config_path);
        Ok(())
    }

    pub fn effective_save_path(&self) -> String {
        self.save_path.clone().unwrap_or_else(get_default_save_path)
    }

    fn configure_logger(&self) {
        if let Some(logger) = crate::logger::get_logger() {
            logger.set_log_file_path(Some(self.effective_save_path()));
            logger.set_log_to_file(self.save_logs_to_file);
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

pub fn save_config(app_handle: AppHandle) -> Result<(), String> {
    app_handle
        .state::<crate::state::AppState>()
        .config()
        .save()
        .map_err(|e| e.to_string())
}

pub fn initialize_config(app_handle: AppHandle) -> Result<(), String> {
    let loaded_config = AppConfig::load();
    let original_default_camera_id = loaded_config.default_camera_id;
    let (config, runtime_camera_id) = loaded_config.prepare_for_runtime();
    let state = app_handle.state::<crate::state::AppState>();
    if let Some(camera_id) = runtime_camera_id {
        state.set_camera_id(camera_id);
    }
    config.configure_logger();

    if config.default_camera_id != original_default_camera_id {
        if let Err(error) = config.save() {
            log::warn!("保存已修正的默认摄像头配置失败: {}", error);
        }
    }

    *state.config.lock().unwrap() = config;
    Ok(())
}

#[tauri::command]
pub fn get_config(app_handle: AppHandle) -> AppConfig {
    app_handle.state::<crate::state::AppState>().config()
}

#[derive(Debug, Deserialize)]
#[serde(tag = "key", content = "value", rename_all = "snake_case")]
pub enum ConfigChange {
    ShortcutKey(String),
    SavePath(Option<String>),
    ShowDebugLogs(bool),
    SaveLogsToFile(bool),
    DarkMode(bool),
    ExitOnLock(bool),
    EnableNotifications(bool),
    PostTriggerAction(PostTriggerAction),
    DefaultCameraId(Option<u32>),
    CaptureDelaySeconds(u32),
}

impl ConfigChange {
    fn apply(self, config: &mut AppConfig) -> Result<(), String> {
        match self {
            Self::ShortcutKey(value) => {
                let parts: Vec<_> = value.split('+').collect();
                let modifiers = ["Ctrl", "Alt", "Shift", "Meta", "Cmd"];
                if parts.len() < 2
                    || parts[..parts.len() - 1]
                        .iter()
                        .any(|part| !modifiers.contains(part))
                    || parts
                        .last()
                        .is_none_or(|key| key.trim().is_empty() || modifiers.contains(key))
                {
                    return Err("无效的快捷键格式".into());
                }
                config.shortcut_key = value;
            }
            Self::SavePath(value) => config.save_path = value,
            Self::ShowDebugLogs(value) => config.show_debug_logs = value,
            Self::SaveLogsToFile(value) => config.save_logs_to_file = value,
            Self::DarkMode(value) => config.dark_mode = value,
            Self::ExitOnLock(value) => config.exit_on_lock = value,
            Self::EnableNotifications(value) => config.enable_notifications = value,
            Self::PostTriggerAction(value) => config.post_trigger_action = value,
            Self::DefaultCameraId(value) => {
                if let Some(id) = value {
                    crate::camera::ensure_camera_available(id)?;
                }
                config.default_camera_id = value;
            }
            Self::CaptureDelaySeconds(value) => {
                if value > MAX_CAPTURE_DELAY_SECONDS {
                    return Err("拍摄延迟必须在 0 到 60 秒之间".into());
                }
                config.capture_delay_seconds = value;
            }
        }
        Ok(())
    }
}

/// Serialize updates and publish only persisted values, including logger side effects.
#[tauri::command]
pub fn update_config(app_handle: AppHandle, change: ConfigChange) -> Result<AppConfig, String> {
    let state = app_handle.state::<crate::state::AppState>();
    let _update = state.settings_update.lock().unwrap();
    let previous = state.config();
    let mut next = previous.clone();
    let camera_changed = matches!(change, ConfigChange::DefaultCameraId(_));
    change.apply(&mut next)?;
    let path = AppConfig::get_config_path().map_err(|error| format!("保存配置失败: {error}"))?;
    let shortcut_changed = previous.shortcut_key != next.shortcut_key;
    if shortcut_changed {
        crate::app_setup::update_global_shortcut(
            &app_handle,
            &previous.shortcut_key,
            &next.shortcut_key,
        )
        .map_err(|error| format!("快捷键注册失败: {error}"))?;
    }
    if let Err(error) = commit_config(&state, &next, &path) {
        if shortcut_changed {
            if let Err(rollback_error) = crate::app_setup::update_global_shortcut(
                &app_handle,
                &next.shortcut_key,
                &previous.shortcut_key,
            ) {
                *state.config.lock().unwrap() = next;
                return Err(format!(
                    "保存配置失败，且回滚快捷键失败: {error}; {rollback_error}"
                ));
            }
        }
        return Err(format!("保存配置失败: {error}"));
    }
    next.configure_logger();
    if camera_changed {
        if let Some(id) = next.default_camera_id {
            state.set_camera_id(id);
        }
    }
    log::info!("Settings updated");
    Ok(next)
}

fn commit_config(
    state: &crate::state::AppState,
    next: &AppConfig,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    next.save_to(path)?;
    *state.config.lock().unwrap() = next.clone();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, CaptureMode, ConfigChange, PostTriggerAction, commit_config};
    use crate::state::AppState;
    use std::path::Path;

    #[test]
    fn config_updates_publish_only_after_real_storage_succeeds() {
        let directory =
            crate::media_files::capture_path(Path::new(r"D:\Temp\Pi"), "snaplock-config-", "test");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("config.json");
        let state = AppState::new(7);
        let mut next = state.config();
        ConfigChange::DarkMode(true).apply(&mut next).unwrap();
        commit_config(&state, &next, &path).unwrap();
        let saved = std::fs::read(&path).unwrap();
        let restored: AppConfig = serde_json::from_slice(&saved).unwrap();
        assert!(restored.dark_mode && state.config().dark_mode);
        next.dark_mode = false;
        // The existing config file is a real invalid directory, not a mocked failure.
        assert!(commit_config(&state, &next, &path.join("config.json")).is_err());
        assert!(state.config().dark_mode);
        assert_eq!(std::fs::read(path).unwrap(), saved);
        assert_eq!(state.camera_id(), 7);
    }

    #[test]
    fn nullable_settings_and_invalid_changes_preserve_the_config_contract() {
        let mut config = AppConfig {
            default_camera_id: Some(7),
            ..AppConfig::default()
        };
        let change: ConfigChange =
            serde_json::from_str(r#"{"key":"default_camera_id","value":null}"#).unwrap();
        change.apply(&mut config).unwrap();
        assert_eq!(config.default_camera_id, None);
        for change in [
            ConfigChange::CaptureDelaySeconds(61),
            ConfigChange::ShortcutKey("Alt".into()),
        ] {
            assert!(change.apply(&mut config).is_err());
        }
        assert_eq!(config.capture_delay_seconds, 0);
        assert_eq!(config.shortcut_key, "Alt+L");
    }

    #[test]
    fn older_configs_keep_defaults_for_newer_fields() {
        let config: AppConfig = serde_json::from_str(r#"{"shortcut_key":"Alt+L","save_path":null,"show_debug_logs":false,"save_logs_to_file":false,"dark_mode":false,"exit_on_lock":false}"#).unwrap();
        assert!(config.enable_notifications);
        assert_eq!(
            config.post_trigger_action,
            PostTriggerAction::CaptureAndLock
        );
        assert_eq!(config.capture_delay_seconds, 0);
        assert_eq!(config.capture_mode, CaptureMode::Video);
    }

    #[test]
    fn config_roundtrip_preserves_security_related_fields() {
        let config = AppConfig {
            save_path: Some("D:/captures".into()),
            post_trigger_action: PostTriggerAction::ScreenRecording,
            default_camera_id: Some(42),
            capture_delay_seconds: 15,
            capture_mode: CaptureMode::Video,
            ..AppConfig::default()
        };

        assert_eq!(config.save_path.as_deref(), Some("D:/captures"));
        assert_eq!(
            config.post_trigger_action,
            PostTriggerAction::ScreenRecording
        );
        assert_eq!(config.default_camera_id, Some(42));
        assert_eq!(config.capture_delay_seconds, 15);

        let restored: AppConfig =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(restored.save_path.as_deref(), Some("D:/captures"));
        assert_eq!(
            restored.post_trigger_action,
            PostTriggerAction::ScreenRecording
        );
        assert_eq!(restored.default_camera_id, Some(42));
        assert_eq!(restored.capture_delay_seconds, 15);
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
