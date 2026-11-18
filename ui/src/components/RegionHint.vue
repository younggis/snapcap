<script setup lang="ts">
import type { Rect } from '../types'
defineProps<{ rect: Rect | null; snapping: boolean; mode: 'screenshot' | 'record' | 'window' }>()
</script>

<template>
  <div class="region-hint">
    <div class="row">
      <span class="keycap">单击</span><span>{{ mode === 'window' ? '选择窗口' : '框选区域' }}</span>
      <i />
      <span class="keycap">Enter</span><span>{{ mode === 'window' ? '截图并保存' : '确认' }}</span>
      <i />
      <span class="keycap">Esc</span><span>取消</span>
    </div>
    <div v-if="mode === 'window'" class="row">
      <span>窗口标题和边框会自动识别，无需手动对齐</span>
    </div>
    <div v-if="rect" class="coords" :class="{ snapping }">
      <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="var(--stroke-icon)" stroke-linecap="round">
        <path d="M4 8V6a2 2 0 012-2h2M16 4h2a2 2 0 012 2v2M20 16v2a2 2 0 01-2 2h-2M8 20H6a2 2 0 01-2-2v-2" />
      </svg>
      {{ rect.x }}, {{ rect.y }} · {{ rect.width }} × {{ rect.height }}
      <span v-if="snapping" class="snap-tag">已吸附</span>
    </div>
  </div>
</template>

<style scoped>
.region-hint {
  position: absolute;
  top: max(var(--sp-4), env(safe-area-inset-top));
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--sp-1);
  padding: var(--sp-2) var(--sp-4);
  border-radius: var(--r-card);
  background: var(--surface);
  border: 1px solid var(--border);
  box-shadow: var(--shadow-hover);
  pointer-events: none;
  white-space: nowrap;
}
.row {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  font-size: var(--fs-caption);
  color: var(--text-secondary);
}
.row i {
  width: 3px;
  height: 3px;
  border-radius: var(--r-pill);
  background: var(--text-tertiary);
  margin: 0 var(--sp-1);
}
.keycap {
  display: inline-block;
  padding: 2px var(--sp-2);
  border: 1px solid var(--border);
  border-radius: var(--r-keycap);
  color: var(--text-secondary);
  font-family: var(--font-mono);
  font-size: var(--fs-keycap);
  font-weight: var(--fw-keycap);
}
.coords {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  margin-top: var(--sp-1);
  font-family: var(--font-mono);
  font-size: var(--fs-caption);
  font-weight: var(--fw-body);
  color: var(--primary);
  font-variant-numeric: tabular-nums;
}
.coords.snapping { color: var(--text-secondary); }
.snap-tag {
  margin-left: var(--sp-1);
  padding: 1px var(--sp-2);
  border-radius: var(--r-pill);
  background: var(--primary-tint);
  color: var(--primary);
  font-size: var(--fs-keycap);
  font-family: inherit;
}
</style>
