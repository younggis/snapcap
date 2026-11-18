<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { emitScreenshotSaved, onCaptureError, openOutputDir, openRegionOverlay } from '../ipc'
import { useSettings } from '../composables/useSettings'
import { useCapture } from '../composables/useCapture'
import { useHotkey, parseCombo } from '../composables/useHotkey'
import { useTheme } from '../composables/useTheme'
import { showEditor, showRecorder, hideSettings } from '../window'
import Toast from '../components/Toast.vue'
import {
  ERROR_REMEDIATIONS,
  type CaptureTarget,
  type HotkeyPayload,
  type RecordingOptions,
  type ScreenshotOptions,
} from '../types'

const { settings, loading, load, save } = useSettings()
const capture = useCapture()
const { theme, set: setTheme } = useTheme()

const ready = ref(false)
type Tab = 'home' | 'screenshot' | 'capture' | 'hotkeys' | 'about' | 'preferences'
const activeTab = ref<Tab>('home')
const prevTab = ref<Tab>('home')
const savedFlash = ref(false)
const toast = ref('')
const toastKind = ref<'info' | 'error' | 'success'>('info')
let toastTimer: number | undefined
let savedTimer: number | undefined

function notify(message: string, kind: 'info' | 'error' | 'success' = 'info') {
  toast.value = message
  toastKind.value = kind
  window.clearTimeout(toastTimer)
  toastTimer = window.setTimeout(() => (toast.value = ''), 3800)
}

watch(settings, () => {
  if (!ready.value) return
  window.clearTimeout(savedTimer)
  savedTimer = window.setTimeout(async () => {
    await save()
    savedFlash.value = true
    window.clearTimeout(savedTimer)
    savedTimer = window.setTimeout(() => (savedFlash.value = false), 1400)
  }, 350)
}, { deep: true })

onMounted(async () => {
  await load()
  await capture.listen()
  await onCaptureError((err) => {
    const tip = ERROR_REMEDIATIONS[err.code] ?? err.remediation
    notify(`${err.code}: ${err.message}${tip ? ` — ${tip}` : ''}`, 'error')
  })
  ready.value = true
})

onBeforeUnmount(() => {
  window.clearTimeout(toastTimer)
  window.clearTimeout(savedTimer)
})

/* ---------------- hotkeys ---------------- */
type HotkeyKey = 'region' | 'full' | 'window' | 'record' | 'pause' | 'annotate'
const HOTKEY_LABELS: Record<HotkeyKey, string> = {
  region: '区域截图', full: '全屏截图', window: '窗口截图',
  record: '开始录制', pause: '暂停 / 继续', annotate: '打开标注',
}
const hotkeyKeys = Object.keys(HOTKEY_LABELS) as HotkeyKey[]
const capturingKey = ref<HotkeyKey | null>(null)

function startCaptureKey(key: HotkeyKey) { capturingKey.value = key }
function onHotkeyCapture(e: KeyboardEvent) {
  if (!capturingKey.value) return
  e.preventDefault()
  e.stopPropagation()
  if (e.key === 'Escape') { capturingKey.value = null; return }
  const mods = [e.ctrlKey ? 'Ctrl' : '', e.altKey ? 'Alt' : '', e.shiftKey ? 'Shift' : ''].filter(Boolean)
  const key = e.key.length === 1 ? e.key.toUpperCase() : e.key
  settings.value.hotkeys[capturingKey.value] = [...mods, key].join('+')
  capturingKey.value = null
  notify('快捷键已更新', 'success')
}

function handleHotkey(action: HotkeyPayload['action']) {
  if (action === 'region') quickCapture('region')
  else if (action === 'full') quickCapture('full')
  else if (action === 'window') quickCapture('window')
  else if (action === 'record') startRecording()
  else if (action === 'pause') capture.sessionState.value === 'paused' ? capture.resumeRecording() : capture.pauseRecording()
  else if (action === 'annotate') showEditor()
  else if (action === 'stop') capture.stopRecording()
}
useHotkey(handleHotkey)

/* ---------------- actions ---------------- */
const screenshotOptions = (): ScreenshotOptions => ({
  format: settings.value.screenshot.format,
  jpeg_quality: settings.value.screenshot.jpeg_quality,
  include_cursor: settings.value.screenshot.include_cursor,
})

function errorText(e: unknown): string {
  if (e instanceof Error) return e.message
  if (e && typeof e === 'object' && 'message' in e) return String((e as { message: unknown }).message)
  return String(e)
}

async function quickCapture(kind: 'region' | 'full' | 'window') {
  notify('正在抓取屏幕…', 'info')
  let target: CaptureTarget
  try {
    if (kind === 'region') {
      const selection = await openRegionOverlay('screenshot')
      if (selection.cancelled) return
      target = { kind: 'region', rect: selection.rect }
    } else if (kind === 'full') {
      target = { kind: 'full' }
    } else {
      // 打开窗口选择器，由用户点击要捕获的窗口（不再用前台窗口哨兵值）。
      const selection = await openRegionOverlay('window')
      if (selection.cancelled || selection.hwnd == null) return
      target = { kind: 'window', hwnd: selection.hwnd, include_border: true }
    }
    await capture.capture(target, screenshotOptions())
  } catch (e) {
    notify(`截图失败：${errorText(e)}`, 'error')
    return
  }

  // Enter / 完成即落盘到输出目录，再打开标注编辑器；无需再点一次保存。
  let saved = ''
  try {
    saved = await capture.output('file')
  } catch (e) {
    notify(`截图已生成，但自动保存失败：${errorText(e)}`, 'error')
  }
  if (saved) notify(`截图已保存：${saved}`, 'success')
  else if (!capture.error.value) notify('截图完成，正在打开标记', 'success')
  await showEditor()
  if (saved) await emitScreenshotSaved(saved)
}

async function startRecording() {
  const options: RecordingOptions = {
    fps: settings.value.capture.fps,
    capture_cursor: settings.value.capture.cursor,
    click_highlight: settings.value.capture.click_highlight,
    system_audio: settings.value.audio.system,
    mic_audio: settings.value.audio.mic,
  }
  notify('正在启动录制…', 'info')
  await capture.startRecording({ kind: 'full' }, options)
  notify('录制已开始，已切换到录制控制台', 'success')
  // 录制开始后隐藏主界面，只保留录制控制台（单一界面，避免两个窗口干扰）。
  await showRecorder()
  await hideSettings()
}

function comboValid(combo: string) { return parseCombo(combo).key.length > 0 }

function openPreferences() { prevTab.value = activeTab.value; activeTab.value = 'preferences' }
function closePreferences() { activeTab.value = prevTab.value }
function goHotkeys() { activeTab.value = 'hotkeys' }

/* ---------------- platform-aware keycaps ---------------- */
const isMac = /Mac|iPhone|iPad/i.test(navigator.platform) || /Mac/i.test(navigator.userAgent)
function keyLabel(token: string): string {
  if (token === 'Alt') return isMac ? '⌥' : 'Alt'
  if (token === 'Ctrl') return isMac ? '⌘' : 'Ctrl'
  if (token === 'Shift') return isMac ? '⇧' : 'Shift'
  return token
}
function comboKeys(combo: string): string[] {
  return combo.split('+').map((s) => keyLabel(s.trim())).filter(Boolean)
}
/** 单个键帽里的完整组合：Alt + A（macOS 为 ⌥ + A）。 */
function comboLabel(combo: string): string {
  return comboKeys(combo).join(' + ')
}
function formatDuration(ms: number): string {
  const total = Math.floor(ms / 1000)
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
}

/* ---------------- tabs (one Fluent icon set, regular + filled) ---------------- */
type TabDef = { id: Tab; label: string; icon: string; filled: string }
const tabs: TabDef[] = [
  { id: 'home', label: '首页', icon: 'M3 11l9-8 9 8M5.5 9.5V21h13V9.5', filled: 'M12 2 2 11h3v10h14V11h3z' },
  { id: 'screenshot', label: '截图', icon: 'M4 8V4h4M20 8V4h-4M4 16v4h4M20 16v4h-4M8 8h8v8H8z', filled: 'M4 4h5v2H6v3H4zM15 4h5v5h-2V6h-3zM4 15h2v3h3v2H4zM18 15h2v5h-5v-2h3zM8 8h8v8H8z' },
  { id: 'capture', label: '录制', icon: 'M3 6h12v12H3zM15 10l6-3v10l-6-3z', filled: 'M3 6h12v12H3zM15 10l6-3v10l-6-3z' },
  { id: 'hotkeys', label: '热键', icon: 'M3 6h18v12H3zM6 9h.01M10 9h.01M14 9h.01M18 9h.01M7 13h10', filled: 'M3 6h18v12H3zM6 9h2v2H6zM11 9h2v2h-2zM16 9h2v2h-2zM7 13v2h10v-2z' },
  { id: 'about', label: '关于', icon: 'M12 8h.01M11 12h1v5h1M12 3a9 9 0 100 18 9 9 0 000-18z', filled: 'M12 2a10 10 0 100 20 10 10 0 000-20zm1 15h-2v-6h2zm0-8h-2V7h2z' },
]

type EntryDef = { id: 'region' | 'full' | 'window' | 'record'; label: string; tone: string; path: string; solid?: boolean }
const entries: EntryDef[] = [
  { id: 'region', label: '区域', tone: 'solid', path: 'M4 9V6a2 2 0 012-2h3M15 4h3a2 2 0 012 2v3M20 15v3a2 2 0 01-2 2h-3M9 20H6a2 2 0 01-2-2v-3M9 9h6v6H9z' },
  { id: 'full', label: '全屏', tone: 'tint', path: 'M8 3H5a2 2 0 00-2 2v3M16 3h3a2 2 0 012 2v3M21 16v3a2 2 0 01-2 2h-3M8 21H5a2 2 0 01-2-2v-3' },
  { id: 'window', label: '窗口', tone: 'tint', path: 'M3 5h18v14H3zM3 9h18M6 7h.01M9 7h.01' },
  { id: 'record', label: '录制', tone: 'danger', solid: true, path: 'M12 4a8 8 0 100 16 8 8 0 000-16z' },
]

function entryHotkey(id: EntryDef['id']): string {
  return id === 'record' ? settings.value.hotkeys.record : settings.value.hotkeys[id]
}
function onEntry(id: EntryDef['id']) {
  if (id === 'record') {
    // 已在录制中：点按「录制中」直接切到录制控制台（暂停/停止）。
    if (capture.isRecording.value) { showRecorder(); return }
    startRecording()
  } else quickCapture(id)
}
</script>

<template>
  <div class="main-window" @keydown="onHotkeyCapture">
    <Toast :message="toast" :kind="toastKind" @close="toast = ''" />

    <div v-if="loading" class="loading-state">
      <div class="spinner" />
      <span>正在载入…</span>
    </div>

    <template v-else>
      <div class="scroll-area">
        <!-- ============ HOME ============ -->
        <template v-if="activeTab === 'home'">
          <section class="entry-grid">
            <button
              v-for="e in entries"
              :key="e.id"
              class="entry-card"
              :class="[`tone-${e.tone}`, { recording: e.id === 'record' && capture.isRecording.value }]"
              @click="onEntry(e.id)"
            >
              <span class="entry-icon">
                <svg viewBox="0 0 24 24" width="var(--size-entry-glyph)" height="var(--size-entry-glyph)"
                  :fill="e.solid ? 'currentColor' : 'none'" :stroke="e.solid ? 'none' : 'currentColor'"
                  :stroke-width="e.solid ? 0 : 'var(--stroke-icon)'"
                  stroke-linecap="round" stroke-linejoin="round">
                  <path :d="e.path" />
                </svg>
              </span>
              <span class="entry-label">
                {{ e.id === 'record' && capture.isRecording.value ? '录制中' : e.label }}
              </span>
              <span v-if="e.id === 'record' && capture.isRecording.value" class="entry-timer mono">
                {{ formatDuration(capture.stats.value.duration_ms) }}
              </span>
              <span v-else class="entry-keys">
                <span class="keycap">{{ comboLabel(entryHotkey(e.id)) }}</span>
              </span>
            </button>
          </section>

          <div class="ios-group home-group">
            <button class="ios-row clickable settings-entry" @click="openPreferences">
              <span class="list-icon">
                <svg viewBox="0 0 24 24" width="var(--size-list-glyph)" height="var(--size-list-glyph)" fill="none" stroke="currentColor" stroke-width="var(--stroke-icon)" stroke-linecap="round">
                  <path d="M4 7h16M4 12h16M4 17h10" /><circle cx="18" cy="17" r="2.5" />
                </svg>
              </span>
              <span class="ios-row-main">
                <span class="ios-row-label">偏好设置</span>
                <span class="ios-row-detail">启动 · 托盘 · 输出 · 语言 · 外观</span>
              </span>
              <span class="ios-chevron">›</span>
            </button>
            <button class="ios-row clickable settings-entry" @click="showRecorder">
              <span class="list-icon">
                <svg viewBox="0 0 24 24" width="var(--size-list-glyph)" height="var(--size-list-glyph)" fill="none" stroke="currentColor" stroke-width="var(--stroke-icon)" stroke-linecap="round">
                  <circle cx="12" cy="12" r="9" /><circle cx="12" cy="12" r="3.4" />
                </svg>
              </span>
              <span class="ios-row-main">
                <span class="ios-row-label">录制控制台</span>
                <span class="ios-row-detail">查看实时状态与录制文件</span>
              </span>
              <span class="ios-chevron">›</span>
            </button>
          </div>

          <div class="ios-section-footer home-hint">
            所有功能也可通过全局快捷键触发，详见
            <button class="link" @click="goHotkeys">热键</button>
            标签。
          </div>
        </template>

        <!-- ============ PREFERENCES ============ -->
        <div v-else-if="activeTab === 'preferences'" class="panel">
          <button class="back-row" @click="closePreferences">
            <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M15 5l-7 7 7 7" /></svg>
            返回
          </button>

          <div class="ios-section-header">外观</div>
          <div class="ios-group">
            <div class="ios-row">
              <span class="ios-row-label">主题</span>
              <div class="segmented">
                <button :class="{ active: theme === 'system' }" @click="setTheme('system')">跟随系统</button>
                <button :class="{ active: theme === 'light' }" @click="setTheme('light')">浅色</button>
                <button :class="{ active: theme === 'dark' }" @click="setTheme('dark')">深色</button>
              </div>
            </div>
          </div>

          <div class="ios-section-header">启动与托盘</div>
          <div class="ios-group">
            <div class="ios-row">
              <div class="ios-row-main">
                <span class="ios-row-label">开机自启动</span>
                <span class="ios-row-detail">登录 Windows 后自动常驻托盘</span>
              </div>
              <button class="switch" :class="{ on: settings.general.autostart }" @click="settings.general.autostart = !settings.general.autostart" />
            </div>
            <div class="ios-row">
              <div class="ios-row-main">
                <span class="ios-row-label">关闭时隐藏到托盘</span>
                <span class="ios-row-detail">关闭窗口不退出程序</span>
              </div>
              <button class="switch" :class="{ on: settings.general.minimize_to_tray }" @click="settings.general.minimize_to_tray = !settings.general.minimize_to_tray" />
            </div>
            <div class="ios-row">
              <div class="ios-row-main"><span class="ios-row-label">语言</span></div>
              <select v-model="settings.general.language" class="compact-select">
                <option value="zh-CN">简体中文</option>
                <option value="en-US">English</option>
              </select>
            </div>
          </div>

          <div class="ios-section-header">输出</div>
          <div class="ios-group">
            <div class="ios-row column">
              <div class="ios-row-main">
                <span class="ios-row-label">保存位置</span>
                <span class="ios-row-detail">{{ settings.paths.output_dir }}</span>
              </div>
              <input v-model="settings.paths.output_dir" type="text" class="full-input" />
            </div>
            <div class="ios-row column">
              <div class="ios-row-main">
                <span class="ios-row-label">文件命名</span>
                <span class="ios-row-detail">{yyyyMMdd-HHmmss}</span>
              </div>
              <input v-model="settings.paths.naming" type="text" class="full-input" />
            </div>
            <div class="ios-row clickable" @click="openOutputDir">
              <span class="ios-row-label link">在文件资源管理器中打开</span>
              <span class="ios-chevron">›</span>
            </div>
          </div>
        </div>

        <!-- ============ SCREENSHOT ============ -->
        <div v-else-if="activeTab === 'screenshot'" class="panel">
          <div class="ios-section-header">图片格式</div>
          <div class="ios-group">
            <div class="ios-row">
              <span class="ios-row-label">默认格式</span>
              <div class="segmented">
                <button :class="{ active: settings.screenshot.format === 'png' }" @click="settings.screenshot.format = 'png'">PNG</button>
                <button :class="{ active: settings.screenshot.format === 'jpeg' }" @click="settings.screenshot.format = 'jpeg'">JPEG</button>
              </div>
            </div>
            <div v-if="settings.screenshot.format === 'jpeg'" class="ios-row column">
              <div class="row-between">
                <span class="ios-row-label">JPEG 质量</span>
                <span class="ios-row-value mono">{{ settings.screenshot.jpeg_quality }}</span>
              </div>
              <input v-model.number="settings.screenshot.jpeg_quality" type="range" min="50" max="100" class="full-slider" />
            </div>
            <div class="ios-row">
              <div class="ios-row-main">
                <span class="ios-row-label">截图包含光标</span>
                <span class="ios-row-detail">录制光标由程序单独绘制</span>
              </div>
              <button class="switch" :class="{ on: settings.screenshot.include_cursor }" @click="settings.screenshot.include_cursor = !settings.screenshot.include_cursor" />
            </div>
          </div>
          <div class="ios-section-footer">PNG 为无损格式，适合含文字与线条的画面；JPEG 体积更小，可调节质量。</div>
        </div>

        <!-- ============ CAPTURE ============ -->
        <div v-else-if="activeTab === 'capture'" class="panel">
          <div class="ios-section-header">视频</div>
          <div class="ios-group">
            <div class="ios-row">
              <span class="ios-row-label">帧率</span>
              <div class="segmented">
                <button :class="{ active: settings.capture.fps === 60 }" @click="settings.capture.fps = 60">60 fps</button>
                <button :class="{ active: settings.capture.fps === 30 }" @click="settings.capture.fps = 30">30 fps</button>
              </div>
            </div>
            <div class="ios-row">
              <span class="ios-row-label">编码器</span>
              <select v-model="settings.video.hardware" class="compact-select wide">
                <option value="auto">自动（推荐）</option>
                <option value="nvidia">NVIDIA NVENC</option>
                <option value="intel">Intel Quick Sync</option>
                <option value="amd">AMD AMF</option>
                <option value="software">软件 H.264</option>
              </select>
            </div>
            <div class="ios-row">
              <span class="ios-row-label">质量模式</span>
              <select v-model="settings.video.quality.mode" class="compact-select">
                <option value="cqp">恒定质量 CQP</option>
                <option value="vbr">恒定码率 VBR</option>
              </select>
            </div>
            <div v-if="settings.video.quality.mode === 'cqp'" class="ios-row column">
              <div class="row-between">
                <span class="ios-row-label">质量值 <span class="ios-row-detail">越小越清晰</span></span>
                <span class="ios-row-value mono">{{ settings.video.quality.cqp }}</span>
              </div>
              <input v-model.number="settings.video.quality.cqp" type="range" min="16" max="28" class="full-slider" />
            </div>
            <div class="ios-row column">
              <div class="row-between">
                <span class="ios-row-label">关键帧间隔</span>
                <span class="ios-row-value mono">{{ settings.video.keyframe_sec }} 秒</span>
              </div>
              <input v-model.number="settings.video.keyframe_sec" type="range" min="1" max="10" class="full-slider" />
            </div>
          </div>

          <div class="ios-section-header">画面效果</div>
          <div class="ios-group">
            <div class="ios-row">
              <span class="ios-row-label">绘制光标</span>
              <button class="switch" :class="{ on: settings.capture.cursor }" @click="settings.capture.cursor = !settings.capture.cursor" />
            </div>
            <div class="ios-row">
              <span class="ios-row-label">点击高亮</span>
              <button class="switch" :class="{ on: settings.capture.click_highlight }" @click="settings.capture.click_highlight = !settings.capture.click_highlight" />
            </div>
          </div>

          <div class="ios-section-header">音频</div>
          <div class="ios-group">
            <div class="ios-row">
              <span class="ios-row-label">系统声音</span>
              <button class="switch" :class="{ on: settings.audio.system }" @click="settings.audio.system = !settings.audio.system" />
            </div>
            <div class="ios-row">
              <span class="ios-row-label">麦克风</span>
              <button class="switch" :class="{ on: settings.audio.mic }" @click="settings.audio.mic = !settings.audio.mic" />
            </div>
            <div v-if="settings.audio.system" class="ios-row column">
              <div class="row-between">
                <span class="ios-row-label">系统音量</span>
                <span class="ios-row-value mono">{{ settings.audio.system_gain.toFixed(1) }}×</span>
              </div>
              <input v-model.number="settings.audio.system_gain" type="range" min="0" max="2" step="0.1" class="full-slider" />
            </div>
            <div v-if="settings.audio.mic" class="ios-row column">
              <div class="row-between">
                <span class="ios-row-label">麦克风音量</span>
                <span class="ios-row-value mono">{{ settings.audio.mic_gain.toFixed(1) }}×</span>
              </div>
              <input v-model.number="settings.audio.mic_gain" type="range" min="0" max="2" step="0.1" class="full-slider" />
            </div>
          </div>
        </div>

        <!-- ============ HOTKEYS ============ -->
        <div v-else-if="activeTab === 'hotkeys'" class="panel">
          <div class="ios-section-header">全局快捷键</div>
          <div class="ios-group">
            <button v-for="key in hotkeyKeys" :key="key" class="ios-row clickable" @click="startCaptureKey(key)">
              <span class="ios-row-label">{{ HOTKEY_LABELS[key] }}</span>
              <span v-if="capturingKey === key" class="hotkey-capturing">按下组合键…</span>
              <span v-else class="entry-keys">
                <span class="keycap" :class="{ invalid: !comboValid(settings.hotkeys[key]) }">{{ comboLabel(settings.hotkeys[key]) }}</span>
              </span>
            </button>
          </div>
          <div class="ios-section-footer">点按任意一行后按下新的组合键，按 Esc 取消。修改会自动存储并重新注册全局热键。</div>
        </div>

        <!-- ============ ABOUT ============ -->
        <div v-else class="panel about">
          <div class="about-hero">
            <div class="about-icon">
              <svg viewBox="0 0 24 24" width="30" height="30" fill="none" stroke="var(--on-accent)" stroke-width="var(--stroke-icon)" stroke-linecap="round">
                <path d="M4 8V6a2 2 0 012-2h2M16 4h2a2 2 0 012 2v2M20 16v2a2 2 0 01-2 2h-2M8 20H6a2 2 0 01-2-2v-2" />
                <rect x="8" y="8" width="8" height="8" rx="1" />
              </svg>
            </div>
            <h2>SnapCap</h2>
            <p class="version">版本 0.1.0</p>
          </div>

          <div class="ios-section-header">隐私</div>
          <div class="ios-group">
            <div class="ios-row">
              <div class="ios-row-main">
                <span class="ios-row-label">完全本地处理</span>
                <span class="ios-row-detail">零网络请求 · 零遥测，内容不上传</span>
              </div>
              <span class="privacy-check">✓</span>
            </div>
          </div>
          <div class="ios-section-footer">基于 Tauri 2 + Vue 3 构建。系统采集授权弹窗由 Windows 提供，程序不会绕过。</div>
          <button class="ios-btn ios-btn--tinted about-action" @click="openOutputDir">打开输出目录</button>
        </div>
      </div>

      <!-- ============ TAB BAR ============ -->
      <nav v-if="activeTab !== 'preferences'" class="tab-bar">
        <button
          v-for="t in tabs"
          :key="t.id"
          class="tab-item"
          :class="{ active: activeTab === t.id }"
          @click="activeTab = t.id"
        >
          <span class="tab-icon">
            <svg v-if="activeTab === t.id" viewBox="0 0 24 24" width="22" height="22" fill="currentColor" stroke="none">
              <path :d="t.filled" />
            </svg>
            <svg v-else viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="var(--stroke-icon)" stroke-linecap="round" stroke-linejoin="round">
              <path :d="t.icon" />
            </svg>
          </span>
          <span class="tab-label">{{ t.label }}</span>
        </button>
      </nav>
    </template>
  </div>
</template>

<style scoped>
.main-window {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg);
}

/* ---------------- loading ---------------- */
.loading-state {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--sp-3);
  color: var(--text-secondary);
  font-size: var(--fs-caption);
}
.spinner {
  width: 24px;
  height: 24px;
  border: 2px solid var(--border);
  border-top-color: var(--primary);
  border-radius: var(--r-pill);
  animation: spin 0.8s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }

/* ---------------- scroll area ---------------- */
.scroll-area {
  flex: 1;
  overflow-y: auto;
  padding: var(--window-pad) var(--window-pad) var(--sp-6);
}

/* ---------------- entry cards ---------------- */
.entry-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--sp-3);
}
@media (max-width: 440px) {
  .entry-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
}
.entry-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--sp-2);
  min-width: 0;
  /* 卡片高度 ≈ 48 图标 + 8 + 20 文字 + 8 + 20 键帽 + 32 内边距 = 136 */
  padding: var(--sp-4);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--r-card);
  box-shadow: var(--shadow-rest);
  transition: background var(--dur) var(--ease), box-shadow var(--dur) var(--ease), transform var(--dur) var(--ease);
}
.entry-card:hover { box-shadow: var(--shadow-hover); transform: translateY(-1px); }
.entry-card:active { transform: translateY(0); background: var(--surface-pressed); }
.entry-icon {
  display: grid;
  place-items: center;
  width: var(--size-entry-icon);
  height: var(--size-entry-icon);
  border-radius: var(--r-icon-entry);
  flex: none;
}
.tone-solid .entry-icon { background: var(--primary); color: var(--on-accent); }
.tone-tint .entry-icon { background: var(--primary-tint); color: var(--primary); }
.tone-danger .entry-icon { background: var(--danger-tint); color: var(--danger); }
.entry-card.recording .entry-icon { background: var(--danger); color: var(--on-accent); }
.entry-label {
  font-size: var(--fs-body);
  font-weight: var(--fw-body);
  color: var(--text-primary);
  text-align: center;
  white-space: nowrap;
}
.entry-keys { display: flex; align-items: center; gap: var(--sp-1); }
.entry-timer {
  font-size: var(--fs-caption);
  font-weight: var(--fw-body);
  color: var(--danger);
  font-variant-numeric: tabular-nums;
}

/* ---------------- keycaps ---------------- */
.keycap {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 62px;
  height: var(--h-keycap);
  padding: 0 var(--sp-2);
  white-space: nowrap;
  border: 1px solid var(--border);
  border-radius: var(--r-keycap);
  background: var(--surface);
  color: var(--text-secondary);
  font-family: var(--font-mono);
  font-size: var(--fs-keycap);
  font-weight: var(--fw-keycap);
  line-height: 1;
}
.keycap.invalid { border-color: var(--danger); color: var(--danger); }
.hotkey-capturing {
  font-size: var(--fs-caption);
  color: var(--primary);
  background: var(--primary-tint);
  padding: var(--sp-1) var(--sp-2);
  border-radius: var(--r-keycap);
}

/* ---------------- home list ---------------- */
.home-group { margin-top: var(--sp-4); }
.settings-entry { min-height: var(--h-row); }
.settings-entry .ios-row-main {
  flex: 1;
  align-items: flex-start;
  gap: var(--sp-1);
}
.list-icon {
  display: grid;
  place-items: center;
  width: var(--size-list-icon);
  height: var(--size-list-icon);
  border-radius: var(--r-icon-list);
  flex: none;
  background: var(--primary-tint);
  color: var(--primary);
}
.home-hint { text-align: center; margin-top: var(--sp-3); }
.link {
  color: var(--primary);
  font: inherit;
  padding: 0;
}
.link:hover { text-decoration: underline; }
.ios-row-label.link { color: var(--primary); }

/* ---------------- preferences ---------------- */
.back-row {
  display: inline-flex;
  align-items: center;
  gap: var(--sp-1);
  margin-bottom: var(--sp-3);
  padding: var(--sp-1) var(--sp-2) var(--sp-1) 0;
  color: var(--primary);
  font-size: var(--fs-body);
  font-weight: var(--fw-body);
}
.back-row:hover { text-decoration: underline; }
.compact-select { max-width: 152px; }
.compact-select.wide { max-width: 168px; }
.full-input { width: 100%; }
.full-slider { width: 100%; }
.row-between { display: flex; align-items: center; justify-content: space-between; width: 100%; }
.ios-row.column { flex-direction: column; align-items: stretch; gap: var(--sp-2); padding-top: var(--sp-3); padding-bottom: var(--sp-3); }
.mono { font-family: var(--font-mono); font-variant-numeric: tabular-nums; }
.privacy-check {
  display: grid;
  place-items: center;
  width: 22px;
  height: 22px;
  border-radius: var(--r-pill);
  color: var(--primary);
  background: var(--primary-tint);
  font-weight: var(--fw-title);
}

/* ---------------- about ---------------- */
.about { text-align: center; }
.about-hero { padding: var(--sp-2) 0 var(--sp-4); }
.about-icon {
  display: grid;
  place-items: center;
  width: 60px;
  height: 60px;
  margin: 0 auto var(--sp-2);
  border-radius: var(--r-icon-entry);
  background: var(--primary);
}
.about-hero h2 {
  margin: 0;
  font-size: var(--fs-title);
  font-weight: var(--fw-title);
  color: var(--text-primary);
}
.about-hero .version {
  margin: var(--sp-1) 0 0;
  font-size: var(--fs-caption);
  color: var(--text-secondary);
}
.about-action { width: 100%; margin-top: var(--sp-4); }

/* ---------------- tab bar ---------------- */
.tab-bar {
  display: flex;
  height: var(--h-tabbar);
  min-height: var(--h-tabbar);
  background: var(--bg);
  border-top: 1px solid var(--border);
  flex: none;
}
.tab-item {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 2px;
  min-height: 44px;
  margin: var(--sp-1) var(--sp-2);
  padding: var(--sp-1) 0;
  border-radius: var(--r-pill);
  color: var(--text-secondary);
  transition: color var(--dur) var(--ease), background var(--dur) var(--ease);
}
.tab-item:hover { color: var(--text-primary); background: var(--surface-hover); }
/* 选中态：primary 色 + 填充图标 + 胶囊底包住「图标 + 标签」 */
.tab-item.active {
  color: var(--primary);
  background: var(--primary-tint);
}
.tab-item.active:hover { background: var(--primary-tint); }
.tab-icon {
  display: grid;
  place-items: center;
  height: 22px;
}
.tab-label {
  font-size: var(--fs-caption);
  font-weight: var(--fw-caption);
}
.tab-item.active .tab-label { font-weight: var(--fw-body); }
</style>
