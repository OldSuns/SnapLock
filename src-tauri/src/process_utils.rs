use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const FFMPEG_STOP_TIMEOUT: Duration = Duration::from_secs(5);

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

/// Keeps FFmpeg's control input available without leaving unread output pipes.
pub fn configure_recording_command(command: &mut Command) {
    configure_background_command(command);
    command.stdin(Stdio::piped()).stdout(Stdio::null());
    #[cfg(all(windows, not(debug_assertions)))]
    command.stderr(Stdio::null());
    #[cfg(not(all(windows, not(debug_assertions))))]
    command.stderr(Stdio::inherit());
}

/// Lets FFmpeg write its container trailer before falling back to forced termination.
pub fn terminate_child_process(child: &mut Child, process_name: &str) {
    stop_recording_with_timeout(child, process_name, FFMPEG_STOP_TIMEOUT);
}

fn stop_recording_with_timeout(child: &mut Child, process_name: &str, timeout: Duration) {
    match child.try_wait() {
        Ok(Some(_)) => return,
        Ok(None) => {}
        Err(error) => {
            log::error!("Failed to check {process_name} before stopping: {error}");
            kill_child_process(child, process_name);
            return;
        }
    }

    let quit_request = child.stdin.take().map(|mut stdin| stdin.write_all(b"q\n"));
    match quit_request {
        Some(Ok(())) => {
            let deadline = Instant::now() + timeout;
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if status.success() {
                            log::info!("{process_name} finalized its recording and exited");
                        } else {
                            log::error!("{process_name} exited with recording error: {status}");
                        }
                        return;
                    }
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Ok(None) => {
                        log::warn!(
                            "{process_name} did not stop within {timeout:?}; forcing termination"
                        );
                        break;
                    }
                    Err(error) => {
                        log::error!("Failed to wait for {process_name}: {error}");
                        break;
                    }
                }
            }
        }
        Some(Err(error)) => log::warn!("Failed to request {process_name} to stop: {error}"),
        None => log::warn!("{process_name} has no control input; forcing termination"),
    }
    kill_child_process(child, process_name);
}

fn kill_child_process(child: &mut Child, process_name: &str) {
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn ffmpeg_tool(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("libs/ffmpeg/bin")
            .join(format!("{name}.exe"))
    }

    fn output_directory() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = PathBuf::from(r"D:\Temp\Pi").join(format!(
            "snaplock-ffmpeg-regression-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("D:\\Temp\\Pi must exist for recording tests");
        directory
    }

    fn wait_for_frames(child: &mut Child, progress: &PathBuf) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let recorded_frames = fs::read_to_string(progress)
                .unwrap_or_default()
                .lines()
                .filter_map(|line| line.strip_prefix("frame="))
                .filter_map(|frame| frame.trim().parse::<u32>().ok())
                .max()
                .unwrap_or_default();
            if recorded_frames >= 5 {
                return;
            }
            if child.try_wait().unwrap().is_some() || Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("FFmpeg did not record five frames; progress: {progress:?}");
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    #[test]
    fn stopping_recordings_finalizes_the_container_before_the_process_exits() {
        let directory = output_directory();
        for duration in [None, Some("60")] {
            let mode = if duration.is_some() {
                "camera"
            } else {
                "screen"
            };
            let recording = directory.join(format!("{mode}.mkv"));
            let progress = directory.join(format!("{mode}-progress.txt"));
            let mut command = Command::new(ffmpeg_tool("ffmpeg"));
            command
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-re",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=320x240:rate=30",
                    "-c:v",
                    "libx264",
                    "-preset",
                    "ultrafast",
                    "-pix_fmt",
                    "yuv420p",
                    "-stats_period",
                    "0.1",
                    "-progress",
                ])
                .arg(&progress);
            if let Some(duration) = duration {
                command.args(["-t", duration]);
            }
            command.arg(&recording);
            configure_recording_command(&mut command);
            command.stderr(Stdio::null());
            let mut child = command.spawn().expect("Bundled FFmpeg must be available");
            wait_for_frames(&mut child, &progress);
            terminate_child_process(&mut child, "test ffmpeg");
            assert!(
                child.try_wait().unwrap().is_some(),
                "Recording process is still running"
            );

            let probe = Command::new(ffmpeg_tool("ffprobe"))
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "format=duration",
                    "-of",
                    "json",
                ])
                .arg(&recording)
                .output()
                .unwrap();
            assert!(
                probe.status.success(),
                "{}",
                String::from_utf8_lossy(&probe.stderr)
            );
            let metadata: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
            let recorded_duration = metadata["format"]["duration"]
                .as_str()
                .and_then(|value| value.parse::<f64>().ok())
                .unwrap_or_default();
            assert!(
                recorded_duration > 0.0 && recorded_duration < 10.0,
                "{mode} recording has incorrect duration: {metadata}; file: {recording:?}"
            );

            let decode = Command::new(ffmpeg_tool("ffmpeg"))
                .args(["-v", "error", "-i"])
                .arg(&recording)
                .args(["-f", "null", "-"])
                .output()
                .unwrap();
            assert!(
                decode.status.success() && decode.stderr.is_empty(),
                "{mode} recording is incomplete: {}; file: {recording:?}",
                String::from_utf8_lossy(&decode.stderr)
            );
        }
    }

    #[test]
    fn unresponsive_recordings_are_terminated_after_the_deadline() {
        let directory = output_directory();
        let progress = directory.join("unresponsive-progress.txt");
        let mut command = Command::new(ffmpeg_tool("ffmpeg"));
        command
            .args([
                "-nostdin",
                "-re",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=30",
                "-stats_period",
                "0.1",
                "-progress",
            ])
            .arg(&progress)
            .args(["-f", "null", "-"]);
        configure_recording_command(&mut command);
        command.stderr(Stdio::null());
        let mut child = command.spawn().unwrap();
        wait_for_frames(&mut child, &progress);

        let timeout = Duration::from_millis(200);
        let started = Instant::now();
        stop_recording_with_timeout(&mut child, "unresponsive ffmpeg", timeout);
        assert!(started.elapsed() >= timeout && started.elapsed() < Duration::from_secs(2));
        assert!(!child.try_wait().unwrap().unwrap().success());
    }

    #[test]
    fn stopping_an_exited_recording_is_idempotent() {
        let mut command = Command::new(ffmpeg_tool("ffmpeg"));
        command.arg("-version");
        configure_recording_command(&mut command);
        command.stderr(Stdio::null());
        let mut child = command.spawn().unwrap();
        assert!(child.wait().unwrap().success());
        terminate_child_process(&mut child, "finished ffmpeg");
        terminate_child_process(&mut child, "finished ffmpeg");
        assert!(child.try_wait().unwrap().unwrap().success());
    }
}
