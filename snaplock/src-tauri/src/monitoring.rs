use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crate::camera;
use rdev::{Event, listen};
use tokio::runtime::Runtime;

/// Triggers the Windows screen lock by calling `rundll32.exe user32.dll,LockWorkStation`.
pub fn lock_screen() {
    if let Err(_e) = Command::new("rundll32.exe")
        .args(["user32.dll,LockWorkStation"])
        .spawn()
    {
        // Error handling can be improved, e.g., by writing to a log file.
    }
}

/// Starts a global input monitor in a separate thread.
///
/// This function listens for any keyboard or mouse events. When an event is detected
/// and the `monitoring_flag` is true, it triggers the photo capture and screen lock.
///
/// # Arguments
/// * `camera_index` - The index of the camera to use for taking a photo.
/// * `monitoring_flag` - An atomic boolean that controls whether the monitoring is active.
pub fn start_monitoring(camera_index: usize, monitoring_flag: Arc<AtomicBool>) {
    let rt = Runtime::new().unwrap();
    thread::spawn(move || {
        if let Err(_error) = listen(move |event| {
            if !monitoring_flag.load(Ordering::SeqCst) {
                // rdev 的 listen 是阻塞的，我们无法直接中断它。
                // 因此，我们在 callback 内部检查标志。
                return;
            }
            callback(event, camera_index, &rt, &monitoring_flag);
        }) {
            // eprintln!("Error listening to events: {:?}", error);
        }
    });
}

fn callback(_event: Event, camera_index: usize, rt: &Runtime, monitoring_flag: &Arc<AtomicBool>) {
    if !monitoring_flag.load(Ordering::SeqCst) {
        return;
    }

    rt.block_on(async {
        match camera::take_photo_internal(camera_index).await {
            Ok(_path) => {} // 照片已保存，此处无需操作
            Err(_e) => {}   // 拍照失败，此处可添加日志
        }
    });
    lock_screen();
    // 延时一小会确保锁屏命令已发出
    std::thread::sleep(std::time::Duration::from_millis(500));
    std::process::exit(0);
}
