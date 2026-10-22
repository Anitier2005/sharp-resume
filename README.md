# Sharp Resume

> AI 简历工作台 · 一份简历，多个页面，一条对话线。

基于 **Tauri 2 + React 19 + TypeScript + Tailwind CSS + shadcn/ui** 的桌面应用。左侧简历库、中间 AI 会话、右侧简历编辑区；一份简历可以同时包含中文简历、英文简历与作品集，各自独立排版、统一导出。

## 当前阶段

仓库处于 **页面原型（UI prototype）** 阶段：三栏布局、页签分页、可编辑简历、文件上传、导出菜单等交互均已可点，AI 与真实导出为演示占位。

- [x] 三栏可拖拽布局
- [x] 简历列表（搜索 / 排序 / 新建 / 右键操作）
- [x] 会话区（消息流、改稿卡片、任意文件上传与拖放）
- [x] 简历区（多页签：中文 / English / 作品集，新增与删除页面）
- [x] 简历内联编辑（contentEditable，非受控写入避免光标跳动）
- [x] 导出菜单（PDF / Word / Markdown / PNG / 打印）
- [x] 浅色 / 深色主题
- [ ] 接入真实模型与会话持久化
- [ ] 真实导出（PDF / DOCX）
- [ ] Tauri 打包绿色版 exe

## 技术栈

| 层 | 选型 |
| --- | --- |
| 桌面壳 | Tauri 2（WebView2） |
| 前端 | React 19 + TypeScript（strict） |
| 样式 | Tailwind CSS 3.4 + shadcn/ui（Radix UI） |
| 布局 | react-resizable-panels |
| 图标 | lucide-react |
| 提示 | sonner |

## 开发

```bash
npm install
npm run dev        # 浏览器预览 http://localhost:1420
npm run tauri:dev  # 桌面窗口调试（需要 Rust + MSVC Build Tools + WebView2）
```

## 打包

```bash
npm run tauri:build
```

- 安装包：`src-tauri/target/release/bundle/nsis/*.exe`
- 绿色版：`src-tauri/target/release/sharp-resume.exe` 可直接分发（目标机需 WebView2 运行时，Win10 1803+ 一般自带）

> 首次打包前需要先生成图标：`npm run tauri icon path/to/logo.png`，会写入 `src-tauri/icons/`。

## 目录结构

```
src/
├─ components/
│  ├─ app-title-bar.tsx      # 自定义窗口标题栏（无边框窗口）
│  ├─ resume-list-panel.tsx  # 左：简历库
│  ├─ chat-panel.tsx         # 中：会话 + 任意文件上传
│  ├─ resume-panel.tsx       # 右：页签 + 画布 + 导出 + 状态栏
│  ├─ resume-documents.tsx   # A4 简历 / 作品集排版与内联编辑
│  ├─ editable.tsx           # contentEditable 封装
│  └─ ui/                    # shadcn/ui 组件
├─ data/mock.ts              # 演示数据（4 份简历 / 10 个页面）
├─ lib/                      # 工具函数
└─ types/index.ts            # ResumeItem / ResumePage / Doc 数据模型
src-tauri/                   # Rust 侧：窗口、文件、导出命令
```

## 数据模型

```
ResumeItem (一份简历 = 一条会话)
└─ pages: ResumePage[]
   ├─ resume-zh  → ResumeDoc
   ├─ resume-en  → ResumeDoc
   └─ portfolio  → PortfolioDoc
```

简历与会话一一对应：切换左侧简历时，中间的对话上下文和右侧的页面集合同步切换。

## 设计约定

- 简历纸面始终为白色（贴近打印效果），与应用外壳主题解耦
- 编辑态采用块级 hover 工具栏（AI 改写 / 拖拽），不做右侧属性面板
- 导出按钮区分「当前页面」与「整份简历」
- 所有占位交互统一使用 toast 说明，不产生静默失败
