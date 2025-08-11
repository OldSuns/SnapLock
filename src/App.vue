<script setup lang="ts">
import { ref, onMounted, computed, watch, nextTick } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from '@tauri-apps/plugin-dialog';
import { desktopDir } from '@tauri-apps/api/path';

interface CameraInfo {
  id: number;
  name: string;
}

const cameraList = ref<CameraInfo[]>([]);
const selectedCameraId = ref<number>(0);
const monitoringStatus = ref<string>("空闲"); // '空闲', '准备中', '警戒中'
const savePath = ref<string>("");
const showSettings = ref<boolean>(false);
const currentShortcut = ref<string>("Alt+L");
const tempShortcut = ref<string>("Alt+L");
const tempSavePath = ref<string>("");
const isCapturingShortcut = ref<boolean>(false);

// 日志相关状态
const showDebugLogs = ref<boolean>(false);
const saveLogsToFile = ref<boolean>(false);
const tempShowDebugLogs = ref<boolean>(false);
const tempSaveLogsToFile = ref<boolean>(false);
const logEntries = ref<any[]>([]);
const logPanelExpanded = ref<boolean>(false);

interface LogEntry {
  timestamp: string;
  level: string;
  message: string;
  target: string;
}

const statusClass = computed(() => {
  switch (monitoringStatus.value) {
    case "警戒中":
      return "status-active";
    case "准备中":
      return "status-pending";
    default:
      return "status-idle";
  }
});

watch(selectedCameraId, async (newId) => {
  if (cameraList.value.length > 0) {
    try {
      await invoke("set_camera_id", { cameraId: newId });
    } catch (error) {
      console.error("Failed to set camera ID:", error);
    }
  }
});

onMounted(async () => {
  // 获取摄像头列表
  cameraList.value = await invoke<CameraInfo[]>("get_camera_list");
  if (cameraList.value.length > 0) {
    selectedCameraId.value = cameraList.value[0].id;
  }

  // 设置默认保存路径为桌面
  const desktop = await desktopDir();
  savePath.value = desktop;
  tempSavePath.value = desktop;
  await invoke("set_save_path", { path: desktop });

  // 获取当前快捷键
  try {
    currentShortcut.value = await invoke<string>("get_shortcut_key");
    tempShortcut.value = currentShortcut.value;
  } catch (error) {
    console.error("Failed to get shortcut key:", error);
  }

  // 监听状态变化
  listen<string>("monitoring_status_changed", (event) => {
    monitoringStatus.value = event.payload;
  });

  // 监听日志事件
  listen<LogEntry>("log_entry", (event) => {
    if (showDebugLogs.value) {
      logEntries.value.push(event.payload);
      // 限制日志条数，避免内存溢出
      if (logEntries.value.length > 500) {
        logEntries.value.shift();
      }
      // 自动滚动到最新日志
      nextTick(() => {
        const logContainer = document.querySelector('.log-content');
        if (logContainer) {
          logContainer.scrollTop = logContainer.scrollHeight;
        }
      });
    }
  });

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
});

async function toggleMonitoring() {
  if (monitoringStatus.value === "空闲") {
    try {
      await invoke("start_monitoring_command", { cameraId: selectedCameraId.value });
    } catch (err) {
      // 可以在这里向用户显示一个错误通知
    }
  }
}


function openSettings() {
  tempShortcut.value = currentShortcut.value;
  tempSavePath.value = savePath.value;
  tempShowDebugLogs.value = showDebugLogs.value;
  tempSaveLogsToFile.value = saveLogsToFile.value;
  showSettings.value = true;
}

async function closeSettings() {
  // 如果正在捕获快捷键，先取消并重新启用快捷键
  if (isCapturingShortcut.value) {
    await cancelCaptureShortcut();
  }
  showSettings.value = false;
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
      await invoke("set_save_path", { path: tempSavePath.value });
      savePath.value = tempSavePath.value;
      
      // 更新日志文件路径
      if (saveLogsToFile.value) {
        await invoke("set_log_file_path", { path: tempSavePath.value });
      }
      
      console.log("保存路径已更新为:", tempSavePath.value);
    }
  } catch (error) {
    console.error("Failed to save path:", error);
    alert(`保存路径设置失败: ${error}`);
    // 恢复到之前的值
    tempSavePath.value = savePath.value;
  }
}

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

function validateShortcut(shortcut: string): boolean {
  if (!shortcut || shortcut === "按下快捷键...") return false;
  
  const parts = shortcut.split('+');
  if (parts.length < 2) return false;
  
  const modifiers = parts.slice(0, -1);
  const mainKey = parts[parts.length - 1];
  
  // 检查修饰键是否有效
  const validModifiers = ['Ctrl', 'Alt', 'Shift', 'Meta', 'Cmd'];
  for (const modifier of modifiers) {
    if (!validModifiers.includes(modifier)) return false;
  }
  
  // 检查主键是否有效（不能是修饰键）
  if (validModifiers.includes(mainKey)) return false;
  if (!mainKey || mainKey.trim() === '') return false;
  
  return true;
}

// 日志相关函数
function toggleLogPanel() {
  logPanelExpanded.value = !logPanelExpanded.value;
}

function clearLogs() {
  logEntries.value = [];
  invoke("clear_debug_logs").catch(console.error);
}

function getLogLevelClass(level: string): string {
  switch (level.toLowerCase()) {
    case 'error': return 'log-error';
    case 'warn': return 'log-warn';
    case 'info': return 'log-info';
    case 'debug': return 'log-debug';
    default: return 'log-default';
  }
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
        <span class="status-text">{{ monitoringStatus }}</span>
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
              :disabled="monitoringStatus !== '空闲'"
              class="main-action-button"
              :class="{ 'disabled': monitoringStatus !== '空闲' }"
            >
              <span class="button-icon">
                {{ monitoringStatus === '空闲' ? '▶️' : (monitoringStatus === '准备中' ? '⏳' : '🛡️') }}
              </span>
              <span class="button-text">
                {{ monitoringStatus === '空闲' ? '启动监控' : (monitoringStatus === '准备中' ? '准备中...' : `警戒中 (${currentShortcut} 停止)`) }}
              </span>
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
        </div>
        
      </div>
    </div>
  </main>
</template>

<style scoped>
/* 应用容器 - 针对Tauri小窗口优化 */
.app-container {
  height: 100vh;
  background: #ffffff;
  display: flex;
  flex-direction: column;
  padding: 1rem;
  box-sizing: border-box;
  overflow: hidden;
}

/* 应用头部 - 紧凑设计 */
.app-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 1rem;
  background: #f8fafc;
  border-radius: 16px;
  padding: 0.75rem 1rem;
  border: 1px solid #e2e8f0;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.05);
  flex-shrink: 0;
}

.app-title {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.app-icon {
  font-size: 1.5rem;
  animation: pulse 2s infinite;
}

@keyframes pulse {
  0%, 100% { transform: scale(1); }
  50% { transform: scale(1.05); }
}

.app-title h1 {
  margin: 0;
  color: #2d3748;
  font-size: 1.5rem;
  font-weight: 700;
}

/* 状态指示器 - 紧凑版 */
.status-indicator {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.5rem 0.75rem;
  border-radius: 20px;
  background: white;
  border: 1px solid #e2e8f0;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
  transition: all 0.3s ease;
}

.status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  animation: statusPulse 2s infinite;
}

@keyframes statusPulse {
  0%, 100% { opacity: 1; transform: scale(1); }
  50% { opacity: 0.7; transform: scale(1.1); }
}

.status-text {
  color: #4a5568;
  font-weight: 600;
  font-size: 0.8rem;
}

.status-active .status-dot {
  background: #4caf50;
  box-shadow: 0 0 6px rgba(76, 175, 80, 0.6);
}

.status-pending .status-dot {
  background: #ff9800;
  box-shadow: 0 0 6px rgba(255, 152, 0, 0.6);
}

.status-idle .status-dot {
  background: #9e9e9e;
  box-shadow: 0 0 6px rgba(158, 158, 158, 0.6);
}

/* 应用内容 - 充满剩余空间 */
.app-content {
  flex: 1;
  display: flex;
  justify-content: center;
  align-items: stretch;
  overflow: hidden;
}

.control-card {
  background: #ffffff;
  border-radius: 16px;
  padding: 1rem;
  width: 100%;
  max-width: 100%;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.05);
  border: 1px solid #e2e8f0;
  transition: transform 0.2s ease;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  box-sizing: border-box;
  min-height: 0;
  flex: 1;
}

.control-card:hover {
  transform: translateY(-2px);
  box-shadow: 0 8px 16px rgba(0, 0, 0, 0.1);
}

.control-section {
  margin-bottom: 1.25rem;
  flex-shrink: 0;
}

.control-section:last-child {
  margin-bottom: 0;
}

.control-label {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-bottom: 0.5rem;
  font-weight: 600;
  color: #2c3e50;
  font-size: 0.9rem;
}

.label-icon {
  font-size: 1rem;
  opacity: 0.8;
}

/* 选择器样式 - 紧凑版 */
.select-wrapper {
  position: relative;
}

.custom-select {
  width: 100%;
  max-width: 100%;
  padding: 0.6rem 2rem 0.6rem 0.8rem;
  border: 2px solid #e1e8ed;
  border-radius: 8px;
  background: white;
  font-size: 0.85rem;
  color: #2c3e50;
  cursor: pointer;
  transition: all 0.3s ease;
  appearance: none;
  background-image: url("data:image/svg+xml,%3csvg xmlns='http://www.w3.org/2000/svg' fill='none' viewBox='0 0 20 20'%3e%3cpath stroke='%236b7280' stroke-linecap='round' stroke-linejoin='round' stroke-width='1.5' d='M6 8l4 4 4-4'/%3e%3c/svg%3e");
  background-position: right 0.6rem center;
  background-repeat: no-repeat;
  background-size: 0.7rem;
  box-sizing: border-box;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.custom-select:focus {
  outline: none;
  border-color: #667eea;
  box-shadow: 0 0 0 2px rgba(102, 126, 234, 0.1);
}

.custom-select:hover {
  border-color: #667eea;
}

/* 路径输入组 - 紧凑版 */
.path-input-group {
  display: flex;
  gap: 0.5rem;
  align-items: center;
}

.path-input {
  flex: 1;
  padding: 0.6rem 0.8rem;
  border: 2px solid #e1e8ed;
  border-radius: 8px;
  background: #f8fafc;
  font-size: 0.75rem;
  color: #2c3e50;
  transition: all 0.3s ease;
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  box-sizing: border-box;
}

.path-input:focus {
  outline: none;
  border-color: #667eea;
  box-shadow: 0 0 0 2px rgba(102, 126, 234, 0.1);
}

.path-button {
  padding: 0.6rem;
  border: 2px solid #e2e8f0;
  border-radius: 8px;
  background: white;
  color: #4a5568;
  font-size: 0.9rem;
  cursor: pointer;
  transition: all 0.3s ease;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  flex-shrink: 0;
  box-sizing: border-box;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
}

.path-button:hover {
  background: #f7fafc;
  border-color: #cbd5e0;
  transform: translateY(-1px);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
}

.path-button:active {
  transform: translateY(0);
}

/* 主要操作按钮 - 针对小窗口优化 */
.main-action-button {
  width: 100%;
  max-width: 100%;
  padding: 0.9rem 1rem;
  border: 2px solid #e2e8f0;
  border-radius: 12px;
  background: white;
  color: #2d3748;
  font-size: 0.9rem;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.3s ease;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 0.4rem;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1);
  position: relative;
  overflow: hidden;
  flex-shrink: 0;
  box-sizing: border-box;
  text-align: center;
  min-height: 45px;
}

.main-action-button:hover {
  transform: translateY(-1px);
  background: #f7fafc;
  border-color: #cbd5e0;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
}

.main-action-button:active {
  transform: translateY(0);
}

.main-action-button.disabled {
  background: #f7fafc;
  color: #a0aec0;
  border-color: #e2e8f0;
  cursor: not-allowed;
  transform: none;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.05);
}

.main-action-button.disabled:hover {
  transform: none;
  background: #f7fafc;
  border-color: #e2e8f0;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.05);
}

.button-icon {
  font-size: 1.1rem;
  color: #4a5568;
}

.button-text {
  font-size: 0.85rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 100%;
  color: #2d3748;
}

.main-action-button.disabled .button-icon,
.main-action-button.disabled .button-text {
  color: #a0aec0;
}

/* 操作按钮组 */
.action-buttons {
  display: flex;
  gap: 0.5rem;
  align-items: center;
}

.main-action-button {
  flex: 1;
}

.settings-button {
  width: 45px;
  height: 45px;
  padding: 0;
  border: 2px solid #e2e8f0;
  border-radius: 12px;
  background: white;
  color: #4a5568;
  font-size: 1.1rem;
  cursor: pointer;
  transition: all 0.3s ease;
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1);
  flex-shrink: 0;
}

.settings-button:hover {
  background: #f7fafc;
  border-color: #cbd5e0;
  transform: translateY(-1px);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
}

.settings-button:active {
  transform: translateY(0);
}

/* 设置对话框样式 */
.settings-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
  backdrop-filter: blur(4px);
}

.settings-dialog {
  background: white;
  border-radius: 16px;
  box-shadow: 0 20px 40px rgba(0, 0, 0, 0.15);
  width: 90%;
  max-width: 400px;
  max-height: 85vh;
  overflow: hidden;
  animation: slideIn 0.3s ease;
  display: flex;
  flex-direction: column;
}

@keyframes slideIn {
  from {
    opacity: 0;
    transform: translateY(-20px) scale(0.95);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}

.settings-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.75rem 1rem;
  border-bottom: 1px solid #e2e8f0;
  background: #f8fafc;
  min-height: 50px;
}

.settings-header h2 {
  margin: 0;
  color: #2d3748;
  font-size: 1.1rem;
  font-weight: 600;
}

.close-button {
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: #718096;
  font-size: 1rem;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.2s ease;
  flex-shrink: 0;
}

.close-button:hover {
  background: #e2e8f0;
  color: #4a5568;
}


.settings-content {
  padding: 1.5rem;
  flex: 1;
  overflow-y: auto;
  min-height: 0;
}

.setting-item {
  margin-bottom: 1.5rem;
}

.setting-item:last-child {
  margin-bottom: 0;
}

.setting-label {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-bottom: 0.75rem;
  font-weight: 600;
  color: #2d3748;
  font-size: 0.9rem;
}

.setting-icon {
  font-size: 1rem;
  opacity: 0.8;
}

.setting-input {
  width: 100%;
  padding: 0.75rem;
  border: 2px solid #e2e8f0;
  border-radius: 8px;
  background: white;
  font-size: 0.9rem;
  color: #2d3748;
  transition: all 0.3s ease;
  box-sizing: border-box;
}

.setting-input:focus {
  outline: none;
  border-color: #667eea;
  box-shadow: 0 0 0 3px rgba(102, 126, 234, 0.1);
}

.setting-input.path-display {
  background: #f8fafc;
  flex: 1;
  font-size: 0.8rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.path-select-button {
  width: 40px;
  height: 40px;
  border: 2px solid #e2e8f0;
  border-radius: 8px;
  background: white;
  color: #4a5568;
  font-size: 1rem;
  cursor: pointer;
  transition: all 0.3s ease;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
}

.path-select-button:hover {
  background: #f7fafc;
  border-color: #cbd5e0;
  transform: translateY(-1px);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
}


/* 快捷键输入组样式 */
.shortcut-input-group {
  display: flex;
  gap: 0.5rem;
  align-items: center;
}

.shortcut-input {
  flex: 1;
}

.shortcut-input.capturing {
  border-color: #667eea;
  box-shadow: 0 0 0 3px rgba(102, 126, 234, 0.1);
  background: #f0f4ff;
}

.shortcut-input.invalid {
  border-color: #e53e3e;
  box-shadow: 0 0 0 3px rgba(229, 62, 62, 0.1);
}

.capture-button,
.cancel-capture-button {
  width: 40px;
  height: 40px;
  border: 2px solid #e2e8f0;
  border-radius: 8px;
  background: white;
  color: #4a5568;
  font-size: 1rem;
  cursor: pointer;
  transition: all 0.3s ease;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
}

.capture-button:hover {
  background: #667eea;
  border-color: #667eea;
  color: white;
  transform: translateY(-1px);
  box-shadow: 0 4px 12px rgba(102, 126, 234, 0.3);
}

.cancel-capture-button {
  background: #fed7d7;
  border-color: #feb2b2;
  color: #c53030;
}

.cancel-capture-button:hover {
  background: #fbb6b6;
  border-color: #f56565;
  color: #9b2c2c;
  transform: translateY(-1px);
  box-shadow: 0 4px 12px rgba(245, 101, 101, 0.3);
}

.shortcut-error {
  margin-top: 0.5rem;
  padding: 0.5rem;
  background: #fed7d7;
  border: 1px solid #feb2b2;
  border-radius: 6px;
  color: #c53030;
  font-size: 0.8rem;
  line-height: 1.4;
}

/* 日志面板样式 */
.log-panel {
  margin-top: 1rem;
  border: 2px solid #e2e8f0;
  border-radius: 12px;
  background: white;
  overflow: hidden;
  transition: all 0.3s ease;
  flex-shrink: 0;
  min-height: 0;
}

.log-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.75rem 1rem;
  background: #f8fafc;
  border-bottom: 1px solid #e2e8f0;
  cursor: pointer;
  transition: all 0.3s ease;
}

.log-header:hover {
  background: #edf2f7;
}

.log-title {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-weight: 600;
  color: #2d3748;
  font-size: 0.9rem;
}

.log-icon {
  font-size: 1rem;
  opacity: 0.8;
}

.log-controls {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.log-clear-button {
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: #718096;
  font-size: 0.9rem;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.2s ease;
}

.log-clear-button:hover {
  background: #e2e8f0;
  color: #e53e3e;
}

.log-toggle-button {
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: #718096;
  font-size: 0.8rem;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.2s ease;
}

.log-toggle-button:hover {
  background: #e2e8f0;
  color: #4a5568;
}

.log-toggle-button.expanded {
  transform: rotate(0deg);
}

.log-content {
  height: 150px;
  min-height: 100px;
  max-height: 40vh;
  overflow-y: auto;
  background: #fafafa;
  resize: vertical;
}

@media (max-height: 600px) {
  .log-content {
    height: 100px;
    max-height: 25vh;
  }
}

@media (max-height: 400px) {
  .log-content {
    height: 80px;
    max-height: 20vh;
  }
}

.log-empty {
  padding: 2rem;
  text-align: center;
  color: #a0aec0;
  font-size: 0.85rem;
  font-style: italic;
}

.log-entries {
  padding: 0.5rem;
}

.log-entry {
  display: flex;
  align-items: flex-start;
  gap: 0.3rem;
  padding: 0.2rem 0.4rem;
  margin-bottom: 0.15rem;
  border-radius: 4px;
  font-size: 0.7rem;
  font-family: 'Consolas', 'Monaco', 'Courier New', monospace;
  border-left: 3px solid transparent;
  transition: all 0.2s ease;
  line-height: 1.2;
  word-wrap: break-word;
  overflow-wrap: break-word;
}

.log-entry:hover {
  background: rgba(0, 0, 0, 0.05);
}

.log-timestamp {
  color: #718096;
  white-space: nowrap;
  font-weight: 500;
  flex-shrink: 0;
  font-size: 0.65rem;
}

.log-level {
  font-weight: 600;
  white-space: nowrap;
  min-width: 45px;
  flex-shrink: 0;
  font-size: 0.65rem;
}

.log-message {
  flex: 1;
  word-break: break-word;
  overflow-wrap: break-word;
  line-height: 1.3;
  min-width: 0;
  hyphens: auto;
}

@media (max-width: 400px) {
  .log-entry {
    flex-direction: column;
    gap: 0.1rem;
    font-size: 0.65rem;
  }
  
  .log-timestamp,
  .log-level {
    font-size: 0.6rem;
    min-width: auto;
  }
  
  .log-message {
    margin-top: 0.1rem;
    margin-left: 0;
  }
}

.log-error {
  border-left-color: #e53e3e;
  background: rgba(229, 62, 62, 0.05);
}

.log-error .log-level {
  color: #e53e3e;
}

.log-warn {
  border-left-color: #ed8936;
  background: rgba(237, 137, 54, 0.05);
}

.log-warn .log-level {
  color: #ed8936;
}

.log-info {
  border-left-color: #3182ce;
  background: rgba(49, 130, 206, 0.05);
}

.log-info .log-level {
  color: #3182ce;
}

.log-debug {
  border-left-color: #805ad5;
  background: rgba(128, 90, 213, 0.05);
}

.log-debug .log-level {
  color: #805ad5;
}

.log-default {
  border-left-color: #718096;
  background: rgba(113, 128, 150, 0.05);
}

.log-default .log-level {
  color: #718096;
}

/* 复选框样式 */
.checkbox-group {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.checkbox-item {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  cursor: pointer;
  transition: all 0.2s ease;
}

.checkbox-item:hover {
  color: #4a5568;
}

.checkbox-input {
  width: 18px;
  height: 18px;
  border: 2px solid #e2e8f0;
  border-radius: 4px;
  background: white;
  cursor: pointer;
  transition: all 0.3s ease;
  position: relative;
  flex-shrink: 0;
}

.checkbox-input:checked {
  background: #667eea;
  border-color: #667eea;
}

.checkbox-input:checked::after {
  content: '✓';
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  color: white;
  font-size: 12px;
  font-weight: bold;
}

.checkbox-input:hover {
  border-color: #667eea;
}

.checkbox-label {
  font-size: 0.9rem;
  color: #2d3748;
  user-select: none;
}
</style>
<style>
:root {
  font-family: 'Inter', 'SF Pro Display', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  font-size: 16px;
  line-height: 1.6;
  font-weight: 400;
  
  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  -webkit-text-size-adjust: 100%;
}


body {
  margin: 0;
  padding: 0;
  min-height: 100vh;
  overflow-x: hidden;
}

#app {
  min-height: 100vh;
}

/* 移除默认的Pico CSS样式覆盖 */
.app-container *,
* {
  box-sizing: border-box;
}

/* 确保自定义样式优先 */
.app-container input,
.app-container button,
.app-container select {
  all: unset;
}

/* 滚动条样式 */
::-webkit-scrollbar {
  width: 8px;
}

::-webkit-scrollbar-track {
  background: rgba(255, 255, 255, 0.1);
  border-radius: 4px;
}

::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.3);
  border-radius: 4px;
}

::-webkit-scrollbar-thumb:hover {
  background: rgba(255, 255, 255, 0.5);
}
</style>