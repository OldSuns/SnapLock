<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useSettings } from './composables/useSettings';
import { useCamera } from './composables/useCamera';
import { useMonitoring } from './composables/useMonitoring';
import { getStatusIcon, getStatusText } from './utils/helpers';
import SettingsPanel from './components/SettingsPanel.vue';
import LogPanel from './components/LogPanel.vue';

const settings = useSettings();
const camera = useCamera();
const monitoring = useMonitoring();
const { config } = settings;
const { cameras, selected } = camera;
const { status, error, busy } = monitoring;
const showSettings = ref(false);
const ready = ref(false);
const statusClass = computed(() => status.value === '准备中' ? 'status-pending' : status.value === '空闲' ? 'status-idle' : 'status-active');
onMounted(async () => {
  try {
    await monitoring.load();
    await settings.load();
    await camera.load(config.value.default_camera_id);
  } catch (cause) { error.value = `初始化失败：${cause}`; }
  finally { ready.value = true; }
});
</script>

<template>
  <main class="app-container">
    <div class="app-header">
      <div class="app-title"><div class="app-icon">📷</div><h1>SnapLock</h1></div>
      <div class="status-indicator" :class="statusClass">
        <div class="status-dot"></div>
        <span class="status-text">{{ status === '锁定中' ? '警戒中' : status }}</span>
      </div>
    </div>
    <div class="app-content">
      <div class="control-card">
        <p v-if="error" class="monitoring-error" role="alert">{{ error }}</p>
        <div v-if="config.post_trigger_action !== 'ScreenRecording'" class="control-section">
          <label for="cameraSelect" class="control-label"><span class="label-icon">🎥</span>选择摄像头</label>
          <div class="select-wrapper"><select id="cameraSelect" v-model="selected" class="custom-select">
            <option v-for="item in cameras" :key="item.id" :value="item.id">{{ item.name }}</option>
          </select></div>
        </div>
        <div class="control-section">
          <div class="action-buttons">
            <button @click="monitoring.toggle(selected)" :disabled="!ready || busy" class="main-action-button">
              <span class="button-icon">{{ getStatusIcon(status) }}</span>
              <span class="button-text">{{ getStatusText(status, config.shortcut_key) }}</span>
            </button>
            <button @click="showSettings = true" :disabled="!ready" class="settings-button" title="设置">⚙️</button>
          </div>
        </div>
        <LogPanel v-if="config.show_debug_logs" />
      </div>
    </div>
    <SettingsPanel v-if="showSettings" :settings="settings" :camera="camera" @close="showSettings = false" />
  </main>
</template>
