use chrono::Local;
use image::{ImageBuffer, RgbImage};
use nokhwa::{
    Camera,
    pixel_format::RgbFormat,
    query,
    utils::{ApiBackend, CameraIndex, CameraInfo, RequestedFormat, RequestedFormatType},
};
use std::path::PathBuf;
use tauri::{command, Manager, AppHandle};
use serde::Serialize;
use std::sync::Mutex;
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use base64::{Engine as _, engine::general_purpose};
use std::io::Cursor;

/// Camera information for frontend
#[derive(Serialize)]
pub struct CameraListItem {
    pub id: u32,
    pub name: String,
}

lazy_static::lazy_static! {
    static ref VIDEO_PROCESSES: Mutex<HashMap<u32, Child>> = Mutex::new(HashMap::new());
}

/// A Tauri command that retrieves a list of available cameras with their actual indices.
#[command]
pub async fn get_camera_list() -> Result<Vec<CameraListItem>, String> {
    query(ApiBackend::Auto)
        .map(|cameras| {
            let camera_list: Vec<CameraListItem> = cameras
                .into_iter()
                .enumerate()
                .map(|(array_index, info)| {
                    let actual_id = match info.index() {
                        CameraIndex::Index(id) => *id,
                        CameraIndex::String(s) => s.parse::<u32>().unwrap_or(array_index as u32),
                    };
                    CameraListItem {
                        id: actual_id,
                        name: info.human_name().to_string(),
                    }
                })
                .collect();
            
            println!("Found {} cameras with IDs: {:?}",
                camera_list.len(),
                camera_list.iter().map(|c| c.id).collect::<Vec<_>>());
            camera_list
        })
        .map_err(|e| {
            eprintln!("Camera enumeration failed: {}", e);
            format!("Failed to get camera list: {}", e)
        })
}

/// Validates if the given camera ID is available and returns the corresponding CameraInfo
fn validate_camera_id(camera_id: u32) -> Result<CameraInfo, String> {
    let cameras = query(ApiBackend::Auto)
        .map_err(|e| format!("Failed to query cameras for validation: {}", e))?;
    
    if cameras.is_empty() {
        return Err("No cameras available on the system".to_string());
    }
    
    // 查找具有指定ID的摄像头
    for camera_info in cameras.iter() {
        let info_id = match camera_info.index() {
            CameraIndex::Index(id) => *id,
            CameraIndex::String(s) => s.parse::<u32>().unwrap_or(u32::MAX),
        };
        
        if info_id == camera_id {
            return Ok(camera_info.clone());
        }
    }
    
    // 如果没有找到指定ID的摄像头，列出所有可用的ID
    let available_ids: Vec<u32> = cameras.iter().map(|info| {
        match info.index() {
            CameraIndex::Index(id) => *id,
            CameraIndex::String(s) => s.parse::<u32>().unwrap_or(0),
        }
    }).collect();
    
    Err(format!(
        "Camera ID {} not found. Available camera IDs: {:?}",
        camera_id,
        available_ids
    ))
}

/// Ensures camera stream is properly closed, even if an error occurs
struct CameraGuard {
    camera: Option<Camera>,
}

impl CameraGuard {
    fn new(camera: Camera) -> Self {
        Self { camera: Some(camera) }
    }
    
    fn get_mut(&mut self) -> Option<&mut Camera> {
        self.camera.as_mut()
    }
}

impl Drop for CameraGuard {
    fn drop(&mut self) {
        if let Some(mut camera) = self.camera.take() {
            if let Err(e) = camera.stop_stream() {
                eprintln!("Warning: Failed to stop camera stream during cleanup: {}", e);
            }
        }
    }
}

/// 通用的相机初始化函数
fn init_camera(camera_id: u32) -> Result<Camera, String> {
    validate_camera_id(camera_id)?;
    
    let index = CameraIndex::Index(camera_id);
    let requested = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution);
    
    Camera::new(index, requested)
        .map_err(|e| format!("Failed to initialize camera ID {}: {}", camera_id, e))
}

/// 通用的图像捕获函数
fn capture_frame(camera: &mut Camera, camera_id: u32) -> Result<(u32, u32, Vec<u8>), String> {
    camera.open_stream()
        .map_err(|e| format!("Failed to open stream for camera ID {}: {}", camera_id, e))?;
    
    let frame = camera.frame()
        .map_err(|e| format!("Failed to capture frame from camera ID {}: {}", camera_id, e))?;
    
    let decoded_buffer = frame.decode_image::<RgbFormat>()
        .map_err(|e| format!("Failed to decode image from camera ID {}: {}", camera_id, e))?;
    
    Ok((decoded_buffer.width(), decoded_buffer.height(), decoded_buffer.into_raw()))
}

/// 通用的保存路径处理函数
fn get_save_path(save_path: Option<String>) -> Result<PathBuf, String> {
    let base_path = match save_path {
        Some(path) => PathBuf::from(path),
        None => dirs::desktop_dir()
            .ok_or_else(|| "Desktop directory not found".to_string())?,
    };

    if !base_path.exists() {
        std::fs::create_dir_all(&base_path)
            .map_err(|e| format!("Failed to create save directory '{}': {}", base_path.display(), e))?;
    }

    Ok(base_path)
}

/// Captures a photo using the specified camera and saves it to a configurable path.
pub async fn take_photo(camera_id: u32, save_path: Option<String>) -> Result<String, String> {
    println!("Starting async photo capture with camera ID: {}", camera_id);

    tokio::task::spawn_blocking(move || {
        let camera_info = validate_camera_id(camera_id)?;
        println!("Using camera: {} (ID: {})", camera_info.human_name(), camera_id);

        let camera = init_camera(camera_id)?;
        let mut camera_guard = CameraGuard::new(camera);

        let (width, height, raw_buffer) = {
            let cam = camera_guard.get_mut()
                .ok_or("Camera guard failed to provide camera reference")?;
            capture_frame(cam, camera_id)?
        };
        
        let rgb_image: RgbImage = ImageBuffer::from_raw(width, height, raw_buffer)
            .ok_or("Failed to create image buffer from raw data")?;

        let base_path = get_save_path(save_path)?;
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("snaplock_capture_{}.jpg", timestamp);
        let filepath = base_path.join(&filename);

        println!("Saving image to: {}", filepath.display());
        rgb_image.save(&filepath)
            .map_err(|e| format!("Failed to save image to '{}': {}", filepath.display(), e))?;

        Ok(filepath.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

use crate::state::AppState;

/// Sets the custom save path for photos.
#[command]
pub fn set_save_path(path: String, app_handle: tauri::AppHandle) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    state.set_save_path(Some(path));
    
    if let Err(e) = crate::config::save_config(app_handle.clone()) {
        log::warn!("保存配置失败: {}", e);
    }
    
    Ok(())
}

/// 检查相机权限
#[command]
pub async fn check_camera_permission(camera_id: u32) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || {
        match init_camera(camera_id) {
            Ok(mut camera) => {
                match camera.open_stream() {
                    Ok(_) => {
                        let _ = camera.stop_stream();
                        Ok(true)
                    }
                    Err(e) => {
                        log::warn!("相机权限检查失败: {}", e);
                        Ok(false)
                    }
                }
            }
            Err(e) => {
                log::warn!("相机初始化失败: {}", e);
                Ok(false)
            }
        }
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// 获取相机预览帧（base64编码的JPEG）
#[command]
pub async fn get_camera_preview(camera_id: u32) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let camera = init_camera(camera_id)?;
        let mut camera_guard = CameraGuard::new(camera);
        
        let (width, height, raw_buffer) = {
            let cam = camera_guard.get_mut()
                .ok_or("Camera guard failed to provide camera reference")?;
            
            cam.open_stream()
                .map_err(|e| format!("Failed to open stream for camera ID {}: {}", camera_id, e))?;
            
            // 等待几帧以获得稳定的图像
            for _ in 0..3 {
                let _ = cam.frame();
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            
            capture_frame(cam, camera_id)?
        };
        
        let rgb_image: RgbImage = ImageBuffer::from_raw(width, height, raw_buffer)
            .ok_or("Failed to create image buffer from raw preview data")?;
        
        // 调整图像大小以减少数据传输
        let preview_image = image::imageops::resize(&rgb_image, 320, 240, image::imageops::FilterType::Lanczos3);
        
        // 转换为JPEG格式
        let mut jpeg_buffer = Vec::new();
        {
            let mut cursor = Cursor::new(&mut jpeg_buffer);
            preview_image.write_to(&mut cursor, image::ImageFormat::Jpeg)
                .map_err(|e| format!("Failed to encode preview as JPEG: {}", e))?;
        }
        
        // 转换为base64
        let base64_image = general_purpose::STANDARD.encode(&jpeg_buffer);
        Ok(format!("data:image/jpeg;base64,{}", base64_image))
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// 开始录像
pub async fn start_video_recording(app_handle: AppHandle, camera_id: u32, save_path: Option<String>, duration_seconds: Option<u32>) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        validate_camera_id(camera_id)?;
        
        let base_path = get_save_path(save_path)?;
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("snaplock_video_{}.mkv", timestamp);
        let filepath = base_path.join(&filename);

        let ffmpeg_path = match app_handle.path().resolve("libs/ffmpeg/bin/ffmpeg.exe", tauri::path::BaseDirectory::Resource) {
            Ok(path) => path,
            Err(e) => return Err(format!("无法解析ffmpeg路径: {}", e)),
        };

        let result = try_simple_recording(&ffmpeg_path.to_string_lossy(), camera_id, &filepath, duration_seconds);
        
        match result {
            Ok(child) => {
                let mut processes = VIDEO_PROCESSES.lock().unwrap();
                processes.insert(camera_id, child);
                
                println!("Started video recording for camera {} to: {}", camera_id, filepath.display());
                Ok(filepath.to_string_lossy().to_string())
            }
            Err(e) => Err(format!("Failed to start video recording: {}", e))
        }
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// 尝试使用简化的录像命令
fn try_simple_recording(ffmpeg_path: &str, camera_id: u32, filepath: &PathBuf, duration_seconds: Option<u32>) -> Result<Child, String> {
    let duration = duration_seconds.unwrap_or(5);
    let mut command = Command::new(ffmpeg_path);
    
    if cfg!(target_os = "windows") {
        let camera_info = validate_camera_id(camera_id)?;
        let device_name = camera_info.human_name();
        
        command
            .arg("-f").arg("dshow")
            .arg("-i").arg(format!("video={}", device_name))
            .arg("-c:v").arg("libx264")
            .arg("-preset").arg("ultrafast")
            .arg("-crf").arg("25")
            .arg("-pix_fmt").arg("yuv420p")
            .arg("-t").arg(&duration.to_string())
            .arg("-y")
            .arg(filepath);
    } else {
        command
            .arg("-f").arg("v4l2")
            .arg("-i").arg(format!("/dev/video{}", camera_id))
            .arg("-c:v").arg("libx264")
            .arg("-preset").arg("ultrafast")
            .arg("-crf").arg("25")
            .arg("-pix_fmt").arg("yuv420p")
            .arg("-t").arg(&duration.to_string())
            .arg("-y")
            .arg(filepath);
    }

    println!("Trying simple recording command: {:?}", command);
    
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            let cmd_str = format!("{:?}", command);
            format!("Failed to start ffmpeg process: {}. Command: {}", e, cmd_str)
        })
}

/// 停止录像
pub async fn stop_video_recording() -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let mut processes = VIDEO_PROCESSES.lock().unwrap();
        
        for (camera_id, child) in processes.iter_mut() {
            println!("Stopping video recording for camera {}", camera_id);
            
            if let Err(e) = child.kill() {
                eprintln!("Failed to kill ffmpeg process for camera {}: {}", camera_id, e);
            }
            
            let timeout = std::time::Duration::from_secs(3);
            let start = std::time::Instant::now();
            
            while start.elapsed() < timeout {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        println!("FFmpeg process for camera {} exited with status: {:?}", camera_id, status);
                        break;
                    }
                    Ok(None) => {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(e) => {
                        eprintln!("Error waiting for ffmpeg process for camera {}: {}", camera_id, e);
                        break;
                    }
                }
            }
            
            if child.try_wait().unwrap_or(None).is_none() {
                println!("Force killing ffmpeg process for camera {}", camera_id);
                if let Err(e) = child.kill() {
                    eprintln!("Failed to force kill ffmpeg process for camera {}: {}", camera_id, e);
                }
            }
        }
        
        processes.clear();
        println!("All video recordings stopped");
        Ok(())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}
