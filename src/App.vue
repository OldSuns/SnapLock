<script setup lang="ts">
import { ref, onMounted, computed, watch, nextTick, onBeforeUnmount } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { desktopDir } from "@tauri-apps/api/path";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface CameraInfo { id: number; name: string; }

const cameraList = ref<CameraInfo[]>([]);
const selectedCameraId = ref<number>(0);
const monitoringStatus = ref<string>("空闲"); // '空闲' | '准备中' | '警戒中'
const savePath = ref<string>("");
const exitOnLock = ref(true);
const showSettings = ref(false);
const currentShortcut = ref("Alt+L");
const tempShortcut = ref("Alt+L");
const tempSavePath = ref("");
const isCapturingShortcut = ref(false);

// 预览
const previewActive = ref(false);
const previewError = ref<string>("");
const videoEl = ref<HTMLVideoElement | null>(null);
let previewStream: MediaStream | null = null;

async function ensureCameraPermission(): Promise<void> {
  const devs = await navigator.mediaDevices.enumerateDevices();
  const hasLabel = devs.some(d => d.kind === "videoinput" && d.label);
  if (hasLabel) return;
  const tmp = await navigator.mediaDevices.getUserMedia({ video: true, audio: false });
  tmp.getTracks().forEach(t => t.stop());
}

async function pickDeviceIdBySelection(): Promise<string | null> {
  await ensureCameraPermission();
  const cam = cameraList.value.find(c => c.id === selectedCameraId.value);
  const devices = (await navigator.mediaDevices.enumerateDevices())
    .filter(d => d.kind === "videoinput");
  if (devices.length === 0) return null;
  if (cam?.name) {
    const byName = devices.find(d => (d.label || "").toLowerCase().includes(cam.name.toLowerCase().trim()));
    if (byName) return byName.deviceId;
  }
  const idx = Number.isInteger(selectedCameraId.value) ? selectedCameraId.value : 0;
  return (devices[idx] ?? devices[0]).deviceId;
}

async function startPreview() {
  previewError.value = "";
  previewActive.value = false;
  stopPreview();
  if (monitoringStatus.value !== "空闲") return;
  try {
    const deviceId = await pickDeviceIdBySelection();
    if (!deviceId) { previewError.value = "未找到可用摄像头"; return; }
    const stream = await navigator.mediaDevices.getUserMedia({
      video: { deviceId: { exact: deviceId }, width: { ideal: 1280 }, height: { ideal: 720 } },
      audio: false
    });
    previewStream = stream;
    await nextTick();
    if (videoEl.value) {
      videoEl.value.srcObject = stream;
      await videoEl.value.play().catch(() => { });
    }
    previewActive.value = true;
  } catch (e: any) {
    previewError.value = "无法打开摄像头预览：" + (e?.message || e);
    stopPreview();
  }
}

function stopPreview() {
  previewActive.value = false;
  if (previewStream) {
    previewStream.getTracks().forEach(t => t.stop());
    previewStream = null;
  }
  if (videoEl.value) videoEl.value.srcObject = null;
}

watch(selectedCameraId, () => { if (previewActive.value) startPreview(); });
watch(monitoringStatus, (v) => { if (v !== "空闲" && previewActive.value) stopPreview(); });
onBeforeUnmount(() => stopPreview());

const statusClass = computed(() => {
  switch (monitoringStatus.value) {
    case "警戒中": return "status-active";
    case "准备中": return "status-pending";
    default: return "status-idle";
  }
});

watch(selectedCameraId, async (newId) => {
  if (cameraList.value.length > 0) {
    try { await invoke("set_camera_id", { cameraId: newId }); } catch { }
  }
});
watch(exitOnLock, async (newVal) => { try { await invoke("set_exit_on_lock", { exit: newVal }); } catch { } });

onMounted(async () => {
  cameraList.value = await invoke<CameraInfo[]>("get_camera_list");
  if (cameraList.value.length > 0) selectedCameraId.value = cameraList.value[0].id;

  const desktop = await desktopDir();
  savePath.value = desktop;
  tempSavePath.value = desktop;
  await invoke("set_save_path", { path: desktop });

  try {
    currentShortcut.value = await invoke<string>("get_shortcut_key");
    tempShortcut.value = currentShortcut.value;
  } catch { }

  listen<string>("monitoring_status_changed", (e) => {
    monitoringStatus.value = e.payload;
  });

  const appWin = getCurrentWindow();
  const unlistenClose = await appWin.onCloseRequested(async () => { await stopPreview(); });
  onBeforeUnmount(async () => { await unlistenClose?.(); });
});

async function toggleMonitoring() {
  if (monitoringStatus.value === "空闲") {
    try { await invoke("start_monitoring_command", { cameraId: selectedCameraId.value }); } catch { }
  }
}

function openSettings() {
  tempShortcut.value = currentShortcut.value;
  tempSavePath.value = savePath.value;
  showSettings.value = true;
}
async function closeSettings() {
  if (isCapturingShortcut.value) await cancelCaptureShortcut();
  showSettings.value = false;
}

// Esc 关闭设置（正在捕获快捷键时，closeSettings 会自动取消并恢复全局快捷键）
const onEscKey = (e: KeyboardEvent) => {
  if (e.key === "Escape" && showSettings.value) {
    e.preventDefault();
    e.stopPropagation();
    closeSettings();
  }
};

// 打开设置时挂载监听，关闭时移除
watch(showSettings, (open) => {
  if (open) {
    document.addEventListener("keydown", onEscKey);
  } else {
    document.removeEventListener("keydown", onEscKey);
  }
});

// 组件卸载时兜底清理
onBeforeUnmount(() => {
  document.removeEventListener("keydown", onEscKey);
});


async function selectSavePath() {
  const selected = await open({ directory: true, multiple: false, defaultPath: savePath.value || tempSavePath.value, title: "选择照片保存位置" });
  if (typeof selected === "string" && selected) {
    try {
      await invoke("set_save_path", { path: selected });
      savePath.value = selected;
      tempSavePath.value = selected;
    } catch (e) { alert(`保存路径设置失败: ${e}`); }
  }
}
async function selectSavePathInSettings() {
  const selected = await open({ directory: true, multiple: false, defaultPath: tempSavePath.value, title: "选择照片保存位置" });
  if (typeof selected === "string" && selected) { tempSavePath.value = selected; await savePathSetting(); }
}

async function saveShortcut() {
  try {
    if (tempShortcut.value !== currentShortcut.value && validateShortcut(tempShortcut.value)) {
      await invoke("set_shortcut_key", { shortcut: tempShortcut.value });
      currentShortcut.value = tempShortcut.value;
    }
  } catch (e) { alert(`快捷键保存失败: ${e}`); tempShortcut.value = currentShortcut.value; }
}
async function savePathSetting() {
  try {
    if (tempSavePath.value !== savePath.value) {
      await invoke("set_save_path", { path: tempSavePath.value });
      savePath.value = tempSavePath.value;
    }
  } catch (e) { alert(`保存路径设置失败: ${e}`); tempSavePath.value = savePath.value; }
}

async function startCaptureShortcut() {
  isCapturingShortcut.value = true;
  tempShortcut.value = "按下快捷键...";
  try { await invoke("disable_shortcuts"); } catch { }
  nextTick(() => (document.querySelector(".shortcut-input") as HTMLInputElement | null)?.focus());
}
async function handleShortcutKeyDown(event: KeyboardEvent) {
  if (!isCapturingShortcut.value) return;
  event.preventDefault(); event.stopPropagation();
  const keys: string[] = [];
  if (event.ctrlKey) keys.push("Ctrl");
  if (event.altKey) keys.push("Alt");
  if (event.shiftKey) keys.push("Shift");
  if (event.metaKey) keys.push("Meta");
  if (!["Control", "Alt", "Shift", "Meta"].includes(event.key)) {
    let mainKey = event.key;
    if (mainKey === " ") mainKey = "Space";
    else if (mainKey.length === 1) mainKey = mainKey.toUpperCase();
    keys.push(mainKey);
    if (keys.length >= 2) {
      tempShortcut.value = keys.join("+");
      isCapturingShortcut.value = false;
      try { await invoke("enable_shortcuts"); } catch { }
      await saveShortcut();
    }
  }
}
async function cancelCaptureShortcut() {
  isCapturingShortcut.value = false;
  tempShortcut.value = currentShortcut.value;
  try { await invoke("enable_shortcuts"); } catch { }
}
function validateShortcut(shortcut: string): boolean {
  if (!shortcut || shortcut === "按下快捷键...") return false;
  const parts = shortcut.split("+"); if (parts.length < 2) return false;
  const modifiers = parts.slice(0, -1); const mainKey = parts.at(-1)!;
  const validModifiers = ["Ctrl", "Alt", "Shift", "Meta", "Cmd"];
  if (validModifiers.includes(mainKey)) return false;
  return modifiers.every(m => validModifiers.includes(m));
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
              <option v-for="cam in cameraList" :key="cam.id" :value="cam.id">{{ cam.name }}</option>
            </select>
          </div>

          <!-- 预览 -->
          <section class="preview-section">
            <label class="control-label">摄像头预览</label>
            <div style="display:flex; gap:10px; align-items:center; margin-bottom:8px;">
              <button @click="previewActive ? stopPreview() : startPreview()"
                :disabled="monitoringStatus !== '空闲' && !previewActive">
                {{ previewActive ? '停止预览' : '开启预览' }}
              </button>
              <small v-if="monitoringStatus !== '空闲' && !previewActive" style="opacity:.7;">
                监控进行中，已暂停预览
              </small>
            </div>
            <div v-show="previewActive" class="preview-card">
              <video ref="videoEl" autoplay playsinline muted class="preview-video"></video>
            </div>
            <p v-if="previewError" class="error-text">{{ previewError }}</p>
          </section>

          <!-- 保存路径 -->
          <section>
            <label for="savePathInput" class="control-label">照片保存路径</label>
            <div class="path-input-group">
              <input id="savePathInput" class="path-input" :value="savePath" readonly>
              <button @click="selectSavePath" class="path-select-button">…</button>
            </div>

            <div style="margin-top:12px;">
              <label>
                <input type="checkbox" v-model="exitOnLock" role="switch">
                锁屏后退出程序
              </label>
            </div>

            <div class="action-buttons" style="margin-top:12px;">
              <button @click="toggleMonitoring" :disabled="monitoringStatus !== '空闲'" class="main-action-button"
                :class="{ 'disabled': monitoringStatus !== '空闲' }">
                <span class="button-icon">{{ monitoringStatus === '空闲' ? '▶️' : (monitoringStatus === '准备中' ? '⏳' :
                  '🛡️') }}</span>
                <span class="button-text">
                  {{ monitoringStatus === '空闲' ? '启动监控' : (monitoringStatus === '准备中' ? '准备中...' : `警戒中
                  (${currentShortcut} 停止)`) }}
                </span>
              </button>
              <button @click="openSettings" class="settings-button" title="设置">⚙️</button>
            </div>
          </section>
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
            <label class="setting-label"><span class="setting-icon">⌨️</span>快捷键</label>
            <div class="shortcut-input-group">
              <input v-model="tempShortcut" type="text" class="setting-input shortcut-input"
                :class="{ 'capturing': isCapturingShortcut, 'invalid': !validateShortcut(tempShortcut) && tempShortcut !== '按下快捷键...' }"
                placeholder="例如: Alt+L, Ctrl+Shift+S" readonly @keydown="handleShortcutKeyDown" />
              <button v-if="!isCapturingShortcut" @click="startCaptureShortcut" class="capture-button"
                title="点击捕获快捷键">🎯</button>
              <button v-else @click="cancelCaptureShortcut" class="cancel-capture-button" title="取消捕获">✕</button>
            </div>
            <div v-if="!validateShortcut(tempShortcut) && tempShortcut !== '按下快捷键...' && tempShortcut"
              class="shortcut-error">
              请输入有效的快捷键组合（至少包含一个修饰键和一个主键）
            </div>
          </div>

          <div class="setting-item">
            <label class="setting-label"><span class="setting-icon">📁</span>照片保存路径</label>
            <div class="path-input-group">
              <input type="text" :value="tempSavePath" readonly class="setting-input path-display"
                :title="tempSavePath">
              <button @click="selectSavePathInSettings" class="path-select-button">📂</button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </main>
</template>

<style scoped>
/* 布局 */
.app-container {
  height: 100vh;
  background: #ffffff;
  display: flex;
  flex-direction: column;
  padding: 1rem;
  box-sizing: border-box;
  overflow-y: auto;
  /* 允许页面滚动 */
}

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

  0%,
  100% {
    transform: scale(1)
  }

  50% {
    transform: scale(1.05)
  }
}

.app-title h1 {
  margin: 0;
  color: #2d3748;
  font-size: 1.5rem;
  font-weight: 700;
}

.status-indicator {
  display: flex;
  align-items: center;
  gap: .5rem;
  padding: .5rem .75rem;
  border-radius: 20px;
  background: white;
  border: 1px solid #e2e8f0;
  box-shadow: 0 1px 3px rgba(0, 0, 0, .1);
}

.status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  animation: statusPulse 2s infinite;
}

@keyframes statusPulse {

  0%,
  100% {
    opacity: 1;
    transform: scale(1)
  }

  50% {
    opacity: .7;
    transform: scale(1.1)
  }
}

.status-text {
  color: #4a5568;
  font-weight: 600;
  font-size: .8rem;
}

.status-active .status-dot {
  background: #4caf50;
  box-shadow: 0 0 6px rgba(76, 175, 80, .6);
}

.status-pending .status-dot {
  background: #ff9800;
  box-shadow: 0 0 6px rgba(255, 152, 0, .6);
}

.status-idle .status-dot {
  background: #9e9e9e;
  box-shadow: 0 0 6px rgba(158, 158, 158, .6);
}

.app-content {
  flex: 1;
  display: flex;
  justify-content: center;
  align-items: stretch;
  overflow-y: auto;
}

.control-card {
  background: #fff;
  border-radius: 16px;
  padding: 1rem;
  width: 100%;
  max-width: 100%;
  box-shadow: 0 4px 12px rgba(0, 0, 0, .05);
  border: 1px solid #e2e8f0;
  transition: transform .2s;
  display: flex;
  flex-direction: column;
  overflow: visible;
}

.control-card:hover {
  transform: translateY(-2px);
  box-shadow: 0 8px 16px rgba(0, 0, 0, .1);
}

.control-section {
  margin-bottom: 1.25rem;
}

.control-label {
  display: flex;
  align-items: center;
  gap: .5rem;
  margin-bottom: .5rem;
  font-weight: 600;
  color: #2c3e50;
  font-size: .9rem;
}

.label-icon {
  font-size: 1rem;
  opacity: .8;
}

/* 选择框 */
.select-wrapper {
  position: relative;
}

.custom-select {
  width: 100%;
  padding: .6rem 2rem .6rem .8rem;
  border: 2px solid #e1e8ed;
  border-radius: 8px;
  background: white;
  font-size: .85rem;
  color: #2c3e50;
  cursor: pointer;
  transition: all .3s ease;
  appearance: none;
  background-image: url("data:image/svg+xml,%3csvg xmlns='http://www.w3.org/2000/svg' fill='none' viewBox='0 0 20 20'%3e%3cpath stroke='%236b7280' stroke-linecap='round' stroke-linejoin='round' stroke-width='1.5' d='M6 8l4 4 4-4'/%3e%3c/svg%3e");
  background-repeat: no-repeat;
  background-position: right .6rem center;
  background-size: .7rem;
}

.custom-select:focus {
  outline: none;
  border-color: #667eea;
  box-shadow: 0 0 0 2px rgba(102, 126, 234, .1);
}

.custom-select:hover {
  border-color: #667eea;
}

/* 预览块 */
.preview-card {
  border: 1px solid #ddd;
  border-radius: 12px;
  overflow: hidden;
  background: #000;
}

.preview-video {
  width: 100%;
  max-height: 320px;
  display: block;
  object-fit: cover;
}

.error-text {
  color: #d32f2f;
  margin-top: 6px;
}

/* 路径输入组 & 按钮 */
.path-input-group {
  display: flex;
  gap: .5rem;
  align-items: center;
}

.path-input {
  flex: 1;
  padding: .6rem .8rem;
  border: 2px solid #e1e8ed;
  border-radius: 8px;
  background: #f8fafc;
  font-size: .85rem;
  color: #2c3e50;
  transition: all .3s;
  min-width: 0;
}

.path-input:focus {
  outline: none;
  border-color: #667eea;
  box-shadow: 0 0 0 2px rgba(102, 126, 234, .1);
}

.path-select-button {
  width: 40px;
  height: 40px;
  border: 2px solid #e2e8f0;
  border-radius: 8px;
  background: white;
  color: #4a5568;
  font-size: 1.1rem;
  cursor: pointer;
  transition: all .3s;
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 1px 3px rgba(0, 0, 0, .1);
  flex-shrink: 0;
}

.path-select-button:hover {
  background: #f7fafc;
  border-color: #cbd5e0;
  transform: translateY(-1px);
}

/* 主操作按钮 */
.action-buttons {
  display: flex;
  gap: .5rem;
  align-items: center;
}

.main-action-button {
  flex: 1;
  padding: .9rem 1rem;
  border: 2px solid #e2e8f0;
  border-radius: 12px;
  background: white;
  color: #2d3748;
  font-size: .9rem;
  font-weight: 600;
  cursor: pointer;
  transition: all .3s;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: .4rem;
  box-shadow: 0 2px 8px rgba(0, 0, 0, .1);
  min-height: 45px;
}

.main-action-button:hover {
  transform: translateY(-1px);
  background: #f7fafc;
  border-color: #cbd5e0;
  box-shadow: 0 4px 12px rgba(0, 0, 0, .15);
}

.main-action-button.disabled {
  background: #f7fafc;
  color: #a0aec0;
  border-color: #e2e8f0;
  cursor: not-allowed;
  box-shadow: 0 1px 3px rgba(0, 0, 0, .05);
}

.button-icon {
  font-size: 1.1rem;
  color: #4a5568;
}

.button-text {
  font-size: .85rem;
  color: #2d3748;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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
  transition: all .3s;
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 2px 8px rgba(0, 0, 0, .1);
  flex-shrink: 0;
}

.settings-button:hover {
  background: #f7fafc;
  border-color: #cbd5e0;
  transform: translateY(-1px);
}

/* 设置弹窗 */
.settings-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, .5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
  backdrop-filter: blur(4px);
}

.settings-dialog {
  background: white;
  border-radius: 16px;
  box-shadow: 0 20px 40px rgba(0, 0, 0, .15);
  width: 90%;
  max-width: 400px;
  max-height: 85vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.settings-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: .75rem 1rem;
  border-bottom: 1px solid #e2e8f0;
  background: #f8fafc;
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
}

.close-button:hover {
  background: #e2e8f0;
  color: #4a5568;
}

.settings-content {
  padding: 1.5rem;
  flex: 1;
  overflow-y: auto;
}

/* 弹窗内部可滚动 */
.setting-item {
  margin-bottom: 1.5rem;
}

.setting-label {
  display: flex;
  align-items: center;
  gap: .5rem;
  margin-bottom: .75rem;
  font-weight: 600;
  color: #2d3748;
  font-size: .9rem;
}

.setting-icon {
  font-size: 1rem;
  opacity: .8;
}

.setting-input {
  width: 100%;
  padding: .75rem;
  border: 2px solid #e2e8f0;
  border-radius: 8px;
  background: white;
  font-size: .9rem;
  color: #2d3748;
  transition: all .3s;
}

.setting-input:focus {
  outline: none;
  border-color: #667eea;
  box-shadow: 0 0 0 3px rgba(102, 126, 234, .1);
}

.setting-input.path-display {
  background: #f8fafc;
  font-size: .85rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.shortcut-input-group {
  display: flex;
  gap: .5rem;
  align-items: center;
}

.shortcut-input.capturing {
  border-color: #667eea;
  box-shadow: 0 0 0 3px rgba(102, 126, 234, .1);
  background: #f0f4ff;
}

.shortcut-input.invalid {
  border-color: #e53e3e;
  box-shadow: 0 0 0 3px rgba(229, 62, 62, .1);
}

.capture-button,
.cancel-capture-button,
.path-select-button {
  width: 40px;
  height: 40px;
  border: 2px solid #e2e8f0;
  border-radius: 8px;
  background: white;
  color: #4a5568;
  font-size: 1rem;
  cursor: pointer;
  transition: all .3s;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  box-shadow: 0 1px 3px rgba(0, 0, 0, .1);
}

.capture-button:hover {
  background: #667eea;
  border-color: #667eea;
  color: white;
  transform: translateY(-1px);
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
}

.shortcut-error {
  margin-top: .5rem;
  padding: .5rem;
  background: #fed7d7;
  border: 1px solid #feb2b2;
  border-radius: 6px;
  color: #c53030;
  font-size: .8rem;
  line-height: 1.4;
}


/* 允许页面滚动：原来 overflow: hidden 会挡滚动 */
.app-container {
  overflow-y: auto;
}

/* ===== Dark Mode（放在 scoped 里，能覆盖浅色）===== */
@media (prefers-color-scheme: dark) {

  /* 背景 & 文本 */
  .app-container {
    background: #0f1720;
    color: #e5e7eb;
  }

  .app-title h1,
  .control-label,
  .status-text {
    color: #e5e7eb;
  }

  .settings-header h2 {
    color: #e5e7eb !important; /* 提高优先级，确保覆盖 */
  }

  /* 顶部栏 */
  .app-header {
    background: #111827;
    border-color: #1f2937;
    box-shadow: 0 2px 10px rgba(0, 0, 0, .45);
  }

  /* 状态胶囊 */
  .status-indicator {
    background: #0f1720;
    border-color: #1f2937;
    box-shadow: 0 1px 3px rgba(0, 0, 0, .4);
  }

  .status-idle .status-dot {
    background: #6b7280;
    box-shadow: 0 0 6px rgba(107, 114, 128, .6);
  }

  /* 主卡片 */
  .control-card {
    background: #111827;
    border-color: #1f2937;
    box-shadow: 0 16px 40px rgba(0, 0, 0, .55);
  }

  /* 下拉选择：注意 no-repeat 防止重复箭头 */
  .custom-select {
    background: #0f1720;
    color: #e5e7eb;
    border-color: #1f2937;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, .06), 0 1px 3px rgba(0, 0, 0, .3);
    background-image: url("data:image/svg+xml,%3csvg xmlns='http://www.w3.org/2000/svg' fill='none' viewBox='0 0 20 20'%3e%3cpath stroke='%239ca3af' stroke-linecap='round' stroke-linejoin='round' stroke-width='1.5' d='M6 8l4 4 4-4'/%3e%3c/svg%3e");
    background-position: right 0.6rem center;
    background-size: 0.7rem;
    background-repeat: no-repeat;
    /* 关键 */
  }

  .custom-select:hover {
    border-color: #334155;
  }

  .custom-select:focus {
    border-color: #88a8ff;
    box-shadow: 0 0 0 3px rgba(136, 168, 255, .18);
  }

  /* 输入框（主页面 & 设置弹窗） */
  .path-input,
  #savePathInput,
  .setting-input {
    background: #0f1720;
    color: #e5e7eb;
    border-color: #1f2937;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, .06);
  }

  .path-input:focus,
  #savePathInput:focus,
  .setting-input:focus {
    border-color: #88a8ff;
    box-shadow: 0 0 0 3px rgba(136, 168, 255, .18);
  }

  .setting-input.path-display {
    background: #0b111a;
  }

  /* 按钮 */
  button {
    /* 普通按钮（如“开启预览”、“…”） */
    background: #7aa2ff;
    color: #0b1020;
    border-color: #1f2937;
    box-shadow: 0 2px 8px rgba(0, 0, 0, .5);
  }

  button:hover {
    background: #88a8ff;
    box-shadow: 0 8px 24px rgba(0, 0, 0, .55);
  }

  .main-action-button {
    /* 大的“启动监控”卡片按钮 */
    background: #0f1720;
    color: #e5e7eb;
    border-color: #1f2937;
    box-shadow: 0 16px 40px rgba(0, 0, 0, .55);
  }

  .main-action-button:hover {
    background: #121a25;
    border-color: #2a3442;
    box-shadow: 0 24px 50px rgba(0, 0, 0, .6);
  }

  .main-action-button.disabled {
    background: #0f1720;
    color: #71717a;
    border-color: #1f2937;
    box-shadow: 0 2px 8px rgba(0, 0, 0, .35);
  }

  .button-icon {
    color: #88a8ff;
  }

  /* 小圆角功能按钮（设置齿轮、文件夹、…） */
  .settings-button,
  .path-select-button,
  .path-button {
    background: #0f1720;
    color: #cbd5e1;
    border-color: #1f2937;
    box-shadow: 0 2px 8px rgba(0, 0, 0, .45);
  }

  .settings-button:hover,
  .path-select-button:hover,
  .path-button:hover {
    background: rgba(99, 102, 241, .12);
    color: #a5b4fc;
    border-color: rgba(99, 102, 241, .35);
  }

  /* 预览视频容器 */
  .preview-card,
  [style*="background:#000"] {
    background: #000;
    border-color: #1f2937;
    box-shadow: 0 2px 10px rgba(0, 0, 0, .45);
  }

  /* 设置弹窗 */
  .settings-overlay {
    background: rgba(0, 0, 0, .55);
  }

  .settings-dialog {
    background: #111827;
    color: #e5e7eb;
    border: 1px solid #1f2937;
  }

  .settings-header {
    background: #0f1720;
    border-bottom: 1px solid #1f2937;
  }

  .close-button {
    color: #cbd5e1;
  }

  .close-button:hover {
    background: #1f2937;
    color: #e5e7eb;
  }

  /* 快捷键捕获状态 */
  .shortcut-input.capturing {
    border-color: #88a8ff;
    box-shadow: 0 0 0 3px rgba(136, 168, 255, .18);
    background: #0b111a;
  }

  .shortcut-input.invalid {
    border-color: #ef4444;
    box-shadow: 0 0 0 3px rgba(239, 68, 68, .18);
  }

  .shortcut-error {
    background: rgba(239, 68, 68, .12);
    border-color: rgba(239, 68, 68, .35);
    color: #f87171;
  }

  /* 文本颜色修正：按钮主文字 & 设置面板标签 */
  .button-text {
    color: #e5e7eb;
  }

  .setting-label {
    color: #e5e7eb;
  }

  /* 主按钮禁用时的文字颜色（暗色） */
  .main-action-button.disabled .button-text {
    color: #71717a;
  }

  /* 下拉选择（暗色）— 修复箭头被重复铺满的问题 */
  .custom-select {
    background: #0f1720;
    color: #e5e7eb;
    border-color: #1f2937;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, .06), 0 1px 3px rgba(0, 0, 0, .3);
    background-image: url("data:image/svg+xml,%3csvg xmlns='http://www.w3.org/2000/svg' fill='none' viewBox='0 0 20 20'%3e%3cpath stroke='%239ca3af' stroke-linecap='round' stroke-linejoin='round' stroke-width='1.5' d='M6 8l4 4 4-4'/%3e%3c/svg%3e");
    background-repeat: no-repeat;
    /* 关键：不要平铺 */
    background-position: right 0.6rem center;
    /* 关键：位置 */
    background-size: 0.7rem;
    /* 关键：尺寸 */
  }

  /* 设置对话框内：捕获/选择按钮（原本发白） */
  .capture-button,
  .cancel-capture-button,
  .path-select-button {
    background: #0f1720;
    color: #cbd5e1;
    border: 2px solid #1f2937;
    box-shadow: 0 2px 8px rgba(0, 0, 0, .45);
  }

  .capture-button:hover,
  .path-select-button:hover {
    background: rgba(99, 102, 241, .12);
    color: #a5b4fc;
    border-color: rgba(99, 102, 241, .35);
  }

  /* 取消捕获按钮做一点区分（微红） */
  .cancel-capture-button {
    background: #1f2937;
    border-color: #334155;
    color: #fca5a5;
  }

  .cancel-capture-button:hover {
    background: rgba(239, 68, 68, .18);
    border-color: #ef4444;
    color: #fecaca;
  }

  /* 输入框里的文字颜色（确保对话框内看得清） */
  .setting-input {
    color: #e5e7eb;
  }

  /* 额外：标签/状态在暗色里统一提亮（若还没加） */
  .control-label,
  .status-text,
  .app-title h1 {
    color: #e5e7eb;
  }

}
</style>
