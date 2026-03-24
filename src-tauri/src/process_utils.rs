use std::process::{Child, Command};

#[cfg(all(windows, not(debug_assertions)))]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
#[cfg(windows)]
use std::sync::Mutex;
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
#[cfg(windows)]
use windows::Win32::System::Threading::GetCurrentProcess;

#[cfg(all(windows, not(debug_assertions)))]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
struct SafeHandle(HANDLE);
#[cfg(windows)]
unsafe impl Send for SafeHandle {}
#[cfg(windows)]
unsafe impl Sync for SafeHandle {}

#[cfg(windows)]
lazy_static::lazy_static! {
    static ref JOB_HANDLE: Mutex<Option<SafeHandle>> = Mutex::new(None);
}

/// Applies release-only Windows background process settings so helper tools
/// like ffmpeg do not spawn a visible console window in packaged builds.
#[cfg_attr(not(all(windows, not(debug_assertions))), allow(unused_variables))]
pub fn configure_background_command(command: &mut Command) {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

pub fn terminate_child_process(child: &mut Child, process_name: &str) {
    match child.kill() {
        Ok(_) => {
            log::info!("已发送终止信号到{}进程", process_name);
        }
        Err(error) => {
            log::error!("无法终止{}进程: {}", process_name, error);
        }
    }

    if let Err(error) = child.wait() {
        log::error!("等待{}进程退出失败: {}", process_name, error);
    }
}

#[cfg(windows)]
fn ensure_job_object() -> Result<HANDLE, String> {
    let mut job_guard = JOB_HANDLE.lock().unwrap();
    if let Some(handle) = &*job_guard {
        return Ok(handle.0);
    }

    unsafe {
        let job_handle = CreateJobObjectW(None, None)
            .map_err(|error| format!("Failed to create Job Object: {}", error))?;

        let mut limit_info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limit_info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        let info_size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
        if SetInformationJobObject(
            job_handle,
            JobObjectExtendedLimitInformation,
            &limit_info as *const _ as *const c_void,
            info_size,
        )
        .is_err()
        {
            let error = windows::core::Error::from_win32();
            let _ = CloseHandle(job_handle);
            return Err(format!("Failed to set Job Object information: {}", error));
        }

        if AssignProcessToJobObject(job_handle, GetCurrentProcess()).is_err() {
            let error = windows::core::Error::from_win32();
            let _ = CloseHandle(job_handle);
            return Err(format!(
                "Failed to assign main process to Job Object: {}",
                error
            ));
        }

        *job_guard = Some(SafeHandle(job_handle));
        Ok(job_handle)
    }
}

#[cfg_attr(not(windows), allow(unused_variables))]
pub fn assign_child_to_kill_on_close_job(child: &mut Child) -> Result<(), String> {
    #[cfg(windows)]
    {
        let job_handle = ensure_job_object()?;
        let child_handle = HANDLE(child.as_raw_handle() as *mut c_void);

        unsafe {
            if AssignProcessToJobObject(job_handle, child_handle).is_err() {
                let error = windows::core::Error::from_win32();
                return Err(format!("无法将进程分配给 Job Object: {}", error));
            }
        }
    }

    Ok(())
}
