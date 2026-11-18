import { onMounted, onUnmounted } from 'vue'
import { isTauri, onHotkey, type Unlisten } from '../ipc'
import { useSettings } from './useSettings'
import type { HotkeyPayload } from '../types'

type HotkeyAction = HotkeyPayload['action']

/** 把 "Alt+A" 解析为可与 KeyboardEvent 比较的结构。 */
export function parseCombo(combo: string) {
  const parts = combo.split('+').map((p) => p.trim().toLowerCase())
  return {
    alt: parts.includes('alt') || parts.includes('option'),
    ctrl: parts.includes('ctrl') || parts.includes('control') || parts.includes('cmd'),
    shift: parts.includes('shift'),
    key: parts.find((p) => !['alt', 'option', 'ctrl', 'control', 'cmd', 'shift'].includes(p)) ?? '',
  }
}

function matches(e: KeyboardEvent, combo: string): boolean {
  const c = parseCombo(combo)
  if (!c.key) return false
  return (
    e.altKey === c.alt &&
    e.ctrlKey === c.ctrl &&
    e.shiftKey === c.shift &&
    e.key.toLowerCase() === c.key.toLowerCase()
  )
}

/**
 * 监听热键动作。Tauri 下监听 Rust 的 hotkey:triggered 事件；
 * 浏览器演示下退化为页面 keydown 监听（按设置里的组合键触发）。
 */
export function useHotkey(onAction: (action: HotkeyAction) => void) {
  const { settings } = useSettings()
  let unlisten: Unlisten | null = null

  function browserKeydown(e: KeyboardEvent) {
    const map: Array<[HotkeyAction, string]> = [
      ['region', settings.value.hotkeys.region],
      ['full', settings.value.hotkeys.full],
      ['window', settings.value.hotkeys.window],
      ['record', settings.value.hotkeys.record],
      ['pause', settings.value.hotkeys.pause],
      ['annotate', settings.value.hotkeys.annotate],
    ]
    for (const [action, combo] of map) {
      if (matches(e, combo)) {
        e.preventDefault()
        onAction(action)
        return
      }
    }
  }

  onMounted(async () => {
    if (isTauri()) {
      unlisten = await onHotkey((p) => onAction(p.action))
    } else {
      window.addEventListener('keydown', browserKeydown)
    }
  })

  onUnmounted(() => {
    unlisten?.()
    window.removeEventListener('keydown', browserKeydown)
  })
}
