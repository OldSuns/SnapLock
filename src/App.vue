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

      <section>
        <label for="savePathInput">照片保存路径</label>
        <div style="display: flex; align-items: center;">
          <input type="text" id="savePathInput" :value="savePath" readonly style="flex-grow: 1; margin-right: 10px;">
          <button @click="selectSavePath" style="width: auto; padding: 0.4em 0.8em;">...</button>
        </div>
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