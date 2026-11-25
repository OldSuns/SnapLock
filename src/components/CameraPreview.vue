<script setup lang="ts">
import type { Camera } from '../composables/useCamera';
import { getPermissionStatusClass } from '../utils/helpers';
const props = defineProps<{ camera: Camera }>();
const { permission, checking, preview, showing, error, checkPermission, togglePreview, refresh } = props.camera;
</script>

<template>
  <div class="camera-controls">
    <div class="camera-status">
      <span class="status-text">权限状态: </span>
      <span class="permission-status" :class="getPermissionStatusClass(permission)">{{ permission }}</span>
      <button @click="checkPermission" :disabled="checking" class="check-permission-button" title="检查相机权限">
        {{ checking ? '⏳' : '🔄' }}
      </button>
    </div>
    <div class="preview-controls">
      <button @click="togglePreview" :disabled="checking" class="preview-toggle-button" :class="{ active: showing }">
        {{ showing ? '隐藏预览' : '显示预览' }}
      </button>
    </div>
    <div v-if="showing && preview" class="camera-preview">
      <img :src="preview" alt="相机预览" class="preview-image" />
      <button @click="refresh" class="refresh-preview-button" title="刷新预览">🔄</button>
    </div>
    <p v-if="error" class="form-error" role="alert">{{ error }}</p>
  </div>
</template>
