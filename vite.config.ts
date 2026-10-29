import path from "path"
import react from "@vitejs/plugin-react"
// vitest/config 的 defineConfig 才认 test 字段（用 vite 那个会被 tsc 拒）
import { defineConfig } from "vitest/config"

// https://vite.dev/config/
export default defineConfig(() => {
  return {
    base: './',
    plugins: [react()],
    // Tauri 需要固定端口，且不要清屏，方便同时看 Rust 日志
    clearScreen: false,
    server: {
      port: 1420,
      strictPort: true,
      watch: {
        ignored: ['**/src-tauri/**'],
      },
    },
    resolve: {
      alias: { "@": path.resolve(__dirname, "./src") },
    },
    // 只测纯函数（contentVersion / measureTip）：不需要 DOM，也不许拿 jsdom 的假布局
    // 去「证明」测量逻辑 —— 那部分是真实渲染冒烟的事。
    test: {
      environment: "node",
      include: ["src/**/*.test.ts"],
    },
  }
});
