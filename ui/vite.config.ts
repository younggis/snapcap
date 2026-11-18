import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// Tauri 在 dev 模式下通过 TAURI_ 环境变量注入窗口信息。
// clearScreen=false 避免 tauri dev 时清掉 Rust 的编译日志。
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  envPrefix: ['VITE_', 'TAURI_'],
  server: {
    port: 5173,
    strictPort: true,
    host: '127.0.0.1',
  },
})
