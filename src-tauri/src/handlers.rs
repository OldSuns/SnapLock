use crate::{camera, monitoring::MonitoringController, state::AppState};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub fn set_camera_id(app_handle: AppHandle, camera_id: u32) -> Result<(), String> {
    camera::ensure_camera_available(camera_id)?;
    app_handle.state::<AppState>().set_camera_id(camera_id);
    Ok(())
}

#[tauri::command]
pub async fn start_monitoring_command(app_handle: AppHandle, camera_id: u32) -> Result<(), String> {
    app_handle
        .state::<MonitoringController>()
        .start(&app_handle, camera_id)
        .await
}

#[tauri::command]
pub async fn stop_monitoring_command(app_handle: AppHandle) -> Result<(), String> {
    app_handle
        .state::<MonitoringController>()
        .stop(&app_handle)
        .await
}

#[tauri::command]
pub fn set_shortcuts_disabled(app_handle: AppHandle, disabled: bool) {
    app_handle
        .state::<AppState>()
        .set_shortcuts_disabled(disabled);
}
