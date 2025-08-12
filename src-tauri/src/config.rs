use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// 为启用锁屏功能提供默认值
fn default_enable_screen_lock() -> bool {
    true
}

/// 为启用系统通知提供默认值
fn default_enable_notifications() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub shortcut_key: String,
    pub save_path: Option<String>,
    pub show_debug_logs: bool,
    pub save_logs_to_file: bool,
    pub dark_mode: bool,
    pub exit_on_lock: bool,
    #[serde(default = "default_enable_screen_lock")]
    pub enable_screen_lock: bool,
    #[serde(default = "default_enable_notifications")]
    pub enable_notifications: bool,
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
            enable_screen_lock: true, // 默认启用锁屏功能
            enable_notifications: true, // 默认启用系统通知
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
        self.enable_screen_lock = state.enable_screen_lock();
        self.enable_notifications = state.enable_notifications();
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
        state.set_enable_screen_lock(self.enable_screen_lock);
        state.set_enable_notifications(self.enable_notifications);
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