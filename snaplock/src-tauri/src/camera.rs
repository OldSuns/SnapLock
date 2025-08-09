use chrono::Local;
use image::{ImageBuffer, RgbImage};
use nokhwa::{
    Camera,
    pixel_format::RgbFormat,
    query,
    utils::{ApiBackend, CameraIndex},
};
use std::path::PathBuf;
use tauri::command;

/// A Tauri command that retrieves a list of available cameras.
///
/// Returns a `Vec<String>` of camera names or an error string if the query fails.
#[command]
pub async fn get_camera_list() -> Result<Vec<String>, String> {
    match query(ApiBackend::Auto) {
        Ok(cameras) => {
            let names = cameras
                .into_iter()
                .map(|info| info.human_name().to_string())
                .collect();
            Ok(names)
        }
        Err(e) => Err(format!("Failed to get camera list: {}", e)),
    }
}

/// Captures a photo using the specified camera and saves it to the desktop.
///
/// # Arguments
/// * `camera_index` - The index of the camera to use.
///
/// # Returns
/// A `Result` containing the path to the saved image file as a string, or an error string.
pub async fn take_photo_internal(camera_index: usize) -> Result<String, String> {
    use nokhwa::utils::{RequestedFormat, RequestedFormatType};

    let index = CameraIndex::Index(camera_index as u32);
    let requested =
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution);
    let mut camera =
        Camera::new(index, requested).map_err(|e| format!("Failed to initialize camera: {}", e))?;
    camera
        .open_stream()
        .map_err(|e| format!("Failed to open stream: {}", e))?;

    let frame = camera
        .frame()
        .map_err(|e| format!("Failed to capture frame: {}", e))?;
    let buffer = frame
        .decode_image::<RgbFormat>()
        .map_err(|e| format!("Failed to decode image: {}", e))?;
    let (width, height) = (buffer.width(), buffer.height());
    let rgb_image: RgbImage = ImageBuffer::from_raw(width, height, buffer.into_raw())
        .ok_or_else(|| "Failed to create image buffer".to_string())?;

    let desktop_path: PathBuf =
        dirs::desktop_dir().ok_or_else(|| "Failed to get desktop path".to_string())?;
    let filename = format!(
        "snaplock_capture_{}.jpg",
        Local::now().format("%Y%m%d_%H%M%S")
    );
    let filepath = desktop_path.join(filename);

    rgb_image
        .save(&filepath)
        .map_err(|e| format!("Failed to save image: {}", e))?;

    camera
        .stop_stream()
        .map_err(|e| format!("Failed to stop stream: {}", e))?;

    Ok(filepath.to_string_lossy().to_string())
}
