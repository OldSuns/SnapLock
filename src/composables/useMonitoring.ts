import { ref, onScopeDispose } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { MonitoringStatus } from '../types';

export function useMonitoring() {
  const status = ref<MonitoringStatus>('空闲');
  const error = ref('');
  const busy = ref(false);
  const unlisteners: UnlistenFn[] = [];
  let disposed = false;
  onScopeDispose(() => { disposed = true; unlisteners.splice(0).forEach(unlisten => unlisten()); });

  async function load() {
    for (const [event, handler] of [
      ['monitoring_status_changed', (value: string) => {
        status.value = value as MonitoringStatus;
        if (value === '准备中') error.value = '';
      }],
      ['monitoring_error', (value: string) => { error.value = value; }],
    ] as const) {
      const unlisten = await listen<string>(event, message => handler(message.payload));
      if (disposed) unlisten(); else unlisteners.push(unlisten);
    }
  }

  async function toggle(cameraId: number) {
    if (busy.value) return;
    busy.value = true;
    try {
      if (status.value === '空闲') {
        error.value = '';
        await invoke('start_monitoring_command', { cameraId });
      } else await invoke('stop_monitoring_command');
    } catch (cause) { error.value = String(cause); }
    finally { busy.value = false; }
  }
  return { status, error, busy, load, toggle };
}
