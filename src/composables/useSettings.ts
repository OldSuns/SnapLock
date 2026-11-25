import { ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { AppConfig } from '../types';

export function useSettings() {
  const config = ref<AppConfig>({
    shortcut_key: 'Alt+L', save_path: null, show_debug_logs: false,
    save_logs_to_file: false, dark_mode: false, exit_on_lock: false,
    enable_notifications: true, post_trigger_action: 'CaptureAndLock',
    default_camera_id: null, capture_delay_seconds: 0, capture_mode: 'Video',
    capture_on_failed_unlock: false,
  });
  let pending: Promise<unknown> = Promise.resolve();
  watch(() => config.value.dark_mode, dark => {
    document.documentElement.classList.toggle('dark-mode', dark);
  }, { immediate: true });

  async function load() {
    config.value = await invoke<AppConfig>('get_config');
  }

  function update<K extends Exclude<keyof AppConfig, 'capture_mode'>>(key: K, value: AppConfig[K]) {
    const result = pending.then(async () => {
      config.value = await invoke<AppConfig>('update_config', { change: { key, value } });
    });
    // A failed update must not prevent later settings from being saved.
    pending = result.catch(() => {});
    return result;
  }
  return { config, load, update };
}

export type Settings = ReturnType<typeof useSettings>;
