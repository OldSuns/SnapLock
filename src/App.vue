<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed, watch, nextTick } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from '@tauri-apps/plugin-dialog';
import { desktopDir } from '@tauri-apps/api/path';

// 导入类型定义
import type { CameraInfo, AppConfig, LogEntry, MonitoringStatus, PermissionStatus } from './types';

// 导入工具函数
import {
  getStatusIcon,
  getStatusText,
  getLogLevelClass,
  getPermissionStatusClass,
  validateShortcut,
  scrollToBottom,
  scrollToTop
} from './utils/helpers';


// ===== 状态定义 =====
const cameraList = ref<CameraInfo[]>([]);
const selectedCameraId = ref<number>(0);
const monitoringStatus = ref<MonitoringStatus | '锁定中'>("空闲");
const savePath = ref<string>("");
const showSettings = ref<boolean>(false);
const currentShortcut = ref<string>("Alt+L");
const tempShortcut = ref<string>("Alt+L");
const tempSavePath = ref<string>("");
const isCapturingShortcut = ref<boolean>(false);

// 相机相关状态
const cameraPermissionStatus = ref<PermissionStatus>("未检查");
const cameraPreviewUrl = ref<string>("");
const showCameraPreview = ref<boolean>(false);
const isCheckingPermission = ref<boolean>(false);

// 暗色模式状态
const isDarkMode = ref<boolean>(false);
const tempIsDarkMode = ref<boolean>(false);

// 锁定时退出状态
const exitOnLock = ref<boolean>(false);
const tempExitOnLock = ref<boolean>(false);


// 触发后动作状态
const postTriggerAction = ref<'CaptureAndLock' | 'CaptureOnly' | 'ScreenRecording'>('CaptureAndLock');
const tempPostTriggerAction = ref<'CaptureAndLock' | 'CaptureOnly' | 'ScreenRecording'>('CaptureAndLock');

// 通知开关状态
const enableNotifications = ref<boolean>(true);
const tempEnableNotifications = ref<boolean>(true);

// 默认摄像头状态
const defaultCameraId = ref<number | null>(null);
const tempDefaultCameraId = ref<number | null>(null);

// 拍摄延时设置状态
const captureDelaySeconds = ref<number>(0);
const tempCaptureDelaySeconds = ref<number>(0);
const captureMode = ref<'Video'>('Video');
const tempCaptureMode = ref<'Video'>('Video');

// 日志相关状态
const showDebugLogs = ref<boolean>(false);
const saveLogsToFile = ref<boolean>(false);
const tempShowDebugLogs = ref<boolean>(false);
const tempSaveLogsToFile = ref<boolean>(false);
const logEntries = ref<LogEntry[]>([]);
const logPanelExpanded = ref<boolean>(false);
const eventUnlisteners: UnlistenFn[] = [];
let cameraPreviewRequestToken = 0;

type PreviewLoadResult = 'loaded' | 'failed' | 'stale';

// ===== 计算属性 =====
const statusClass = computed(() => {
  switch (monitoringStatus.value) {
    case "警戒中":
    case "锁定中":
      return "status-active";
    case "准备中":
      return "status-pending";
    default:
      return "status-idle";
  }
});

// ===== 监听器 =====
watch(selectedCameraId, async (newId, oldId) => {
  if (cameraList.value.length > 0) {
    try {
      await invoke("set_camera_id", { cameraId: newId });
    } catch (error) {
      console.error("Failed to set camera ID:", error);
    }
  }

  if (newId !== oldId) {
    cameraPreviewRequestToken += 1;
    cameraPermissionStatus.value = "未检查";
    cameraPreviewUrl.value = "";

    if (showCameraPreview.value) {
      const hasPermission = await checkCameraPermission();
      if (!hasPermission) {
        showCameraPreview.value = false;
        return;
      }

      const previewResult = await updateCameraPreview();
      if (previewResult === 'failed') {
        showCameraPreview.value = false;
      }
    }
  }
});

// ===== 主要功能函数 =====

// 检查相机权限
async function checkCameraPermission() {
  if (selectedCameraId.value === null || selectedCameraId.value === undefined) {
    cameraPermissionStatus.value = "未检查";
    return false;
  }
  
  const cameraId = selectedCameraId.value;
  isCheckingPermission.value = true;
  try {
    const hasPermission = await invoke<boolean>("check_camera_permission", {
      cameraId
    });

    if (cameraId !== selectedCameraId.value) {
      return false;
    }

    cameraPermissionStatus.value = hasPermission ? "已授权" : "被拒绝";
    return hasPermission;
  } catch (error) {
    console.error("检查相机权限失败:", error);
    if (cameraId === selectedCameraId.value) {
      cameraPermissionStatus.value = "被拒绝";
    }
    return false;
  } finally {
    if (cameraId === selectedCameraId.value) {
      isCheckingPermission.value = false;
    }
  }
}

// 更新相机预览
async function updateCameraPreview(): Promise<PreviewLoadResult> {
  if (selectedCameraId.value === null || selectedCameraId.value === undefined) {
    cameraPreviewUrl.value = "";
    return 'failed';
  }
  
  const cameraId = selectedCameraId.value;
  const requestToken = ++cameraPreviewRequestToken;

  try {
    const previewData = await invoke<string>("get_camera_preview", {
      cameraId
    });

    if (requestToken !== cameraPreviewRequestToken || cameraId !== selectedCameraId.value) {
      return 'stale';
    }

    cameraPreviewUrl.value = previewData;
    return 'loaded';
  } catch (error) {
    console.error("获取相机预览失败:", error);
    if (requestToken === cameraPreviewRequestToken && cameraId === selectedCameraId.value) {
      cameraPreviewUrl.value = "";
      return 'failed';
    }
    return 'stale';
  }
}

// 切换相机预览显示
async function toggleCameraPreview() {
  if (showCameraPreview.value) {
    cameraPreviewRequestToken += 1;
    showCameraPreview.value = false;
    cameraPreviewUrl.value = "";
    return;
  }

  const hasPermission = await checkCameraPermission();
  if (!hasPermission) {
    showCameraPreview.value = false;
    cameraPreviewUrl.value = "";
    return;
  }

  const previewResult = await updateCameraPreview();
  if (previewResult === 'failed') {
    showCameraPreview.value = false;
    cameraPreviewUrl.value = "";
    return;
  }

  if (previewResult === 'stale') {
    return;
  }

  showCameraPreview.value = true;
}

async function refreshCameraPreview() {
  const previewResult = await updateCameraPreview();
  if (previewResult === 'failed') {
    showCameraPreview.value = false;
    cameraPreviewUrl.value = "";
  }
}

function normalizeCaptureDelayValue(value: number): number {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    return captureDelaySeconds.value;
  }

  return Math.min(60, Math.max(0, Math.trunc(parsed)));
}

// 加载应用配置的统一函数
async function loadAppConfig(): Promise<boolean> {
  try {
    const config = await invoke<AppConfig>("load_config");
    isDarkMode.value = config.dark_mode;
    exitOnLock.value = config.exit_on_lock;
    enableNotifications.value = config.enable_notifications ?? true; // 默认启用
    postTriggerAction.value = config.post_trigger_action ?? 'CaptureAndLock'; // 默认拍摄并锁屏
    defaultCameraId.value = config.default_camera_id ?? null;
    captureDelaySeconds.value = config.capture_delay_seconds ?? 0; // 默认0秒
    captureMode.value = config.capture_mode ?? 'Video'; // 默认录像模式
    tempIsDarkMode.value = isDarkMode.value;
    tempExitOnLock.value = exitOnLock.value;
    tempEnableNotifications.value = enableNotifications.value;
    tempPostTriggerAction.value = postTriggerAction.value;
    tempDefaultCameraId.value = defaultCameraId.value;
    tempCaptureDelaySeconds.value = captureDelaySeconds.value;
    tempCaptureMode.value = captureMode.value;
    
    // 设置保存路径
    const targetPath = config.save_path ?? await desktopDir();
    savePath.value = targetPath;
    tempSavePath.value = targetPath;
    
    applyTheme();
    return true;
  } catch (error) {
    console.error("Failed to load config:", error);
    
    // 配置加载失败时使用默认设置
    const desktop = await desktopDir();
    savePath.value = desktop;
    tempSavePath.value = desktop;
    
    applyTheme();
    return false;
  }
}

// 启动/停止监控
async function toggleMonitoring() {
  try {
    if (monitoringStatus.value === "空闲") {
      await invoke("start_monitoring_command", { cameraId: selectedCameraId.value });
    } else {
      await invoke("stop_monitoring_command");
    }
  } catch (error) {
    console.error("Failed to toggle monitoring:", error);
    alert(`监控操作失败: ${error}`);
  }
}

// ===== 设置相关函数 =====

function openSettings() {
  // 统一复制当前设置到临时变量
  tempShortcut.value = currentShortcut.value;
  tempSavePath.value = savePath.value;
  tempShowDebugLogs.value = showDebugLogs.value;
  tempSaveLogsToFile.value = saveLogsToFile.value;
  tempIsDarkMode.value = isDarkMode.value;
  tempExitOnLock.value = exitOnLock.value;
  tempEnableNotifications.value = enableNotifications.value;
  tempPostTriggerAction.value = postTriggerAction.value;
  tempDefaultCameraId.value = defaultCameraId.value;
  tempCaptureDelaySeconds.value = captureDelaySeconds.value;
  tempCaptureMode.value = captureMode.value;
  
  showSettings.value = true;
  
  // 添加ESC键监听
  document.addEventListener('keydown', handleEscapeKey);
}

async function closeSettings() {
  // 如果正在捕获快捷键，先取消并重新启用快捷键
  if (isCapturingShortcut.value) {
    await cancelCaptureShortcut();
  }
  showSettings.value = false;
  
  // 移除ESC键监听
  document.removeEventListener('keydown', handleEscapeKey);
}

// 处理ESC键按下事件
function handleEscapeKey(event: KeyboardEvent) {
  if (event.key === 'Escape' && showSettings.value) {
    event.preventDefault();
    closeSettings();
  }
}

async function selectSavePathInSettings() {
  const selected = await open({
    directory: true,
    multiple: false,
    defaultPath: tempSavePath.value,
    title: "选择保存位置"
  });

  if (typeof selected === 'string' && selected !== null) {
    tempSavePath.value = selected;
    // 立即保存路径设置
    await savePathSetting();
  }
}

async function saveShortcut() {
  try {
    if (tempShortcut.value !== currentShortcut.value && validateShortcut(tempShortcut.value)) {
      await invoke("set_shortcut_key", { shortcut: tempShortcut.value });
      currentShortcut.value = tempShortcut.value;
      console.log("快捷键已更新为:", tempShortcut.value);
    }
  } catch (error) {
    console.error("Failed to save shortcut:", error);
    alert(`快捷键保存失败: ${error}`);
    // 恢复到之前的值
    tempShortcut.value = currentShortcut.value;
  }
}

async function savePathSetting() {
  try {
    if (tempSavePath.value !== savePath.value) {
      const oldPath = savePath.value;
      await invoke("set_save_path", { path: tempSavePath.value });
      savePath.value = tempSavePath.value;
      
      // 更新日志文件路径
      if (saveLogsToFile.value) {
        await invoke("set_log_file_path", { path: tempSavePath.value });
      }
      
      // 记录路径更改日志到后端
      await invoke("log_save_path_change", {
        oldPath: oldPath,
        newPath: tempSavePath.value
      });
      
      console.log(`保存路径已更新: ${oldPath} -> ${tempSavePath.value}`);
    }
  } catch (error) {
    console.error("Failed to save path:", error);
    alert(`保存路径设置失败: ${error}`);
    // 恢复到之前的值
    tempSavePath.value = savePath.value;
  }
}

// ===== 快捷键相关函数 =====

async function startCaptureShortcut() {
  isCapturingShortcut.value = true;
  tempShortcut.value = "按下快捷键...";
  
  // 禁用全局快捷键
  try {
    await invoke("disable_shortcuts");
  } catch (error) {
    console.error("Failed to disable shortcuts:", error);
  }
  
  // 确保输入框获得焦点
  nextTick(() => {
    const input = document.querySelector('.shortcut-input') as HTMLInputElement;
    if (input) {
      input.focus();
    }
  });
}

async function handleShortcutKeyDown(event: KeyboardEvent) {
  if (!isCapturingShortcut.value) return;
  
  event.preventDefault();
  event.stopPropagation();
  
  const keys: string[] = [];
  
  // 添加修饰键
  if (event.ctrlKey) keys.push('Ctrl');
  if (event.altKey) keys.push('Alt');
  if (event.shiftKey) keys.push('Shift');
  if (event.metaKey) keys.push('Meta');
  
  // 添加主键（非修饰键）
  if (!['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) {
    let mainKey = event.key;
    
    // 标准化一些特殊键名
    if (mainKey === ' ') mainKey = 'Space';
    else if (mainKey.length === 1) mainKey = mainKey.toUpperCase();
    
    keys.push(mainKey);
    
    // 只有在有修饰键和主键时才完成捕获
    if (keys.length >= 2) {
      tempShortcut.value = keys.join('+');
      isCapturingShortcut.value = false;
      
      // 重新启用全局快捷键
      try {
        await invoke("enable_shortcuts");
      } catch (error) {
        console.error("Failed to enable shortcuts:", error);
      }
      
      // 立即保存快捷键
      await saveShortcut();
    }
  }
}

async function cancelCaptureShortcut() {
  isCapturingShortcut.value = false;
  tempShortcut.value = currentShortcut.value;
  
  // 重新启用全局快捷键
  try {
    await invoke("enable_shortcuts");
  } catch (error) {
    console.error("Failed to enable shortcuts:", error);
  }
}

// ===== 日志相关函数 =====

function toggleLogPanel() {
  logPanelExpanded.value = !logPanelExpanded.value;
}

function clearLogs() {
  logEntries.value = [];
  invoke("clear_debug_logs").catch(console.error);
  // 清空后确保滚动位置重置
  nextTick(() => {
    const logContainer = document.querySelector('.log-content') as HTMLElement;
    if (logContainer) {
      scrollToTop(logContainer);
    }
  });
}

async function saveLogSettings() {
  try {
    if (tempShowDebugLogs.value !== showDebugLogs.value) {
      await invoke("set_show_debug_logs", { show: tempShowDebugLogs.value });
      showDebugLogs.value = tempShowDebugLogs.value;
      
      // 如果开启了日志显示，获取现有日志
      if (showDebugLogs.value) {
        logEntries.value = await invoke<LogEntry[]>("get_debug_logs");
      } else {
        logEntries.value = [];
      }
    }

    if (tempSaveLogsToFile.value !== saveLogsToFile.value) {
      await invoke("set_save_logs_to_file", { save: tempSaveLogsToFile.value });
      saveLogsToFile.value = tempSaveLogsToFile.value;
    }

    console.log("日志设置已保存");
  } catch (error) {
    console.error("Failed to save log settings:", error);
    alert(`日志设置保存失败: ${error}`);
    // 恢复到之前的值
    tempShowDebugLogs.value = showDebugLogs.value;
    tempSaveLogsToFile.value = saveLogsToFile.value;
  }
}

// ===== 主题相关函数 =====

// 保存暗色模式设置
async function saveDarkModeSettings() {
  try {
    if (tempIsDarkMode.value !== isDarkMode.value) {
      const nextDarkMode = tempIsDarkMode.value;
      await invoke("set_dark_mode", { enabled: nextDarkMode });
      isDarkMode.value = nextDarkMode;
      applyTheme();
      console.log("暗色模式设置已更新为:", isDarkMode.value);
    }
  } catch (error) {
    console.error("Failed to save dark mode settings:", error);
    tempIsDarkMode.value = isDarkMode.value;
    applyTheme();
  }
}

// 应用主题
function applyTheme() {
  const root = document.documentElement;
  if (isDarkMode.value) {
    root.classList.add('dark-mode');
  } else {
    root.classList.remove('dark-mode');
  }
}

// 保存锁定时退出设置
async function saveExitOnLockSettings() {
  try {
    if (tempExitOnLock.value !== exitOnLock.value) {
      await invoke("set_exit_on_lock", { enabled: tempExitOnLock.value });
      exitOnLock.value = tempExitOnLock.value;
      console.log("锁定时退出设置已更新为:", exitOnLock.value);
    }
  } catch (error) {
    console.error("Failed to save exit on lock settings:", error);
    // 恢复到之前的值
    tempExitOnLock.value = exitOnLock.value;
  }
}


// 保存通知开关设置
async function saveEnableNotificationsSettings() {
  try {
    if (tempEnableNotifications.value !== enableNotifications.value) {
      await invoke("set_enable_notifications", { enabled: tempEnableNotifications.value });
      enableNotifications.value = tempEnableNotifications.value;
      console.log("系统通知开关设置已更新为:", enableNotifications.value);
    }
  } catch (error) {
    console.error("Failed to save notifications settings:", error);
    // 恢复到之前的值
    tempEnableNotifications.value = enableNotifications.value;
  }
}

// 保存触发后动作设置
async function savePostTriggerActionSettings() {
  try {
    if (tempPostTriggerAction.value !== postTriggerAction.value) {
      await invoke("set_post_trigger_action", { action: tempPostTriggerAction.value });
      postTriggerAction.value = tempPostTriggerAction.value;
      console.log("触发后动作设置已更新为:", postTriggerAction.value);
    }
  } catch (error) {
    console.error("Failed to save post trigger action settings:", error);
    // 恢复到之前的值
    tempPostTriggerAction.value = postTriggerAction.value;
  }
}

// 保存默认摄像头设置
async function saveDefaultCameraSettings() {
  try {
    if (tempDefaultCameraId.value !== defaultCameraId.value) {
      const nextDefaultCameraId = tempDefaultCameraId.value;
      await invoke("set_default_camera_id", { cameraId: tempDefaultCameraId.value });
      defaultCameraId.value = nextDefaultCameraId;
      if (nextDefaultCameraId !== null) {
        selectedCameraId.value = nextDefaultCameraId;
      }
      console.log("默认摄像头设置已更新为:", defaultCameraId.value);
    }
  } catch (error) {
    console.error("Failed to save default camera settings:", error);
    tempDefaultCameraId.value = defaultCameraId.value;
  }
}

// 保存拍摄延迟时间设置
async function saveCaptureDelaySettings() {
  try {
    const normalizedDelay = normalizeCaptureDelayValue(tempCaptureDelaySeconds.value);
    tempCaptureDelaySeconds.value = normalizedDelay;

    if (normalizedDelay !== captureDelaySeconds.value) {
      await invoke("set_capture_delay_seconds", { delay: normalizedDelay });
      captureDelaySeconds.value = normalizedDelay;
      tempCaptureDelaySeconds.value = normalizedDelay;
      console.log("拍摄延迟时间设置已更新为:", captureDelaySeconds.value);
    }
  } catch (error) {
    console.error("Failed to save capture delay settings:", error);
    tempCaptureDelaySeconds.value = captureDelaySeconds.value;
  }
}



// ===== 自定义拖拽调整功能 =====
let isDragging = false;
let startY = 0;
let startHeight = 0;
let logContentElement: HTMLElement | null = null;
let resizeHandleRef: HTMLElement | null = null;
let handleMouseDownRef: ((e: Event) => void) | null = null;

// 声明全局函数引用以便清理
let handleMouseMove: ((e: Event) => void) | null = null;
let handleMouseUp: (() => void) | null = null;

function initCustomResize() {
  logContentElement = document.querySelector('.log-content') as HTMLElement;
  resizeHandleRef = document.querySelector('.custom-resize-handle') as HTMLElement;
  
  if (!logContentElement || !resizeHandleRef) return;
  
  const handleMouseDownFn = (e: Event) => {
    const mouseEvent = e as MouseEvent;
    isDragging = true;
    startY = mouseEvent.clientY;
    startHeight = logContentElement!.offsetHeight;
    
    // 使用passive listener for better performance
    document.addEventListener('mousemove', handleMouseMove!, { passive: false });
    document.addEventListener('mouseup', handleMouseUp!, { passive: true });
    document.body.style.cursor = 'ns-resize';
    document.body.style.userSelect = 'none';
    
    // 提高更新优先级
    logContentElement!.style.willChange = 'height';
    
    e.preventDefault();
  };
  
  handleMouseMove = (e: Event) => {
    if (!isDragging || !logContentElement) return;
    
    const mouseEvent = e as MouseEvent;
    const deltaY = mouseEvent.clientY - startY;
    const newHeight = Math.max(20, Math.min(400, startHeight + deltaY));
    
    // 使用requestAnimationFrame确保最佳性能
    requestAnimationFrame(() => {
      if (!logContentElement) return;
      logContentElement.style.height = `${newHeight}px`;
      
      // 动态调整紧凑模式
      if (newHeight < 100) {
        logContentElement.classList.add('compact-mode');
      } else {
        logContentElement.classList.remove('compact-mode');
      }
    });
    
    e.preventDefault();
  };
  
  handleMouseUp = () => {
    if (!isDragging) return;
    
    isDragging = false;
    if (handleMouseMove) document.removeEventListener('mousemove', handleMouseMove);
    if (handleMouseUp) document.removeEventListener('mouseup', handleMouseUp);
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
    
    // 清理性能优化标记
    if (logContentElement) {
      logContentElement.style.willChange = 'auto';
    }
  };
  
  handleMouseDownRef = handleMouseDownFn;
  resizeHandleRef.addEventListener('mousedown', handleMouseDownFn, { passive: false });
}

// 清理事件监听器
function cleanupCustomResize() {
  if (resizeHandleRef && handleMouseDownRef) {
    resizeHandleRef.removeEventListener('mousedown', handleMouseDownRef);
  }
  if (isDragging && handleMouseMove && handleMouseUp) {
    document.removeEventListener('mousemove', handleMouseMove);
    document.removeEventListener('mouseup', handleMouseUp);
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
  }
  isDragging = false;
  resizeHandleRef = null;
  handleMouseDownRef = null;
  logContentElement = null;
}

// ===== 生命周期钩子 =====

onMounted(async () => {
  // 获取摄像头列表
  try {
    cameraList.value = await invoke<CameraInfo[]>("get_camera_list");
    if (cameraList.value.length > 0) {
      selectedCameraId.value = cameraList.value[0].id;
    }
  } catch (error) {
    console.error("Failed to get camera list:", error);
    cameraList.value = [];
  }

  // 加载配置或使用默认设置
  const configLoadedSuccessfully = await loadAppConfig();
  
  // 应用默认摄像头设置
  if (defaultCameraId.value !== null && cameraList.value.length > 0) {
    // 检查默认摄像头ID是否在可用摄像头列表中
    const defaultCamera = cameraList.value.find(cam => cam.id === defaultCameraId.value);
    if (defaultCamera) {
      selectedCameraId.value = defaultCameraId.value;
    } else {
      console.warn("默认摄像头不在可用列表中，使用第一个摄像头");
      selectedCameraId.value = cameraList.value[0].id;
    }
  }

  // 获取当前快捷键
  try {
    currentShortcut.value = await invoke<string>("get_shortcut_key");
    tempShortcut.value = currentShortcut.value;
  } catch (error) {
    console.error("Failed to get shortcut key:", error);
  }

  // 监听状态变化
  const unlistenMonitoringStatus = await listen<MonitoringStatus | '锁定中'>("monitoring_status_changed", (event) => {
    monitoringStatus.value = event.payload;
  });
  eventUnlisteners.push(unlistenMonitoringStatus);

  // 监听日志事件
  const unlistenLogEntry = await listen<LogEntry>("log_entry", (event) => {
    if (showDebugLogs.value) {
      logEntries.value.push(event.payload);
      // 限制日志条数，避免内存溢出
      if (logEntries.value.length > 500) {
        logEntries.value.shift();
      }
      // 自动滚动到最新日志
      nextTick(() => {
        const logContainer = document.querySelector('.log-content') as HTMLElement;
        if (logContainer) {
          scrollToBottom(logContainer);
        }
      });
    }
  });
  eventUnlisteners.push(unlistenLogEntry);

  // 获取日志设置
  try {
    showDebugLogs.value = await invoke<boolean>("get_show_debug_logs");
    tempShowDebugLogs.value = showDebugLogs.value;
    
    saveLogsToFile.value = await invoke<boolean>("get_save_logs_to_file");
    tempSaveLogsToFile.value = saveLogsToFile.value;
    
    // 如果启用了日志保存，设置日志文件路径
    if (saveLogsToFile.value) {
      await invoke("set_log_file_path", { path: savePath.value });
    }
  } catch (error) {
    console.error("Failed to get log settings:", error);
  }

  // 如果显示日志已启用，获取现有日志
  if (showDebugLogs.value) {
    try {
      logEntries.value = await invoke<LogEntry[]>("get_debug_logs");
    } catch (error) {
      console.error("Failed to get debug logs:", error);
    }
  }

  // 如果配置加载失败，获取单独的设置
  if (!configLoadedSuccessfully) {
    // 获取暗色模式设置
    try {
      isDarkMode.value = await invoke<boolean>("get_dark_mode");
      tempIsDarkMode.value = isDarkMode.value;
    } catch (error) {
      console.error("Failed to get dark mode setting:", error);
    }
    
    // 获取通知开关设置
    try {
      enableNotifications.value = await invoke<boolean>("get_enable_notifications");
      tempEnableNotifications.value = enableNotifications.value;
    } catch (error) {
      console.error("Failed to get notifications setting:", error);
    }
    
    // 获取触发后动作设置
    try {
      postTriggerAction.value = await invoke<"CaptureAndLock" | "CaptureOnly" | "ScreenRecording">("get_post_trigger_action");
      tempPostTriggerAction.value = postTriggerAction.value;
    } catch (error) {
      console.error("Failed to get post trigger action setting:", error);
    }
    
    // 获取默认摄像头设置
    try {
      defaultCameraId.value = await invoke<number | null>("get_default_camera_id");
      tempDefaultCameraId.value = defaultCameraId.value;
    } catch (error) {
      console.error("Failed to get default camera setting:", error);
    }
    
    // 获取拍摄延时设置
    try {
      captureDelaySeconds.value = await invoke<number>("get_capture_delay_seconds");
      tempCaptureDelaySeconds.value = captureDelaySeconds.value;
    } catch (error) {
      console.error("Failed to get capture delay setting:", error);
    }
    
    try {
      captureMode.value = await invoke<'Video'>("get_capture_mode");
      tempCaptureMode.value = captureMode.value;
    } catch (error) {
      console.error("Failed to get capture mode setting:", error);
    }
  }

  // 在所有初始化完成后设置自定义拖拽
  nextTick(() => {
    initCustomResize();
  });
});

// 组件卸载时清理事件监听器
onUnmounted(() => {
  cleanupCustomResize();
  document.removeEventListener('keydown', handleEscapeKey);
  for (const unlisten of eventUnlisteners.splice(0)) {
    unlisten();
  }
});
</script>

<template>
  <main class="app-container">
    <div class="app-header">
      <div class="app-title">
        <div class="app-icon">📷</div>
        <h1>SnapLock</h1>
      </div>
      <div class="status-indicator" :class="statusClass">
        <div class="status-dot"></div>
        <span class="status-text">{{ monitoringStatus === '锁定中' ? '警戒中' : monitoringStatus }}</span>
      </div>
    </div>

    <div class="app-content">
      <div class="control-card">
        <div class="control-section">
          <label for="cameraSelect" class="control-label">
            <span class="label-icon">🎥</span>
            选择摄像头
          </label>
          <div class="select-wrapper">
            <select id="cameraSelect" v-model="selectedCameraId" class="custom-select">
              <option v-for="cam in cameraList" :key="cam.id" :value="cam.id">
                {{ cam.name }}
              </option>
            </select>
          </div>
        </div>

        <div class="control-section">
          <div class="action-buttons">
            <button
              @click="toggleMonitoring"
              class="main-action-button"
            >
              <span class="button-icon">{{ getStatusIcon(monitoringStatus) }}</span>
              <span class="button-text">{{ getStatusText(monitoringStatus, currentShortcut) }}</span>
            </button>
            <button @click="openSettings" class="settings-button" title="设置">
              ⚙️
            </button>
          </div>
        </div>

        <!-- 日志面板 -->
        <div v-if="showDebugLogs" class="log-panel">
          <div class="log-header" @click="toggleLogPanel">
            <div class="log-title">
              <span class="log-icon">📋</span>
              调试日志
            </div>
            <div class="log-controls">
              <button @click.stop="clearLogs" class="log-clear-button" title="清空日志">
                🗑️
              </button>
              <button class="log-toggle-button" :class="{ 'expanded': logPanelExpanded }">
                {{ logPanelExpanded ? '▼' : '▶' }}
              </button>
            </div>
          </div>
          
          <div v-if="logPanelExpanded" class="log-content">
            <div v-if="logEntries.length === 0" class="log-empty">
              暂无日志记录
            </div>
            <div v-else class="log-entries">
              <div
                v-for="(entry, index) in logEntries"
                :key="index"
                class="log-entry"
                :class="getLogLevelClass(entry.level)"
              >
                <span class="log-timestamp">{{ entry.timestamp }}</span>
                <span class="log-level">[{{ entry.level }}]</span>
                <span class="log-message">{{ entry.message }}</span>
              </div>
            </div>
          </div>
        </div>
        
      </div>
    </div>

    <!-- 设置对话框 -->
    <div v-if="showSettings" class="settings-overlay" @click="closeSettings">
      <div class="settings-dialog" @click.stop>
        <div class="settings-header">
          <h2>设置</h2>
          <button @click="closeSettings" class="close-button">✕</button>
        </div>
        
        <div class="settings-content">
          <div class="setting-item">
            <label class="setting-label">
              <span class="setting-icon">⌨️</span>
              快捷键
            </label>
            <div class="shortcut-input-group">
              <input
                v-model="tempShortcut"
                type="text"
                class="setting-input shortcut-input"
                :class="{ 'capturing': isCapturingShortcut, 'invalid': !validateShortcut(tempShortcut) && tempShortcut !== '按下快捷键...' }"
                placeholder="例如: Alt+L, Ctrl+Shift+S"
                readonly
                @keydown="handleShortcutKeyDown"
              />
              <button
                v-if="!isCapturingShortcut"
                @click="startCaptureShortcut"
                class="capture-button"
                title="点击捕获快捷键"
              >
                🎯
              </button>
              <button
                v-else
                @click="cancelCaptureShortcut"
                class="cancel-capture-button"
                title="取消捕获"
              >
                ✕
              </button>
            </div>
            <div v-if="!validateShortcut(tempShortcut) && tempShortcut !== '按下快捷键...' && tempShortcut" class="shortcut-error">
              请输入有效的快捷键组合（至少包含一个修饰键和一个主键）
            </div>
          </div>
          
          <div class="setting-item">
            <label class="setting-label">
              <span class="setting-icon">📁</span>
              照片&日志保存路径
            </label>
            <div class="path-input-group">
              <input
                type="text"
                :value="tempSavePath"
                readonly
                class="setting-input path-display"
                :title="tempSavePath"
              >
              <button @click="selectSavePathInSettings" class="path-select-button">
                📂
              </button>
            </div>
          </div>

          <div class="setting-item">
            <label class="setting-label">
              <span class="setting-icon">🐛</span>
              调试选项
            </label>
            <div class="checkbox-group">
              <label class="checkbox-item">
                <input
                  type="checkbox"
                  v-model="tempShowDebugLogs"
                  @change="saveLogSettings"
                  class="checkbox-input"
                />
                <span class="checkbox-label">显示调试日志</span>
              </label>
              <label class="checkbox-item">
                <input
                  type="checkbox"
                  v-model="tempSaveLogsToFile"
                  @change="saveLogSettings"
                  class="checkbox-input"
                />
                <span class="checkbox-label">保存日志到文件</span>
              </label>
            </div>
          </div>

          <div class="setting-item">
            <label class="setting-label">
              <span class="setting-icon">📷</span>
              相机设置
            </label>
            <div class="camera-settings">
              <div class="setting-sub-item">
                <label class="setting-sub-label">默认摄像头</label>
                <div class="select-wrapper">
                  <select v-model="tempDefaultCameraId" @change="saveDefaultCameraSettings" class="custom-select">
                    <option :value="null">不设置默认摄像头</option>
                    <option v-for="cam in cameraList" :key="cam.id" :value="cam.id">
                      {{ cam.name }}
                    </option>
                  </select>
                </div>
                <div class="setting-description">
                  设置启动时自动选择的摄像头，留空则使用第一个可用摄像头
                </div>
              </div>
              
              <div class="setting-sub-item">
                <label class="setting-sub-label">相机预览</label>
                <div class="camera-controls">
                  <div class="camera-status">
                    <span class="status-text">权限状态: </span>
                    <span class="permission-status" :class="getPermissionStatusClass(cameraPermissionStatus)">
                      {{ cameraPermissionStatus }}
                    </span>
                    <button
                      @click="checkCameraPermission"
                      :disabled="isCheckingPermission"
                      class="check-permission-button"
                      title="检查相机权限"
                    >
                      {{ isCheckingPermission ? '⏳' : '🔄' }}
                    </button>
                  </div>
                  <div class="preview-controls">
                    <button
                      @click="toggleCameraPreview"
                      :disabled="isCheckingPermission"
                      class="preview-toggle-button"
                      :class="{ 'active': showCameraPreview }"
                    >
                      {{ showCameraPreview ? '隐藏预览' : '显示预览' }}
                    </button>
                  </div>
                  <div v-if="showCameraPreview && cameraPreviewUrl" class="camera-preview">
                    <img :src="cameraPreviewUrl" alt="相机预览" class="preview-image" />
                    <button @click="refreshCameraPreview" class="refresh-preview-button" title="刷新预览">
                      🔄
                    </button>
                  </div>
                  <div v-else-if="showCameraPreview && !cameraPreviewUrl" class="preview-placeholder">
                    <span>正在加载预览...</span>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <div class="setting-item">
            <label class="setting-label">
              <span class="setting-icon">🌙</span>
              外观主题
            </label>
            <div class="theme-controls">
              <label class="checkbox-item">
                <input
                  type="checkbox"
                  v-model="tempIsDarkMode"
                  @change="saveDarkModeSettings"
                  class="checkbox-input"
                />
                <span class="checkbox-label">暗色模式</span>
              </label>
            </div>
          </div>

          <div class="setting-item">
            <label class="setting-label">
              <span class="setting-icon">🔒</span>
              安全选项
            </label>
            <div class="security-controls">
              <div class="setting-label">触发后动作</div>
              <div class="radio-group">
                <label class="radio-item">
                  <input
                    type="radio"
                    v-model="tempPostTriggerAction"
                    value="CaptureAndLock"
                    @change="savePostTriggerActionSettings"
                    class="radio-input"
                  />
                  <span class="radio-label">拍摄并锁屏</span>
                </label>
                <label class="radio-item">
                  <input
                    type="radio"
                    v-model="tempPostTriggerAction"
                    value="CaptureOnly"
                    @change="savePostTriggerActionSettings"
                    class="radio-input"
                  />
                  <span class="radio-label">只拍摄</span>
                </label>
                <label class="radio-item">
                  <input
                    type="radio"
                    v-model="tempPostTriggerAction"
                    value="ScreenRecording"
                    @change="savePostTriggerActionSettings"
                    class="radio-input"
                  />
                  <span class="radio-label">屏幕录制</span>
                </label>
              </div>
              <label class="checkbox-item">
                <input
                  type="checkbox"
                  v-model="tempExitOnLock"
                  @change="saveExitOnLockSettings"
                  class="checkbox-input"
                />
                <span class="checkbox-label">电脑锁定时自动退出程序</span>
              </label>
            </div>
          </div>

          <div class="setting-item">
            <label class="setting-label">
              <span class="setting-icon">⏱️</span>
              拍摄延时设置
            </label>
            <div class="capture-time-controls">
              <div class="delay-input-group">
                <span class="delay-unit">持续</span>
                <input
                  type="number"
                  v-model.number="tempCaptureDelaySeconds"
                  @change="saveCaptureDelaySettings"
                  min="0"
                  max="60"
                  class="setting-input delay-input"
                  placeholder="0"
                />
                <span class="delay-unit">秒</span>
              </div>
              <div class="setting-description">
                检测到行为后持续录制的时间，<br>范围0-60秒（0秒表示不录像直接拍照）
              </div>
            </div>
          </div>

          <div class="setting-item">
            <label class="setting-label">
              <span class="setting-icon">📢</span>
              通知选项
            </label>
            <div class="notification-controls">
              <label class="checkbox-item">
                <input
                  type="checkbox"
                  v-model="tempEnableNotifications"
                  @change="saveEnableNotificationsSettings"
                  class="checkbox-input"
                />
                <span class="checkbox-label">启用系统通知</span>
              </label>
              <div class="setting-description">
                启用后，监控过程中检测到行为时会显示系统通知
              </div>
            </div>
          </div>
        </div>
        
      </div>
    </div>
  </main>
</template>

<style scoped>
/* 强制应用关键样式 - 确保复选框和选择器正确显示 */

/* 选择器样式强制应用 */
.custom-select {
  width: 100% !important;
  padding: 0.6rem 2rem 0.6rem 0.8rem !important;
  border: 2px solid var(--border-primary) !important;
  border-radius: 8px !important;
  background: var(--bg-primary) !important;
  font-size: 0.85rem !important;
  color: var(--text-primary) !important;
  cursor: pointer !important;
  transition: all 0.3s ease !important;
  appearance: none !important;
  -webkit-appearance: none !important;
  -moz-appearance: none !important;
  background-image: url("data:image/svg+xml,%3csvg xmlns='http://www.w3.org/2000/svg' fill='none' viewBox='0 0 20 20'%3e%3cpath stroke='%236b7280' stroke-linecap='round' stroke-linejoin='round' stroke-width='1.5' d='M6 8l4 4 4-4'/%3e%3c/svg%3e") !important;
  background-position: right 0.6rem center !important;
  background-repeat: no-repeat !important;
  background-size: 0.7rem !important;
  box-sizing: border-box !important;
}

.custom-select:focus {
  outline: none !important;
  border-color: #667eea !important;
  box-shadow: 0 0 0 2px rgba(102, 126, 234, 0.1) !important;
}

.custom-select:hover {
  border-color: #667eea !important;
}

/* 复选框样式强制应用 */
.checkbox-input {
  width: 18px !important;
  height: 18px !important;
  border: 2px solid var(--border-primary) !important;
  border-radius: 4px !important;
  background: var(--bg-primary) !important;
  cursor: pointer !important;
  transition: all 0.3s ease !important;
  position: relative !important;
  flex-shrink: 0 !important;
  appearance: none !important;
  -webkit-appearance: none !important;
  -moz-appearance: none !important;
  margin: 0 !important;
  padding: 0 !important;
}

.checkbox-input:checked {
  background: #667eea !important;
  border-color: #667eea !important;
}

.checkbox-input:checked::after {
  content: '✓' !important;
  position: absolute !important;
  top: 50% !important;
  left: 50% !important;
  transform: translate(-50%, -50%) !important;
  color: white !important;
  font-size: 12px !important;
  font-weight: bold !important;
  line-height: 1 !important;
}

.checkbox-input:hover {
  border-color: #667eea !important;
}

.checkbox-label {
  font-size: 0.9rem !important;
  color: var(--text-primary) !important;
  user-select: none !important;
  margin: 0 !important;
  padding: 0 !important;
}

.checkbox-item {
  display: flex !important;
  align-items: center !important;
  gap: 0.5rem !important;
  cursor: pointer !important;
  transition: all 0.2s ease !important;
  margin: 0 !important;
  padding: 0 !important;
}

.checkbox-group {
  display: flex !important;
  flex-direction: column !important;
  gap: 0.75rem !important;
}

/* 按钮样式强制应用 */
.main-action-button {
  width: 100% !important;
  padding: 0.9rem 1rem !important;
  border: 2px solid var(--border-primary) !important;
  border-radius: 12px !important;
  background: var(--bg-primary) !important;
  color: var(--text-primary) !important;
  font-size: 0.9rem !important;
  font-weight: 600 !important;
  cursor: pointer !important;
  transition: all 0.3s ease !important;
  display: flex !important;
  align-items: center !important;
  justify-content: center !important;
  gap: 0.4rem !important;
  box-shadow: 0 2px 8px var(--shadow-medium) !important;
  min-height: 45px !important;
  box-sizing: border-box !important;
}

.main-action-button:hover {
  transform: translateY(-1px) !important;
  background: var(--bg-secondary) !important;
  border-color: var(--border-secondary) !important;
  box-shadow: 0 4px 12px var(--shadow-heavy) !important;
}

.main-action-button.disabled {
  background: var(--bg-secondary) !important;
  color: var(--text-tertiary) !important;
  border-color: var(--border-primary) !important;
  cursor: not-allowed !important;
  transform: none !important;
  box-shadow: 0 1px 3px var(--shadow-light) !important;
}

.settings-button {
  width: 45px !important;
  height: 45px !important;
  padding: 0 !important;
  border: 2px solid var(--border-primary) !important;
  border-radius: 12px !important;
  background: var(--bg-primary) !important;
  color: var(--text-secondary) !important;
  font-size: 1.1rem !important;
  cursor: pointer !important;
  transition: all 0.3s ease !important;
  display: flex !important;
  align-items: center !important;
  justify-content: center !important;
  box-shadow: 0 2px 8px var(--shadow-medium) !important;
  flex-shrink: 0 !important;
}

.settings-button:hover {
  background: var(--bg-secondary) !important;
  border-color: var(--border-secondary) !important;
  transform: translateY(-1px) !important;
  box-shadow: 0 4px 12px var(--shadow-heavy) !important;
}

/* 输入框样式强制应用 */
.setting-input {
  width: 100% !important;
  padding: 0.75rem !important;
  border: 2px solid var(--border-primary) !important;
  border-radius: 8px !important;
  background: var(--bg-primary) !important;
  font-size: 0.9rem !important;
  color: var(--text-primary) !important;
  transition: all 0.3s ease !important;
  box-sizing: border-box !important;
}

.setting-input:focus {
  outline: none !important;
  border-color: #667eea !important;
  box-shadow: 0 0 0 3px rgba(102, 126, 234, 0.1) !important;
}

/* 其他按钮样式 */
.path-select-button,
.capture-button,
.cancel-capture-button,
.check-permission-button,
.preview-toggle-button {
  border: 2px solid var(--border-primary) !important;
  border-radius: 8px !important;
  background: var(--bg-primary) !important;
  color: var(--text-secondary) !important;
  cursor: pointer !important;
  transition: all 0.3s ease !important;
  display: flex !important;
  align-items: center !important;
  justify-content: center !important;
  box-shadow: 0 1px 3px var(--shadow-light) !important;
}

.capture-button:hover {
  background: #667eea !important;
  border-color: #667eea !important;
  color: white !important;
}

.cancel-capture-button {
  background: #fed7d7 !important;
  border-color: #feb2b2 !important;
  color: #c53030 !important;
}

.preview-toggle-button.active {
  background: #667eea !important;
  border-color: #667eea !important;
  color: white !important;
}

/* 确保容器样式正确 */
.theme-controls,
.security-controls,
.screen-lock-controls,
.notification-controls,
.camera-settings,
.capture-time-controls {
  display: flex !important;
  flex-direction: column !important;
  gap: 0.75rem !important;
}

/* 相机设置子项样式 */
.setting-sub-item {
  display: flex !important;
  flex-direction: column !important;
  gap: 0.5rem !important;
  padding: 0.75rem !important;
  border: 1px solid var(--border-primary) !important;
  border-radius: 8px !important;
  background: var(--bg-secondary) !important;
}

.setting-sub-label {
  font-size: 0.9rem !important;
  font-weight: 600 !important;
  color: var(--text-primary) !important;
  margin: 0 !important;
}

/* 设置描述文本样式 */
.setting-description {
  font-size: 0.75rem !important;
  color: var(--text-tertiary) !important;
  margin-top: 0.25rem !important;
  line-height: 1.4 !important;
  opacity: 0.8 !important;
}

/* 延迟输入组样式 */
.delay-input-group {
  display: flex !important;
  align-items: center !important;
  gap: 0.5rem !important;
}

.delay-input {
  width: 80px !important;
  text-align: center !important;
}

.delay-unit {
  font-size: 0.9rem !important;
  color: var(--text-secondary) !important;
  font-weight: 500 !important;
}

/* 单选按钮样式 */
.radio-input {
  width: 18px !important;
  height: 18px !important;
  border: 2px solid var(--border-primary) !important;
  border-radius: 50% !important;
  background: var(--bg-primary) !important;
  cursor: pointer !important;
  transition: all 0.3s ease !important;
  position: relative !important;
  flex-shrink: 0 !important;
  appearance: none !important;
  -webkit-appearance: none !important;
  -moz-appearance: none !important;
  margin: 0 !important;
  padding: 0 !important;
}

.radio-input:checked {
  background: #667eea !important;
  border-color: #667eea !important;
}

.radio-input:checked::after {
  content: '' !important;
  position: absolute !important;
  top: 50% !important;
  left: 50% !important;
  transform: translate(-50%, -50%) !important;
  width: 8px !important;
  height: 8px !important;
  border-radius: 50% !important;
  background: white !important;
}

.radio-input:hover {
  border-color: #667eea !important;
}

.radio-label {
  font-size: 0.9rem !important;
  color: var(--text-primary) !important;
  user-select: none !important;
  margin: 0 !important;
  padding: 0 !important;
}

.radio-item {
  display: flex !important;
  align-items: center !important;
  gap: 0.5rem !important;
  cursor: pointer !important;
  transition: all 0.2s ease !important;
  margin: 0 !important;
  padding: 0 !important;
}

.radio-group {
  display: flex !important;
  flex-direction: column !important;
  gap: 0.75rem !important;
}
</style>
