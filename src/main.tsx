import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './styles/index.css'
import App from './App.tsx'
import { SettingsRoot } from './settings'
import { hydrateSettings } from '@/lib/settings-store'

/**
 * 同一个入口有三种下落：
 *   无参数           → 主窗口
 *   ?view=model|look → 设置窗口（由 lib/settings-window 打开）
 *
 * 用查询参数分流而不是第二个 HTML 入口：多入口要维护两份 vite/tauri 配置，
 * 而这里只差一个 if。设置窗不挂工作区 store —— 它不需要整份数据。
 */
const view = new URLSearchParams(window.location.search).get('view')

/**
 * 设置存在 app.db（settings 表）。`loadAi()` 这类读取是同步的（组件初始化就要用），
 * 所以启动先把它整张灌进内存，再挂载界面 —— 挂载后才读会读到默认值，
 * 那一瞬间用户会看到「模型被重置」。
 */
void (async () => {
  await hydrateSettings()
  createRoot(document.getElementById('root')!).render(
    <StrictMode>{view ? <SettingsRoot /> : <App />}</StrictMode>,
  )
})()
