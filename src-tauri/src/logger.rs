use log::{Level, Log, Metadata, Record};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use chrono::Local;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
    pub target: String,
}

impl LogEntry {
    pub fn new(record: &Record) -> Self {
        Self {
            timestamp: Local::now().format("%H:%M:%S%.3f").to_string(),
            level: record.level().to_string(),
            message: record.args().to_string(),
            target: record.target().to_string(),
        }
    }

    pub fn format_for_file(&self) -> String {
        format!("[{}] [{}] [{}] {}\n", 
            self.timestamp, 
            self.level, 
            self.target, 
            self.message
        )
    }
}

pub struct AppLogger {
    logs: Arc<Mutex<VecDeque<LogEntry>>>,
    app_handle: Option<AppHandle>,
    max_logs: usize,
    log_to_file: Arc<Mutex<bool>>,
    log_file_path: Arc<Mutex<Option<String>>>,
}

impl AppLogger {
    pub fn new(max_logs: usize) -> Self {
        Self {
            logs: Arc::new(Mutex::new(VecDeque::with_capacity(max_logs))),
            app_handle: None,
            max_logs,
            log_to_file: Arc::new(Mutex::new(false)),
            log_file_path: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_app_handle(&mut self, app_handle: AppHandle) {
        self.app_handle = Some(app_handle);
    }

    pub fn set_log_to_file(&self, enabled: bool) {
        *self.log_to_file.lock().unwrap() = enabled;
    }

    pub fn set_log_file_path(&self, path: Option<String>) {
        *self.log_file_path.lock().unwrap() = path;
    }

    pub fn get_logs(&self) -> Vec<LogEntry> {
        self.logs.lock().unwrap().iter().cloned().collect()
    }

    pub fn clear_logs(&self) {
        self.logs.lock().unwrap().clear();
    }

    fn write_to_file(&self, entry: &LogEntry) {
        if !*self.log_to_file.lock().unwrap() {
            return;
        }

        if let Some(base_path) = self.log_file_path.lock().unwrap().as_ref() {
            let log_file_path = Path::new(base_path).join("snaplock_debug.log");
            
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_file_path) 
            {
                if let Err(e) = file.write_all(entry.format_for_file().as_bytes()) {
                    eprintln!("Failed to write to log file: {}", e);
                }
            } else {
                eprintln!("Failed to open log file: {:?}", log_file_path);
            }
        }
    }
}

impl Log for AppLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let target = metadata.target();
        let level = metadata.level();
        
        // 完全过滤掉winit相关的日志
        if target.contains("winit") {
            return false;
        }
        
        // 过滤掉wgpu相关的调试日志
        if target.contains("wgpu") && level > Level::Error {
            return false;
        }
        
        // 过滤掉tao相关的日志（Tauri的窗口库）
        if target.contains("tao") {
            return false;
        }
        
        // 过滤掉其他图形相关的库
        if target.contains("wry") && level > Level::Error {
            return false;
        }
        
        // 只记录我们应用的日志和重要的系统错误
        target.starts_with("snaplock") ||
        target.starts_with("crate") ||
        (level <= Level::Info && !target.contains("winit") && !target.contains("tao"))
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let entry = LogEntry::new(record);
        
        // 添加到内存缓冲区
        {
            let mut logs = self.logs.lock().unwrap();
            if logs.len() >= self.max_logs {
                logs.pop_front();
            }
            logs.push_back(entry.clone());
        }

        // 写入文件
        self.write_to_file(&entry);

        // 发送到前端
        if let Some(ref app_handle) = self.app_handle {
            if let Err(e) = app_handle.emit("log_entry", &entry) {
                eprintln!("Failed to emit log entry: {}", e);
            }
        }
    }

    fn flush(&self) {
        // 日志立即写入，无需特殊刷新
    }
}

// 全局日志实例
use std::sync::OnceLock;
static LOGGER: OnceLock<AppLogger> = OnceLock::new();

pub fn init_logger(app_handle: AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let mut logger = AppLogger::new(1000); // 最多保存1000条日志
    logger.set_app_handle(app_handle);
    
    if LOGGER.set(logger).is_err() {
        return Err("Logger already initialized".into());
    }
    
    if let Some(logger) = LOGGER.get() {
        if let Err(e) = log::set_logger(logger) {
            return Err(format!("Failed to set logger: {:?}", e).into());
        }
        log::set_max_level(log::LevelFilter::Debug);
    }
    
    Ok(())
}

pub fn get_logger() -> Option<&'static AppLogger> {
    LOGGER.get()
}

// Tauri 命令
#[tauri::command]
pub fn get_debug_logs() -> Vec<LogEntry> {
    if let Some(logger) = get_logger() {
        logger.get_logs()
    } else {
        Vec::new()
    }
}

#[tauri::command]
pub fn clear_debug_logs() {
    if let Some(logger) = get_logger() {
        logger.clear_logs();
    }
}

#[tauri::command]
pub fn set_log_to_file(enabled: bool) {
    if let Some(logger) = get_logger() {
        logger.set_log_to_file(enabled);
    }
}

#[tauri::command]
pub fn set_log_file_path(path: String) {
    if let Some(logger) = get_logger() {
        logger.set_log_file_path(Some(path));
    }
}