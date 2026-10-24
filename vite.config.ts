import path from "path"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
  const isProd = mode === "production"

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
      // 顺序有意义：更具体的规则要排在 "@" 之前，否则会被前缀规则吞掉。
      // 生产构建把 seed 换成空实现，发布产物因此不含任何示例简历；
      // 开发构建保持原样，方便调样式时有内容可看。
      alias: [
        ...(isProd
          ? [{ find: /^@\/data\/seed$/, replacement: path.resolve(__dirname, "./src/data/seed.prod.ts") }]
          : []),
        { find: "@", replacement: path.resolve(__dirname, "./src") },
      ],
    },
  }
});
