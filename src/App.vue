<script setup lang="ts">
import { ref, onMounted, computed, watch } from "vue";
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
  await invoke("set_save_path", { path: desktop });

  // 监听状态变化
  listen<string>("monitoring_status_changed", (event) => {
    monitoringStatus.value = event.payload;
  });
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

async function selectSavePath() {
  const selected = await open({
    directory: true,
    multiple: false,
    defaultPath: savePath.value,
    title: "选择照片保存位置"
  });

  if (typeof selected === 'string' && selected !== null) {
    savePath.value = selected;
    try {
      await invoke("set_save_path", { path: selected });
    } catch (error) {
      console.error("Failed to set save path:", error);
      alert("设置保存路径失败");
    }
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
          <label for="savePathInput" class="control-label">
            <span class="label-icon">📁</span>
            照片保存路径
          </label>
          <div class="path-input-group">
            <input
              type="text"
              id="savePathInput"
              :value="savePath"
              readonly
              class="path-input"
              :title="savePath"
            >
            <button @click="selectSavePath" class="path-button" title="选择文件夹">
              📂
            </button>
          </div>
        </div>

        <div class="control-section">
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
              {{ monitoringStatus === '空闲' ? '启动监控' : (monitoringStatus === '准备中' ? '准备中...' : '警戒中 (Alt+L 停止)') }}
            </span>
          </button>
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