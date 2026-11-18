<script setup lang="ts">
defineProps<{
  message: string
  kind?: 'info' | 'error' | 'success'
}>()
defineEmits<{ close: [] }>()

const glyph = (kind?: string) => (kind === 'error' ? '!' : kind === 'success' ? '✓' : 'i')
</script>

<template>
  <Transition name="toast">
    <div v-if="message" class="ios-toast" :class="kind" role="status">
      <span class="mark">{{ glyph(kind) }}</span>
      <span class="text">{{ message }}</span>
      <button class="close" aria-label="关闭" @click="$emit('close')">×</button>
    </div>
  </Transition>
</template>

<style scoped>
.ios-toast {
  position: fixed;
  bottom: var(--sp-4);
  left: 50%;
  transform: translateX(-50%);
  z-index: 100;
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  max-width: calc(100% - (var(--sp-4) * 2));
  min-width: 240px;
  padding: var(--sp-2) var(--sp-3);
  border-radius: var(--r-card);
  background: var(--surface);
  border: 1px solid var(--border);
  box-shadow: var(--shadow-hover);
  font-size: var(--fs-caption);
  color: var(--text-primary);
}
.ios-toast.error { border-color: var(--danger); }
.ios-toast.success { border-color: var(--text-secondary); }
.mark {
  flex: none;
  display: grid;
  place-items: center;
  width: 22px;
  height: 22px;
  border-radius: var(--r-pill);
  background: var(--primary);
  color: var(--on-accent);
  font-weight: var(--fw-title);
  font-size: var(--fs-caption);
}
.error .mark { background: var(--danger); }
.success .mark { background: var(--text-secondary); }
.text { flex: 1; line-height: var(--lh); }
.close {
  flex: none;
  width: 22px;
  height: 22px;
  display: grid;
  place-items: center;
  border-radius: var(--r-pill);
  background: var(--surface-hover);
  color: var(--text-secondary);
  font-size: var(--fs-body);
  line-height: 1;
}
.close:hover { background: var(--surface-pressed); }
.toast-enter-active, .toast-leave-active {
  transition: opacity var(--dur) var(--ease), transform var(--dur) var(--ease);
}
.toast-enter-from, .toast-leave-to {
  opacity: 0;
  transform: translate(-50%, var(--sp-3));
}
</style>
