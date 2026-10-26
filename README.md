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
- [x] **附件进上下文**：txt / md / csv / json 等纯文本附件随消息送进模型（单份上限 200KB），PDF、图片等只报文件名
- [x] 简历区：多页签（中文 / English / 作品集）、新增与删除页面、缩放
- [x] 纸面内联编辑（contentEditable，非受控写入，编辑即写回数据）
- [x] 中英双语字段（公司、职位、学历、技能、作品各有一份英文）
- [x] **经历库**：人这一级的共享资料，经历 / 作品 / 教育 / 技能 / 人物五个分页，可增删、排序、中英切换
- [x] **本页取用**：每条经历与作品都有「本页」勾选，决定它出不出现于当前页（作品集页只取作品）
- [x] 本地持久化：桌面端写用户数据目录 `workspace.json`，浏览器回落 localStorage
- [x] 导出：打印 / 存 PDF（当前页）、**Word .docx（当前页）**、Markdown（当前页 / 整份）
- [x] **A4 页数估算**：页脚按纸张宽高比实时算「≈ N 页 A4」，超过一页转警示色，同时在纸面第 529px 处画分页虚线
- [x] 主题：白（默认）/ 靛 / 松 / 朱 + 明暗两套
- [x] **接入真实模型**：OpenAI 兼容接口，流式输出，返回的改写块可一键写回经历库
- [ ] 导出 PNG
- [ ] Tauri 打包绿色版 exe

## 模型接入

会话里点左下角模型名（或右上角）打开设置：

| 项 | 说明 |
| --- | --- |
| 接口地址 | 任何 OpenAI 兼容端点，如 `https://api.deepseek.com/v1`；带不带 `/chat/completions` 都行 |
| 模型 | 端点上可用的模型名，如 `deepseek-chat`、`gpt-4o-mini`、`qwen2.5:7b` |
| API Key | 只存本机 `localStorage`，**不写进工作区 JSON，不会随简历导出** |

没填 Key 时继续用 `src/lib/rewrite.ts` 的本地规则（不联网、不编造数字），填了之后所有对话走模型。

**改写如何回到纸面**：系统提示要求模型在回答末尾附一个 json 代码块，形如
`[{"label":"第 1 段 · 公司","before":"原文逐字","after":"改写后"}]`。
前端用 `before` 去匹配经历库里每一句（忽略空白与标点），匹配上就挂上写回目标；点「应用 N 处改写」时按经历合并成一次写入，避免同一段多次覆盖。

> 跨域提醒：请求由 WebView 直接发出，接口必须允许跨域（DeepSeek 会回显 Origin）。
> 如果某个自建网关不放行，把它的 CORS 打开即可；桌面端后续可换成 Rust 侧请求绕开该限制。

## 经历库怎么用

纸面上能改字，但**增删一段经历**、**决定这一页放哪几段**，要去经历库（简历区顶栏「经历库」）：

| 操作 | 结果 |
| --- | --- |
| 改公司 / 职位 / 描述 | 即时同步到这个人名下**所有**简历 |
| 勾选「本页」 | 只影响当前这一页，其他页不受影响 |
| 上移 / 下移 | 调整顺序：库里是全局顺序，页面按同一顺序取用 |
| 切到 EN | 编辑英文版字段；英文为空时英文页回落中文 |

删除一段经历时，会同时把它从所有页面的引用里摘掉，不会留下悬空引用。

## DOCX 导出

`src/lib/docx.ts` 里手写 OOXML + 一个 store 模式的 zip（不压缩），**零依赖**。
版式对齐纸面：A4、2cm 边距、姓名 20pt 字距 3pt、章节标题用当前主题色、时间右对齐（制表位 9638 twips）。

## 主题规则

这是这个项目最容易做错的地方，写成硬规则：

1. **表面恒为白**（深色模式为石墨），表面与线条**不参与主题染色**。
2. 主题只改 `--acc` 这一组，作用域限定在**组件**：选中条与选中底、图标、发送按钮、页签下划线、聚焦环、选区、纸面标记。
3. 一句话：换主题只换「点缀」，不换「底色」。

令牌分六组：`--bg-*` 表面 / `--line-*` 描边 / `--fg-*` 文字 / `--acc-*` 强调 / `--ok|warn|danger` 语义 / `--paper-*` 纸面。

## 字号规则

初版 UI 主体只有 9–11.5px，实测 69% 的文字元素小于 11px，偏小。现在改为**可缩放**：

1. **UI 字号全部用 `rem`**，根字号 `html{font-size:calc(16px * var(--text-scale))}`。
   改 `--text-scale` 一处，侧栏、会话、顶栏、页脚、抽屉整体等比缩放。
   布局尺寸（三栏宽、纸面宽、控件高度）仍用 `px`，只放大文字、不放大骨架。
2. **默认 `--text-scale: 1.2`**（比初版大 20%），这是可读下限。
3. **两条跟随通道**，优先级：应用内档位 > 系统设置 > 默认值
   - 系统：启动时读 `HKCU\Software\Microsoft\Accessibility\TextScaleFactor`
     （即 Windows「设置 → 辅助功能 → 文本大小」）。这个值**独立于 DPI**，
     网页不会自动跟随，必须自己读；DPI 缩放由 WebView2 处理，无需干预。
   - 应用内：「外观」菜单 → 字号：自动 / 小 / 标准 / 大 / 特大。选「自动」即跟随系统。
4. **纸面（`.paper` 内）刻意保留 `px`，不参与缩放。**
   纸面是 1mm ≈ 1.78px 的 A4 等比预览，字号代表印刷字号，已经按可读性放大过；
   再跟着 UI 缩放会让「预览 ≈ 导出」失真，A4 分页估算也会漂移。需要更大的阅读尺寸请用页脚右侧的缩放控件。


## 技术栈

| 层 | 选型 |
| --- | --- |
| 桌面壳 | Tauri 2（WebView2，无边框窗口 + 自定义标题栏） |
| 前端 | React 19 + TypeScript（strict） |
| 样式 | 纯 CSS + CSS Modules 作用域隔离 + 自定义属性令牌 |
| 图标 | 内联 SVG（`src/components/ui/icons.tsx`） |
| 状态 | `useReducer` + Context，自动保存到磁盘 |

## 开发

```bash
npm install
npm run dev            # 纯前端热更新，浏览器开 http://localhost:1420
.tools\dev.cmd        # 桌面窗口热更新（前端即时，Rust 改动自动重建重启）
.tools\build.bat      # release exe
npm run package:clean  # 把 exe 放进 ../release-clean（清空、自检、单文件）
```

## 可以自己加东西的两个目录

| 目录 | 放什么 | 生效方式 |
|---|---|---|
| `config/models.json` | 模型清单：地址、上下文窗口、思维强度档位、默认温度 | 保存即生效（dev），发布版重新打包 |
| `prompts/system.md` | system 模板，`{{context}}` 是材料占位 | 同上（`include_str!` 编进 Rust） |
| `prompts/skills/*.md` | 技能：每个文件一段「什么时候用 / 怎么做 / 输出格式」 | 同上（vite glob 编进前端，自动带上新文件） |

技能不需要注册：丢一个 `.md` 进 `prompts/skills/`，刷新界面对话时它就在 system 提示词里。

热重载范围：

| 改了什么 | 结果 |
|---|---|
| `.tsx` / `.ts` / `.css` | 保存即生效，窗口不刷新、应用状态不丢 |
| `src-tauri/**`（Rust） | tauri 自动增量重建并重启窗口，debug 档十几秒 |
| `tauri.conf.json` | 重启窗口 |

只调界面时用 `npm run dev` 更轻：浏览器里同样是热更新，只是没有文件对话框等 Tauri 能力
（状态回落 localStorage、导出回落浏览器下载，功能照样能点）。

> 首屏一律是空白工作区，跟正式版一致。要载入示例数据（8 个虚构候选人 + 一段对话，
> 调版式用）就在浏览器地址后面加参数：`http://localhost:1420/?seed=1`。
> 桌面 dev 窗口不带参数，所以不会再被示例数据占住。

> 两个 `.tools\*.cmd` 只负责把 MSVC 环境拼好再调命令 —— 本机 `cl.exe` 不在 PATH 里，
> 且 Windows SDK 的 ucrt 库不全（缺 `legacy_stdio_definitions.lib` / `oldnames.lib`），
> 由 `.tools\libstub` 的空壳导入库顶上。细节见脚本里注释。

> **Windows 打包前提**：Rust 的 `x86_64-pc-windows-msvc` 目标需要 MSVC 编译器（`cl.exe`）。
> 只装 Rust 不装 C++ 工具链时，`cargo check` 会报 `failed to find tool "cl.exe"`。
> 装法：Visual Studio Installer → 勾选「使用 C++ 的桌面开发」。
>
> 装完 `cl.exe` 也不在 PATH 里，必须先跑 `vcvarsall`。本机只有 x86 宿主编译器（`Hostx86/x64`），
> 所以参数是 `x86_amd64`，不是 `x64`：
>
> ```bat
> call "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvarsall.bat" x86_amd64
> cd /d <repo>\src-tauri
> cargo build --release
> ```
>
> 产物是 `src-tauri/target/release/sharp-resume.exe`——单文件，前端资源已内嵌，
> 拷到别的机器上双击就能跑（目标机需要 WebView2 Runtime，Win10 1803+ 与 Win11 自带）。
> 要装到系统里再去掉 `--no-bundle` 的限制跑 `npm run tauri:build`，产物在 `src-tauri/target/release/bundle/nsis/`。

## 数据

工作区是一个 JSON：`{ version, people[], ui{} }`。

- 桌面端（便携）：**exe 同级的 `SharpResumeData\workspace.json`**
  —— 整个目录可整体拷贝 / 备份 / 放 U 盘，数据跟着 exe 走。
- 桌面端（回退）：若 exe 所在位置不可写（如 `Program Files`、只读介质），
  自动回退到 `%LOCALAPPDATA%\com.sharpresume.desktop\workspace.json`。
  旧版数据会自动复制一份到新位置，原文件保留。
- 浏览器：`localStorage["sharp-resume:workspace:v1"]`

开发构建首次启动写入 `src/data/seed.ts` 的示例数据（2 个完整候选人 + 6 个占位）；
生产构建经 `vite.config` 的 alias 换成空工作区，**发布产物不含任何示例数据**。

## 目录

```
src/
  components/
    chat/       会话：panel（编排）+ message（渲染）+ composer（输入）+ ai-settings（设置）
    library/    经历库抽屉：drawer（外壳）+ pane-*（5 个面板）+ parts（共用件）
    resume/     页签与画布 + paper（纸面）+ portfolio（作品集页）
    ui/         基础件：editable / menu / toast / icons / shared.module.css
  lib/          存储、导出、DOCX 生成、改写规则、字数统计、类名拼接
  store/
    workspace/  状态分层：types / helpers / reducer / context / provider / use-workspace
  styles/       全局层：tokens / base / layout / print + index 汇总
  types/        数据模型
design/         视觉探索过程，editor.html 是定稿方向
```

### 样式组织（CSS Modules）

组件样式一律与组件同目录，`Component.module.css`；`src/styles/` 只放**全局层**：

| 文件 | 职责 | 为什么全局 |
| --- | --- | --- |
| `tokens.css` | 设计令牌、主题、明暗、字号缩放、`.win` 作用域 | 主题靠 `html[data-accent=x] .win` 选中令牌根，类名一旦哈希就失效 |
| `base.css` | reset、字体、按钮、滚动条、选区 | 元素级规则，无归属组件 |
| `layout.css` | `.app` 三栏网格与窄屏断点 | 唯一布局根，只有一个实例 |
| `print.css` | 打印时只留纸面 | 见下方「打印契约」 |

三条约定，改动时容易踩：

1. **复合选择器要保留复合形态。** 例如 `.row.em` 的特异度是 `(0,2,0)`，若拆成单独的
   `.em`（`0,1,0`）就会被 `.row:hover`（`0,1,1`）压过，hover 时强调色丢失。
   同理 `.item.itemOn`、`.tab.tabOn`、`.menu-item.danger`。
2. **带前缀的选择器不能拍平。** `.zoomCtl button` 是 `(0,1,1)`，所以覆盖它的规则必须写
   `.zoomCtl .zoomVal` 而不是 `.zoomVal`。
3. **暗色覆盖用 `:global(html[data-mode="dark"]) .本地类`。** `:global()` 只豁免外层，
   内层类名照常哈希。

### 打印契约

打印规则**不按类名**选中元素（类名会被哈希），改用数据属性：

`data-print="hide"` 隐藏外壳 chrome｜`flow` 改为块级流｜`page` 重置纸面外层宽度与缩放｜`sheet` 纸面本体去边框铺满

新增需要参与打印的元素时，加对应属性即可，不必碰样式实现。

## 说明

- `design/` 下是设计阶段的文件，不参与构建，保留作参照。
- 没配模型时，回复来自 `src/lib/rewrite.ts` 的本地规则，只做「主动语态」这类可解释的文本变换，**不编造数字**，也不联网。
- 附件读取在浏览器侧完成（`src/lib/attach.ts`）：选完文件立刻把纯文本读进内存，发送时直接拼进最后一条 user 消息，不上传、不落盘。PDF / 图片 / Office 文档当前只把文件名告诉模型——要提取正文得另接解析器，或交给支持多模态的接口。
- 纸面按 A4 比例排版：宽度自适应，`min-height` 等于一页 A4 的等比高度。超出后画布内部滚动（整列高度被 `grid-template-rows:minmax(0,1fr)` 锁住，页脚不会被顶出窗口），页脚的「≈ N 页 A4」用纸张宽高比换算，缩放不影响结果。内容真超一页时，纸面上会有一条 A4 分页虚线标出打印会被切断的位置。
