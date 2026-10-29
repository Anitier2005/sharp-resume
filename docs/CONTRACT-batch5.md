# 批次五契约（多 agent 并行）

## 全局红线

- 只改「文件所有权」里列出的文件，别的一律不碰（尤其别碰 `src/components/knowledge/`、
  `src/components/settings/`、`src-tauri/`、`src/types/index.ts`）。
- **不要跑 `npm run build`**（会写 dist，多路并发会踩）。类型自检只用
  `npx tsc --noEmit -p tsconfig.json`（项目根目录）。
- **不要跑任何 cargo 命令**：磁盘空间紧张，构建由主控统一做。
- 样式只用令牌（`--bg-elev` / `--line` / `--fg-*` / `--acc-*` / `--shadow-*`），
  禁止硬编码颜色；禁止原生 `<select>` / `type=range` / `type=checkbox` / `<details>` / `title=""`。
  组件从 `@/components/ui` 取（Button / Field / Input / Tooltip / Modal …）。
- 注释写「为什么」，中文，不用 emoji。
- 交付：用 SendMessage 给 team-lead 报 ≤12 行摘要（改了哪些文件、对外 API、没做到的部分）。

## 稳定接口（两边都按这个写，别改签名）

```tsx
// src/components/ui/image-view.tsx —— 主控与另一路都会调用
export function ImageView(props: { src: string; name: string; onClose: () => void }): JSX.Element;
export function ImagePeek(props: { src: string; name: string }): JSX.Element; // 悬停缩略图，保持现状
```

## 工作流 A：图片查看器（`agent-image-viewer`）

所有权：`src/components/ui/image-view.tsx`、`src/components/ui/image-view.module.css`。

把现有 `ImageView` 升级成「查看器」，`src` / `name` / `onClose` 签名不变：

1. **缩放**：底部工具条 `−` / 百分比 / `+`，档位 50%–400%，`+`/`−` 每次 ×1.25（到界限停住）；
   鼠标滚轮按 ×1.1 连续缩放（非 ctrl 也拦，因为这个模态里滚动没有别的含义）；
   双击图片在「适应窗口」与 100% 之间切换。
2. **平移**：放大到超出容器后可拖动（pointer 事件 + transform），拖动时 `cursor: grab/grabbing`；
   缩放到适应窗口时自动回到居中。
3. **格式转换**：工具条右侧给「下载」菜单（`Dropdown` + `MenuItem`）：PNG / JPEG / WebP。
   用 canvas `toBlob` 转换（JPEG 需要先铺白底，否则透明区变黑），A 标签 `download` 落盘，
   文件名取 `name` 去掉原扩展名再加新扩展名。转换失败要 toast 一句，不要静默。
4. **信息**：工具条左侧显示原始像素尺寸（`img.naturalWidth × naturalHeight`），
   加载中显示占位，加载失败显示「这张图读不出来」而不是空白。
5. 沿用 `Modal`（只传 `title` + `onClose`，不传 `onCancel`），图片区域 `background: var(--paper-fg)`
   这类深色衬底；`data-print="hide"`。
6. `ImagePeek` 行为不变（那是输入框/消息里悬停用的），只保证它和新 `ImageView` 共用样式文件时不互相污染。

## 工作流 C：会话消息里的图片（`agent-chat-images`）

所有权：`src/components/chat/message.tsx`、`src/components/chat/message.module.css`、`src/lib/assets.ts`。

背景：知识库的图片块会以 `asset:<stored>` 的 Markdown 形式被模型引用
（`stored` 是材料池里的文件名，例如 `![门店照片](asset:ab12cd34ef56.png)`），
需要它在会话里**真的显示成图片**，而不是一行字。

1. `src/lib/assets.ts` 新增并导出：

   ```ts
   /** 按材料池里的存储名取显示地址；读不到返回 null（不抛） */
   export function resolveStored(stored: string): Promise<string | null>;
   ```

   内部复用 `urlCache` 与现有的 `pool_get` 逻辑，不要复制一份新的缓存。

2. `message.tsx`：助手（assistant）消息正文渲染前，先把 `asset:<stored>` 的 Markdown 图片语法
   （`![alt](asset:xxx.png)`，也允许没有 `![]` 包裹的裸 `asset:xxx.png`）切成
   「文字段 / 图片段」。图片段渲染成一张缩略图（最大宽 260、最大高 180、圆角 6、
   边框 `--line`、`object-fit: cover`），点击用 `ImageView` 打开查看器（签名见上）。
   - 地址异步解析：模块级缓存 `Map<stored, string|null>`，解析中用占位块，解析不到就退回显示原文本。
   - 图片段落不要被 markdown/代码块处理逻辑吃掉：先替换成占位符，处理完文字再拼回去，
     或者按现有的分段渲染链路插入新的 segment 类型 —— 以现有实现为准，别重写整条链路。
   - 不要影响：用户消息、已有行内附件（`.inlineAsset`）、工具卡片、代码块。
3. `message.module.css` 只加图片段需要的类（缩略图/占位/失败态），不要改动既有类。
