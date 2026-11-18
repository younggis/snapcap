<script setup lang="ts">
import type { CaptureStats, SessionState } from '../types'

defineProps<{
  stats: CaptureStats
  state: SessionState | null
}>()

function formatDuration(ms: number): string {
  const total = Math.floor(ms / 1000)
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  const pad = (n: number) => n.toString().padStart(2, '0')
  return h > 0 ? `${pad(h)}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`
}
</script>

<template>
  <div class="stats-wrap">
    <div class="status-line">
      <span class="dot" :class="{ live: state === 'recording' || state === 'starting', paused: state === 'paused' }" />
      <span class="status-text">
        {{ state === 'paused' ? '已暂停' : state === 'recording' ? '正在录制' : state === 'starting' ? '正在启动' : state === 'stopping' ? '正在停止' : state === 'done' ? '已完成' : '准备就绪' }}
      </span>
    </div>
    <div class="metrics">
      <div class="metric">
        <span class="value mono">{{ formatDuration(stats.duration_ms) }}</span>
        <span class="label">时长</span>
      </div>
      <div class="metric">
        <span class="value mono">{{ stats.fps }}</span>
        <span class="label">帧率</span>
      </div>
      <div class="metric">
        <span class="value mono">{{ stats.bitrate_kbps }}</span>
        <span class="label">码率 Kbps</span>
      </div>
      <div class="metric">
        <span class="value mono" :class="{ warn: stats.dropped_frames > 0 }">{{ stats.dropped_frames }}</span>
        <span class="label">丢帧</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.stats-wrap { padding: 0; }
.status-line {
  display: flex; align-items: center; gap: var(--sp-2);
  margin-bottom: var(--sp-2);
}
.status-text {
  font-size: var(--fs-body);
  font-weight: var(--fw-body);
  color: var(--text-primary);
}
.metrics {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--sp-2);
}
.metric {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding: var(--sp-2) 2px;
  background: var(--surface-hover);
  border-radius: var(--r-button);
}
.value {
  font-size: var(--fs-body);
  font-weight: var(--fw-title);
  color: var(--text-primary);
}
.mono {
  font-family: var(--font-mono);
  font-variant-numeric: tabular-nums;
}
.value.warn { color: var(--text-secondary); }
.label {
  font-size: var(--fs-keycap);
  color: var(--text-secondary);
  white-space: nowrap;
}
</style>
