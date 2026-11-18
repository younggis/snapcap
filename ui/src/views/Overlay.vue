<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import RegionHint from '../components/RegionHint.vue'
import { getVirtualDesktop, listWindows, onOverlayConfirm, onOverlayEscape, submitOverlaySelection } from '../ipc'
import type { Rect, WindowInfo } from '../types'

type OverlayMode = 'screenshot' | 'record' | 'window'
const requestedMode = new URLSearchParams(location.search).get('mode')
const mode: OverlayMode = requestedMode === 'record' || requestedMode === 'window' ? requestedMode : 'screenshot'
const isWindowMode = mode === 'window'

const canvas = ref<HTMLCanvasElement | null>(null)
const rect = ref<Rect | null>(null)
const snapping = ref(false)
const dragStart = ref<{ x: number; y: number } | null>(null)
const selectedHwnd = ref<number | null>(null)
const submitting = ref(false)

const windows = ref<WindowInfo[]>([])
/** 窗口在覆盖层坐标系中的矩形（覆盖层 = 虚拟桌面）。 */
const windowZones = ref<Array<{ hwnd: number; rect: Rect; title: string }>>([])
const virtualDesktop = ref<Rect>({ x: 0, y: 0, width: 1920, height: 1080 })

let ctx: CanvasRenderingContext2D | null = null
let dpr = 1
let pointer: { x: number; y: number } | null = null
const hoverZone = ref<{ hwnd: number; rect: Rect; title: string } | null>(null)
let unescape: (() => void) | null = null
let unconfirm: (() => void) | null = null

const norm = (a: number, b: number) => ({ min: Math.min(a, b), max: Math.max(a, b) })

function resize() {
  const cv = canvas.value
  if (!cv) return
  dpr = window.devicePixelRatio || 1
  cv.width = Math.floor(window.innerWidth * dpr)
  cv.height = Math.floor(window.innerHeight * dpr)
  cv.style.width = `${window.innerWidth}px`
  cv.style.height = `${window.innerHeight}px`
  ctx = cv.getContext('2d')
  buildZones()
  draw()
}

function buildZones() {
  const v = virtualDesktop.value
  // 覆盖层窗口被 Rust 放到 (vx, vy)，尺寸为 (vw, vh)，因此窗口坐标减去原点即为覆盖层内坐标。
  windowZones.value = windows.value
    .filter((w) => w.is_visible && w.rect.width > 0 && w.rect.height > 0)
    .map((w) => ({
      hwnd: w.hwnd,
      title: w.title,
      // Rust 返回物理像素，鼠标/Canvas 使用 CSS 像素；高分屏下必须换算。
      rect: {
        x: (w.rect.x - v.x) / dpr,
        y: (w.rect.y - v.y) / dpr,
        width: w.rect.width / dpr,
        height: w.rect.height / dpr,
      },
    }))
}

function zoneAt(x: number, y: number) {
  for (const z of windowZones.value) {
    const r = z.rect
    if (x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height) return z
  }
  return null
}

function color(name: string, fallback: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback
}

function draw() {
  if (!ctx) return
  const c = ctx
  const w = window.innerWidth
  const h = window.innerHeight
  const primary = color('--primary', '#2F6BFF')
  const surface = color('--surface', '#FFFFFF')

  c.setTransform(dpr, 0, 0, dpr, 0, 0)
  c.clearRect(0, 0, w, h)
  c.fillStyle = 'rgba(0, 0, 0, 0.35)'
  c.fillRect(0, 0, w, h)

  // 窗口模式：标出所有可选窗口，命中的高亮。
  if (isWindowMode) {
    for (const z of windowZones.value) {
      const active = hoverZone.value?.hwnd === z.hwnd || selectedHwnd.value === z.hwnd
      c.save()
      c.strokeStyle = active ? primary : surface
      c.globalAlpha = active ? 1 : 0.45
      c.lineWidth = active ? 2 : 1
      c.setLineDash(active ? [] : [6, 5])
      c.strokeRect(z.rect.x + 0.5, z.rect.y + 0.5, z.rect.width - 1, z.rect.height - 1)
      c.globalAlpha = active ? 0.12 : 0.04
      c.fillStyle = primary
      c.fillRect(z.rect.x, z.rect.y, z.rect.width, z.rect.height)
      c.restore()
    }
  }

  const r = rect.value
  if (r && r.width > 0 && r.height > 0) {
    c.clearRect(r.x, r.y, r.width, r.height)
    c.save()
    c.strokeStyle = primary
    c.lineWidth = 2
    c.strokeRect(r.x + 1, r.y + 1, r.width - 2, r.height - 2)
    c.globalAlpha = 0.25
    for (let i = 1; i < 3; i++) {
      const x = r.x + (r.width * i) / 3
      const y = r.y + (r.height * i) / 3
      c.beginPath(); c.moveTo(x, r.y); c.lineTo(x, r.y + r.height); c.stroke()
      c.beginPath(); c.moveTo(r.x, y); c.lineTo(r.x + r.width, y); c.stroke()
    }
    c.restore()
  }

  if (pointer) {
    c.save()
    c.strokeStyle = surface
    c.globalAlpha = 0.35
    c.lineWidth = 1
    c.beginPath(); c.moveTo(pointer.x + 0.5, 0); c.lineTo(pointer.x + 0.5, h); c.stroke()
    c.beginPath(); c.moveTo(0, pointer.y + 0.5); c.lineTo(w, pointer.y + 0.5); c.stroke()
    c.restore()
  }
}

function onMove(e: MouseEvent) {
  pointer = { x: e.clientX, y: e.clientY }
  if (isWindowMode) {
    hoverZone.value = zoneAt(e.clientX, e.clientY)
    snapping.value = !!hoverZone.value
  } else if (dragStart.value) {
    const a = norm(dragStart.value.x, e.clientX)
    const b = norm(dragStart.value.y, e.clientY)
    rect.value = { x: a.min, y: b.min, width: a.max - a.min, height: b.max - b.min }
  } else {
    const zone = zoneAt(e.clientX, e.clientY)
    snapping.value = !!zone
  }
  draw()
}

function onDown(e: MouseEvent) {
  if (e.button !== 0) return
  if (isWindowMode) {
    const zone = zoneAt(e.clientX, e.clientY)
    if (zone) {
      selectedHwnd.value = zone.hwnd
      rect.value = { ...zone.rect }
      snapping.value = true
      draw()
      return
    }
    notifyNoWindow()
    return
  }
  dragStart.value = { x: e.clientX, y: e.clientY }
  rect.value = { x: e.clientX, y: e.clientY, width: 0, height: 0 }
  draw()
}

function notifyNoWindow() {
  hint.value = '请点击一个窗口'
  window.setTimeout(() => (hint.value = ''), 1500)
}

const hint = ref('')

function onUp() {
  dragStart.value = null
}

/** 把覆盖层坐标还原为虚拟桌面绝对坐标。 */
function absoluteRect(r: Rect): Rect {
  return {
    x: Math.round(r.x * dpr) + virtualDesktop.value.x,
    y: Math.round(r.y * dpr) + virtualDesktop.value.y,
    width: Math.max(1, Math.round(r.width * dpr)),
    height: Math.max(1, Math.round(r.height * dpr)),
  }
}

async function confirm() {
  if (submitting.value || !rect.value || rect.value.width < 1 || rect.value.height < 1) return
  if (isWindowMode && selectedHwnd.value == null) {
    notifyNoWindow()
    return
  }
  submitting.value = true
  try {
    await submitOverlaySelection({
      rect: absoluteRect(rect.value),
      hwnd: selectedHwnd.value,
      cancelled: false,
    })
    leave()
  } catch (e) {
    hint.value = `确认失败：${e instanceof Error ? e.message : String(e)}`
    submitting.value = false
  }
}

async function cancel() {
  await submitOverlaySelection({
    rect: { x: 0, y: 0, width: 0, height: 0 },
    hwnd: null,
    cancelled: true,
  })
  leave()
}

function leave() {
  if (!('__TAURI_INTERNALS__' in window)) location.href = '?view=settings'
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Enter') { e.preventDefault(); confirm() }
  else if (e.key === 'Escape') { e.preventDefault(); cancel() }
}

const modeLabel = computed(() =>
  isWindowMode
    ? `选择窗口 · ${windowZones.value.length} 个可识别窗口`
    : mode === 'record' ? '选择录制区域' : '选择截图区域',
)
const activeWindow = computed(() => {
  if (!isWindowMode) return null
  // 已选中的窗口优先于 hover：选择后鼠标轻微移动不应让标题变回其它窗口。
  const selected = windowZones.value.find((z) => z.hwnd === selectedHwnd.value)
  if (selected) return selected
  return hoverZone.value
})

onMounted(async () => {
  unescape = await onOverlayEscape(() => cancel())
  unconfirm = await onOverlayConfirm(() => confirm())
  window.addEventListener('resize', resize)
  window.addEventListener('mousemove', onMove)
  window.addEventListener('mousedown', onDown)
  window.addEventListener('mouseup', onUp)
  window.addEventListener('keydown', onKey)
  try {
    virtualDesktop.value = await getVirtualDesktop()
  } catch { /* 浏览器演示使用默认值 */ }
  try {
    windows.value = await listWindows()
  } catch {
    windows.value = []
  }
  resize()
})

onBeforeUnmount(() => {
  unescape?.()
  unconfirm?.()
  window.removeEventListener('resize', resize)
  window.removeEventListener('mousemove', onMove)
  window.removeEventListener('mousedown', onDown)
  window.removeEventListener('mouseup', onUp)
  window.removeEventListener('keydown', onKey)
})
</script>

<template>
  <div
    class="overlay-root"
    :data-mode="mode"
    :data-window-count="windowZones.length"
    :data-selected-hwnd="selectedHwnd ?? ''"
  >
    <canvas ref="canvas" />
    <RegionHint :rect="rect" :snapping="snapping" :mode="mode" />
    <div class="mode-tag"><span class="mode-dot" />{{ modeLabel }}</div>
    <div v-if="isWindowMode" class="window-chip" data-testid="active-window">
      {{ activeWindow ? `已识别：${activeWindow.title}` : '移动鼠标高亮窗口，单击选中' }}
    </div>

    <div class="confirm" v-if="rect && rect.width > 0 && rect.height > 0">
      <button class="ios-btn ios-btn--gray ios-btn--small" :disabled="submitting" @click.stop="cancel">取消</button>
      <button class="ios-btn ios-btn--filled ios-btn--small" :disabled="submitting" @click.stop="confirm">
        {{ submitting ? '正在提交…' : isWindowMode ? '截图并保存' : '完成' }}
      </button>
    </div>

    <!-- Esc 无效时的兜底：始终可见的可点击取消按钮 -->
    <button class="cancel-fab" title="取消 (Esc)" @click.stop="cancel">✕</button>
    <div v-if="hint" class="hint-toast">{{ hint }}</div>
  </div>
</template>

<style>
html, body, #app { background: transparent !important; }
</style>

<style scoped>
.overlay-root { position: fixed; inset: 0; background: transparent; cursor: crosshair; }
canvas { display: block; width: 100%; height: 100%; }
.mode-tag {
  position: absolute;
  top: max(var(--sp-4), env(safe-area-inset-top));
  left: var(--window-pad);
  display: inline-flex;
  align-items: center;
  gap: var(--sp-2);
  padding: var(--sp-2) var(--sp-3);
  border-radius: var(--r-pill);
  background: var(--surface);
  border: 1px solid var(--border);
  box-shadow: var(--shadow-rest);
  color: var(--text-primary);
  font-size: var(--fs-body);
  font-weight: var(--fw-body);
  pointer-events: none;
}
.mode-dot { width: 8px; height: 8px; border-radius: var(--r-pill); background: var(--primary); }
.window-chip {
  position: absolute;
  top: calc(max(var(--sp-4), env(safe-area-inset-top)) + 44px);
  left: var(--window-pad);
  max-width: min(520px, 70vw);
  padding: var(--sp-2) var(--sp-3);
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: var(--r-button);
  background: var(--surface);
  box-shadow: var(--shadow-rest);
  color: var(--text-primary);
  font-size: var(--fs-caption);
  text-overflow: ellipsis;
  white-space: nowrap;
  pointer-events: none;
}
.confirm {
  position: absolute;
  bottom: calc(var(--sp-6) + env(safe-area-inset-bottom));
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  gap: var(--sp-2);
  padding: var(--sp-2);
  border-radius: var(--r-card);
  background: var(--surface);
  border: 1px solid var(--border);
  box-shadow: var(--shadow-hover);
}
.cancel-fab {
  position: absolute;
  top: max(var(--sp-4), env(safe-area-inset-top));
  right: var(--window-pad);
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  border-radius: var(--r-pill);
  background: var(--surface);
  border: 1px solid var(--border);
  box-shadow: var(--shadow-rest);
  color: var(--text-secondary);
  font-size: 14px;
}
.cancel-fab:hover { background: var(--surface-hover); color: var(--text-primary); }
.hint-toast {
  position: absolute;
  top: 50%; left: 50%;
  transform: translate(-50%, -50%);
  padding: var(--sp-2) var(--sp-4);
  border-radius: var(--r-card);
  background: var(--surface);
  border: 1px solid var(--border);
  box-shadow: var(--shadow-hover);
  color: var(--text-primary);
  font-size: var(--fs-body);
  pointer-events: none;
}
</style>
