# Sharp Resume

> AI 简历工作台 · 一个人一套经历，想投几家就派生几份简历。

桌面应用（Tauri 2 + React 19 + TypeScript），**不使用任何 UI 框架**：样式是一层 CSS 变量令牌，组件自己写，图标是内联 SVG。

## 界面

三栏，一条 1px 线贯穿：

| 左 · 候选人 → 简历 | 中 · 会话 | 右 · 简历 |
| --- | --- | --- |
| 一级是人，展开是这个人名下的简历 | 一份简历一条独立会话 | 顶栏分页器 + 纸面编辑 + 导出 |

关键设计：**经历与作品挂在「人」这一级，被这个人所有简历共享**。所以改一段经历，这个人所有简历都会变；但每份简历的会话、摘要、页面组合互相独立。

## 当前进度

- [x] 两级列表：候选人 → 简历（展开/收起、搜索、新建、重命名、复制、删除）
- [x] 会话：消息流、任意文件上传与拖放、改写建议、一键写回共享经历
- [x] 简历区：多页签（中文 / English / 作品集）、新增与删除页面、缩放
- [x] 纸面内联编辑（contentEditable，非受控写入，编辑即写回数据）
- [x] 中英双语字段（公司、职位、学历、技能、作品各有一份英文）
- [x] 本地持久化：桌面端写用户数据目录 `workspace.json`，浏览器回落 localStorage
- [x] 导出：打印 / 存 PDF（当前页）、Markdown（当前页 / 整份）
- [x] 主题：白（默认）/ 靛 / 松 / 朱 + 明暗两套
- [ ] 接入真实模型（当前是本地改写规则，不联网）
- [ ] 导出 DOCX / PNG
- [ ] 经历库管理面板（现在只能在纸面上改，改的就是共享库）
- [ ] Tauri 打包绿色版 exe

## 主题规则

这是这个项目最容易做错的地方，写成硬规则：

1. **表面恒为白**（深色模式为石墨），表面与线条**不参与主题染色**。
2. 主题只改 `--acc` 这一组，作用域限定在**组件**：选中条与选中底、图标、发送按钮、页签下划线、聚焦环、选区、纸面标记。
3. 一句话：换主题只换「点缀」，不换「底色」。

令牌分六组：`--bg-*` 表面 / `--line-*` 描边 / `--fg-*` 文字 / `--acc-*` 强调 / `--ok|warn|danger` 语义 / `--paper-*` 纸面。

## 技术栈

| 层 | 选型 |
| --- | --- |
| 桌面壳 | Tauri 2（WebView2，无边框窗口 + 自定义标题栏） |
| 前端 | React 19 + TypeScript（strict） |
| 样式 | 纯 CSS + 自定义属性令牌（单文件 `src/styles/app.css`） |
| 图标 | 内联 SVG（`src/components/ui/icons.tsx`） |
| 状态 | `useReducer` + Context，自动保存到磁盘 |

## 开发

```bash
npm install
npm run dev        # 浏览器预览 http://localhost:1420
npm run tauri:dev  # 桌面窗口（需要 Rust + MSVC Build Tools + WebView2）
npm run tauri:build
```

## 数据

工作区是一个 JSON：`{ version, people[], ui{} }`。

- 桌面端：`%APPDATA%\com.sharpresume.desktop\workspace.json`（由 Rust 侧读写）
- 浏览器：`localStorage["sharp-resume:workspace:v1"]`

首次启动写入 `src/data/seed.ts` 的示例数据（2 个完整候选人 + 6 个占位）。

## 目录

```
src/
  components/
    chat/       会话面板、消息、输入框
    resume/     页签、纸面、作品集
    ui/         自己写的基础件：可编辑文本、下拉菜单、提示、图标
  lib/          存储、导出、改写规则、字数统计
  store/        工作区状态与自动保存
  styles/       app.css（令牌 + 全部样式）
  types/        数据模型
design/         视觉探索过程，editor.html 是定稿方向
```

## 说明

- `design/` 下是设计阶段的文件，不参与构建，保留作参照。
- AI 还没接。当前回复来自 `src/lib/rewrite.ts` 的本地规则，只做「主动语态」这类可解释的文本变换，**不编造数字**，也不联网。
