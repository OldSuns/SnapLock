use crate::recorder::{RecordingSource, Recordings, recording_command};
use base64::{Engine as _, engine::general_purpose};
use image::{ImageBuffer, RgbImage};
use nokhwa::{
    Camera,
    pixel_format::RgbFormat,
    query,
    utils::{ApiBackend, CameraIndex, CameraInfo, RequestedFormat, RequestedFormatType},
};
use serde::Serialize;
use std::io::Cursor;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, command};

/// Camera information for frontend
#[derive(Serialize)]
pub struct CameraListItem {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CameraSelection {
    pub persisted_default_camera_id: Option<u32>,
    pub runtime_camera_id: Option<u32>,
}

fn camera_numeric_id(index: &CameraIndex, fallback_id: u32) -> u32 {
    match index {
        CameraIndex::Index(id) => *id,
        CameraIndex::String(value) => value.parse::<u32>().unwrap_or(fallback_id),
    }
}

fn enumerate_camera_info() -> Result<Vec<(u32, CameraInfo)>, String> {
    query(ApiBackend::Auto)
        .map_err(|error| format!("Failed to query cameras: {}", error))
        .map(|cameras| {
            cameras
                .into_iter()
                .enumerate()
                .map(|(array_index, info)| {
                    let resolved_id = camera_numeric_id(info.index(), array_index as u32);
                    (resolved_id, info)
                })
                .collect()
        })
}

fn resolve_camera_info(camera_id: u32) -> Result<CameraInfo, String> {
    let cameras = enumerate_camera_info()?;

    if cameras.is_empty() {
        return Err("No cameras available on the system".to_string());
    }

    for (resolved_id, info) in cameras.iter() {
        if *resolved_id == camera_id {
            return Ok(info.clone());
        }
    }

    let available_ids: Vec<u32> = cameras.into_iter().map(|(id, _)| id).collect();
    Err(format!(
        "Camera ID {} not found. Available camera IDs: {:?}",
        camera_id, available_ids
    ))
}

/// A Tauri command that retrieves a list of available cameras with their actual indices.
#[command]
pub async fn get_camera_list() -> Result<Vec<CameraListItem>, String> {
    enumerate_camera_info()
        .map(|cameras| {
            let camera_list: Vec<CameraListItem> = cameras
                .iter()
                .map(|(camera_id, info)| CameraListItem {
                    id: *camera_id,
                    name: info.human_name().to_string(),
                })
                .collect();

            println!(
                "Found {} cameras with IDs: {:?}",
                camera_list.len(),
                camera_list.iter().map(|c| c.id).collect::<Vec<_>>()
            );
            camera_list
        })
        .map_err(|error| {
            eprintln!("Camera enumeration failed: {}", error);
            format!("Failed to get camera list: {}", error)
        })
}

/// Validates if the given camera ID is available and returns the corresponding CameraInfo
fn validate_camera_id(camera_id: u32) -> Result<CameraInfo, String> {
    resolve_camera_info(camera_id)
}

/// Public validation helper for callers that only need availability checks.
pub fn ensure_camera_available(camera_id: u32) -> Result<(), String> {
    validate_camera_id(camera_id).map(|_| ())
}

pub fn resolve_camera_selection(preferred: Option<u32>) -> Result<CameraSelection, String> {
    let cameras = enumerate_camera_info()?;
    if cameras.is_empty() {
        return Ok(CameraSelection::default());
    }

    if let Some(preferred_id) = preferred {
        if cameras
            .iter()
            .any(|(resolved_id, _)| *resolved_id == preferred_id)
        {
            return Ok(CameraSelection {
                persisted_default_camera_id: Some(preferred_id),
                runtime_camera_id: Some(preferred_id),
            });
        }

        log::warn!(
            "默认摄像头 {} 当前不可用，回退到第一个可用摄像头",
            preferred_id
        );
    }

    Ok(CameraSelection {
        persisted_default_camera_id: None,
        runtime_camera_id: Some(cameras[0].0),
    })
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
                eprintln!(
                    "Warning: Failed to stop camera stream during cleanup: {}",
                    e
                );
            }
        }
    }
}

/// 通用的相机初始化函数
fn init_camera(camera_id: u32) -> Result<Camera, String> {
    let camera_info = validate_camera_id(camera_id)?;

    let requested =
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution);

    Camera::new(camera_info.index().clone(), requested)
        .map_err(|e| format!("Failed to initialize camera ID {}: {}", camera_id, e))
}

/// 通用的图像捕获函数
fn capture_frame_from_open_stream(
    camera: &mut Camera,
    camera_id: u32,
) -> Result<(u32, u32, Vec<u8>), String> {
    let frame = camera.frame().map_err(|e| {
        format!(
            "Failed to capture frame from camera ID {}: {}",
            camera_id, e
        )
    })?;

    let decoded_buffer = frame
        .decode_image::<RgbFormat>()
        .map_err(|e| format!("Failed to decode image from camera ID {}: {}", camera_id, e))?;

    Ok((
        decoded_buffer.width(),
        decoded_buffer.height(),
        decoded_buffer.into_raw(),
    ))
}

/// 通用的图像捕获函数
fn capture_frame(camera: &mut Camera, camera_id: u32) -> Result<(u32, u32, Vec<u8>), String> {
    camera
        .open_stream()
        .map_err(|e| format!("Failed to open stream for camera ID {}: {}", camera_id, e))?;

    capture_frame_from_open_stream(camera, camera_id)
}

/// 通用的保存路径处理函数
fn get_save_path(save_path: Option<String>) -> Result<PathBuf, String> {
    let base_path = match save_path {
        Some(path) => PathBuf::from(path),
        None => dirs::desktop_dir().ok_or_else(|| "Desktop directory not found".to_string())?,
    };

    if !base_path.exists() {
        std::fs::create_dir_all(&base_path).map_err(|e| {
            format!(
                "Failed to create save directory '{}': {}",
                base_path.display(),
                e
            )
        })?;
    }

    Ok(base_path)
}

/// Captures a photo using the specified camera and saves it to a configurable path.
pub async fn take_photo(camera_id: u32, save_path: Option<String>) -> Result<String, String> {
    println!("Starting async photo capture with camera ID: {}", camera_id);

    tokio::task::spawn_blocking(move || {
        let camera_info = validate_camera_id(camera_id)?;
        println!(
            "Using camera: {} (ID: {})",
            camera_info.human_name(),
            camera_id
        );

        let camera = init_camera(camera_id)?;
        let mut camera_guard = CameraGuard::new(camera);

        let (width, height, raw_buffer) = {
            let cam = camera_guard
                .get_mut()
                .ok_or("Camera guard failed to provide camera reference")?;
            capture_frame(cam, camera_id)?
        };

        let rgb_image: RgbImage = ImageBuffer::from_raw(width, height, raw_buffer)
            .ok_or("Failed to create image buffer from raw data")?;

        let base_path = get_save_path(save_path)?;
        let (filepath, mut file) = crate::media_files::create_photo(&base_path)
            .map_err(|error| format!("Failed to create photo: {error}"))?;

        println!("Saving image to: {}", filepath.display());
        rgb_image
            .write_to(&mut file, image::ImageFormat::Jpeg)
            .map_err(|e| format!("Failed to save image to '{}': {}", filepath.display(), e))?;

        Ok(filepath.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// 检查相机权限
#[command]
pub async fn check_camera_permission(camera_id: u32) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || match init_camera(camera_id) {
        Ok(mut camera) => match camera.open_stream() {
            Ok(_) => {
                let _ = camera.stop_stream();
                Ok(true)
            }
            Err(e) => {
                log::warn!("相机权限检查失败: {}", e);
                Ok(false)
            }
        },
        Err(e) => {
            log::warn!("相机初始化失败: {}", e);
            Ok(false)
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
            let cam = camera_guard
                .get_mut()
                .ok_or("Camera guard failed to provide camera reference")?;

            cam.open_stream()
                .map_err(|e| format!("Failed to open stream for camera ID {}: {}", camera_id, e))?;

            // 等待几帧以获得稳定的图像
            for _ in 0..3 {
                let _ = cam.frame();
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            capture_frame_from_open_stream(cam, camera_id)?
        };

        let rgb_image: RgbImage = ImageBuffer::from_raw(width, height, raw_buffer)
            .ok_or("Failed to create image buffer from raw preview data")?;

        // 调整图像大小以减少数据传输
        let preview_image =
            image::imageops::resize(&rgb_image, 320, 240, image::imageops::FilterType::Lanczos3);

        // 转换为JPEG格式
        let mut jpeg_buffer = Vec::new();
        {
            let mut cursor = Cursor::new(&mut jpeg_buffer);
            preview_image
                .write_to(&mut cursor, image::ImageFormat::Jpeg)
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
pub async fn start_video_recording(
    app_handle: AppHandle,
    camera_id: u32,
    save_path: Option<String>,
    duration_seconds: Option<u32>,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let camera_info = validate_camera_id(camera_id)?;
        let base_path = get_save_path(save_path)?;
        let (mut command, filepath) =
            recording_command(&app_handle, &base_path, "snaplock_video_")?;
        if cfg!(target_os = "windows") {
            command
                .args(["-f", "dshow", "-i"])
                .arg(format!("video={}", camera_info.human_name()));
        } else {
            command
                .args(["-f", "v4l2", "-i"])
                .arg(format!("/dev/video{camera_id}"));
        }
        command
            .args([
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-crf",
                "25",
                "-pix_fmt",
                "yuv420p",
                "-t",
            ])
            .arg(duration_seconds.unwrap_or(5).to_string())
            .arg(&filepath);
        app_handle
            .state::<Recordings>()
            .start(RecordingSource::Camera(camera_id), &mut command)?;
        Ok(filepath.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// 停止录像
pub async fn stop_video_recording(app_handle: AppHandle) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        app_handle
            .state::<Recordings>()
            .stop_matching(|source| matches!(source, RecordingSource::Camera(_)));
        Ok(())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}
