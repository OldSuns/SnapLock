use crate::process_utils::RecordingProcess;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

lazy_static::lazy_static! {
    pub static ref FFMPEG_PROCESS: Arc<Mutex<Option<RecordingProcess>>> = Arc::new(Mutex::new(None));
}
static SCREEN_RECORDING_ACTIVE: AtomicBool = AtomicBool::new(false);
static SCREEN_RECORDING_STARTING: AtomicBool = AtomicBool::new(false);
static LAST_SCREEN_RECORDING_FAILURE_MS: AtomicU64 = AtomicU64::new(0);
const SCREEN_RECORDING_RETRY_COOLDOWN_MS: u64 = 5_000;

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn mark_screen_recording_failure() {
    LAST_SCREEN_RECORDING_FAILURE_MS.store(now_millis(), Ordering::SeqCst);
}

fn clear_screen_recording_failure() {
    LAST_SCREEN_RECORDING_FAILURE_MS.store(0, Ordering::SeqCst);
}

pub fn screen_recording_retry_remaining_ms() -> Option<u64> {
    let last_failure = LAST_SCREEN_RECORDING_FAILURE_MS.load(Ordering::SeqCst);
    if last_failure == 0 {
        return None;
    }

    let elapsed = now_millis().saturating_sub(last_failure);
    if elapsed >= SCREEN_RECORDING_RETRY_COOLDOWN_MS {
        None
    } else {
        Some(SCREEN_RECORDING_RETRY_COOLDOWN_MS - elapsed)
    }
}

fn refresh_screen_recording_state(
    process_guard: &mut Option<RecordingProcess>,
) -> Result<bool, String> {
    let Some(process) = process_guard.as_mut() else {
        return Ok(false);
    };
    let error = match process.try_wait() {
        Ok(None) => return Ok(true),
        Ok(Some(status)) => format!(
            "屏幕录像进程意外退出 ({status}): {}",
            process.error_details()
        ),
        Err(error) => {
            process.stop("ffmpeg");
            format!("无法检查屏幕录像进程: {error}")
        }
    };
    *process_guard = None;
    SCREEN_RECORDING_ACTIVE.store(false, Ordering::SeqCst);
    mark_screen_recording_failure();
    Err(error)
}

/// Input hooks only read an atomic flag; process checks run outside the native hook.
pub fn is_screen_recording_running() -> bool {
    SCREEN_RECORDING_ACTIVE.load(Ordering::SeqCst)
}

pub fn check_screen_recording() -> Result<bool, String> {
    refresh_screen_recording_state(&mut FFMPEG_PROCESS.lock().unwrap())
}

/// 启动屏幕录制并拍照
pub async fn start_screen_recording(app_handle: AppHandle) -> Result<(), String> {
    start_screen_recording_with_options(app_handle, true).await
}

pub async fn start_screen_recording_with_options(
    app_handle: AppHandle,
    capture_photo: bool,
) -> Result<(), String> {
    if SCREEN_RECORDING_STARTING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log::info!("屏幕录制启动已在进行中，跳过重复请求");
        return Ok(());
    }

    struct StartGuard;
    impl Drop for StartGuard {
        fn drop(&mut self) {
            SCREEN_RECORDING_STARTING.store(false, Ordering::SeqCst);
        }
    }
    let _start_guard = StartGuard;

    if let Some(remaining_ms) = screen_recording_retry_remaining_ms() {
        return Err(format!(
            "屏幕录制启动冷却中，请在 {} ms 后重试",
            remaining_ms
        ));
    }

    if is_screen_recording_running() {
        log::warn!("录制进程已在运行，跳过启动请求");
        return Ok(());
    }

    let (camera_id, save_path, effective_save_path) = {
        let state = app_handle.state::<crate::state::AppState>();
        (
            state.camera_id(),
            state.save_path(),
            state.get_effective_save_path(),
        )
    };

    if capture_photo {
        log::info!("开始拍照后启动屏幕录制...");
        if let Err(error) = crate::camera::take_photo(camera_id, save_path).await {
            log::error!("拍照失败: {}", error);
        } else {
            log::info!("拍照完成");
        }
    }

    let ffmpeg_path = match app_handle.path().resolve(
        "libs/ffmpeg/bin/ffmpeg.exe",
        tauri::path::BaseDirectory::Resource,
    ) {
        Ok(path) => path,
        Err(e) => return Err(format!("无法解析ffmpeg路径: {}", e)),
    };

    let result = tokio::task::spawn_blocking(move || -> Result<RecordingProcess, String> {
        let directory = std::path::Path::new(&effective_save_path);
        std::fs::create_dir_all(directory).map_err(|error| format!("无法创建录像目录: {error}"))?;
        let output_path = crate::media_files::capture_path(directory, "snaplock_screen_", "mkv");
        log::info!("准备启动屏幕录制，保存至: {:?}", output_path);
        let mut command = Command::new(ffmpeg_path);
        command
            .args([
                "-f",
                "gdigrab",
                "-framerate",
                "30",
                "-i",
                "desktop",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-vf",
                "scale=iw/2:-2",
                "-b:v",
                "2000k",
                "-maxrate",
                "3000k",
                "-bufsize",
                "2000k",
            ])
            .arg(&output_path);
        let mut process = crate::process_utils::spawn_recording(&mut command)?;
        if let Err(error) = process.assign_to_job() {
            process.stop("ffmpeg");
            return Err(error);
        }
        Ok(process)
    })
    .await
    .map_err(|error| format!("Screen recording startup task failed: {error}"))?;
    match result {
        Ok(mut child) => {
            let mut process_guard = FFMPEG_PROCESS.lock().unwrap();
            match refresh_screen_recording_state(&mut process_guard) {
                Ok(false) => {}
                other => {
                    child.stop("duplicate ffmpeg");
                    return other.map(|_| ());
                }
            }
            log::info!("屏幕录像已开始，PID: {}", child.id());
            *process_guard = Some(child);
            SCREEN_RECORDING_ACTIVE.store(true, Ordering::SeqCst);
            clear_screen_recording_failure();
            Ok(())
        }
        Err(error) => {
            let err_msg = format!("启动ffmpeg失败: {}", error);
            log::error!("{}", err_msg);
            mark_screen_recording_failure();
            Err(err_msg)
        }
    }
}

/// 停止屏幕录制
pub async fn stop_screen_recording() {
    if let Err(error) = tokio::task::spawn_blocking(|| {
        let child = {
            let mut process_guard = FFMPEG_PROCESS.lock().unwrap();
            SCREEN_RECORDING_ACTIVE.store(false, Ordering::SeqCst);
            process_guard.take()
        };
        // Input callbacks must not wait on the process mutex while FFmpeg finalizes.
        if let Some(mut child) = child {
            log::info!("正在停止ffmpeg录制进程 (PID: {})...", child.id());
            child.stop("ffmpeg");
        }
    })
    .await
    {
        log::error!("Failed to join screen recording cleanup: {error}");
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::{Duration, Instant};

    #[test]
    fn unexpected_recording_exit_is_reported_and_clears_active_state() {
        let directory = crate::media_files::capture_path(
            Path::new(r"D:\Temp\Pi"),
            "snaplock-runtime-exit-",
            "test",
        );
        std::fs::create_dir(&directory).unwrap();
        let tool = Path::new(env!("CARGO_MANIFEST_DIR")).join("libs/ffmpeg/bin/ffmpeg.exe");
        let mut command = Command::new(tool);
        command
            .args([
                "-re",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=30",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-t",
                "1",
            ])
            .arg(directory.join("short.mkv"));
        let process = crate::process_utils::spawn_recording(&mut command).unwrap();
        let mut process_guard = Some(process);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match refresh_screen_recording_state(&mut process_guard) {
                Err(error) => {
                    assert!(error.contains("意外退出"), "{error}");
                    assert!(process_guard.is_none());
                    assert!(!is_screen_recording_running());
                    break;
                }
                Ok(true) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25))
                }
                other => panic!("Unexpected recording state: {other:?}"),
            }
        }
    }
}
