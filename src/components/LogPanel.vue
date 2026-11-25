<script setup lang="ts">
import { ref, onMounted, onUnmounted, nextTick } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { LogEntry } from '../types';
import { getLogLevelClass } from '../utils/helpers';

const entries = ref<LogEntry[]>([]);
const expanded = ref(false);
const content = ref<HTMLElement>();
let unlisten: UnlistenFn | undefined;
let disposed = false;
onMounted(async () => {
  const buffered: LogEntry[] = [];
  let loading = true;
  const listener = await listen<LogEntry>('log_entry', async event => {
    if (disposed) return;
    if (loading) { buffered.push(event.payload); return; }
    entries.value = [...entries.value, event.payload].slice(-500);
    await nextTick();
    if (content.value) content.value.scrollTop = content.value.scrollHeight;
  });
  if (disposed) { listener(); return; }
  unlisten = listener;
  try {
    const history = await invoke<LogEntry[]>('get_debug_logs');
    if (!disposed) {
      const seen = new Set(history.map(entry => JSON.stringify(entry)));
      entries.value = [...history, ...buffered.filter(entry => !seen.has(JSON.stringify(entry)))].slice(-500);
    }
  } catch (error) { console.error('Failed to load logs:', error); }
  finally { loading = false; }
});
onUnmounted(() => { disposed = true; unlisten?.(); });
async function clear() {
  try { await invoke('clear_debug_logs'); entries.value = []; }
  catch (error) { console.error('Failed to clear logs:', error); }
}
</script>

<template>
  <div class="log-panel">
    <div class="log-header">
      <button class="log-title log-heading-button" @click="expanded = !expanded" :aria-expanded="expanded">
        <span class="log-icon">📋</span>调试日志
      </button>
      <div class="log-controls">
        <button @click="clear" class="log-clear-button" title="清空日志">🗑️</button>
        <button @click="expanded = !expanded" class="log-toggle-button" :aria-expanded="expanded" aria-label="展开或收起日志">
          {{ expanded ? '▼' : '▶' }}
        </button>
      </div>
    </div>
    <div v-if="expanded" ref="content" class="log-content">
      <div v-if="!entries.length" class="log-empty">暂无日志记录</div>
      <div v-else class="log-entries">
        <div v-for="(entry, index) in entries" :key="index" class="log-entry" :class="getLogLevelClass(entry.level)">
          <span class="log-timestamp">{{ entry.timestamp }}</span>
          <span class="log-level">[{{ entry.level }}]</span>
          <span class="log-message">{{ entry.message }}</span>
        </div>
      </div>
    </div>
  </div>
</template>
