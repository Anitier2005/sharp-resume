import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './styles/index.css'
import App from './App.tsx'
import { SettingsRoot } from './settings'

/**
 * 同一个入口有三种下落：
 *   无参数           → 主窗口
 *   ?view=model|look → 设置窗口（由 lib/settings-window 打开）
 *
 * 用查询参数分流而不是第二个 HTML 入口：多入口要维护两份 vite/tauri 配置，
 * 而这里只差一个 if。设置窗不挂工作区 store —— 它不需要整份数据。
 */
const view = new URLSearchParams(window.location.search).get('view')

createRoot(document.getElementById('root')!).render(
  <StrictMode>{view ? <SettingsRoot /> : <App />}</StrictMode>,
)
