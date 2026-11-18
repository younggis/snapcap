<script setup lang="ts">
import { computed } from 'vue'
import SettingsView from './views/Settings.vue'
import OverlayView from './views/Overlay.vue'
import EditorView from './views/Editor.vue'
import RecorderView from './views/Recorder.vue'

/**
 * Tauri 为每个窗口加载同一份前端，通过 ?view= 查询参数区分视图；
 * 浏览器直接打开时可用 ?view= 或 #view 切换，便于验证。
 */
function resolveView(): string {
  const fromQuery = new URLSearchParams(window.location.search).get('view')
  if (fromQuery) return fromQuery
  const fromHash = window.location.hash.replace(/^#\/?/, '')
  if (fromHash) return fromHash
  return 'settings'
}

const view = resolveView()

const currentView = computed(() => {
  switch (view) {
    case 'overlay':
      return OverlayView
    case 'editor':
      return EditorView
    case 'recorder':
      return RecorderView
    case 'settings':
    default:
      return SettingsView
  }
})
</script>

<template>
  <component :is="currentView" />
</template>
