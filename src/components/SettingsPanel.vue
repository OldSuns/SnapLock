<script setup lang="ts">
import { ref, onMounted, onUnmounted, nextTick } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { enable, disable, isEnabled } from '@tauri-apps/plugin-autostart';
import { desktopDir } from '@tauri-apps/api/path';
import type { AppConfig } from '../types';
import type { Settings } from '../composables/useSettings';
import type { Camera } from '../composables/useCamera';
import { validateShortcut } from '../utils/helpers';
import CameraPreview from './CameraPreview.vue';

const props = defineProps<{ settings: Settings; camera: Camera }>();
const emit = defineEmits<{ close: [] }>();
const { config, update } = props.settings;
const { cameras, selected } = props.camera;
const error = ref('');
const busy = ref(false);
const autostartEnabled = ref<boolean | null>(null);
const formVersion = ref(0);
const capturing = ref(false);
const shortcut = ref(config.value.shortcut_key);
const shortcutInput = ref<HTMLInputElement>();
const content = ref<HTMLElement>();
const displayPath = ref(config.value.save_path ?? '');
const actions: { value: AppConfig['post_trigger_action']; label: string }[] = [
  { value: 'CaptureAndLock', label: '拍摄并锁屏' },
  { value: 'CaptureOnly', label: '只拍摄' },
  { value: 'ScreenRecording', label: '屏幕录制' },
];

async function save<K extends Exclude<keyof AppConfig, 'capture_mode'>>(key: K, value: AppConfig[K]) {
  busy.value = true;
  error.value = '';
  try {
    await update(key, value);
    if (key === 'default_camera_id' && config.value.default_camera_id !== null) selected.value = config.value.default_camera_id;
    return true;
  } catch (cause) {
    error.value = `设置保存失败：${cause}`;
    // Native radio groups change both controls; recreate from the saved config on failure.
    formVersion.value++;
    await nextTick();
    if (content.value) content.value.scrollTop = 0;
    return false;
  } finally { busy.value = false; }
}

async function selectPath() {
  try {
    const path = await open({ directory: true, multiple: false, defaultPath: displayPath.value, title: '选择保存位置' });
    if (typeof path === 'string' && await save('save_path', path)) displayPath.value = path;
  } catch (cause) { error.value = String(cause); }
}

async function saveAutostart(event: Event) {
  const input = event.target as HTMLInputElement;
  const enabled = input.checked;
  // Keep the checkbox at the confirmed system state until the operation succeeds.
  input.checked = autostartEnabled.value ?? false;
  if (busy.value || autostartEnabled.value === null) return;
  busy.value = true;
  error.value = '';
  try {
    if (enabled) await enable();
    else await disable();
    autostartEnabled.value = enabled;
  } catch (cause) { error.value = `开机自启动设置失败：${cause}`; }
  finally { busy.value = false; }
}

async function startCapture() {
  try {
    await invoke('set_shortcuts_disabled', { disabled: true });
    capturing.value = true;
    shortcut.value = '按下快捷键...';
    await nextTick();
    shortcutInput.value?.focus();
  } catch (cause) { error.value = String(cause); }
}

async function cancelCapture() {
  try {
    await invoke('set_shortcuts_disabled', { disabled: false });
    capturing.value = false;
    shortcut.value = config.value.shortcut_key;
    return true;
  } catch (cause) { error.value = String(cause); return false; }
}

async function captureKey(event: KeyboardEvent) {
  if (!capturing.value) return;
  event.preventDefault();
  event.stopPropagation();
  if (event.key === 'Escape') { await cancelCapture(); return; }
  if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return;
  const modifiers = [event.ctrlKey && 'Ctrl', event.altKey && 'Alt', event.shiftKey && 'Shift', event.metaKey && 'Meta'].filter(Boolean);
  const key = event.key === ' ' ? 'Space' : event.key.length === 1 ? event.key.toUpperCase() : event.key;
  const value = [...modifiers, key].join('+');
  if (!validateShortcut(value)) return;
  await save('shortcut_key', value);
  await cancelCapture();
}

async function close() { if (!busy.value && (!capturing.value || await cancelCapture())) emit('close'); }
function escape(event: KeyboardEvent) { if (event.key === 'Escape') { event.preventDefault(); void close(); } }
function saveDelay(event: Event) {
  const input = event.target as HTMLInputElement;
  const parsed = Number(input.value);
  const value = Number.isFinite(parsed) ? Math.min(60, Math.max(0, Math.trunc(parsed))) : config.value.capture_delay_seconds;
  input.value = String(value);
  void save('capture_delay_seconds', value);
}
onMounted(async () => {
  document.addEventListener('keydown', escape);
  try { autostartEnabled.value = await isEnabled(); }
  catch (cause) { error.value = `开机自启动状态读取失败：${cause}`; }
  if (!displayPath.value) {
    try { displayPath.value = await desktopDir(); }
    catch (cause) { error.value = String(cause); }
  }
  shortcutInput.value?.focus();
});
onUnmounted(() => {
  document.removeEventListener('keydown', escape);
  if (capturing.value) void invoke('set_shortcuts_disabled', { disabled: false }).catch(console.error);
});
</script>

<template>
  <div class="settings-overlay" @click.self="close">
    <div class="settings-dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title">
      <div class="settings-header">
        <h2 id="settings-title">设置</h2>
        <button @click="close" :disabled="busy" class="close-button" aria-label="关闭设置">✕</button>
      </div>
      <div ref="content" class="settings-content">
        <p v-if="error" class="form-error" role="alert">{{ error }}</p>
        <fieldset :key="formVersion" class="settings-fields" :disabled="busy">
          <div class="setting-item">
            <div class="setting-label"><span class="setting-icon">🚀</span>启动选项</div>
            <label class="checkbox-item"><input type="checkbox" :checked="autostartEnabled ?? false" :disabled="autostartEnabled === null" @change="saveAutostart" class="checkbox-input" /><span class="checkbox-label">开机自启动</span></label>
            <div class="setting-description">登录电脑后自动打开 SnapLock</div>
          </div>
          <div class="setting-item">
            <label for="shortcut" class="setting-label"><span class="setting-icon">⌨️</span>快捷键</label>
            <div class="shortcut-input-group">
              <input id="shortcut" ref="shortcutInput" :value="shortcut" readonly class="setting-input shortcut-input" :class="{ capturing }" @keydown="captureKey" />
              <button v-if="!capturing" @click="startCapture" class="capture-button" title="点击捕获快捷键">🎯</button>
              <button v-else @click="cancelCapture" class="cancel-capture-button" title="取消捕获">✕</button>
            </div>
          </div>
          <div class="setting-item">
            <label for="save-path" class="setting-label"><span class="setting-icon">📁</span>照片&日志保存路径</label>
            <div class="path-input-group">
              <input id="save-path" :value="displayPath" :title="displayPath" readonly class="setting-input path-display" />
              <button @click="selectPath" class="path-select-button" aria-label="选择保存路径">📂</button>
            </div>
          </div>
          <div class="setting-item">
            <div class="setting-label"><span class="setting-icon">🐛</span>调试选项</div>
            <div class="checkbox-group">
              <label class="checkbox-item"><input type="checkbox" :checked="config.show_debug_logs" @change="save('show_debug_logs', ($event.target as HTMLInputElement).checked)" class="checkbox-input" /><span class="checkbox-label">显示调试日志</span></label>
              <label class="checkbox-item"><input type="checkbox" :checked="config.save_logs_to_file" @change="save('save_logs_to_file', ($event.target as HTMLInputElement).checked)" class="checkbox-input" /><span class="checkbox-label">保存日志到文件</span></label>
            </div>
          </div>
          <div class="setting-item">
            <div class="setting-label"><span class="setting-icon">📷</span>相机设置</div>
            <div class="camera-settings">
              <div class="setting-sub-item">
                <label for="default-camera" class="setting-sub-label">默认摄像头</label>
                <select id="default-camera" :value="config.default_camera_id ?? ''" @change="save('default_camera_id', ($event.target as HTMLSelectElement).value === '' ? null : Number(($event.target as HTMLSelectElement).value))" class="custom-select">
                  <option value="">不设置默认摄像头</option>
                  <option v-for="camera in cameras" :key="camera.id" :value="camera.id">{{ camera.name }}</option>
                </select>
                <div class="setting-description">设置启动时自动选择的摄像头，留空则使用第一个可用摄像头</div>
              </div>
              <div class="setting-sub-item"><div class="setting-sub-label">相机预览</div><CameraPreview :camera="props.camera" /></div>
            </div>
          </div>
          <div class="setting-item">
            <div class="setting-label"><span class="setting-icon">🌙</span>外观主题</div>
            <label class="checkbox-item"><input type="checkbox" :checked="config.dark_mode" @change="save('dark_mode', ($event.target as HTMLInputElement).checked)" class="checkbox-input" /><span class="checkbox-label">暗色模式</span></label>
          </div>
          <div class="setting-item">
            <div class="setting-label"><span class="setting-icon">🔒</span>安全选项</div>
            <div class="security-controls">
              <div class="setting-label">触发后动作</div>
              <div class="radio-group">
                <label v-for="action in actions" :key="action.value" class="radio-item"><input type="radio" name="post-trigger-action" :value="action.value" :checked="config.post_trigger_action === action.value" @change="save('post_trigger_action', action.value)" class="radio-input" /><span class="radio-label">{{ action.label }}</span></label>
              </div>
              <label class="checkbox-item"><input type="checkbox" :checked="config.exit_on_lock" @change="save('exit_on_lock', ($event.target as HTMLInputElement).checked)" class="checkbox-input" /><span class="checkbox-label">电脑锁定时自动退出程序</span></label>
            </div>
          </div>
          <div class="setting-item">
            <label for="capture-delay" class="setting-label"><span class="setting-icon">⏱️</span>拍摄延时设置</label>
            <div class="capture-time-controls">
              <div class="delay-input-group"><span class="delay-unit">持续</span><input id="capture-delay" type="number" :value="config.capture_delay_seconds" @change="saveDelay" min="0" max="60" class="setting-input delay-input" /><span class="delay-unit">秒</span></div>
              <div class="setting-description">检测到行为后持续录制的时间，<br />范围0-60秒（0秒表示不录像直接拍照）</div>
            </div>
          </div>
          <div class="setting-item">
            <div class="setting-label"><span class="setting-icon">📢</span>通知选项</div>
            <label class="checkbox-item"><input type="checkbox" :checked="config.enable_notifications" @change="save('enable_notifications', ($event.target as HTMLInputElement).checked)" class="checkbox-input" /><span class="checkbox-label">启用系统通知</span></label>
            <div class="setting-description">启用后，监控过程中检测到行为时会显示系统通知</div>
          </div>
        </fieldset>
      </div>
    </div>
  </div>
</template>
