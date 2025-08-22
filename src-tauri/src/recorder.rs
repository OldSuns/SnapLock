use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};
use chrono::Local;

// 使用 lazy_static 确保FFMPEG进程的Arc<Mutex<Option<Child>>>只被创建一次
lazy_static::lazy_static! {
    pub static ref FFMPEG_PROCESS: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
}

/// 启动屏幕录制
pub fn start_screen_recording(app_handle: &AppHandle) -> Result<(), String> {
    let mut process_guard = FFMPEG_PROCESS.lock().unwrap();

    // 如果已经有一个ffmpeg进程在运行，则先停止它
    if let Some(mut child) = process_guard.take() {
        log::warn!("检测到已存在的ffmpeg进程，将先停止它");
        if let Err(e) = child.kill() {
            log::error!("无法停止旧的ffmpeg进程: {}", e);
        }
        // 等待进程完全终止
        match child.wait() {
            Ok(status) => log::info!("旧的ffmpeg进程已停止，退出状态: {}", status),
            Err(e) => log::error!("等待旧的ffmpeg进程终止时出错: {}", e),
        }
    }

    let state = app_handle.state::<crate::state::AppState>();
    let save_path = state.get_effective_save_path();
    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let output_filename = format!("{}.mkv", timestamp);
    let output_path = std::path::Path::new(&save_path).join(output_filename);

    log::info!("准备启动屏幕录制，保存至: {:?}", output_path);

    // 获取ffmpeg可执行文件的路径
    let ffmpeg_path = match app_handle.path().resolve("libs/ffmpeg/bin/ffmpeg.exe", tauri::path::BaseDirectory::Resource) {
        Ok(path) => path,
        Err(e) => {
            let err_msg = format!("无法解析ffmpeg路径: {}", e);
            log::error!("{}", err_msg);
            return Err(err_msg);
        }
    };

    log::info!("ffmpeg路径: {:?}", ffmpeg_path);

    // 构建ffmpeg命令
    let mut command = Command::new(ffmpeg_path);
    command.args([
        "-f", "gdigrab",
        "-framerate", "30",
        "-i", "desktop",
        "-c:v", "libx264",
        "-preset", "ultrafast",
        "-vf", "scale=iw/2:-2",
        "-b:v", "2000k",
        "-maxrate", "3000k",
        "-bufsize", "2000k",
        output_path.to_str().unwrap(),
    ]);

    // 在生产环境中隐藏ffmpeg窗口
    #[cfg(not(debug_assertions))]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    // 启动ffmpeg进程
    match command
        .spawn()
    {
        Ok(child) => {
            log::info!("ffmpeg进程已成功启动，PID: {}", child.id());
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

        // 在Windows上，向ffmpeg发送'q'来优雅地停止
        // 这需要能够写入进程的stdin，但我们以null stdin启动了它
        // 因此，我们只能直接kill掉它
        match child.kill() {
            Ok(_) => {
                log::info!("已发送终止信号到ffmpeg进程");
                // 等待进程退出并清理资源
                match child.wait() {
                    Ok(status) => log::info!("ffmpeg进程已成功终止，退出状态: {}", status),
                    Err(e) => log::error!("等待ffmpeg进程终止时出错: {}", e),
                }
            }
            Err(e) => {
                log::error!("无法终止ffmpeg进程: {}", e);
            }
        }
    } else {
        log::info!("没有正在运行的ffmpeg录制进程");
    }
}