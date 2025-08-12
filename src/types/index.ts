// 类型定义文件

export interface CameraInfo {
  id: number;
  name: string;
}

export interface AppConfig {
  shortcut_key: string;
  save_path: string;
  show_debug_logs: boolean;
  save_logs_to_file: boolean;
  dark_mode: boolean;
  exit_on_lock: boolean;
  enable_screen_lock: boolean;
  enable_notifications: boolean;
  post_trigger_action: 'CaptureAndLock' | 'CaptureOnly';
}

export interface LogEntry {
  timestamp: string;
  level: string;
  message: string;
  target: string;
}

export type MonitoringStatus = '空闲' | '准备中' | '警戒中';

export type PermissionStatus = '未检查' | '已授权' | '被拒绝';

export type LogLevel = 'error' | 'warn' | 'info' | 'debug';

// 拖拽相关接口
export interface DragState {
  isDragging: boolean;
  startY: number;
  startHeight: number;
}

// 事件处理器类型
export type MouseEventHandler = (e: MouseEvent) => void;
export type KeyboardEventHandler = (e: KeyboardEvent) => void;

// 组件props类型（如果需要的话）
export interface StatusIndicatorProps {
  status: MonitoringStatus;
  className?: string;
}

export interface ButtonProps {
  disabled?: boolean;
  loading?: boolean;
  variant?: 'primary' | 'secondary' | 'danger';
  size?: 'small' | 'medium' | 'large';
}