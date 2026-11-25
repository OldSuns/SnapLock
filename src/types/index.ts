// 类型定义文件

export interface CameraInfo {
  id: number;
  name: string;
}

export interface AppConfig {
  shortcut_key: string;
  save_path: string | null;
  show_debug_logs: boolean;
  save_logs_to_file: boolean;
  dark_mode: boolean;
  exit_on_lock: boolean;
  enable_notifications: boolean;
  post_trigger_action: 'CaptureAndLock' | 'CaptureOnly' | 'ScreenRecording';
  default_camera_id: number | null;
  capture_delay_seconds: number;
  capture_mode: 'Video';
  capture_on_failed_unlock: boolean;
}

export interface LogEntry {
  timestamp: string;
  level: string;
  message: string;
  target: string;
}

export type MonitoringStatus = '空闲' | '准备中' | '警戒中' | '锁定中';

export type PermissionStatus = '未检查' | '已授权' | '被拒绝';
