use crate::process_utils::{RecordingProcess, spawn_recording};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordingSource {
    Screen,
    Camera(u32),
}

/// Owns all recording children. Native input callbacks only read the active flag.
#[derive(Default)]
pub struct Recordings {
    processes: Mutex<HashMap<RecordingSource, RecordingProcess>>,
    screen_active: AtomicBool,
    screen_failure: Mutex<Option<Instant>>,
}

impl Recordings {
    pub fn screen_active(&self) -> bool {
        self.screen_active.load(Ordering::SeqCst)
    }

    pub fn screen_retry_remaining(&self) -> Option<Duration> {
        self.screen_failure
            .lock()
            .unwrap()
            .and_then(|failure| Duration::from_secs(5).checked_sub(failure.elapsed()))
    }

    pub fn start(&self, source: RecordingSource, command: &mut Command) -> Result<(), String> {
        let mut processes = self.processes.lock().unwrap();
        if let Some(process) = processes.get_mut(&source) {
            if process
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_none()
            {
                return if source == RecordingSource::Screen {
                    Ok(())
                } else {
                    Err(format!("{source:?} is already recording"))
                };
            }
            processes.remove(&source);
        }
        let mut process = spawn_recording(command)?;
        process.assign_to_job()?;
        log::info!("Recording started: {source:?}, PID: {}", process.id());
        processes.insert(source, process);
        if source == RecordingSource::Screen {
            self.screen_active.store(true, Ordering::SeqCst);
            *self.screen_failure.lock().unwrap() = None;
        }
        Ok(())
    }

    pub fn check_screen(&self) -> Result<bool, String> {
        let mut processes = self.processes.lock().unwrap();
        let Some(process) = processes.get_mut(&RecordingSource::Screen) else {
            return Ok(false);
        };
        let error = match process.try_wait() {
            Ok(None) => return Ok(true),
            Ok(Some(status)) => format!(
                "屏幕录像进程意外退出 ({status}): {}",
                process.error_details()
            ),
            Err(error) => format!("无法检查屏幕录像进程: {error}"),
        };
        processes.remove(&RecordingSource::Screen);
        self.screen_active.store(false, Ordering::SeqCst);
        *self.screen_failure.lock().unwrap() = Some(Instant::now());
        Err(error)
    }

    pub fn stop_matching(&self, select: impl Fn(RecordingSource) -> bool) {
        let processes = {
            let mut guard = self.processes.lock().unwrap();
            let sources: Vec<_> = guard
                .keys()
                .copied()
                .filter(|source| select(*source))
                .collect();
            sources
                .into_iter()
                .filter_map(|source| {
                    if source == RecordingSource::Screen {
                        self.screen_active.store(false, Ordering::SeqCst);
                    }
                    guard.remove(&source)
                })
                .collect::<Vec<_>>()
        };
        // Release shared state before waiting for FFmpeg to finalize its files.
        for mut process in processes {
            process.stop("ffmpeg");
        }
    }
}

pub fn recording_command(
    app: &AppHandle,
    directory: &Path,
    prefix: &str,
) -> Result<(Command, PathBuf), String> {
    std::fs::create_dir_all(directory).map_err(|error| format!("无法创建录像目录: {error}"))?;
    let tool = app
        .path()
        .resolve(
            "libs/ffmpeg/bin/ffmpeg.exe",
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|error| format!("无法解析ffmpeg路径: {error}"))?;
    let output = crate::media_files::capture_path(directory, prefix, "mkv");
    Ok((Command::new(ffmpeg_program(tool)), output))
}

/// Prefers the bundled FFmpeg. A standalone executable has no resources beside it,
/// so a bare program name is resolved from the executable's directory and `PATH`.
fn ffmpeg_program(bundled: PathBuf) -> PathBuf {
    if bundled.is_file() {
        bundled
    } else {
        PathBuf::from("ffmpeg")
    }
}

pub async fn start_screen_recording(app: AppHandle) -> Result<(), String> {
    let result = tokio::task::spawn_blocking(move || {
        let recordings = app.state::<Recordings>();
        if let Some(remaining) = recordings.screen_retry_remaining() {
            return Err(format!(
                "屏幕录制启动冷却中，请在 {} ms 后重试",
                remaining.as_millis()
            ));
        }
        let directory = app
            .state::<crate::state::AppState>()
            .config()
            .effective_save_path();
        let result = (|| {
            let (mut command, output) =
                recording_command(&app, Path::new(&directory), "snaplock_screen_")?;
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
                .arg(output);
            recordings.start(RecordingSource::Screen, &mut command)
        })();
        if result.is_err() {
            *recordings.screen_failure.lock().unwrap() = Some(Instant::now());
        }
        result
    })
    .await
    .map_err(|error| format!("Screen recording task failed: {error}"))?;
    result.map_err(|error| format!("启动ffmpeg失败: {error}"))
}

pub async fn stop_screen_recording(app: AppHandle) {
    if let Err(error) = tokio::task::spawn_blocking(move || {
        app.state::<Recordings>()
            .stop_matching(|source| source == RecordingSource::Screen);
    })
    .await
    {
        log::error!("Screen recording cleanup failed: {error}");
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn missing_bundled_ffmpeg_is_found_through_path() {
        let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("libs/ffmpeg/bin");
        let bundled = bin.join("ffmpeg.exe");
        assert_eq!(ffmpeg_program(bundled.clone()), bundled);

        let output = Command::new(ffmpeg_program(bin.join("missing/ffmpeg.exe")))
            .env("PATH", &bin)
            .arg("-version")
            .output()
            .expect("FFmpeg must be found through PATH");
        assert!(output.status.success());
    }

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
        let recordings = Recordings::default();
        recordings
            .start(RecordingSource::Screen, &mut command)
            .unwrap();
        assert!(recordings.screen_active());
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match recordings.check_screen() {
                Err(error) => {
                    assert!(error.contains("意外退出"), "{error}");
                    assert!(!recordings.screen_active());
                    assert!(recordings.processes.lock().unwrap().is_empty());
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
