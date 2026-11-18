import { computed, ref } from 'vue'
import {
  annotationApply,
  onCaptureError,
  onCaptureState,
  onScreenshotReady,
  onCaptureStats,
  recordPause,
  recordResume,
  recordStart,
  recordStop,
  screenshotCapture,
  screenshotOutput,
  type Unlisten,
} from '../ipc'
import type {
  AnnotationOp,
  AppErrorPayload,
  CaptureStats,
  CaptureTarget,
  RecordingOptions,
  ScreenshotOptions,
  ScreenshotResult,
  SessionState,
} from '../types'

const screenshot = ref<ScreenshotResult | null>(null)
const lastSavedPath = ref<string>('')
const sessionId = ref<string | null>(null)
const sessionState = ref<SessionState | null>(null)
const stats = ref<CaptureStats>({
  fps: 0,
  dropped_frames: 0,
  bitrate_kbps: 0,
  duration_ms: 0,
  audio_dropped: 0,
})
const error = ref<AppErrorPayload | null>(null)
const busy = ref(false)

let unlisteners: Unlisten[] = []
let listening = false

const isRecording = computed(
  () => sessionState.value === 'recording' || sessionState.value === 'starting',
)
const isPaused = computed(() => sessionState.value === 'paused')

export function useCapture() {
  async function listen(): Promise<void> {
    if (listening) return
    listening = true
    unlisteners = await Promise.all([
      onCaptureState((p) => {
        sessionId.value = p.session
        sessionState.value = p.state
      }),
      onCaptureStats((p) => {
        stats.value = p
      }),
      onCaptureError((p) => {
        error.value = p
      }),
      onScreenshotReady((p) => {
        screenshot.value = p
      }),
    ])
  }

  function stopListening(): void {
    unlisteners.forEach((u) => u())
    unlisteners = []
    listening = false
  }

  async function capture(
    target: CaptureTarget,
    options: ScreenshotOptions,
  ): Promise<ScreenshotResult> {
    busy.value = true
    error.value = null
    try {
      const result = await screenshotCapture(target, options)
      screenshot.value = result
      return result
    } finally {
      busy.value = false
    }
  }

  async function output(destination: 'file' | 'clipboard', path?: string): Promise<string> {
    if (!screenshot.value) return ''
    const saved = await screenshotOutput(screenshot.value.id, destination, path)
    if (saved) lastSavedPath.value = saved
    return saved
  }

  async function annotate(ops: AnnotationOp[]): Promise<void> {
    if (!screenshot.value) return
    const newId = await annotationApply(screenshot.value.id, ops)
    screenshot.value = { ...screenshot.value, id: newId }
  }

  async function startRecording(
    target: CaptureTarget,
    options: RecordingOptions,
  ): Promise<string> {
    busy.value = true
    error.value = null
    try {
      const id = await recordStart(target, options)
      sessionId.value = id
      return id
    } finally {
      busy.value = false
    }
  }

  async function pauseRecording(): Promise<void> {
    if (sessionId.value) await recordPause(sessionId.value)
  }

  async function resumeRecording(): Promise<void> {
    if (sessionId.value) await recordResume(sessionId.value)
  }

  async function stopRecording() {
    if (!sessionId.value) return null
    const result = await recordStop(sessionId.value)
    sessionState.value = 'done'
    return result
  }

  function dismissError(): void {
    error.value = null
  }

  return {
    screenshot,
    lastSavedPath,
    sessionId,
    sessionState,
    stats,
    error,
    busy,
    isRecording,
    isPaused,
    listen,
    stopListening,
    capture,
    output,
    annotate,
    startRecording,
    pauseRecording,
    resumeRecording,
    stopRecording,
    dismissError,
  }
}
