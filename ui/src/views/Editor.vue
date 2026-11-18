<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import Toolbar from '../components/Toolbar.vue'
import { useCapture } from '../composables/useCapture'
import { onScreenshotSaved, revealFile } from '../ipc'
import type { AnnotationOp, AnnotationTool } from '../types'

const capture = useCapture()

// Canvas 绘制取设计令牌的实际色值（CSS 变量 → canvas 颜色字符串）。
const cssVar = (name: string, fallback: string) =>
  getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback

const wrap = ref<HTMLDivElement | null>(null)
const canvas = ref<HTMLCanvasElement | null>(null)
const activeTool = ref<AnnotationTool>('rect')
const color = ref(0xef4444)
const ops = ref<AnnotationOp[]>([])
const redoStack = ref<AnnotationOp[]>([])
const status = ref('')
const lastPath = ref('')
const busy = ref(false)

// 文字输入
const textInput = ref<{ x: number; y: number; value: string } | null>(null)
const textEl = ref<HTMLInputElement | null>(null)

const img = ref<HTMLImageElement | null>(null)
let ctx: CanvasRenderingContext2D | null = null
let dpr = 1
/** 图片在画布中的绘制矩形（CSS 像素）。 */
let layout = { x: 0, y: 0, w: 0, h: 0 }
let scale = 1

const dragStart = ref<{ x: number; y: number } | null>(null)
const preview = ref<AnnotationOp | null>(null)

const hasImage = computed(() => !!img.value)
const canUndo = computed(() => ops.value.length > 0)
const canRedo = computed(() => redoStack.value.length > 0)

function hex(c: number) {
  return `#${c.toString(16).padStart(6, '0')}`
}

function loadImage(url: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const image = new Image()
    image.onload = () => {
      img.value = image
      nextTick(() => {
        relayout()
        resolve()
      })
    }
    image.onerror = () => reject(new Error('图片加载失败'))
    image.src = url
  })
}

function relayout() {
  const cv = canvas.value
  const container = wrap.value
  const image = img.value
  if (!cv || !container || !image) return
  dpr = window.devicePixelRatio || 1
  const cw = container.clientWidth
  const ch = container.clientHeight
  cv.width = Math.max(1, Math.floor(cw * dpr))
  cv.height = Math.max(1, Math.floor(ch * dpr))
  cv.style.width = `${cw}px`
  cv.style.height = `${ch}px`
  ctx = cv.getContext('2d')

  const iw = image.naturalWidth
  const ih = image.naturalHeight
  scale = Math.min((cw - 32) / iw, (ch - 32) / ih, 1)
  const w = iw * scale
  const h = ih * scale
  layout = { x: (cw - w) / 2, y: (ch - h) / 2, w, h }
  draw()
}

function toImageCoords(e: MouseEvent) {
  const cv = canvas.value!
  const box = cv.getBoundingClientRect()
  const x = e.clientX - box.left
  const y = e.clientY - box.top
  return {
    x: Math.round((x - layout.x) / scale),
    y: Math.round((y - layout.y) / scale),
  }
}

function draw() {
  const c = ctx
  const image = img.value
  if (!c || !image) return
  c.setTransform(dpr, 0, 0, dpr, 0, 0)
  c.clearRect(0, 0, canvas.value!.width / dpr, canvas.value!.height / dpr)

  c.save()
  // Canvas 光栅化无法直接消费 CSS box-shadow 令牌；此处使用与 --shadow-rest
  // 同色相的中性色，且不产生任何彩色发光。
  c.shadowColor = 'rgba(16, 24, 40, 0.12)'
  c.shadowBlur = 12
  c.drawImage(image, layout.x, layout.y, layout.w, layout.h)
  c.restore()

  c.save()
  c.beginPath()
  c.rect(layout.x, layout.y, layout.w, layout.h)
  c.clip()
  c.translate(layout.x, layout.y)
  c.scale(scale, scale)
  for (const op of ops.value) drawOp(c, op, image)
  if (preview.value) drawOp(c, preview.value, image)
  c.restore()
}

function drawOp(c: CanvasRenderingContext2D, op: AnnotationOp, image: HTMLImageElement) {
  c.save()
  c.lineCap = 'round'
  c.lineJoin = 'round'
  switch (op.type) {
    case 'rect': {
      c.strokeStyle = hex(op.color)
      c.lineWidth = op.width
      c.strokeRect(op.x, op.y, op.w, op.h)
      break
    }
    case 'arrow': {
      const [x1, y1] = op.from
      const [x2, y2] = op.to
      c.strokeStyle = hex(op.color)
      c.fillStyle = hex(op.color)
      c.lineWidth = op.width
      c.beginPath(); c.moveTo(x1, y1); c.lineTo(x2, y2); c.stroke()
      const angle = Math.atan2(y2 - y1, x2 - x1)
      const head = Math.max(10, op.width * 3.5)
      c.beginPath()
      c.moveTo(x2, y2)
      c.lineTo(x2 - head * Math.cos(angle - Math.PI / 6), y2 - head * Math.sin(angle - Math.PI / 6))
      c.lineTo(x2 - head * Math.cos(angle + Math.PI / 6), y2 - head * Math.sin(angle + Math.PI / 6))
      c.closePath(); c.fill()
      break
    }
    case 'text': {
      c.fillStyle = hex(op.color)
      c.font = `600 ${op.size}px 'Microsoft YaHei', system-ui, sans-serif`
      c.fillText(op.text, op.x, op.y)
      break
    }
    case 'blur': {
      c.save()
      c.beginPath()
      c.rect(op.x, op.y, op.w, op.h)
      c.clip()
      c.filter = `blur(${op.radius}px)`
      c.drawImage(image, 0, 0)
      c.restore()
      break
    }
    case 'step': {
      const r = 14
      c.fillStyle = cssVar('--primary', '#2F6BFF')
      c.beginPath(); c.arc(op.x, op.y, r, 0, Math.PI * 2); c.fill()
      c.fillStyle = cssVar('--on-accent', '#FFFFFF')
      c.font = 'bold 16px system-ui, sans-serif'
      c.textAlign = 'center'
      c.textBaseline = 'middle'
      c.fillText(String(op.number), op.x, op.y + 1)
      break
    }
  }
  c.restore()
}

function pushOp(op: AnnotationOp) {
  ops.value.push(op)
  redoStack.value = []
  draw()
}

function onDown(e: MouseEvent) {
  const p = toImageCoords(e)
  if (activeTool.value === 'text') {
    textInput.value = { x: p.x, y: p.y, value: '' }
    nextTick(() => textEl.value?.focus())
    return
  }
  if (activeTool.value === 'step') {
    const number = ops.value.filter((o) => o.type === 'step').length + 1
    pushOp({ type: 'step', x: p.x, y: p.y, number })
    return
  }
  if (activeTool.value === 'select') return
  dragStart.value = p
}

function onMove(e: MouseEvent) {
  if (!dragStart.value) return
  const p = toImageCoords(e)
  const s = dragStart.value
  const x = Math.min(s.x, p.x)
  const y = Math.min(s.y, p.y)
  const w = Math.abs(p.x - s.x)
  const h = Math.abs(p.y - s.y)
  if (activeTool.value === 'rect') preview.value = { type: 'rect', x, y, w, h, color: color.value, width: 3 }
  else if (activeTool.value === 'arrow') preview.value = { type: 'arrow', from: [s.x, s.y], to: [p.x, p.y], color: color.value, width: 3 }
  else if (activeTool.value === 'blur') preview.value = { type: 'blur', x, y, w, h, radius: 12 }
  draw()
}

function onUp(_e: MouseEvent) {
  if (preview.value) pushOp(preview.value)
  preview.value = null
  dragStart.value = null
}

function commitText() {
  if (!textInput.value) return
  const { x, y, value } = textInput.value
  if (value.trim()) pushOp({ type: 'text', x, y, text: value.trim(), color: color.value, size: 22 })
  textInput.value = null
}

function undo() {
  const op = ops.value.pop()
  if (op) redoStack.value.push(op)
  draw()
}
function redo() {
  const op = redoStack.value.pop()
  if (op) ops.value.push(op)
  draw()
}
function clearAll() {
  redoStack.value = [...ops.value].reverse()
  ops.value = []
  draw()
}

function setTool(t: AnnotationTool) {
  activeTool.value = t
}

async function outputTo(dest: 'file' | 'clipboard') {
  if (!capture.screenshot.value) return
  busy.value = true
  status.value = '正在合成…'
  try {
    await capture.annotate(ops.value)
    const saved = await capture.output(dest)
    status.value = dest === 'clipboard' ? '已复制到剪贴板' : '已保存到文件'
    if (saved) lastPath.value = saved
  } catch (e) {
    status.value = `失败：${(e as Error).message}`
  } finally {
    busy.value = false
    window.setTimeout(() => (status.value = ''), 2200)
  }
}

function onKey(e: KeyboardEvent) {
  if (textInput.value) return
  const ctrl = e.ctrlKey || e.metaKey
  if (ctrl && e.key.toLowerCase() === 'z') { e.preventDefault(); undo(); return }
  if (ctrl && e.key.toLowerCase() === 'y') { e.preventDefault(); redo(); return }
  if (ctrl && e.key.toLowerCase() === 's') { e.preventDefault(); outputTo('file'); return }
  if (e.key === 'Escape') { activeTool.value = 'select'; return }
  const map: Record<string, AnnotationTool> = { v: 'select', r: 'rect', a: 'arrow', t: 'text', b: 'blur', n: 'step' }
  const t = map[e.key.toLowerCase()]
  if (t && !ctrl) activeTool.value = t
}

let resizeObserver: ResizeObserver | null = null
let unlistenSaved: (() => void) | null = null

onMounted(async () => {
  window.addEventListener('resize', relayout)
  window.addEventListener('keydown', onKey)
  unlistenSaved = await onScreenshotSaved((path) => {
    lastPath.value = path
  })
  // 关键：editor 窗口在隐藏状态下收到 screenshot:ready 时，canvas 尺寸为 0，
  // 图片会被画进 0×0 画布而不可见。用 ResizeObserver 监听画布容器，
  // 窗口显示/尺寸变化时自动重新布局并重绘。
  if (wrap.value && typeof ResizeObserver !== 'undefined') {
    resizeObserver = new ResizeObserver(() => {
      if (img.value && wrap.value && wrap.value.clientWidth > 0 && wrap.value.clientHeight > 0) {
        relayout()
      }
    })
    resizeObserver.observe(wrap.value)
  }
  await capture.listen()
  // 不再自动抓屏：隐藏的 editor 窗口在启动时也会加载，若在这里抓屏会
  // 截到桌面自身。截图由主窗口经 screenshot:ready 事件推送过来。
  if (capture.screenshot.value) await loadImage(capture.screenshot.value.data_url)
})

watch(() => capture.screenshot.value?.data_url, (url) => {
  if (url) loadImage(url)
})

onBeforeUnmount(() => {
  unlistenSaved?.()
  resizeObserver?.disconnect()
  window.removeEventListener('resize', relayout)
  window.removeEventListener('keydown', onKey)
})
</script>

<template>
  <div class="editor">
    <Toolbar
      :active-tool="activeTool"
      :color="color"
      :can-undo="canUndo"
      :can-redo="canRedo"
      @update:active-tool="setTool"
      @update:color="color = $event"
      @undo="undo"
      @redo="redo"
      @clear="clearAll"
      @save="outputTo('file')"
      @copy="outputTo('clipboard')"
    />
    <div ref="wrap" class="canvas-wrap">
      <canvas
        ref="canvas"
        :class="{ drawing: activeTool !== 'select' }"
        @mousedown="onDown"
        @mousemove="onMove"
        @mouseup="onUp"
        @mouseleave="onUp"
      />
      <div
        v-if="textInput"
        class="text-editor"
        :style="{ left: `${layout.x + textInput.x * scale}px`, top: `${layout.y + textInput.y * scale}px` }"
      >
        <input
          ref="textEl"
          v-model="textInput.value"
          placeholder="输入文字，回车确定"
          @keydown.enter="commitText"
          @keydown.esc="textInput = null"
        />
      </div>
      <div v-if="!hasImage" class="empty">没有可编辑的截图</div>
      <div v-if="status" class="status">{{ status }}</div>
      <div v-if="lastPath" class="saved-bar">
        <span class="saved-path" :title="lastPath">✓ {{ lastPath }}</span>
        <button class="ios-btn ios-btn--tinted ios-btn--small" @click="revealFile(lastPath)">打开文件位置</button>
        <button class="saved-close" @click="lastPath = ''">✕</button>
      </div>
      <div class="tips">拖拽绘制 · Ctrl+Z 撤销 · Ctrl+S 保存</div>
    </div>
  </div>
</template>

<!-- 让 toolbar 内的 glass 类作用于全局样式：使用 :deep 作用域穿透 -->
<style scoped>
.editor {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg);
}
.canvas-wrap {
  position: relative;
  flex: 1;
  overflow: hidden;
  background: var(--surface-pressed);
}
canvas {
  display: block;
  cursor: default;
  border-radius: var(--r-button);
}
canvas.drawing { cursor: crosshair; }
.empty,
.status,
.tips {
  position: absolute;
  padding: var(--sp-2) var(--sp-3);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--r-card);
  box-shadow: var(--shadow-rest);
  pointer-events: none;
  font-size: var(--fs-body);
}
.empty {
  top: 50%; left: 50%;
  transform: translate(-50%, -50%);
  color: var(--text-secondary);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--sp-2);
  padding: var(--sp-5) var(--sp-6);
}
.status {
  top: var(--sp-4); left: 50%;
  transform: translateX(-50%);
  color: var(--primary);
  font-weight: var(--fw-body);
}
.tips {
  bottom: var(--sp-4); left: 50%;
  transform: translateX(-50%);
  color: var(--text-secondary);
  font-size: var(--fs-caption);
}
.saved-bar {
  position: absolute;
  top: var(--sp-4); right: var(--sp-4);
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  max-width: 70%;
  padding: var(--sp-2);
  border-radius: var(--r-card);
  background: var(--surface);
  border: 1px solid var(--border);
  box-shadow: var(--shadow-hover);
  pointer-events: auto;
}
.saved-path {
  font-size: var(--fs-caption);
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 380px;
}
.saved-close {
  color: var(--text-tertiary);
  font-size: 12px;
  padding: 2px 4px;
}
.saved-close:hover { color: var(--text-primary); }
.text-editor {
  position: absolute;
  z-index: 5;
}
.text-editor input {
  width: 220px;
  height: 36px;
  padding: 0 var(--sp-3);
  background: var(--surface);
  border: 1px solid var(--primary);
  border-radius: var(--r-button);
  box-shadow: var(--shadow-hover);
  color: var(--text-primary);
  font-size: var(--fs-body);
  outline: none;
}
</style>
