import { ref } from 'vue'
import { getSettings, onSettingsChanged, setSettings } from '../ipc'
import { DEFAULT_SETTINGS, type Settings } from '../types'

const settings = ref<Settings>(structuredClone(DEFAULT_SETTINGS))
const loading = ref(true)
const saving = ref(false)
let listening = false

export function useSettings() {
  async function load(): Promise<void> {
    try {
      settings.value = await getSettings()
    } catch (e) {
      console.error('[useSettings] load failed', e)
      settings.value = structuredClone(DEFAULT_SETTINGS)
    } finally {
      loading.value = false
    }
  }

  async function save(next?: Settings): Promise<void> {
    saving.value = true
    try {
      settings.value = await setSettings(next ?? settings.value)
    } finally {
      saving.value = false
    }
  }

  async function reset(): Promise<void> {
    await save(structuredClone(DEFAULT_SETTINGS))
  }

  async function listen(): Promise<() => void> {
    if (listening) return () => {}
    listening = true
    const un = await onSettingsChanged((payload) => {
      settings.value = payload
    })
    return () => {
      un()
      listening = false
    }
  }

  return { settings, loading, saving, load, save, reset, listen }
}
