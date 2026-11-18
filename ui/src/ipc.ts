/**
 * Tauri invoke/emit 的类型化封装。
 *
 * 在浏览器（非 Tauri）环境下，所有命令走本地 mock，事件走 localBus，
 * 这样前端可以在 `pnpm --dir ui dev` 下完整渲染与验证。
 */
import { invoke as tauriInvoke } from '@tauri-apps/api/core'
import { emit as tauriEmit, listen as tauriListen } from '@tauri-apps/api/event'
import type {
  AnnotationOp,
  AppErrorPayload,
  CaptureStatePayload,
  CaptureStats,
  DisplayInfo,
  HotkeyPayload,
  RecordingOptions,
  RecordingResult,
  ScreenshotOptions,
  ScreenshotResult,
  SelectionPayload,
  SessionState,
  Settings,
  WindowInfo,
} from './types'
import { DEFAULT_SETTINGS } from './types'

export type Unlisten = () => void
type Handler<T> = (payload: T) => void

/** 判断当前是否运行在 Tauri WebView 中。 */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

/** 浏览器环境下用于模拟 Rust → UI 事件的本地事件总线。 */
class LocalBus {
  private handlers = new Map<string, Set<Handler<unknown>>>()

  on<T>(event: string, handler: Handler<T>): Unlisten {
    let set = this.handlers.get(event)
    if (!set) {
      set = new Set()
      this.handlers.set(event, set)
    }
    set.add(handler as Handler<unknown>)
    return () => set!.delete(handler as Handler<unknown>)
  }

  emit<T>(event: string, payload: T): void {
    this.handlers.get(event)?.forEach((h) => (h as Handler<T>)(payload))
  }
}

export const localBus = new LocalBus()

/** 在浏览器演示中模拟后端主动推送事件。 */
export function emitLocal<T>(event: string, payload: T): void {
  if (!isTauri()) localBus.emit(event, payload)
}

// ---------------------------------------------------------------------------
// 浏览器 mock 数据
// ---------------------------------------------------------------------------

const MOCK_DISPLAYS: DisplayInfo[] = [
  {
    handle: 1,
    name: '主显示器',
    is_primary: true,
    rect: { x: 0, y: 0, width: 1920, height: 1080 },
    scale_factor: 1,
  },
]

const MOCK_WINDOWS: WindowInfo[] = [
  {
    hwnd: 1001,
    title: 'SnapCap - 文档 - 记事本',
    class_name: 'Notepad',
    rect: { x: 120, y: 90, width: 980, height: 640 },
    is_visible: true,
  },
  {
    hwnd: 1002,
    title: '浏览器示例页面',
    class_name: 'Chrome_WidgetWin_1',
    rect: { x: 260, y: 140, width: 1240, height: 820 },
    is_visible: true,
  },
]

let mockSettings: Settings = structuredClone(DEFAULT_SETTINGS)
let mockSessionCounter = 0

function mockScreenshotDataUrl(width = 1280, height = 760): string {
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  const ctx = canvas.getContext('2d')!
  const grad = ctx.createLinearGradient(0, 0, width, height)
  grad.addColorStop(0, '#1e293b')
  grad.addColorStop(0.5, '#334155')
  grad.addColorStop(1, '#0f172a')
  ctx.fillStyle = grad
  ctx.fillRect(0, 0, width, height)
  ctx.strokeStyle = 'rgba(148,163,184,0.12)'
  ctx.lineWidth = 1
  for (let x = 0; x < width; x += 40) {
    ctx.beginPath()
    ctx.moveTo(x + 0.5, 0)
    ctx.lineTo(x + 0.5, height)
    ctx.stroke()
  }
  for (let y = 0; y < height; y += 40) {
    ctx.beginPath()
    ctx.moveTo(0, y + 0.5)
    ctx.lineTo(width, y + 0.5)
    ctx.stroke()
  }
  ctx.fillStyle = 'rgba(226,232,240,0.9)'
  ctx.font = '600 28px system-ui, sans-serif'
  ctx.fillText('SnapCap 预览（浏览器演示模式）', 48, 72)
  ctx.font = '18px system-ui, sans-serif'
  ctx.fillStyle = 'rgba(148,163,184,0.9)'
  ctx.fillText('选区 → 标注 → 保存 / 复制', 48, 110)
  return canvas.toDataURL('image/png')
}

// ---------------------------------------------------------------------------
// Commands (UI → Rust)
// ---------------------------------------------------------------------------

export function listDisplays(): Promise<DisplayInfo[]> {
  return isTauri() ? tauriInvoke('list_displays') : Promise.resolve(MOCK_DISPLAYS)
}

export function listWindows(): Promise<WindowInfo[]> {
  return isTauri() ? tauriInvoke('list_windows') : Promise.resolve(MOCK_WINDOWS)
}

export function openRegionOverlay(mode: 'screenshot' | 'record' | 'window'): Promise<SelectionPayload> {
  if (isTauri()) return tauriInvoke('open_region_overlay', { mode })
  // 浏览器演示：合成一个默认选区，并广播 overlay:selection 事件。
  const payload: SelectionPayload = {
    rect: { x: 160, y: 120, width: 960, height: 540 },
    hwnd: null,
    cancelled: false,
  }
  setTimeout(() => localBus.emit('overlay:selection', payload), 120)
  return Promise.resolve(payload)
}

/** 选区窗口完成/取消后把结果交回 Rust（浏览器下走 localBus）。 */
/** 虚拟桌面矩形（物理像素，可含负坐标），用于把窗口坐标映射到覆盖层。 */
export function getVirtualDesktop(): Promise<import('./types').Rect> {
  if (!isTauri()) return Promise.resolve({ x: 0, y: 0, width: 1920, height: 1080 })
  return tauriInvoke('get_virtual_desktop')
}

/** Rust 全局 Escape 兜底：通知覆盖层取消选区。 */
export function onOverlayEscape(handler: () => void): Promise<Unlisten> {
  if (!isTauri()) return Promise.resolve(localBus.on('overlay:escape', handler))
  return tauriListen('overlay:escape', () => handler())
}

/** Rust 全局 Enter 兜底：通知覆盖层确认选区（透明窗口拿不到键盘焦点）。 */
export function onOverlayConfirm(handler: () => void): Promise<Unlisten> {
  if (!isTauri()) return Promise.resolve(localBus.on('overlay:confirm', handler))
  return tauriListen('overlay:confirm', () => handler())
}

export function submitOverlaySelection(payload: SelectionPayload): Promise<void> {
  if (!isTauri()) {
    localBus.emit('overlay:selection', payload)
    return Promise.resolve()
  }
  return tauriInvoke('overlay_submit', { payload })
}

/** 通知所有窗口：截图已自动落盘到该路径（标注编辑器显示“打开文件位置”）。 */
export function emitScreenshotSaved(path: string): Promise<void> {
  if (!isTauri()) {
    localBus.emit('screenshot:saved', path)
    return Promise.resolve()
  }
  return tauriEmit('screenshot:saved', path)
}

export function onScreenshotSaved(handler: (path: string) => void): Promise<Unlisten> {
  if (!isTauri()) return Promise.resolve(localBus.on('screenshot:saved', handler))
  return tauriListen<string>('screenshot:saved', (e) => handler(e.payload))
}

export function screenshotCapture(
  target: unknown,
  options: ScreenshotOptions,
): Promise<ScreenshotResult> {
  if (isTauri()) return tauriInvoke('screenshot_capture', { target, options })
  const rect = (target as { rect?: { width: number; height: number } })?.rect
  const w = rect?.width ?? 1280
  const h = rect?.height ?? 760
  const result: ScreenshotResult = {
    id: `mock-${Date.now()}`,
    width: w,
    height: h,
    data_url: mockScreenshotDataUrl(Math.min(w, 1920), Math.min(h, 1080)),
  }
  return Promise.resolve(result)
}

export function screenshotOutput(
  id: string,
  destination: 'file' | 'clipboard',
  path?: string,
): Promise<string> {
  if (isTauri()) return tauriInvoke('screenshot_output', { id, destination, path })
  return Promise.resolve('C:\\Users\\young\\Pictures\\SnapCap\\mock.png')
}

export function annotationApply(id: string, ops: AnnotationOp[]): Promise<string> {
  if (isTauri()) return tauriInvoke('annotation_apply', { id, ops })
  return Promise.resolve(id)
}

export function recordStart(target: unknown, options: RecordingOptions): Promise<string> {
  if (isTauri()) return tauriInvoke('record_start', { target, options })
  const session = `mock-session-${++mockSessionCounter}`
  let state: SessionState = 'starting'
  const pushState = (s: SessionState) => {
    state = s
    localBus.emit('capture:state', { session, state: s } satisfies CaptureStatePayload)
  }
  pushState('starting')
  setTimeout(() => pushState('recording'), 200)

  let elapsed = 0
  const timer = window.setInterval(() => {
    if (state !== 'recording') return
    elapsed += 1
    localBus.emit('capture:stats', {
      fps: 60,
      dropped_frames: 0,
      bitrate_kbps: 11800,
      duration_ms: elapsed * 1000,
      audio_dropped: 0,
    } satisfies CaptureStats)
  }, 1000)
  ;(mockSessions as Map<string, number>).set(session, timer)
  return Promise.resolve(session)
}

const mockSessions = new Map<string, number>()

function stopMockSession(session: string): void {
  const timer = mockSessions.get(session)
  if (timer !== undefined) {
    window.clearInterval(timer)
    mockSessions.delete(session)
  }
  localBus.emit('capture:state', {
    session,
    state: 'done',
  } satisfies CaptureStatePayload)
}

export function recordPause(session: string): Promise<void> {
  if (isTauri()) return tauriInvoke('record_pause', { session })
  localBus.emit('capture:state', { session, state: 'paused' } satisfies CaptureStatePayload)
  return Promise.resolve()
}

export function recordResume(session: string): Promise<void> {
  if (isTauri()) return tauriInvoke('record_resume', { session })
  localBus.emit('capture:state', { session, state: 'recording' } satisfies CaptureStatePayload)
  return Promise.resolve()
}

export function recordStop(session: string): Promise<RecordingResult> {
  if (isTauri()) return tauriInvoke('record_stop', { session })
  stopMockSession(session)
  return Promise.resolve({
    path: 'C:\\Users\\You\\Pictures\\SnapCap\\capture.mp4',
    duration_ms: 12_340,
    size: 18_400_000,
  })
}

export function getSettings(): Promise<Settings> {
  if (isTauri()) return tauriInvoke('get_settings')
  return Promise.resolve(structuredClone(mockSettings))
}

export function setSettings(settings: Settings): Promise<Settings> {
  if (isTauri()) return tauriInvoke('set_settings', { settings })
  mockSettings = structuredClone(settings)
  localBus.emit('settings:changed', structuredClone(mockSettings))
  return Promise.resolve(structuredClone(mockSettings))
}

export function openOutputDir(): Promise<void> {
  if (isTauri()) return tauriInvoke('open_output_dir')
  return Promise.resolve()
}

/** 在文件管理器中定位并选中指定文件。 */
export function revealFile(path: string): Promise<void> {
  if (isTauri()) return tauriInvoke('reveal_file', { path })
  return Promise.resolve()
}

// ---------------------------------------------------------------------------
// Events (Rust → UI)
// ---------------------------------------------------------------------------

/** Rust 完成截图后广播，隐藏的 editor 窗口据此加载同一张图。 */
export function onScreenshotReady(handler: Handler<ScreenshotResult>): Promise<Unlisten> {
  if (!isTauri()) return Promise.resolve(localBus.on('screenshot:ready', handler))
  return tauriListen('screenshot:ready', (e) => handler(e.payload as ScreenshotResult))
}

export function onCaptureState(handler: Handler<CaptureStatePayload>): Promise<Unlisten> {
  if (!isTauri()) return Promise.resolve(localBus.on('capture:state', handler))
  return tauriListen('capture:state', (e) => handler(e.payload as CaptureStatePayload))
}

export function onCaptureStats(handler: Handler<CaptureStats>): Promise<Unlisten> {
  if (!isTauri()) return Promise.resolve(localBus.on('capture:stats', handler))
  return tauriListen('capture:stats', (e) => handler(e.payload as CaptureStats))
}

export function onCaptureError(handler: Handler<AppErrorPayload>): Promise<Unlisten> {
  if (!isTauri()) return Promise.resolve(localBus.on('capture:error', handler))
  return tauriListen('capture:error', (e) => handler(e.payload as AppErrorPayload))
}

export function onOverlaySelection(handler: Handler<SelectionPayload>): Promise<Unlisten> {
  if (!isTauri()) return Promise.resolve(localBus.on('overlay:selection', handler))
  return tauriListen('overlay:selection', (e) => handler(e.payload as SelectionPayload))
}

export function onHotkey(handler: Handler<HotkeyPayload>): Promise<Unlisten> {
  if (!isTauri()) return Promise.resolve(localBus.on('hotkey:triggered', handler))
  return tauriListen('hotkey:triggered', (e) => handler(e.payload as HotkeyPayload))
}

export function onSettingsChanged(handler: Handler<Settings>): Promise<Unlisten> {
  if (!isTauri()) return Promise.resolve(localBus.on('settings:changed', handler))
  return tauriListen('settings:changed', (e) => handler(e.payload as Settings))
}
