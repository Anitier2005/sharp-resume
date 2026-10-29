# UI 组件库（`src/components/ui`）

这个文件的唯一目的：**让界面里只存在一种视觉语言**。凡是你打算手写一个控件，
先在这里找一次；找不到就补到这里，而不是就地写一个。

入口：`@/components/ui`（`src/components/ui/index.ts`）。新代码只从这里 import。

---

## 1. 禁止直接使用的原生件

它们的外观由操作系统或浏览器决定，**不受令牌控制**：深浅主题、字号缩放、
圆角、行高全都对不上。这是「同一个界面里出现两种语言」的唯一来源。

| 不要写 | 用这个 | 为什么 |
|---|---|---|
| `<select>` | `Select` | 系统下拉的弹出层、行高、高亮条不可样式化，也放不了「名称 + 来源 + 勾」 |
| `<input type="range">` | `Slider` | 轨道与滑块各浏览器不同；`Slider` 内部仍用原生 input（键盘/拖拽免费），只是透明化了 |
| `<input type="checkbox">` | `Checkbox` | 原生方框在深浅两套主题下颜色不受控 |
| 自绘开关 | `Switch` | 一按即生效的布尔项（区别于需要核对的复选） |
| `<details>` / `<summary>` | `Disclosure` | summary 的三角位置/大小不可控，焦点框也不是本应用那套 |
| `title="…"` | `Tooltip` | 系统提示是浅黄底、延迟 ~1s、不跟字号缩放、不能换行 |
| `confirm()` / `alert()` | `Confirm` | 会阻塞渲染进程，且长得不像这个应用 |
| `.btnGhost` / `.btnPrimary` / `.fld` 类 | `Button` / `Field` | 旧的两份全局类已删除；各写一遍必然漂移（26/22px 混用、三种圆角） |

`<input type="text|password|number|date">` 本身可以用 —— 它没有系统级弹出层，
外观完全由我们控制；但请走 `Input`，别自己写边框与聚焦底色。

---

## 2. 组件

### 动作

| 组件 | 用途 | 关键 props |
|---|---|---|
| `Button` | 所有按钮 | `variant`: `primary` \| `ghost` \| `quiet` \| `danger`；`size`: `xs` \| `sm` \| `md`；`icon`（正方形）；`leading`（文字左图标） |

- 语义而不是颜色：`primary` 一屏最多一个；`danger` 只给「确认删除」这种不可撤销动作；
  行尾的编辑/删除用 `quiet`（无边、悬停才出底色）。
- 三档尺寸：`26`（与输入框同高）、`22`（面板常规）、`18`（密集列表行内）。

### 表单

| 组件 | 说明 |
|---|---|
| `Field` | 标签 / 控件 / 说明 三段竖排；渲染成 `<label>`，点标签即聚焦 |
| `Input` / `Textarea` | 统一高度、边框、聚焦底色（`--acc-softer` + `--acc-line-2`） |
| `ReadonlyValue` | 只读值的展示（禁用输入框看起来像「能改但坏了」） |
| `Select` | 自绘下拉；键盘 ↑↓/Home/End/Enter/Esc；焦点留在触发按钮上 |
| `Slider` | 滑杆；原生 input 透明覆盖，交互用浏览器的，像素用自己的 |
| `Checkbox` / `Switch` | 勾选 / 开关 |
| `Segmented` | 互斥小集合（明暗、思维强度、字号） |
| `Chip` | 可多选的筹码（标签、表单多选） |

### 结构

| 组件 | 说明 |
|---|---|
| `List` / `ListRow` / `ListEmpty` | 一组可选条目：发丝线分隔、选中整行染色 + 右侧勾、行尾 `actions` 不参与点击 |
| `Disclosure` | 折叠区（思考过程、长说明） |
| `Tooltip` | 悬停/聚焦提示；`pointer-events: none`，不抢点击 |
| `Modal` / `Confirm` | 小窗（420）与确认窗；ESC 只关一层、点遮罩不关、取消与 X 互斥 |
| `Dropdown` / `MenuItem` | 菜单（浮层走 portal，下方放不下自动翻上） |
| `ImageView` / `ImagePeek` | 大图模态 / 悬停缩略图 |

---

## 3. 尺寸与颜色

- **颜色**：只准用令牌（`--fg-*` / `--line-*` / `--bg-*` / `--acc-*` / `--danger`）。
  需要半透明时用 `color-mix(in srgb, var(--danger) 10%, var(--bg-elev))`，
  **不要写 `#fff`** —— 深色主题下会直接亮一块。
- **字号**：`0.6875rem`(11) / `0.625rem`(10) / `0.59375rem`(9.5) / `0.5625rem`(9)；
  全部 rem，跟随 `--text-scale`。纸面（`.paper` 内）例外，仍用 px。
- **圆角**：控件 4、列表/浮层 6、模态卡 8。
- **高度**：26（输入框、常规按钮）、22（小号）、18（行内图标按钮）。
- **浮层层级**：遮罩 120 < 取色器/大图 130 < 下拉 140 < 菜单 200 < 提示 210。

---

## 4. 加新组件时的四条要求

1. **只导出组件**（`*.tsx` 里不要有 hook 之外的可变状态），否则 Fast Refresh 会失效；
   纯函数放 `lib/`（例：浮层定位在 `ui/popover.ts`）。
2. **不硬编码颜色与字号**，一律令牌 + rem。
3. **原生元素只用来承载交互**（点击、键盘、无障碍语义），视觉由自己的 span/div 画。
   参考 `Slider` / `Checkbox`：input 透明铺在上面，像素全归 CSS。
4. **注释写「为什么」**：写清「为什么不用原生件」「为什么是这个尺寸」，
   否则下一个人会顺手改回去。

---

## 5. 窗口与外壳

| 层 | 做法 |
|---|---|
| 主窗口 | `decorations: false` + 自绘 `TitleBar`；三栏 + 一条**贯通全宽**的 `StatusBar` |
| 设置窗（模型 / 外观 / 知识库） | 独立 Tauri 窗口（`decorations: false` 自绘标题栏、固定尺寸），无底栏、无「完成」，改一项即时生效 |
| 确认 / 表单小窗 | `Modal`：所有小窗共用同一套底栏 —— 无分界、无深色底、按钮一律靠右；不再有「带底栏」的变体 |

三条容易踩的规矩：

1. **不要在前端建窗**。`new WebviewWindow` 需要一组 ACL（`allow-create-webview-window`、
   拿到已有窗口又要 `allow-get-all-webviews`），少一条就静默失败。统一走 Rust 命令
   `open_settings`（`src-tauri/src/lib.rs`）。
2. **建窗命令必须是 `async fn`**。同步命令跑在主线程上，而建窗要把任务交回主线程并等待，
   于是自锁：窗口壳出现了、页面永远不导航，表现为**纯白窗**。
3. **新窗口要带上主窗口那套 `additional_browser_args`**（`--disable-gpu-sandbox` 等），
   否则这台机器上同样是白窗。

状态栏只有一条（`components/status-bar.tsx`），三栏不再各带底栏：
左「本地工作区」、中「Enter 发送」、右「页码 / 字数 / 保存状态」。

4. **无边框窗口要自己给标题栏**：`data-tauri-drag-region` 负责拖动，右侧一颗关闭键；
   内容自己铺满（`flex: 1`）。系统边框一露出来，就与主窗口的自绘标题栏不是一套语言了。
