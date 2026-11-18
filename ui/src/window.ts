/** Tauri 窗口路由辅助：浏览器演示模式下为空操作。 */
import { isTauri } from './ipc'

async function show(label: string): Promise<void> {
  if (!isTauri()) return
  try {
    const { WebviewWindow } = await import('@tauri-apps/api/webviewWindow')
    const win = await WebviewWindow.getByLabel(label)
    if (win) {
      await win.show()
      await win.setFocus()
    }
  } catch (e) {
    console.warn(`[window] show ${label} failed`, e)
  }
}

async function hide(label: string): Promise<void> {
  if (!isTauri()) return
  try {
    const { WebviewWindow } = await import('@tauri-apps/api/webviewWindow')
    const win = await WebviewWindow.getByLabel(label)
    if (win) await win.hide()
  } catch (e) {
    console.warn(`[window] hide ${label} failed`, e)
  }
}

export const showEditor = () => show('editor')
export const showRecorder = () => show('recorder')
export const showSettings = () => show('main')
export const hideSettings = () => hide('main')
export const hideRecorder = () => hide('recorder')
export const hideEditor = () => hide('editor')
