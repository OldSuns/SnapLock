<script setup lang="ts">
import { ref, onMounted, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const cameraList = ref<string[]>([]);
const selectedCamera = ref<number>(0);
const monitoringStatus = ref<string>("空闲"); // '空闲', '准备中', '警戒中'

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

onMounted(async () => {
  cameraList.value = await invoke<string[]>("get_camera_list");
  listen<string>("monitoring_status_changed", (event) => {
    monitoringStatus.value = event.payload;
  });
});

async function toggleMonitoring() {
  if (monitoringStatus.value === "空闲") {
    try {
      await invoke("start_monitoring_command", { cameraIndex: selectedCamera.value });
    } catch (err) {
      // 可以考虑在这里向用户显示一个错误通知
    }
  } else if (monitoringStatus.value === "警戒中") {
    // 停止监控的逻辑由后端的全局热键处理
    // 前端按钮仅用于启动
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
        <select id="cameraSelect" v-model="selectedCamera">
          <option v-for="(cam, index) in cameraList" :key="index" :value="index">
            {{ cam }}
          </option>
        </select>
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