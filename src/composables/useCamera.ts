import { ref, watch, onScopeDispose } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { CameraInfo, PermissionStatus } from '../types';

export function useCamera() {
  const cameras = ref<CameraInfo[]>([]);
  const selected = ref(0);
  const permission = ref<PermissionStatus>('未检查');
  const checking = ref(false);
  const preview = ref('');
  const showing = ref(false);
  const error = ref('');
  let request = 0;
  let disposed = false;
  onScopeDispose(() => { disposed = true; request++; });

  async function load(preferred: number | null) {
    try {
      cameras.value = await invoke<CameraInfo[]>('get_camera_list');
      selected.value = cameras.value.find(camera => camera.id === preferred)?.id ?? cameras.value[0]?.id ?? 0;
    } catch (cause) { error.value = String(cause); }
  }

  async function checkPermission() {
    const id = selected.value;
    const token = ++request;
    checking.value = true;
    try {
      const allowed = await invoke<boolean>('check_camera_permission', { cameraId: id });
      if (disposed || token !== request || id !== selected.value) return false;
      permission.value = allowed ? '已授权' : '被拒绝';
      return allowed;
    } catch (cause) {
      if (!disposed && token === request) { permission.value = '被拒绝'; error.value = String(cause); }
      return false;
    } finally { if (token === request) checking.value = false; }
  }

  async function refresh() {
    const id = selected.value;
    const token = ++request;
    try {
      const image = await invoke<string>('get_camera_preview', { cameraId: id });
      if (disposed || token !== request || id !== selected.value) return false;
      preview.value = image;
      return true;
    } catch (cause) {
      if (!disposed && token === request) { preview.value = ''; showing.value = false; error.value = String(cause); }
      return false;
    }
  }

  async function togglePreview() {
    if (showing.value) { request++; showing.value = false; preview.value = ''; return; }
    error.value = '';
    if (await checkPermission()) showing.value = await refresh();
  }

  watch(selected, async id => {
    request++;
    error.value = '';
    checking.value = false;
    permission.value = '未检查';
    preview.value = '';
    if (cameras.value.length) {
      try { await invoke('set_camera_id', { cameraId: id }); }
      catch (cause) { error.value = String(cause); }
    }
    if (disposed || id !== selected.value) return;
    if (showing.value) {
      const allowed = await checkPermission();
      if (disposed || id !== selected.value) return;
      if (allowed) await refresh(); else showing.value = false;
    }
  });
  return { cameras, selected, permission, checking, preview, showing, error, load, checkPermission, refresh, togglePreview };
}

export type Camera = ReturnType<typeof useCamera>;
