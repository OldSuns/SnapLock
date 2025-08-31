use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// 触发后动作选项
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
            enable_notifications: true, // 默认启用系统通知
            default_camera_id: None, // 默认不设置摄像头ID
            capture_delay_seconds: 0, // 默认0秒延迟
            capture_mode: CaptureMode::Video, // 默认录像模式
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

    /// 从文件加载配置
    pub fn load() -> Self {
        match Self::get_config_path() {
            Ok(config_path) => {
                if config_path.exists() {
                    match fs::read_to_string(&config_path) {
                        Ok(content) => {
                            match serde_json::from_str::<AppConfig>(&content) {
                                Ok(config) => {
                                    println!("配置文件加载成功: {:?}", config_path);
                                    return config;
                                }
                                Err(e) => {
                                    log::error!("配置文件解析失败: {}", e);
                                }
                            }
                        }
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
        
        let content = serde_json::to_string_pretty(self)
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
        
        // 直接使用post_trigger_action状态
        self.post_trigger_action = state.post_trigger_action();
        
        self.enable_notifications = state.enable_notifications();
        
        // 更新默认摄像头ID
        self.default_camera_id = Some(state.camera_id());
        
        // 更新拍摄时间设置
        self.capture_delay_seconds = state.capture_delay_seconds();
        self.capture_mode = state.capture_mode();
    }

    /// 将配置应用到应用状态
    pub fn apply_to_state(&self, state: &crate::state::AppState) {
        state.set_shortcut_key(self.shortcut_key.clone());
        
        // 总是设置save_path状态，无论配置中是Some还是None
        // 这确保配置文件中的设置（包括None）总是优先于默认值
        state.set_save_path(self.save_path.clone());
        
        state.set_show_debug_logs(self.show_debug_logs);
        state.set_save_logs_to_file(self.save_logs_to_file);
        state.set_exit_on_lock(self.exit_on_lock);
        state.set_dark_mode(self.dark_mode);
        
        // 根据post_trigger_action设置enable_screen_lock状态
        // 这是为了向后兼容旧代码
        match self.post_trigger_action {
            PostTriggerAction::CaptureAndLock => state.set_enable_screen_lock(true),
            PostTriggerAction::CaptureOnly => state.set_enable_screen_lock(false),
            PostTriggerAction::ScreenRecording => state.set_enable_screen_lock(false), // 录屏时也不锁屏
        }
        
        state.set_enable_notifications(self.enable_notifications);
        
        // 应用默认摄像头ID设置
        if let Some(camera_id) = self.default_camera_id {
            state.set_camera_id(camera_id);
        }
        
        // 应用拍摄时间设置
        state.set_capture_delay_seconds(self.capture_delay_seconds);
        state.set_capture_mode(self.capture_mode.clone());
    }

}

/// 获取默认保存路径（桌面）
pub fn get_default_save_path() -> String {
    match dirs::desktop_dir() {
        Some(desktop) => desktop.to_string_lossy().to_string(),
        None => {
            // 如果无法获取桌面路径，使用当前目录
            match std::env::current_dir() {
                Ok(current) => current.to_string_lossy().to_string(),
                Err(_) => ".".to_string(), // 最后的后备选项
            }
        }
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
    
    // 更新后端状态
    state.set_dark_mode(enabled);
    
    // 获取当前状态并保存配置
    let mut config = AppConfig::load();
    config.update_from_state(&state);
    
    // 保存配置
    config.save().map_err(|e| e.to_string())?;
    log::info!("暗色模式设置已保存: {}", enabled);
    Ok(())
}