<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import StatsBadge from '../components/StatsBadge.vue'
import { useCapture } from '../composables/useCapture'
import { useSettings } from '../composables/useSettings'
import { localBus, onHotkey, openRegionOverlay, openOutputDir, revealFile } from '../ipc'
import { ERROR_REMEDIATIONS, type CaptureTarget, type RecordingResult, type SelectionPayload } from '../types'
import { showSettings } from '../window'

const capture = useCapture()
const { settings } = useSettings()

/** 收起态：窗口缩成一条细栏，只显示录制状态/计时，不遮挡后面的内容。 */
const compact = ref(false)
async function toggleCompact() {
  compact.value = !compact.value
  try {
    const [{ getCurrentWebviewWindow }, { PhysicalSize }] = await Promise.all([
      import('@tauri-apps/api/webviewWindow'),
      import('@tauri-apps/api/dpi'),
    ])
    const win = getCurrentWebviewWindow()
    await win.setSize(compact.value ? new PhysicalSize(300, 96) : new PhysicalSize(380, 520))
  } catch { /* 浏览器演示下仅切换样式 */ }
}

const targetKind = ref<'full' | 'display' | 'region' | 'window'>('full')
const region = ref<SelectionPayload | null>(null)
const selectedHwnd = ref<number | null>(null)
const lastResult = ref<RecordingResult | null>(null)

const target = computed<CaptureTarget>(() => {
  switch (targetKind.value) {
    case 'full': return { kind: 'full' }
    case 'display': return { kind: 'display', monitor_handle: 1 }
    case 'region': return { kind: 'region', rect: region.value?.rect ?? { x: 0, y: 0, width: 1280, height: 720 } }
    case 'window': return { kind: 'window', hwnd: selectedHwnd.value ?? 0, include_border: true }
  }
})

const canStart = computed(() => !capture.isRecording.value && !capture.busy.value)
const stateLabel = computed(() => {
  switch (capture.sessionState.value) {
    case 'recording': return '正在录制'
    case 'starting': return '正在启动…'
    case 'paused': return '已暂停'
    case 'stopping': return '正在停止…'
    default: return '准备就绪'
  }
})

onMounted(async () => {
  await capture.listen()
  localBus.on('overlay:selection', (p) => {
    const sel = p as SelectionPayload
    if (!sel.cancelled) region.value = sel
  })
  await onHotkey((p) => {
    if (p.action === 'stop' && capture.isRecording.value) stop()
  })
})

async function pickRegion() {
  targetKind.value = 'region'
  const sel = await openRegionOverlay('record')
  if (!sel.cancelled) region.value = sel
}

async function pickWindow() {
  targetKind.value = 'window'
  const sel = await openRegionOverlay('window')
  if (!sel.cancelled && sel.hwnd != null) {
    region.value = sel
    selectedHwnd.value = sel.hwnd
  }
}

async function start() {
  lastResult.value = null
  capture.dismissError()
  await capture.startRecording(target.value, {
    fps: settings.value.capture.fps,
    capture_cursor: settings.value.capture.cursor,
    click_highlight: settings.value.capture.click_highlight,
    system_audio: settings.value.audio.system,
    mic_audio: settings.value.audio.mic,
  })
}

async function stop() {
  lastResult.value = await capture.stopRecording()
  // 录制结束后恢复主界面（单界面流程闭环）。
  await showSettings()
}

function formatSize(n: number) {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}
function formatDuration(ms: number) {
  const total = Math.floor(ms / 1000)
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
}

const remediation = computed(() =>
  capture.error.value
    ? ERROR_REMEDIATIONS[capture.error.value.code] ?? capture.error.value.remediation
    : '',
)
</script>

<template>
  <div class="recorder-window">
    <header class="nav-bar">
      <div class="nav-title">{{ compact ? 'SnapCap' : '录制控制台' }}</div>
      <div class="nav-right">
        <span class="status-badge" :class="{ live: capture.isRecording.value, paused: capture.isPaused.value }">
          <span class="dot" :class="{ live: capture.isRecording.value, paused: capture.isPaused.value }" />
          {{ stateLabel }}
        </span>
        <button class="collapse-btn" :title="compact ? '展开' : '收起'" @click="toggleCompact">
          <svg v-if="!compact" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
            <path d="M5 15l7-7 7 7" />
          </svg>
          <svg v-else viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
            <path d="M5 9l7 7 7-7" />
          </svg>
        </button>
      </div>
    </header>

    <!-- 收起态：极简计时条 -->
    <div v-if="compact" class="compact-bar">
      <span class="record-orb" :class="{ live: capture.isRecording.value }"><span class="orb-core" /></span>
      <span class="compact-timer mono">{{ formatDuration(capture.stats.value.duration_ms) }}</span>
      <button v-if="capture.isRecording.value" class="ios-btn ios-btn--red ios-btn--small" @click="stop">停止</button>
      <button v-else class="ios-btn ios-btn--filled ios-btn--small" @click="start">开始</button>
    </div>

    <template v-if="!compact">
    <div v-if="capture.error.value" class="error-card">
      <div class="error-head">
        <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="var(--stroke-icon)" stroke-linecap="round">
          <path d="M12 3L2 20h20L12 3zM12 9v5M12 17h.01" />
        </svg>
        {{ capture.error.value.code }}
      </div>
      <div class="error-msg">{{ capture.error.value.message }}</div>
      <div class="error-tip">{{ remediation }}</div>
      <button class="ios-btn ios-btn--tinted ios-btn--small" @click="capture.dismissError()">知道了</button>
    </div>

    <div class="scroll-area">
      <section class="hero-card" :class="{ recording: capture.isRecording.value }">
        <div class="hero-top">
          <span class="record-orb" :class="{ live: capture.isRecording.value, paused: capture.isPaused.value }">
            <span class="orb-core" />
          </span>
          <div class="hero-status">
            <span class="hero-label">{{ capture.isRecording.value ? '录制中' : capture.isPaused.value ? '已暂停' : '尚未开始' }}</span>
            <span class="hero-timer mono">{{ formatDuration(capture.stats.value.duration_ms) }}</span>
          </div>
        </div>
        <StatsBadge :stats="capture.stats.value" :state="capture.sessionState.value" />
      </section>

      <section class="section">
        <div class="ios-section-header">录制范围</div>
        <div class="segmented targets">
          <button :class="{ active: targetKind === 'full' }" @click="targetKind = 'full'">全屏</button>
          <button :class="{ active: targetKind === 'display' }" @click="targetKind = 'display'">显示器</button>
          <button :class="{ active: targetKind === 'region' }" @click="pickRegion">区域</button>
          <button :class="{ active: targetKind === 'window' }" @click="pickWindow">窗口</button>
        </div>
        <div v-if="targetKind === 'region'" class="region-hint">
          {{ region ? `${region.rect.width} × ${region.rect.height} @ ${region.rect.x}, ${region.rect.y}` : '尚未选择区域，点按「区域」开始框选' }}
        </div>
      </section>

      <section v-if="lastResult" class="result-card">
        <div class="result-head">
          <span class="result-check">✓</span>
          录制完成
        </div>
        <div class="result-row"><span>文件</span><span class="path" :title="lastResult.path">{{ lastResult.path }}</span></div>
        <div class="result-row"><span>时长</span><span class="mono">{{ formatDuration(lastResult.duration_ms) }}</span></div>
        <div class="result-row"><span>大小</span><span class="mono">{{ formatSize(lastResult.size) }}</span></div>
        <div class="result-actions">
          <button class="ios-btn ios-btn--filled ios-btn--small" @click="revealFile(lastResult.path)">打开文件位置</button>
          <button class="ios-btn ios-btn--gray ios-btn--small" @click="openOutputDir">输出目录</button>
        </div>
      </section>

      <section class="controls">
        <template v-if="!capture.isRecording.value">
          <button class="ios-btn ios-btn--red record-btn" :disabled="!canStart" @click="start">
            <span class="btn-orb" />
            开始录制
          </button>
          <div class="hotkey-hint">也可按 Alt + R 开始录制</div>
        </template>
        <template v-else>
          <div class="control-row">
            <button class="ios-btn ios-btn--gray pause-btn" @click="capture.isPaused.value ? capture.resumeRecording() : capture.pauseRecording()">
              <svg v-if="!capture.isPaused.value" viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
                <rect x="6" y="5" width="4" height="14" rx="1" /><rect x="14" y="5" width="4" height="14" rx="1" />
              </svg>
              <svg v-else viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
                <path d="M8 5l11 7-11 7z" />
              </svg>
              {{ capture.isPaused.value ? '继续' : '暂停' }}
            </button>
            <button class="ios-btn ios-btn--red stop-btn" @click="stop">
              <span class="stop-square" />
              停止
            </button>
          </div>
          <div class="hotkey-hint">Alt + P 暂停 / 继续 · 托盘菜单也可停止</div>
        </template>
      </section>
    </div>
    </template>
  </div>
</template>

<style scoped>
.recorder-window {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg);
}
.nav-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: var(--h-nav);
  padding: 0 var(--window-pad);
  border-bottom: 1px solid var(--border);
  background: var(--bg);
  flex: none;
}
.nav-title {
  font-size: var(--fs-title);
  font-weight: var(--fw-title);
  color: var(--text-primary);
}
.nav-right { display: flex; align-items: center; gap: var(--sp-2); }
.collapse-btn {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  border-radius: var(--r-button);
  color: var(--text-secondary);
  transition: background var(--dur) var(--ease);
}
.collapse-btn:hover { background: var(--surface-hover); color: var(--text-primary); }

/* 收起态：一条细栏，始终可见但不遮挡 */
.compact-bar {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  padding: var(--sp-3) var(--window-pad);
}
.compact-bar .record-orb { width: 32px; height: 32px; }
.compact-bar .orb-core { width: 12px; height: 12px; }
.compact-timer {
  flex: 1;
  font-size: 20px;
  font-weight: var(--fw-title);
  color: var(--text-primary);
}
.status-badge {
  display: inline-flex;
  align-items: center;
  gap: var(--sp-1);
  padding: var(--sp-1) var(--sp-2);
  border-radius: var(--r-pill);
  background: var(--surface);
  border: 1px solid var(--border);
  color: var(--text-secondary);
  font-size: var(--fs-keycap);
  font-weight: var(--fw-keycap);
}
.status-badge.live { background: var(--danger-tint); color: var(--danger); border-color: var(--danger-tint); }
.status-badge.paused { background: var(--surface-hover); color: var(--text-secondary); }
.status-badge .dot.live { background: var(--danger); }

.scroll-area { flex: 1; overflow-y: auto; padding: var(--window-pad); }

.hero-card {
  padding: var(--sp-4);
  border-radius: var(--r-card);
  background: var(--surface);
  border: 1px solid var(--border);
  box-shadow: var(--shadow-rest);
  margin-bottom: var(--sp-4);
  transition: box-shadow var(--dur) var(--ease);
}
.hero-card:hover { box-shadow: var(--shadow-hover); }
.hero-card.recording { border-color: var(--danger); }
.hero-top { display: flex; align-items: center; gap: var(--sp-3); margin-bottom: var(--sp-3); }
.record-orb {
  display: grid;
  place-items: center;
  width: 44px;
  height: 44px;
  border-radius: var(--r-pill);
  background: var(--surface-hover);
  flex: none;
  transition: background var(--dur) var(--ease);
}
.record-orb.live { background: var(--danger-tint); animation: orb-pulse 1.8s ease-in-out infinite; }
.record-orb.paused { background: var(--surface-hover); }
.orb-core {
  width: 16px;
  height: 16px;
  border-radius: var(--r-pill);
  background: var(--text-tertiary);
  transition: all var(--dur) var(--ease);
}
.record-orb.live .orb-core { width: 20px; height: 20px; background: var(--danger); }
.record-orb.paused .orb-core { background: var(--text-secondary); }
@keyframes orb-pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.7; }
}
.hero-status { display: flex; flex-direction: column; }
.hero-label { font-size: var(--fs-caption); color: var(--text-secondary); }
.hero-timer {
  font-size: 26px;
  font-weight: var(--fw-title);
  color: var(--text-primary);
  font-variant-numeric: tabular-nums;
  line-height: var(--lh);
}

.section { margin-bottom: var(--sp-4); }
.targets { display: flex; width: 100%; }
.targets button { flex: 1; }
.region-hint {
  margin-top: var(--sp-2);
  padding: var(--sp-2) var(--sp-3);
  border-radius: var(--r-button);
  background: var(--primary-tint);
  color: var(--primary);
  font-size: var(--fs-caption);
  font-family: var(--font-mono);
}

.error-card {
  margin: var(--sp-3) var(--window-pad) 0;
  padding: var(--sp-3);
  border-radius: var(--r-card);
  background: var(--danger-tint);
  border: 1px solid var(--danger);
  box-shadow: var(--shadow-rest);
}
.error-head {
  display: flex; align-items: center; gap: var(--sp-2);
  font-weight: var(--fw-title); color: var(--danger); font-size: var(--fs-body);
}
.error-msg { margin-top: var(--sp-2); font-size: var(--fs-caption); color: var(--text-primary); line-height: var(--lh); }
.error-tip { margin: var(--sp-2) 0 var(--sp-3); font-size: var(--fs-caption); color: var(--text-secondary); line-height: var(--lh); }

.result-card {
  padding: var(--sp-4);
  border-radius: var(--r-card);
  background: var(--surface);
  border: 1px solid var(--border);
  box-shadow: var(--shadow-rest);
  margin-bottom: var(--sp-3);
}
.result-head { display: flex; align-items: center; gap: var(--sp-2); font-weight: var(--fw-title); color: var(--text-primary); font-size: var(--fs-body); margin-bottom: var(--sp-2); }
.result-check {
  display: grid; place-items: center;
  width: 20px; height: 20px; border-radius: var(--r-pill);
  background: var(--primary); color: var(--on-accent); font-size: var(--fs-keycap);
}
.result-row { display: flex; justify-content: space-between; gap: var(--sp-2); padding: 2px 0; font-size: var(--fs-caption); color: var(--text-secondary); }
.result-row .path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-primary); max-width: 200px; }
.result-actions { display: flex; gap: var(--sp-2); margin-top: var(--sp-3); }
.result-actions .ios-btn { flex: 1; }
.mono { font-family: var(--font-mono); font-variant-numeric: tabular-nums; }

.record-btn { width: 100%; font-size: var(--fs-body); }
.btn-orb {
  width: 12px; height: 12px; border-radius: var(--r-pill);
  background: var(--on-accent);
}
.control-row { display: flex; gap: var(--sp-2); }
.pause-btn, .stop-btn { flex: 1; }
.stop-square { width: 12px; height: 12px; border-radius: var(--r-keycap); background: currentColor; }
.hotkey-hint { margin-top: var(--sp-2); text-align: center; font-size: var(--fs-caption); color: var(--text-secondary); }
</style>
