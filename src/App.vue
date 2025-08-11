<script setup lang="ts">
import { ref, onMounted, computed, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from '@tauri-apps/plugin-dialog';
import { desktopDir } from '@tauri-apps/api/path';
import { getCurrentWindow } from '@tauri-apps/api/window';

interface CameraInfo {
  id: number;
  name: string;
}

const cameraList = ref<CameraInfo[]>([]);
const selectedCameraId = ref<number>(0);
const monitoringStatus = ref<string>("空闲"); // '空闲', '准备中', '警戒中'
const savePath = ref<string>("");
const exitOnLock = ref(true);

// 摄像头预览相关
import { onBeforeUnmount, nextTick } from "vue";
const previewActive = ref(false);
const previewError = ref<string>("");
const videoEl = ref<HTMLVideoElement | null>(null);
let previewStream: MediaStream | null = null;

// === 预览相关改动开始 ===
async function ensureCameraPermission(): Promise<void> {
  // 若还没授权，label 会是空串；临时开一下获取权限再关掉
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

  // 1) 名称模糊匹配（授权后才有 label）
  if (cam?.name) {
    const name = cam.name.toLowerCase().trim();
    const byName = devices.find(d => (d.label || "").toLowerCase().includes(name));
    if (byName) return byName.deviceId;
  }

  // 2) 索引回退：后端 id 即 index（越界则回第一个）
  const idx = Number.isInteger(selectedCameraId.value) ? selectedCameraId.value : 0;
  const byIndex = devices[idx] ?? devices[0];
  return byIndex.deviceId;
}

async function startPreview() {
  previewError.value = "";
  previewActive.value = false;
  stopPreview();

  // 监控非空闲时不预览，避免与后端占用冲突
  if (monitoringStatus.value !== "空闲") return;

  try {
    const deviceId = await pickDeviceIdBySelection();
    if (!deviceId) {
      previewError.value = "未找到可用摄像头";
      return;
    }
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
// === 预览相关改动结束 ===

function stopPreview() {
  previewActive.value = false;
  if (previewStream) {
    previewStream.getTracks().forEach(track => track.stop());
    previewStream = null;
  }
  if (videoEl.value) {
    videoEl.value.srcObject = null;
  }
}

watch(selectedCameraId, () => {
  if (previewActive.value) {
    startPreview();
  }
});

watch(monitoringStatus, (val) => {
  // 监控非空闲时自动关闭预览
  if (val !== "空闲" && previewActive.value) {
    stopPreview();
  }
});

onBeforeUnmount(() => {
  stopPreview();
});

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

watch(exitOnLock, async (newVal) => {
  try {
    await invoke("set_exit_on_lock", { exit: newVal });
  } catch (error) {
    console.error("Failed to set exit_on_lock:", error);
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

  // 监听窗口关闭：先停止预览，再允许关闭
  const appWin = getCurrentWindow();
  const unlistenClose = await appWin.onCloseRequested(async () => {
    await stopPreview();
    // 不调用 preventDefault：让窗口继续关闭
  });

  // 组件卸载时，清理监听
  onBeforeUnmount(async () => {
    await unlistenClose?.();
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
  <main class="container">
    <article>
      <hgroup>
        <h1>SnapLock</h1>
        <h2 :class="statusClass">{{ monitoringStatus }}</h2>
      </hgroup>


      <section>
        <label for="cameraSelect">选择摄像头</label>
        <select id="cameraSelect" v-model="selectedCameraId">
          <option v-for="cam in cameraList" :key="cam.id" :value="cam.id">
            {{ cam.name }}
          </option>
        </select>
      </section>


      <!-- 预览区块：NEW -->
      <section>
        <label>摄像头预览</label>
        <div style="display:flex; gap:10px; align-items:center; margin-bottom:8px;">
          <button @click="previewActive ? stopPreview() : startPreview()"
            :disabled="monitoringStatus !== '空闲' && !previewActive">
            {{ previewActive ? '停止预览' : '开启预览' }}
          </button>
          <small v-if="monitoringStatus !== '空闲' && !previewActive" style="opacity:.7;">
            监控进行中，已暂停预览
          </small>
        </div>

        <div v-show="previewActive" style="border:1px solid #ddd;border-radius:12px;overflow:hidden;background:#000;">
          <video ref="videoEl" autoplay playsinline muted
            style="width:100%;max-height:320px;display:block;object-fit:cover;"></video>
        </div>
        <p v-if="previewError" style="color:#d32f2f; margin-top:6px;">{{ previewError }}</p>
      </section>

      <section>
        <label for="savePathInput">照片保存路径</label>
        <div style="display: flex; align-items: center;">
          <input type="text" id="savePathInput" :value="savePath" readonly style="flex-grow: 1; margin-right: 10px;">
          <button @click="selectSavePath" style="width: auto; padding: 0.4em 0.8em;">...</button>
        </div>
      </section>

      <section>
        <label>
          <input type="checkbox" v-model="exitOnLock" role="switch">
          锁屏后退出程序
        </label>
      </section>

      <section>
        <button @click="toggleMonitoring" :disabled="monitoringStatus !== '空闲'">
          {{ monitoringStatus === '空闲' ? '启动监控' : (monitoringStatus === '准备中' ? '准备中...' : '警戒中 (Alt+L 停止)') }}
        </button>
      </section>
    </article>
  </main>
</template>

<style scoped>
.status-active {
  color: #1b5e20; /* 深绿色 */
}
.status-pending {
  color: #ff6f00; /* 橙色 */
}
.status-idle {
  color: #9e9e9e; /* 灰色 */
}

main.container {
  max-width: 600px;
  margin: auto;
  padding: 2rem;
}

hgroup h1 {
  margin-bottom: 0.2rem;
}

hgroup h2 {
  margin-top: 0;
}

section {
  margin-top: 1.5rem;
}

button {
  width: 100%;
}
</style>

<style scoped>
.logo.vite:hover {
  filter: drop-shadow(0 0 2em #747bff);
}

.logo.vue:hover {
  filter: drop-shadow(0 0 2em #249b73);
}

</style>
<style>
:root {
  font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
  font-size: 16px;
  line-height: 24px;
  font-weight: 400;

  color: #0f0f0f;
  background-color: #f6f6f6;

  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  -webkit-text-size-adjust: 100%;
}

.container {
  margin: 0;
  padding-top: 10vh;
  display: flex;
  flex-direction: column;
  justify-content: center;
  text-align: center;
}

.logo {
  height: 6em;
  padding: 1.5em;
  will-change: filter;
  transition: 0.75s;
}

.logo.tauri:hover {
  filter: drop-shadow(0 0 2em #24c8db);
}

.row {
  display: flex;
  justify-content: center;
}

a {
  font-weight: 500;
  color: #646cff;
  text-decoration: inherit;
}

a:hover {
  color: #535bf2;
}

h1 {
  text-align: center;
}

input,
button {
  border-radius: 8px;
  border: 1px solid transparent;
  padding: 0.6em 1.2em;
  font-size: 1em;
  font-weight: 500;
  font-family: inherit;
  color: #0f0f0f;
  background-color: #ffffff;
  transition: border-color 0.25s;
  box-shadow: 0 2px 2px rgba(0, 0, 0, 0.2);
}

button {
  cursor: pointer;
}

button:hover {
  border-color: #396cd8;
}
button:active {
  border-color: #396cd8;
  background-color: #e8e8e8;
}

input,
button {
  outline: none;
}

#greet-input {
  margin-right: 5px;
}

@media (prefers-color-scheme: dark) {
  :root {
    color: #f6f6f6;
    background-color: #2f2f2f;
  }

  a:hover {
    color: #24c8db;
  }

  input,
  button {
    color: #ffffff;
    background-color: #0f0f0f98;
  }
  button:active {
    background-color: #0f0f0f69;
  }
}

</style>