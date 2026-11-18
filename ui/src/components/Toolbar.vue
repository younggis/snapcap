<script setup lang="ts">
import type { AnnotationTool } from '../types'

defineProps<{
  activeTool: AnnotationTool
  color: number
  canUndo: boolean
  canRedo: boolean
}>()

const emit = defineEmits<{
  'update:activeTool': [tool: AnnotationTool]
  'update:color': [color: number]
  undo: []
  redo: []
  clear: []
  save: []
  copy: []
}>()

const tools: Array<{ id: AnnotationTool; label: string; hint: string; path: string }> = [
  { id: 'select', label: '选择', hint: '选择 / 移动', path: 'M5 3l14 9-6.2 1.3L9.5 19 5 3z' },
  { id: 'rect', label: '矩形', hint: '矩形框', path: 'M4 5h16v14H4z' },
  { id: 'arrow', label: '箭头', hint: '箭头', path: 'M5 19L19 5M19 5h-7M19 5v7' },
  { id: 'text', label: '文字', hint: '文字', path: 'M5 6h14M12 6v13M9 19h6' },
  { id: 'blur', label: '马赛克', hint: '马赛克', path: 'M4 5h6v6H4zM14 5h6v6h-6zM4 15h6v4H4zM14 15h6v4h-6z' },
  { id: 'step', label: '序号', hint: '步骤序号', path: 'M12 3a9 9 0 100 18 9 9 0 000-18zM10 8h2v5h2M10 15h4' },
]

// Annotation palette = functional data, not theme tokens.
const colors = [
  { hex: '#F04438', name: '红' },
  { hex: '#FF9500', name: '橙' },
  { hex: '#FFCC00', name: '黄' },
  { hex: '#34C759', name: '绿' },
  { hex: '#30B0C7', name: '青' },
  { hex: '#2F6BFF', name: '蓝' },
  { hex: '#5856D6', name: '靛' },
  { hex: '#AF52DE', name: '紫' },
  { hex: '#FFFFFF', name: '白' },
  { hex: '#1B1F2A', name: '黑' },
]
const colorHex = (c: number) => `#${c.toString(16).padStart(6, '0')}`
</script>

<template>
  <div class="editor-toolbar">
    <div class="brand">
      <span class="app-mark">
        <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="var(--stroke-icon)" stroke-linecap="round">
          <path d="M4 9V6a2 2 0 012-2h2M16 4h2a2 2 0 012 2v2M20 15v3a2 2 0 01-2 2h-3M9 20H6a2 2 0 01-2-2v-3" />
          <rect x="8" y="8" width="8" height="8" rx="1" />
        </svg>
      </span>
      <span>标记</span>
    </div>

    <div class="divider" />

    <div class="tools">
      <button
        v-for="t in tools"
        :key="t.id"
        class="tool"
        :class="{ active: activeTool === t.id }"
        :title="t.hint"
        @click="emit('update:activeTool', t.id)"
      >
        <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor"
          stroke-width="var(--stroke-icon)" stroke-linecap="round" stroke-linejoin="round">
          <path :d="t.path" />
        </svg>
        <span>{{ t.label }}</span>
      </button>
    </div>

    <div class="divider" />

    <div class="colors">
      <button
        v-for="c in colors"
        :key="c.hex"
        class="swatch"
        :class="{ active: color === parseInt(c.hex.slice(1), 16) }"
        :style="{ background: c.hex }"
        :title="c.name"
        @click="emit('update:color', parseInt(c.hex.slice(1), 16))"
      />
      <label class="color-custom" title="自定义颜色">
        <input
          type="color"
          :value="colorHex(color)"
          @input="emit('update:color', parseInt(($event.target as HTMLInputElement).value.slice(1), 16))"
        />
      </label>
    </div>

    <div class="spacer" />

    <div class="actions">
      <button class="icon-btn" :disabled="!canUndo" title="撤销 ⌘Z" @click="emit('undo')">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="var(--stroke-icon)" stroke-linecap="round" stroke-linejoin="round">
          <path d="M9 7L4 12l5 5M4 12h9a6 6 0 010 12" transform="translate(0,-4)" />
        </svg>
      </button>
      <button class="icon-btn" :disabled="!canRedo" title="重做 ⇧⌘Z" @click="emit('redo')">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="var(--stroke-icon)" stroke-linecap="round" stroke-linejoin="round">
          <path d="M15 7l5 5-5 5M20 12h-9a6 6 0 000 12" transform="translate(0,-4)" />
        </svg>
      </button>
      <button class="icon-btn" title="清空" @click="emit('clear')">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="var(--stroke-icon)" stroke-linecap="round" stroke-linejoin="round">
          <path d="M4 7h16M9 7V5h6v2M6 7l1 13h10l1-13M10 11v5M14 11v5" />
        </svg>
      </button>
    </div>

    <div class="divider" />

    <div class="actions">
      <button class="ios-btn ios-btn--gray ios-btn--small" title="复制到剪贴板" @click="emit('copy')">拷贝</button>
      <button class="ios-btn ios-btn--filled ios-btn--small" title="保存到文件" @click="emit('save')">存储</button>
    </div>
  </div>
</template>

<style scoped>
.editor-toolbar {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  height: 56px;
  padding: 0 var(--sp-3);
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  flex: none;
}
.brand {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  font-size: var(--fs-body);
  font-weight: var(--fw-title);
  color: var(--text-primary);
  white-space: nowrap;
}
.app-mark {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  border-radius: var(--r-icon-list);
  background: var(--primary);
  color: var(--on-accent);
}
.divider {
  width: 1px;
  height: 24px;
  background: var(--border);
  flex: none;
}
.tools { display: flex; gap: 2px; }
.tool {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 2px;
  width: 48px;
  height: 44px;
  border-radius: var(--r-button);
  color: var(--text-secondary);
  transition: background var(--dur) var(--ease), color var(--dur) var(--ease);
}
.tool span { font-size: var(--fs-keycap); font-weight: var(--fw-caption); }
.tool:hover { background: var(--surface-hover); color: var(--text-primary); }
.tool:active { background: var(--surface-pressed); }
.tool.active { background: var(--primary-tint); color: var(--primary); }
.colors { display: flex; align-items: center; gap: var(--sp-2); }
.swatch {
  width: 18px;
  height: 18px;
  border-radius: var(--r-pill);
  padding: 0;
  border: 1px solid var(--border);
  transition: transform var(--dur) var(--ease), box-shadow var(--dur) var(--ease);
}
.swatch:hover { transform: scale(1.12); }
.swatch.active { box-shadow: 0 0 0 2px var(--surface), 0 0 0 4px var(--primary); }
.color-custom {
  position: relative;
  width: 20px;
  height: 20px;
  border-radius: var(--r-pill);
  overflow: hidden;
  display: grid;
  place-items: center;
  background: var(--primary-tint);
  border: 1px solid var(--border);
  cursor: pointer;
}
.color-custom input {
  position: absolute;
  inset: -6px;
  width: 40px;
  height: 40px;
  border: none;
  opacity: 0;
  cursor: pointer;
}
.spacer { flex: 1; }
.actions { display: flex; align-items: center; gap: var(--sp-1); }
.icon-btn {
  display: grid;
  place-items: center;
  width: 34px;
  height: 34px;
  border-radius: var(--r-button);
  color: var(--text-secondary);
  transition: background var(--dur) var(--ease), color var(--dur) var(--ease);
}
.icon-btn:hover { background: var(--surface-hover); color: var(--text-primary); }
.icon-btn:active:not(:disabled) { background: var(--surface-pressed); }
.icon-btn:disabled { opacity: 0.4; }
</style>
