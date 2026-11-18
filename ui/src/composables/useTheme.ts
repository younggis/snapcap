import { ref, watch } from 'vue'

export type Theme = 'light' | 'dark' | 'system'

const KEY = 'snapcap.theme'
const theme = ref<Theme>('system')

function apply(t: Theme) {
  const dark =
    t === 'dark' ||
    (t === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)
  document.documentElement.setAttribute('data-theme', dark ? 'dark' : 'light')

  // 让窗口底色（含标题栏区域）与内容区同一纯色，避免出现系统渐变。
  // 注意：overlay 是透明选区窗口，绝不能刷纯色底，否则遮住整个桌面。
  // 浏览器演示环境无 Tauri API，安全忽略。
  import('@tauri-apps/api/webviewWindow')
    .then(({ getCurrentWebviewWindow }) => {
      const win = getCurrentWebviewWindow()
      if (win.label === 'overlay') return
      return win.setBackgroundColor(dark ? '#16181D' : '#F3F5F9')
    })
    .catch(() => {})
}

export function useTheme() {
  function init() {
    const saved = localStorage.getItem(KEY) as Theme | null
    theme.value = saved === 'light' || saved === 'dark' || saved === 'system' ? saved : 'system'
    apply(theme.value)
    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
      if (theme.value === 'system') apply('system')
    })
    // 多个窗口同源，监听 storage 事件保持外观同步。
    window.addEventListener('storage', (e) => {
      if (e.key === KEY && e.newValue) {
        theme.value = e.newValue as Theme
        apply(theme.value)
      }
    })
  }

  function set(t: Theme) {
    theme.value = t
    localStorage.setItem(KEY, t)
    apply(t)
  }

  watch(theme, apply)

  return { theme, init, set }
}
