use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};
use chrono::Local;

// Windows specific imports for Job Objects
#[cfg(windows)]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
    JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
#[cfg(windows)]
use windows::Win32::System::Threading::GetCurrentProcess;
#[cfg(windows)]
use windows::Win32::Foundation::{HANDLE, CloseHandle};
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
#[cfg(windows)]
use std::ffi::c_void;

// A wrapper to make HANDLE Send + Sync
#[cfg(windows)]
struct SafeHandle(HANDLE);
#[cfg(windows)]
unsafe impl Send for SafeHandle {}
#[cfg(windows)]
unsafe impl Sync for SafeHandle {}

// Global state for the ffmpeg process and the Job Object
lazy_static::lazy_static! {
    pub static ref FFMPEG_PROCESS: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
}
#[cfg(windows)]
lazy_static::lazy_static! {
    static ref JOB_HANDLE: Arc<Mutex<Option<SafeHandle>>> = Arc::new(Mutex::new(None));
}

#[cfg(windows)]
/// Ensures the Job Object is created and the main process is assigned to it.
fn ensure_job_object() -> Result<HANDLE, String> {
    let mut job_guard = JOB_HANDLE.lock().unwrap();
    if let Some(handle) = &*job_guard {
        return Ok(handle.0);
    }

    unsafe {
        let job_handle = CreateJobObjectW(None, None)
            .map_err(|e| format!("Failed to create Job Object: {}", e))?;

        let mut limit_info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limit_info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        
        let info_size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
        if SetInformationJobObject(
            job_handle,
            JobObjectExtendedLimitInformation,
            &limit_info as *const _ as *const c_void,
            info_size,
        ).is_err() {
            let err = windows::core::Error::from_win32();
            let _ = CloseHandle(job_handle);
            return Err(format!("Failed to set Job Object information: {}", err));
        }

        if AssignProcessToJobObject(job_handle, GetCurrentProcess()).is_err() {
            let err = windows::core::Error::from_win32();
            let _ = CloseHandle(job_handle);
            return Err(format!("Failed to assign main process to Job Object: {}", err));
        }

        *job_guard = Some(SafeHandle(job_handle));
        Ok(job_handle)
    }
}


/// 启动屏幕录制并拍照
pub async fn start_screen_recording(app_handle: AppHandle) -> Result<(), String> {
    // Take photo first
    let state = app_handle.state::<crate::state::AppState>();
    let camera_id = state.camera_id();
    let save_path = state.save_path();
    log::info!("开始拍照 (恢复录制)...");
    if let Err(e) = crate::camera::take_photo(camera_id, save_path).await {
        log::error!("拍照失败: {}", e);
        // We don't return here, as recording might still be possible
    } else {
        log::info!("拍照完成");
    }

    // Now, start the recording process
    let mut process_guard = FFMPEG_PROCESS.lock().unwrap();

    if process_guard.is_some() {
        log::warn!("录制进程已在运行，跳过启动请求");
        return Ok(());
    }

    let effective_save_path = state.get_effective_save_path();
    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let output_filename = format!("{}.mkv", timestamp);
    let output_path = std::path::Path::new(&effective_save_path).join(output_filename);

    log::info!("准备启动屏幕录制，保存至: {:?}", output_path);

    let ffmpeg_path = match app_handle.path().resolve("libs/ffmpeg/bin/ffmpeg.exe", tauri::path::BaseDirectory::Resource) {
        Ok(path) => path,
        Err(e) => return Err(format!("无法解析ffmpeg路径: {}", e)),
    };

    let mut command = Command::new(ffmpeg_path);
    command.args([
        "-f", "gdigrab", "-framerate", "30", "-i", "desktop",
        "-c:v", "libx264", "-preset", "ultrafast", "-vf", "scale=iw/2:-2",
        "-b:v", "2000k", "-maxrate", "3000k", "-bufsize", "2000k",
        output_path.to_str().unwrap(),
    ]);

    #[cfg(not(debug_assertions))]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    match command.spawn() {
        Ok(mut child) => {
            log::info!("ffmpeg进程已成功启动，PID: {}", child.id());

            #[cfg(windows)]
            {
                match ensure_job_object() {
                    Ok(job_handle) => {
                        let child_handle = HANDLE(child.as_raw_handle() as *mut c_void);
                        unsafe {
                            if AssignProcessToJobObject(job_handle, child_handle).is_err() {
                                let err = windows::core::Error::from_win32();
                                log::error!("无法将ffmpeg进程分配给Job Object: {}", err);
                                let _ = child.kill();
                            } else {
                                log::info!("ffmpeg进程已成功分配给Job Object");
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("无法确保Job Object存在: {}", e);
                        let _ = child.kill();
                    }
                }
            }

            *process_guard = Some(child);
            Ok(())
        }
        Err(e) => {
            let err_msg = format!("启动ffmpeg失败: {}", e);
            log::error!("{}", err_msg);
            Err(err_msg)
        }
    }
}

/// 停止屏幕录制
pub fn stop_screen_recording() {
    let mut process_guard = FFMPEG_PROCESS.lock().unwrap();
    if let Some(mut child) = process_guard.take() {
        log::info!("正在停止ffmpeg录制进程 (PID: {})...", child.id());
        match child.kill() {
            Ok(_) => {
                log::info!("已发送终止信号到ffmpeg进程");
                let _ = child.wait();
            }
            Err(e) => {
                log::error!("无法终止ffmpeg进程: {}", e);
            }
        }
    } else {
        log::info!("没有正在运行的ffmpeg录制进程");
    }
}