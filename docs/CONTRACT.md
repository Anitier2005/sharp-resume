# 接口契约 · 会话 agent 写入通道 / 排版委派 / 确认门

> 这份文件是**唯一事实源**。前端、后端内容域、后端 agent 域、渲染域分别实现，
> 任何一方想改形状，先改这里。写完再改代码，不要反过来。

## 0. 一句话

会话 agent 在**当前候选人**范围内拿到与用户同等的读写权限；它的每一次写入都不是
「后端直接改库」，而是**产出一条和用户点击完全相同的 reducer action**，
交给前端走同一条通道落地 —— 因此校验、确认弹窗、撤销、持久化全部复用同一套。

排版不是会话 agent 的活。会话 agent 只负责**描述与转达**：
`request_layout` 把 brief 投进该页信箱，排版 agent 自己改；排版 agent 有疑问时，
反过来用 `note_on_page` 在纸面上留注、用 `ask_session` 向会话 agent 提问。

---

## 1. 出站通道（Rust → 前端）

`out.actions: Vec<Value>` 现在承载三类：

| type | 用途 | 现状 |
|---|---|---|
| `openPage` / `setView` / `setOpenPages` / `patchUi` / `patchPageStyle` / `patchPageLayout` / `requestLayout` / `ask` | 纯界面指令 | 已有 |
| **`openPerson`** / **`openResume`** | 切候选人 / 切简历（只切界面焦点，不改内容） | 新增 |
| **`mutate`** | **内容写入。信封里装 reducer action 数组** | 新增 |
| **`pageNote`** / **`askSession`** | 排版 agent 的注记与提问 | 新增 |

### 1.1 `mutate` 信封

```json
{ "type": "mutate", "ops": [ { "type": "sectionItems", "personId": "…", "resumeId": "…", "pageId": "…", "sectionId": "…", "items": [ … ] } ] }
```

规则：

1. **ops 里只能是 `src/store/workspace/types.ts` 的 `Action` 类型**，且必须落在
   第 1.2 节的白名单里（前端二次校验，越界整批丢弃并 toast）。
2. **顺序即语义**：前端按数组顺序 `dispatch`，不做合并。
3. **一轮最多一个信封**：`Outbox` 内部维护 `EditState`，所有内容工具改的是它；
   `turn::drain_actions` 在收尾时调用 `EditState::flush()` 产出一条信封。
   这样同一轮里对同一个容器的多次编辑不会互相覆盖（旧快照问题）。
4. `ops` 里引用的 `personId`／`resumeId`／`pageId` 由 `ToolCtx` 填，模型不写。

### 1.2 允许的 op 白名单与载荷

```
personFields  libraryItems  personAssets  itemAssets
addPage  removePage  pageLabel  pageHeader  pageStyle  pageLayout  blockRole
addSection  removeSection  blockFields  removeBlock  sectionTitle  sectionItems  entryPatch
createResume  renameResume  deleteResume  deletePerson
```

| op | 载荷 |
|---|---|
| `addPage` | `personId, resumeId, id, label?, open?` —— `id` 是**新页** id，由前端预生成；`pageId` 若出现请忽略 |
| `pageLabel` | `pageId, label` |
| `pageHeader` | `pageId, header: Field[]` |
| `blockFields` | `pageId, blockId, fields: Field[]` |
| `blockRole` | `pageId, blockId, role?` —— **role 省略 = 清除**，不要传 `null` |
| `addSection` | `pageId, id?, title, afterBlockId?` |
| `sectionTitle` | `pageId, sectionId, title` |
| `sectionItems` | `pageId, sectionId, items: SectionEntry[]` |
| `entryPatch` | `pageId, entryId, patch: { fields?, bullets? }` |
| `removeSection` | `pageId, sectionId` |
| `removePage` | `pageId` |
| `personFields` | `personId, fields: Field[]` |
| `libraryItems` | `personId, items: LibraryItem[]` |
| `createResume` | `personId, resumeId, title?` —— `resumeId` 是**新简历** id，工具预生成 |
| `renameResume` | `resumeId, title` |
| `deleteResume` | `resumeId` |

所有 op 都要带 `personId` / `resumeId`（除纯人级：`personFields` / `libraryItems` / `createResume` / `deletePerson`）。
`id?` 的两个（`addPage` / `addSection`）是「同轮内要引用刚建的东西」用的：
模型一轮里 `add_section` 之后紧接着 `add_entry`，后者要靠前者的 id 才找得到容器 ——
id 必须由**工具侧**生成并写进 op，等前端生成就晚了。

> `entryAdd`/`entryRemove` **不新增** reducer 类型：条目增删由 Rust 在 `EditState`
> 里算好新的 `items` 数组，走 `sectionItems`。少一处前后端形状同步。

### 1.3 确认门

**判据：不可撤销的东西都要问。**

- 前端 `needsConfirm(op)` 决定是否拦。当前为：`removePage`、`deleteResume`、
  `deletePerson`、`removeBlock`、`removeSection`。
  （UI 侧的「删分区 / 删块」原本不问，这次一并接到同一个 `Confirm` 上 ——
  agent 问、用户不问会更奇怪。）
- 拦截时：整批 ops **一起挂起**，弹出悬浮确认窗（`components/ui/confirm`），
  列出「将删除：第 2 页 · 工作经历 3 条」。用户确认 → 一次性按序 dispatch；
  取消 → 整批丢弃，并向会话追加一条 `role:"user", system:true` 的系统消息
  「（已取消：删除第 2 页 等 2 项改动）」。
- **走确认门的 op**（`removePage` / `deleteResume` / `deletePerson` / `removeBlock` /
  `removeSection`）：工具不知道用户会不会确认，返回文案统一为「已提交，待确认」，
  **禁止写成「已删除」**。
- **不进确认门的删除**（`remove_field` / `remove_entry` / `remove_library_item`）：
  它们产出的是整组替换（`personFields` / `blockFields` / `entryPatch` / `sectionItems` /
  `libraryItems`），op 上看不出「少了一项」，**拦不了**。所以文案必须写「已删除…立即生效」——
  说「待确认」就是空头承诺，模型会照着去告诉用户。
  要让条目删除可拦，得新增 `entryRemove` op；当前判断是不值得为它加一种 reducer 类型。

### 1.4 三个实现事实（写代码时会撞上）

1. **`addPage` 的新页 id 由前端预先生成**（`addPage.id?`）。确认通过后才 dispatch，
   而 reducer 里生成的 id 外面拿不到，`open` 就没法落点。
2. **没有 `ui.activePageId`**。「当前页」是 provider 的派生视图态（`resolvedPageId`），
   不在 `ui` 里。切页只能走 `openPage(id)`，不要往 `ui` 里塞。
3. **`mutate` 的 ops 是「容器全量替换」**。确认窗挂起期间如果 agent 又写一轮，
   旧 ops 会盖掉新值。当前靠「遮罩挡住交互 + 排版 agent 不写内容」兜住；
   真要并发写再上向量时钟，不要提前做。

---

## 2. 数据模型增量

```rust
// model.rs
pub struct Block {
    …
    /// 结构化生成的排版提示。语义由**人**给，生成器只照着选容器，不做内容分类。
    /// plain | tight | band | tags | kv | table | grid-2 | grid-3 | cols-2 | cols-3 | row
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

pub struct PageNote {
    pub id: String,
    /// 指向某一块；为空表示整页级注记
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    pub text: String,
    /// unix 秒
    pub at: i64,
}

pub struct ResumePage {
    …
    /// 排版 agent 留在纸面上的批注（不是内容，不参与导出）
    #[serde(default, skip_serializing_if = "is_empty_vec")]
    pub notes: Vec<PageNote>,
}
```

前端 `src/types/index.ts` 同形：`Block.role?: BlockRole`、`PageNote`、`ResumePage.notes?: PageNote[]`。

`role` 只能由 `set_block_role` 工具写（agent 用），UI 暂不暴露。旧数据没有该字段 = `plain`。

---

## 3. 会话 agent 工具（Scope::Session）

已有只读：`read_person` `list_pages` `read_page` `read_library`。
已有界面：`open_page` `set_view` `set_open_pages` `set_ui` `ask_user`。
已有委派：`request_layout`。

### 3.1 字段（field 全局唯一，按 id 定位，不需要 scope 参数）

| 工具 | 参数 | 产物 |
|---|---|---|
| `set_field` | `fieldId, label?, value?, assetId?` | 定位所属容器 → 容器全量替换 op |
| `add_field` | `ownerId, label, value?, afterFieldId?` | 同上，插入到 `afterFieldId` 之后 |
| `remove_field` | `fieldId` | 同上，移除 |

`ownerId` 可以是 `personId` / 字段块 id（页头就是第一个 `fields` 块）/ `entryId` / `itemId`。
工具靠扫描 workspace 判定归属；一个都匹配不上 → 报错并列出可用 id。
`assetId: ""` 表示清空图片位。

### 3.2 条目与分区

| 工具 | 参数 | 产物 |
|---|---|---|
| `add_entry` | `sectionId, fields[], bullets[]?, refId?` | `sectionItems` |
| `update_entry` | `entryId, fields?, bullets?` | `entryPatch` |
| `remove_entry` | `entryId` | `sectionItems` |
| `move_entry` | `entryId, to`（0 基） | `sectionItems` |
| `add_section` | `title, afterBlockId?` | `addSection` |
| `set_section_title` | `sectionId, title` | `sectionTitle` |
| `remove_section` | `sectionId` | `removeSection` |
| `set_block_role` | `blockId, role?` | `blockFields`? **否** —— 见下 |

`set_block_role` 需要新 reducer op `blockRole`（`{personId,resumeId,pageId,blockId,role}`），
加进 1.2 白名单。它不改内容，不进确认门。

### 3.3 页与简历

| 工具 | 参数 | 产物 |
|---|---|---|
| `add_page` | `label?` | `addPage`（新页零内容） |
| `delete_page` | `pageId` | `removePage` ⚠️ 确认门 |
| `rename_page` | `pageId, label` | `pageLabel` |
| `add_resume` | `title` | `createResume` |
| `rename_resume` | `resumeId, title` | `renameResume` |
| `delete_resume` | `resumeId` | `deleteResume` ⚠️ 确认门 |

### 3.4 经历库

| 工具 | 参数 | 产物 |
|---|---|---|
| `save_library_item` | `itemId?, fields[], notes?` | `libraryItems`（无 id 则新建） |
| `remove_library_item` | `itemId` | `libraryItems` |

### 3.5 批量

`apply_patch({ ops: [{ tool, args }] })`，1–40 条。实现上**不重复写一遍逻辑**：
内部逐条调用上表的同一个函数，最后仍由 `EditState::flush()` 出一条信封。
任何一条失败 → 整批回退（`EditState` 用快照回滚），返回失败序号与原因。

### 3.6 边界

- 只能看/改**当前候选人**。`personId` 不在参数里，来自 `ToolCtx`；模型想切人必须先 `open_person`。
- 新增工具 `open_person(personId)`（UI）与 `open_resume(resumeId)`（UI），
  `open_page` 已有。切人/切简历**不写内容**，只切界面焦点。

---

## 4. 排版 agent 的新工具（Scope::Layout）

| 工具 | 参数 | 产物 |
|---|---|---|
| `note_on_page` | `text, blockId?` | `{type:"pageNote", pageId, note:{id,blockId?,text,at}}` |
| `ask_session` | `question, blockId?` | `{type:"askSession", pageId, question, blockId?}` |

- `note_on_page`：纸面上一个批注点（有 `blockId` 贴在该块右侧，否则贴在页边）。
  用户可点开、可关闭。**不进导出**。**每轮最多 2 条**（`Outbox` 里计数，超了直接拒绝，不是静默丢弃）。
- `ask_session`：把问题交给会话 agent。`blockId` 要一起下发 —— 会话 agent 得知道
  「问的是哪一块」，否则只能再调一次 `read_page` 猜。
  前端收到后**追加一条来自「排版」的会话消息**，并**自动让会话 agent 跑一轮**。
  **每轮最多 1 次**（同计数方式），防止两个 agent 互相呼叫打转。

排版 agent **不能**调内容写入工具 —— 它的职责边界是「不改内容」。`Scope` 表照旧：
`set_page_layout` / `read_layout_guide` 只有 `Layout`；内容工具只有 `Session`。

---

## 5. 渲染增量（render）

`Block.role` → 容器 class。仅对 `section` 块与 `fields` 块生效：

| role | 生成 |
|---|---|
| 缺失 / `plain` | `<section class="sec">` —— **不要输出 `sec plain`**：`paper.css` 里的 `.sec.plain` 是「去掉标题下划线」的变体，把默认档写成 plain 会让所有旧页升级后集体丢下划线 |
| `tight` / `band` | `<section class="sec tight">` / `sec band` |
| `tags` | `<section class="sec"><div class="tags">`，**每条条目只取第一个字段的值**做成 `<span class="tag">` |
| `kv` | `<section class="sec"><dl class="kv">`，条目内每个字段一对 `<dt>标签</dt><dd>值</dd>` |
| `table` | `<section class="sec"><table><tbody>`，每条条目一行 `<tr>`，字段值依次成 `<td>`（第一个加 `<b>`） |
| `grid-2` `grid-3` `cols-2` `cols-3` | `<section class="sec {role}">` + 常规条目；分区标题必须占满整行 |
| `row` | 标题在**外**、条目在内：`<section class="sec"><h2 class="t">…</h2><div class="row">条目</div></section>`；`.row > .entry` 逐条两端对齐 |
| ~~`split`~~ | **已撤出 role**。它要求条目内部恰好是「左栏元素 + 右栏元素」，结构化生成器产出的条目塞进 78px 会把公司名挤成三行。写进 role 会得到一条 warning + 回落默认档；`.split` 类保留给手写 `parts` |

`tags` / `kv` / `table` 会丢信息（要点列表、图片字段、第二个之后的字段），
必须调 `warn_if_lossy` 记一条 warning，让 agent 自己决定要不要换 role。

未知 role → `warnings` 加一条 + 回落默认档。`role` 缺失 = 默认档。
`fields` 块只认 `grid-2` / `grid-3` / `cols-2` / `cols-3`，其余 role 记 warning。

### 5.1 块锚点

生成的 HTML 里，**每一个块的根元素**必须带 `data-block="<blockId>"`（手写 `parts` 无法保证，
所以只在结构化生成的分支加）。用途：
1. `note_on_page` 的批注要贴到具体块上；
2. 以后做「点纸上某段 → 跳到表单对应项」。

`data-block` 要加进 `GENERIC_ATTRS`，并确认 `ammonia` 没把它清掉（`data-*` 默认会被丢）。

**canvas 不支持**：`TAGS` 里没有、也不加。`GUIDE` 末尾补一句「矢量图、图表一律用
`<svg>`，canvas 会被清洗掉」。

---

## 6. 文件所有权（不许跨列改）

| owner | 文件 |
|---|---|
| **lead（我）** | `docs/CONTRACT.md`、最终集成与验证 |
| **BE-content** | `src-tauri/src/tools/edit.rs`(新)、`src-tauri/src/model.rs`、`src-tauri/src/agent/turn.rs`、`src-tauri/src/tools/read.rs` |
| **BE-domain** | `src-tauri/src/tools/mod.rs`、`src-tauri/src/tools/page.rs`、`src-tauri/src/tools/layout.rs`、`src-tauri/src/agent/layout.rs`、`prompts/*.md` |
| **FE** | `src/types/index.ts`、`src/store/workspace/*`、`src/lib/agent-actions.ts`、`src/components/ui/confirm*`、`src/components/chat/*`、`src/components/resume/paper.tsx`、`src/components/resume/panel.tsx` |
| **Render** | `src-tauri/src/render/mod.rs`、`src-tauri/src/render/paper.css` |

`src-tauri/src/tools/edit.rs` 只被 BE-domain 在 `mod.rs` 里 `mod` + 注册，两边都别动对方的文件。
