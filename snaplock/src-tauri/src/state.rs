use std::sync::Mutex;

/// Represents the monitoring status of the application.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MonitoringState {
    /// The application is idle and not monitoring for input.
    Idle,
    /// The application is preparing to start monitoring, with a short delay.
    Preparing,
    /// The application is actively monitoring for input.
    Active,
}

/// Holds the shared state of the Tauri application.
pub struct AppState {
    /// The current monitoring status, protected by a Mutex.
    pub status: Mutex<MonitoringState>,
    /// The index of the camera to be used for capturing photos.
    pub camera_index: Mutex<usize>,
}
