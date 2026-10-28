# 批次二 · 五件事的并行契约（2026-10-28）

主控只写契约、做集成与验收。四路子 agent 并行，**文件所有权互斥**：
只改自己名下的文件，需要别人文件的改动一律写进「接口需求」由主控补。

## 全局红线

1. **不要跑 `npm run build`**（会写 `dist/`，四路并发会互相踩）。类型自检一律用
   `npx tsc --noEmit -p tsconfig.json`，只读磁盘、不产出。
2. **不要碰 `src-tauri/`**：本轮所有需求都在前端能解决（会话消息在 Rust 侧是
   opaque JSON，`chat_push` / `chat_history` 原样透传，加字段不用改 Rust）。
3. 不要改 `.tools/`、`scripts/`、`package.json`、`docs/`。
4. 共享类型文件 `src/types/index.ts`、`src/store/workspace/types.ts` **已由主控改好**，
   任何人只读不写。新增字段见下方「已就位的契约」。
5. 注释按仓库既有风格：写「为什么」，不写「做了什么」；中文；不用 emoji。
6. 交付时给主控一份 ≤15 行的变更摘要：改了哪些文件、对外新增/变更的 API、没做到的部分。

## 已就位的契约（主控预改，只读）

- `Attachment.assetId?: string` —— 同一份文件在材料池里的 `Asset.id`
- `ChatMessage.internal?: boolean` —— 内部消息：进模型上下文、**不进对话流**
- `Action "addPage"` 新增 `kind?: "native" | "file"`、`assetId?: string`

## 工作流 A · 预览缩放 + 新建页（空白 / 上传文件）+ 页名去重

**拥有**：`src/components/resume/panel.tsx`、`src/components/resume/resume.module.css`、
`src/store/workspace/reducer.ts`、`src/store/workspace/helpers.ts`、`src/lib/pages.ts`（新建）

### A1 缩放（用户原话：ctrl+滚轮缩放；130% 后直接跳回 100%；页面盖不满预览区）

现状与根因（已经查清，别重复排查）：
- `panel.tsx` 里 `zoom` 是本地 state，`useEffect` 用一个 `ResizeObserver` 观察画布，
  回调 `fit()` 把 zoom **重置**成 `min(1, max(0.68, (clientWidth-40)/374))`。
- 放大到 >100% 后纸面溢出 → 出现滚动条/尺寸变化 → observer 又跑 → zoom 被打回 100%。
  这就是「130% 后跳回 100%」。
- 范围写死 60–150，按钮 ±10；`paper.module.css` 的 `.wrap` 固定 374px 宽
  （A4 等比缩略），所以 150% 也只有 561px，宽面板下盖不满。

要求：
1. **缩放范围 25%–400%**。按钮 `−/+` 每次 ±10（到 25/400 就停住，不回绕）。
2. **Ctrl+滚轮**在画布区缩放：`event.ctrlKey` 时 `preventDefault`，按 ×1.1 / ÷1.1 连续缩放，
   夹在 25–400；缩放后把画布滚动位置按比例校正，尽量保持可视中心不跳。
   （非 ctrl 滚轮保持默认滚动，别拦。）
3. **手动缩放要留住**：用户手动调过（按钮或滚轮）之后，`ResizeObserver` 不再覆盖它；
   只有「切页 / 切到预览 / 点适应宽度 / 容器尺寸变化且当前仍是自动模式」才自动适应。
   建议用 `const manual = useRef(false)` + `fitMode` 两个概念，别把状态揉成一个。
   进预览时若是自动模式，照旧按 `min(1, ...)` 适配（保持现有观感）；点「适应宽度」时
   允许超过 100%（宽面板就该铺满），上限 400%。
4. 状态栏百分比按钮点开一个小菜单：`适应宽度` / `50%` / `75%` / `100%` / `150%` / `200%` / `300%` / `400%`。
   用仓库现成的 `Dropdown` + `MenuItem`（`@/components/ui/menu`）。
5. 纸面缩放继续用 CSS `zoom`（打印走 `data-print` 那套，别改打印链路）。
   `.wrap { width: 374px }` 不动 —— 缩放靠 `zoom`，别改成 transform（会影响打印与测量）。

### A2 新建页：空白 / 上传文件作为页

现状：`+` 按钮只 dispatch 一个空白页；`page.kind === "file"` 的渲染链路已经存在
（Rust `render_file_page` 输出 `<img class="full" src="asset:…">`，`form.tsx` 对 file 页只让改名）。

要求：
1. `+` 改成 `Dropdown`（触发按钮保持现在 22px 的方形 +），菜单两项：
   - `空白页`
   - `上传图片作为页…`（`<input type="file" accept="image/*">`）
2. 上传流程：`saveAsset(file)`（`@/lib/assets`）→ `dispatch({type:"addAssets", personId, assets:[asset]})`
   → `dispatch({type:"addPage", personId, resumeId, kind:"file", assetId: asset.id, label: 唯一页名})`
   → 选中新页、toast「已把「文件名」作为第 N 页」。
   非图片（pdf/docx…）明确 toast：「PDF / Word 不能直接作为页 —— 先导出成图片，
   或者把它当附件发给会话，让 agent 读文字」。**不要**把 pdf 塞进 file 页（渲染层只画 img）。
3. `reducer.ts` 的 `addPage` 吃到 `kind` / `assetId`：`{ ...newPage(id, label), kind, assetId }`
   （kind 缺省 native；assetId 仅 file 页带）。

### A3 页名去重（用户：新建的页名字都一样，没改名之前容易弄混）

`helpers.ts` 里 `newPage` 的「页名留空」策略保留（agent 建页仍然空着），
但**用户点 + 建页时**要起一个不重名的名字：`src/lib/pages.ts` 新增

```ts
/** 在已有页名里找一个不重名的：「未命名页」→「未命名页 2」→「未命名页 3」… */
export function nextPageLabel(pages: ResumePage[], base?: string): string;
```

规则：`base` 缺省 `"未命名页"`；没有重名就用 base 本身，否则从 2 开始试。
上传的文件页用文件名（去扩展名）作 base，同样去重。

## 工作流 B · 会话区只显示与会话 agent 的对话

**拥有**：`src/components/chat/panel.tsx`、`src/lib/agent-actions.ts`、`src/lib/layout.ts`、`src/store/workspace/provider.tsx`

现状（已查清）：排版 agent 的 `ask_session` → action `askSession` → 前端
`sendRef.current?.(layoutQuestion(...), [], pageId)` 把「排版 · 关于第 N 页：…」
**当成一条普通用户消息**塞进对话流，会话 agent 的答复也照常显示。用户要求这两条都不出现。

要求：
1. `send` 增加第 4 个参数 `opts?: { silent?: boolean }`：
   - `silent` 时：用户消息与 assistant 占位**都不进** `remote` 可见列表；
     `chat_push` 仍要发（带 `internal: true`），保证下一轮模型上下文连续；
     `chat_send` 完成后用 `chat_patch` 给这条 assistant 消息补 `internal: true`
     （桌面端；浏览器模式走 `dispatch appendMessage` 同样带 `internal: true`）。
   - `silent` 期间**不要**渲染打字动画 / 思考过程 / 停止按钮：那是给用户看的。
     但 `busy` 仍要置真（同一时刻只能有一轮，Rust 侧 `running` 表按 key 单飞）。
     建议单独一个 `silentBusy` state 控制「隐藏打字体」，`busy` 只管禁用发送。
2. 渲染过滤：`messages.filter((m) => !m.internal)` 之后再走 UserTurn/AssistantTurn/SystemTurn
   与空态判断（空态也只看可见消息）。
3. **答复要真的回到排版 agent 手里**，不能答了没人看：
   silent 轮结束后，用 `submitLayout({ person, resume, page: 目标页, state, kind: "request",
   text: "会话 agent 针对你上一个问题的答复：<正文>" })` 把它重新投进排版信箱
   （`@/lib/layout`，签名已存在，别改）。带 blockId 的提问要把「块 <id>」写进这段文字。
4. `askFromLayout` 的 5 秒防重保留；`provider.tsx` 里那份 `askSession`（也带 5 秒）
   与本处重复，**收敛成一处**：provider 只保留状态与事件，提问一律经 `ChatBridge`
   （`registerChatBridge` 的 `turn` 加第三个参数 `silent`）。
5. `agent-actions.ts` 里 `layoutQuestion()` 文案保留（它现在只进模型上下文，用户看不见），
   但注释要改成「内部提问用的提示词，不进对话流」。

## 工作流 C · 图片附件标签：悬停缩略图 + 点击大图

**拥有**：`src/components/chat/composer.tsx`、`src/components/chat/composer.module.css`、
`src/components/chat/message.tsx`、`src/components/chat/message.module.css`、
`src/components/ui/image-view.tsx`（新建）、`src/components/ui/image-view.module.css`（新建）

要求：
1. 新建 `ImageView`（大图模态）：复用 `ui/modal` 的 `Modal`，`title` = 文件名，
   只有单向关闭（传 `onClose`，组件会自动补右上角 X —— 遵守仓库的「取消/X 互斥」规则）。
   内容 `<img>`：`max-width: min(86vw, 1100px)`、`max-height: 78vh`、`object-fit: contain`、
   深色底衬托图片，图片本身 `background: var(--bg-elev)`。点击关闭也走 `onClose`。
2. **输入框里的标记**（`composer.tsx` 的 mirror 层 `.token`）：
   - `.token` 加 `pointer-events: auto`（mirror 本身仍是 none），这样能拿到 hover/click；
     注意：点了标签就是开大图，别指望再落光标 —— 用户明确要这个行为。
   - 仅当该附件是图片（`isImage(file.ext)`，`@/lib/attach`）才有浮层与点击行为。
   - hover 浮层：`position: fixed`，按标签的 `getBoundingClientRect()` 摆在标签正上方
     （`bottom: rect.top - 6`，水平居中并夹在视口内），`pointer-events: none`（不抢点击），
     显示 `part.file.preview`（本地 object URL）；没有 preview 就退回 `attachmentImage(id)`。
     离开时延迟 ~120ms 关闭（鼠标能移到浮层上—— 但浮层 pointer-events: none，所以直接关也行，
     用 120ms 防抖避免抖动）。缩略图最大 220×160，圆角 + `--shadow-paper`。
   - 点击 → `ImageView`。
3. **消息里的行内附件**（`message.tsx` 的 `.inlineAsset`）：同样 hover 缩略图 + 点击大图。
   图片地址按这个顺序取：`part.file.assetId` → `person.assets` 里找 `Asset` → `resolveUrl(asset)`
   （`@/lib/assets`，异步，缓存结果到组件 state/map）；拿不到再退回
   `attachmentImage(part.file.id)`（`@/lib/attach` 的内存 data URL，只在本会话有效）。
   `UserTurn` 需要拿到 `person`（现在只有 message）—— 加一个可选 prop `person?: Person`，
   由 `chat/panel.tsx` 传入……**不行，panel.tsx 是 B 的文件**。改用 `useWorkspace()` 在
   `UserTurn` 内部取 person（`store/workspace` 已经导出），C 只改 message.tsx。
4. `ChatPanel` 里 `send` 时附件要带上 `assetId`：`toAttachment` 产出的附件没有材料池条目。
   **C 不动 panel.tsx**，改为在 `message.tsx` / `composer.tsx` 侧兜底（2、3 两条已经写了兜底链）：
   `preview` → `attachmentImage` → 无图就不显示浮层。
5. 兼容旧消息：没有 assetId / 没有 preview 的历史消息，不显示浮层、点击不弹窗（别报错）。
6. 视觉：浮层与模态都走令牌（`--bg-elev` / `--line` / `--shadow-paper`），不出现硬编码颜色。

## 工作流 D · 新建自定义模型

**拥有**：`src/lib/model-config.ts`、`src/components/settings/sheet.tsx`、
`src/components/settings/sheet.module.css`、`src/lib/ai.ts`

现状：模型清单来自 `config/models.json`（构建期硬编码），设置页只有「选择已配置的模型」。
用户要**能自己加模型**。

要求：
1. `model-config.ts` 新增（保持 `MODELS` / `modelById` 导出，别破坏现有 import）：

```ts
/** 内置 + 自定义（localStorage `sharp-resume:models:v1`） */
export function listModels(): ModelConfig[];
export function findModel(id: string): ModelConfig | undefined;
export function isCustomModel(id: string): boolean;
export function saveCustomModel(model: ModelConfig): void;   // 同 id 覆盖
export function removeCustomModel(id: string): void;
```

自定义模型形状同 `ModelConfig`（`id` 用用户填的模型名，`label` 显示名，`provider` 显示为「自定义」，
`contextWindow` 缺省 64_000，`maxOutput` 缺省 8_192，`reasoning` 缺省 `[]`，
`vision` 由勾选决定，`defaultTemperature` 缺省 0.4，`note` 可空）。
`listModels()` 顺序：内置在前，自定义在后；读 localStorage 要 try/catch 防坏数据。
2. 设置页「模型」tab：
   - 下拉改用 `listModels()`；自定义项后缀「（自定义）」。
   - 下拉右侧加一个「新建」按钮（`+`），打开 **420 小窗**（用 `ui/modal` 的 `Modal`，
     有取消就不给 X）：字段 = 显示名称、模型名（id）、Base URL、API Key（可空，
     空则沿用当前 key）、上下文窗口、温度、思维强度（逗号分隔，可空）、能看图（复选）。
     保存 → `saveCustomModel` → 切到新模型（写 `saveAi`）→ toast。
   - 当前选中的是自定义模型时，同一行给「删除」（走 `ui/confirm` 的确认窗，删完回退到第一条内置）。
3. `ai.ts`：`DEFAULT_AI` / `AI_PRESETS` 允许继续用 `MODELS`（兜底值），
   但要让「切模型」不再依赖内置表 —— 设置页与别处调 `findModel`。
   `ai.ts` 若新增导出（如 `aiFromModel(model: ModelConfig): AiSettings`），写进摘要。
4. **不要改** `composer.tsx`（底栏模型下拉归工作流 C：它会把 `MODELS` 换成 `listModels()`）。

## 已知的跨流依赖（主控负责收口）

- C 把 `composer.tsx` 的 `MODELS`/`useMemo` 换成 `listModels()`（每渲染重建，模型数很少，别 memo）。
- B 把 `panel.tsx` 的 `modelById` 换成 `findModel`。
- B 在 `panel.tsx` 传 `person` 给 `UserTurn`？**不需要**：C 用 `useWorkspace()` 自取。

## 验收（主控）

- `npx tsc --noEmit` 干净、`npm run build` 通过、`cargo test --lib` 仍 167 项全绿。
- 代码审查：文件所有权无越界、共享类型未被改动、注释风格一致。
