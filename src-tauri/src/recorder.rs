use chrono::Local;
use std::process::{Child, Command};
#[cfg(all(windows, not(debug_assertions)))]
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

lazy_static::lazy_static! {
    pub static ref FFMPEG_PROCESS: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
}
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

fn refresh_screen_recording_state(process_guard: &mut Option<Child>) -> bool {
    let mut should_clear = false;
    let is_running = match process_guard.as_mut() {
        Some(child) => match child.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                log::warn!("ffmpeg 录制进程已退出，状态: {:?}", status);
                should_clear = true;
                false
            }
            Err(error) => {
                log::error!("检查 ffmpeg 录制进程状态失败: {}", error);
                should_clear = true;
                false
            }
        },
        None => false,
    };

    if should_clear {
        *process_guard = None;
    }

    is_running
}

pub fn is_screen_recording_running() -> bool {
    let mut process_guard = FFMPEG_PROCESS.lock().unwrap();
    refresh_screen_recording_state(&mut process_guard)
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

    let mut process_guard = FFMPEG_PROCESS.lock().unwrap();
    if refresh_screen_recording_state(&mut process_guard) {
        log::warn!("录制进程已在运行，跳过启动请求");
        return Ok(());
    }

    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let output_filename = format!("{}.mkv", timestamp);
    let output_path = std::path::Path::new(&effective_save_path).join(output_filename);
    let output_path_str = output_path
        .to_str()
        .ok_or_else(|| format!("输出路径包含无效 Unicode: {:?}", output_path))?;

    log::info!("准备启动屏幕录制，保存至: {:?}", output_path);

    let ffmpeg_path = match app_handle.path().resolve(
        "libs/ffmpeg/bin/ffmpeg.exe",
        tauri::path::BaseDirectory::Resource,
    ) {
        Ok(path) => path,
        Err(e) => return Err(format!("无法解析ffmpeg路径: {}", e)),
    };

    let mut command = Command::new(ffmpeg_path);
    command.args([
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
        output_path_str,
    ]);
    crate::process_utils::configure_background_command(&mut command);
    #[cfg(all(windows, not(debug_assertions)))]
    {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    }

    match command.spawn() {
        Ok(mut child) => {
            log::info!("ffmpeg进程已成功启动，PID: {}", child.id());
            if let Err(error) = crate::process_utils::assign_child_to_kill_on_close_job(&mut child)
            {
                log::error!("无法将 ffmpeg 进程纳入 Job Object: {}", error);
                crate::process_utils::terminate_child_process(&mut child, "ffmpeg");
                mark_screen_recording_failure();
                return Err(error);
            }

            *process_guard = Some(child);
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
pub fn stop_screen_recording() {
    let mut process_guard = FFMPEG_PROCESS.lock().unwrap();
    if !refresh_screen_recording_state(&mut process_guard) {
        log::info!("没有正在运行的ffmpeg录制进程");
        return;
    }

    if let Some(mut child) = process_guard.take() {
        log::info!("正在停止ffmpeg录制进程 (PID: {})...", child.id());
        crate::process_utils::terminate_child_process(&mut child, "ffmpeg");
    } else {
        log::info!("没有正在运行的ffmpeg录制进程");
    }
}
