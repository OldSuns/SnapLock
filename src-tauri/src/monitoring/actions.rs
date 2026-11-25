use std::process::Command;
use std::time::Duration;

use crate::{camera, monitoring::MonitoringController, state::AppState};
use tauri::{AppHandle, Manager};

fn is_action_still_current(app_handle: &AppHandle, action_generation: u64) -> bool {
    app_handle
        .state::<MonitoringController>()
        .is_action_current(action_generation)
}

pub fn lock_screen() {
    log::info!("执行锁屏命令...");
    match Command::new("rundll32.exe")
        .args(["user32.dll,LockWorkStation"])
        .spawn()
    {
        Ok(mut child) => {
            log::info!("锁屏命令已启动，进程ID: {:?}", child.id());
            match child.wait() {
                Ok(status) if status.success() => log::info!("锁屏命令执行成功"),
                Ok(status) => log::error!("锁屏命令执行失败，退出码: {:?}", status.code()),
                Err(error) => log::error!("等待锁屏命令完成时发生错误: {}", error),
            }
        }
        Err(error) => {
            log::error!("启动锁屏命令失败: {}", error);
        }
    }
}

pub(crate) async fn trigger_lockdown(app_handle: AppHandle, action_generation: u64) {
    log::info!("=== 开始执行锁定流程 ===");

    if !is_action_still_current(&app_handle, action_generation) {
        log::info!("锁定流程已失效，取消执行");
        return;
    }

    let state = app_handle.state::<AppState>();
    let camera_id = state.camera_id();
    let config = state.config();
    let save_path = config.save_path;
    let exit_on_lock_enabled = config.exit_on_lock;
    let post_trigger_action = config.post_trigger_action;
    let notifications_enabled = config.enable_notifications;
    let capture_delay_seconds = config.capture_delay_seconds;

    let screen_lock_enabled = matches!(
        post_trigger_action,
        crate::config::PostTriggerAction::CaptureAndLock
    );

    log::info!(
        "监控触发，使用摄像头ID: {}, 触发后动作: {:?}, 通知功能: {}, 锁定时退出: {}, 拍摄延迟: {}秒",
        camera_id,
        post_trigger_action,
        notifications_enabled,
        exit_on_lock_enabled,
        capture_delay_seconds
    );

    if capture_delay_seconds > 0 {
        if !await_delayed_capture(
            app_handle.clone(),
            camera_id,
            save_path.clone(),
            capture_delay_seconds,
            action_generation,
        )
        .await
        {
            return;
        }
    } else if !capture_photo(
        app_handle.clone(),
        camera_id,
        save_path.clone(),
        action_generation,
    )
    .await
    {
        return;
    }

    {
        let controller = app_handle.state::<MonitoringController>();
        let Some(_guard) = controller.lock_action(action_generation).await else {
            return;
        };
        if notifications_enabled {
            send_security_notification(&app_handle);
        }
        if screen_lock_enabled {
            if let Err(error) = tokio::task::spawn_blocking(lock_screen).await {
                log::error!("锁屏任务失败: {}", error);
            }
        }
    }
    let controller = app_handle.state::<MonitoringController>();
    if screen_lock_enabled
        && !controller
            .wait_action_delay(action_generation, Duration::from_millis(1_000))
            .await
    {
        return;
    }
    if exit_on_lock_enabled {
        controller
            .exit_after_trigger(&app_handle, action_generation)
            .await;
    } else if post_trigger_action == crate::config::PostTriggerAction::CaptureOnly {
        controller
            .complete_trigger(&app_handle, action_generation)
            .await;
    }

    log::info!("=== 锁定流程执行完成 ===");
}

fn send_security_notification(app_handle: &AppHandle) {
    use tauri_plugin_notification::NotificationExt;

    let state = app_handle.state::<AppState>();
    if !state.config().enable_notifications {
        return;
    }

    match app_handle
        .notification()
        .builder()
        .title("SnapLock 安全警报")
        .body("检测到未授权访问")
        .icon("📷")
        .show()
    {
        Ok(_) => log::info!("安全通知发送成功"),
        Err(error) => log::error!("发送安全通知失败: {}", error),
    }
}

async fn await_delayed_capture(
    app_handle: AppHandle,
    camera_id: u32,
    save_path: Option<String>,
    delay_seconds: u32,
    action_generation: u64,
) -> bool {
    log::info!("开始延迟拍摄，延迟: {}秒", delay_seconds);

    {
        let controller = app_handle.state::<MonitoringController>();
        let Some(_guard) = controller.lock_action(action_generation).await else {
            return false;
        };
        if let Err(error) = camera::start_video_recording(
            app_handle.clone(),
            camera_id,
            save_path,
            Some(delay_seconds),
        )
        .await
        {
            log::error!("启动录像失败: {}", error);
            controller.report_capture_error(&app_handle, &error);
            return true;
        }
    }

    let controller = app_handle.state::<MonitoringController>();
    if !controller
        .wait_action_delay(
            action_generation,
            Duration::from_secs((delay_seconds + 2).into()),
        )
        .await
    {
        return false;
    }

    {
        let controller = app_handle.state::<MonitoringController>();
        let Some(_guard) = controller.lock_action(action_generation).await else {
            log::info!("延迟拍摄已取消，跳过旧流程的进程清理");
            return false;
        };
        if let Err(error) = camera::stop_video_recording(app_handle.clone()).await {
            log::error!("清理录像进程失败: {}", error);
        }
    }

    controller
        .wait_action_delay(action_generation, Duration::from_millis(2_000))
        .await
}

async fn capture_photo(
    app_handle: AppHandle,
    camera_id: u32,
    save_path: Option<String>,
    action_generation: u64,
) -> bool {
    let controller = app_handle.state::<MonitoringController>();
    let Some(_guard) = controller.lock_action(action_generation).await else {
        return false;
    };

    if let Err(error) = camera::take_photo(camera_id, save_path).await {
        log::error!("拍照失败: {}", error);
        controller.report_capture_error(&app_handle, &error);
    }

    is_action_still_current(&app_handle, action_generation)
}
