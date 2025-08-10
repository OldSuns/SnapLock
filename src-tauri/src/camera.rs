use chrono::Local;
use image::{ImageBuffer, RgbImage};
use nokhwa::{
    Camera,
    pixel_format::RgbFormat,
    query,
    utils::{ApiBackend, CameraIndex, CameraInfo},
};
use std::path::PathBuf;
use tauri::command;
use serde::Serialize;

/// Camera information for frontend
#[derive(Serialize)]
pub struct CameraListItem {
    pub id: u32,
    pub name: String,
}

/// A Tauri command that retrieves a list of available cameras with their actual indices.
///
/// Returns a `Vec<CameraListItem>` containing camera ID and name, or an error string if the query fails.
#[command]
pub async fn get_camera_list() -> Result<Vec<CameraListItem>, String> {
    match query(ApiBackend::Auto) {
        Ok(cameras) => {
            let camera_list: Vec<CameraListItem> = cameras
                .into_iter()
                .enumerate()
                .map(|(array_index, info)| {
                    // 尝试从CameraInfo中获取实际的索引
                    let actual_id = match info.index() {
                        CameraIndex::Index(id) => *id,
                        CameraIndex::String(s) => {
                            // 如果是字符串索引，尝试解析为数字，否则使用数组索引
                            s.parse::<u32>().unwrap_or(array_index as u32)
                        }
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
            Ok(camera_list)
        }
        Err(e) => {
            eprintln!("Camera enumeration failed: {}", e);
            Err(format!("Failed to get camera list: {}", e))
        }
    }
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
            CameraIndex::String(s) => {
                s.parse::<u32>().unwrap_or(u32::MAX) // 无法解析的字符串索引设为MAX，不会匹配
            }
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
        Self {
            camera: Some(camera),
        }
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

/// Captures a photo using the specified camera and saves it to a configurable path.
///
/// This function is asynchronous and offloads blocking I/O operations to a separate thread,
/// preventing the UI from freezing.
///
/// # Arguments
/// * `camera_id` - The ID of the camera to use.
/// * `save_path` - An optional custom path to save the image. If `None`, it defaults to the desktop.
///
/// # Returns
/// A `Result` containing the path to the saved image file as a string, or an error string.
pub async fn take_photo(camera_id: u32, save_path: Option<String>) -> Result<String, String> {
    println!("Starting async photo capture with camera ID: {}", camera_id);

    // The entire blocking operation is spawned onto a blocking thread.
    tokio::task::spawn_blocking(move || {
        use nokhwa::utils::{RequestedFormat, RequestedFormatType};

        // Validate camera ID before proceeding
        let camera_info = validate_camera_id(camera_id)?;
        println!("Using camera: {} (ID: {})", camera_info.human_name(), camera_id);

        let index = CameraIndex::Index(camera_id);
        let requested =
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution);

        // Initialize camera
        let camera = Camera::new(index, requested)
            .map_err(|e| format!("Failed to initialize camera ID {}: {}", camera_id, e))?;
        
        let mut camera_guard = CameraGuard::new(camera);

        // Open stream, capture, and decode
        let (width, height, raw_buffer) = {
            let cam = camera_guard.get_mut()
                .ok_or("Camera guard failed to provide camera reference")?;
            
            cam.open_stream()
                .map_err(|e| format!("Failed to open stream for camera ID {}: {}", camera_id, e))?;
            
            let frame = cam.frame()
                .map_err(|e| format!("Failed to capture frame from camera ID {}: {}", camera_id, e))?;
            
            let decoded_buffer = frame.decode_image::<RgbFormat>()
                .map_err(|e| format!("Failed to decode image from camera ID {}: {}", camera_id, e))?;
            
            (decoded_buffer.width(), decoded_buffer.height(), decoded_buffer.into_raw())
        };
        
        let rgb_image: RgbImage = ImageBuffer::from_raw(width, height, raw_buffer)
            .ok_or("Failed to create image buffer from raw data")?;

        // Determine save path
        let base_path = match save_path {
            Some(path) => PathBuf::from(path),
            None => dirs::desktop_dir()
                .ok_or_else(|| "Desktop directory not found".to_string())?,
        };

        // Ensure the directory exists
        if !base_path.exists() {
            std::fs::create_dir_all(&base_path)
                .map_err(|e| format!("Failed to create save directory '{}': {}", base_path.display(), e))?;
        }

        // Generate filename and save
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("snaplock_capture_{}.jpg", timestamp);
        let filepath = base_path.join(&filename);

        println!("Saving image to: {}", filepath.display());
        rgb_image.save(&filepath)
            .map_err(|e| format!("Failed to save image to '{}': {}", filepath.display(), e))?;

        // The CameraGuard will automatically handle stopping the stream on drop.
        
        Ok(filepath.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))? // Handle task join error
}
use crate::state::AppState;

/// Sets the custom save path for photos.
///
/// # Arguments
/// * `path` - The new save path.
/// * `state` - The application state.
#[command]
pub fn set_save_path(path: String, state: tauri::State<AppState>) -> Result<(), String> {
    state.set_save_path(Some(path));
    Ok(())
}
