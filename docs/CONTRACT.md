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
resumeTargetPages
pageBlocks  removeEmptyPage  resumeNotice
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
| `resumeTargetPages` | `targetPages?, turn?` —— `targetPages` **省略 = 清除**（回到「未设置」），不要传 `null`；`turn` 是写入时对应的用户消息序号，前端缺省 0 |
| `pageBlocks` | `personId, resumeId, pageId, blocks: Block[]` —— **整页全量替换**。之前只有容器级替换（`sectionItems` 等），没有页级；`split_page` 需要这个粒度 |
| `removeEmptyPage` | `personId, resumeId, pageId` —— **不进确认门**（空页没有内容可丢）；reducer 必须**再判一次空**，非空则整条丢弃 |
| `resumeNotice` | `personId, resumeId, key, value` —— `key` 白名单：`targetPagesNotice`；`value` 必须是布尔。**不改内容，不进确认门** |

所有 op 都要带 `personId` / `resumeId`（除纯人级：`personFields` / `libraryItems` / `createResume` / `deletePerson`）。
`id?` 的两个（`addPage` / `addSection`）是「同轮内要引用刚建的东西」用的：
模型一轮里 `add_section` 之后紧接着 `add_entry`，后者要靠前者的 id 才找得到容器 ——
id 必须由**工具侧**生成并写进 op，等前端生成就晚了。

> `entryAdd`/`entryRemove` **不新增** reducer 类型：条目增删由 Rust 在 `EditState`
> 里算好新的 `items` 数组，走 `sectionItems`。少一处前后端形状同步。

`resumeTargetPages` 不进确认门：它不改内容，改的是「这一份简历要压到几页」这个约定。
两个键落进 `Resume.extra`（`targetPages` / `targetPagesTurn`）：reducer 里必须 `{...resume}`
展开再改，手写重建对象会把它们丢掉。

`resumeNotice` 为什么不塞进 `resumeTargetPages` 当一个 `notice` 字段：后者的 `targetPages`
**缺省就是清除**，用它 ack 一条「提示已看过」，会顺手把用户的目标页数清掉。
**宁多一个 op，不要让确认动作带副作用。**

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

**新增不进确认门的两个**：`removeEmptyPage`、`resumeNotice`。
理由：`removeEmptyPage` 的判据是「不可撤销才问」—— 空页没有内容可丢，问一次只是噪音。
双保险：工具侧校验 + reducer 侧再校验（防旧版本 agent、防确认窗重放）。

### 1.4 三个实现事实（写代码时会撞上）

1. **`addPage` 的新页 id 由前端预先生成**（`addPage.id?`）。确认通过后才 dispatch，
   而 reducer 里生成的 id 外面拿不到，`open` 就没法落点。
2. **没有 `ui.activePageId`**。「当前页」是 provider 的派生视图态（`resolvedPageId`），
   不在 `ui` 里。切页只能走 `openPage(id)`，不要往 `ui` 里塞。
3. **`mutate` 的 ops 是「容器全量替换」**。确认窗挂起期间如果 agent 又写一轮，
   旧 ops 会盖掉新值。当前靠「遮罩挡住交互 + 排版 agent 不写内容」兜住；
   真要并发写再上向量时钟，不要提前做。

### 1.5 实测上送（不落库）

页面实测值只有一个来源：前端。它随请求上送，**不进 workspace、不进数据库** ——
落库的测量值下次启动会变成假数据。

| 通道 | 字段 | 形状 |
|---|---|---|
| `chat_send` | `args.measure` | `PageMeasure?` |
| `chat_send` | `result.origin` | `{ name: string, pages?: number }?` |
| `layout_submit` | `args.measure` | `PageMeasure?` |
| `layout_submit` | `args.origin` | `{ name, pages? }?` —— 原件页数，只在本轮有效 |

`chat_send.result.origin` 来自本轮附件的解析结果（`attach::Materials.origin`），**不落库**：
前端拿到后存内存，随下一次 `layout_submit` 的 `args.origin` 转交给排版侧。
刷新或重启即丢失 —— 那时排版侧按「这一轮没有上传原件」处理，不假装知道。

`PageMeasure.status` 只有三个：`ok`（量到了）/ `unknown`（量不到，带 `reason`）/
`stale`（量到过，但内容之后改过 —— 旧值作废，带 `previous`）。

阈值（达标 / 容忍 / 溢出三档）与全部文案只在
`src-tauri/src/agent/facts.rs` 实现，前端与渲染层不许复刻。

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

`Resume` 新增两个一等字段（不再是 extra 里的未知键）：

```rust
/// standard / freeform / file。**存储用 String**（未知值走推断，不整库读不出），
/// 渲染层内部才换成 enum。三选一，不许混用（normalize 会纠正，只降级不删页）。
pub kind: String,

/// 纸张与页边距。毫米，顺序 `[上,右,下,左]`，与 CSS margin 简写同序。
/// 三处同源的唯一来源：预览 --pad-*、打印 @page、docx twips。
/// size 值域 A4 / Letter / custom（未知按 A4）；custom 才看 widthMm / heightMm。
pub paper: Paper,          // 默认 A4 + margin [20.0; 4]
```

前端同形（`src/types/index.ts` 的 `Resume` 没有索引签名，必须显式声明）：
`kind?: ResumeKind`、`paper?: { size: PaperSize; widthMm?: number; heightMm?: number; margin: [number, number, number, number] }`。
`margin` 用**元组**不用具名对象：四值在打印 / `--pad-*` / twips 三处都按顺序用，
具名字段会诱使人写 `paper.margin.left`（不存在）而运行时拿到 `undefined`。

`Resume.extra` 上新增两个键（走 `#[serde(flatten)]`，不改 struct 形状）：

```rust
/// 用户明确说过的目标页数。没有 = 未设置。
/// 前端 `src/types/index.ts` 必须显式声明 `targetPages?: number`（`Resume` 没有索引签名）。
pub target_pages: Option<u32>,       // JSON: targetPages
pub target_pages_turn: Option<u32>,  // JSON: targetPagesTurn —— 第几条用户消息写入的
```

`Resume.extra` 上再记一个「已看过」标记：

```rust
pub target_pages_notice: Option<bool>,   // JSON: targetPagesNotice —— 旧值语义债的一次性提示
```

**`{target}` 槽位文案改写**（路线二：`targetPages` = **每段**上限，取代 photo-fit 那版）：

| 来源 | 文案 |
|---|---|
| `UserThisTurn` | `{n} 页（用户这一轮明确要求：这一段最多 {n} 页）` |
| `UserBefore` | `{n} 页（用户之前要求过，仍然有效：这一段最多 {n} 页）` |
| `FromOrigin` | `{n} 页（沿用原件页数：原件只有 {n} 页，用户没有另说，整份只有这一段）` —— **只在 `pages.len()==1` 时出现** |
| `Unset` | 不变 |

`Asset.extra` 上新增三个键，用于「从 PDF 抽出来的候选图」：

```rust
pub role: Option<String>,       // "candidate" —— 渲染层不会拿它当页头照片
pub source: Option<String>,     // 源文件名（人类可读；源附件不进 person.assets，因为要保住一轮 8 张候选的名额）
pub page_index: Option<u32>,    // 在来源 PDF 里的页序（1 基）
```

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
| `set_target_pages` | `pages?` | `resumeTargetPages`（省略 = 清除） |

`set_target_pages` 的语义修正：`targetPages` 约束的是**当前这一段**，不是整份。
工具描述与返回文案里的「这份简历压到 N 页」全部改为「这一段最多 N 页」。
`split_page` / `delete_empty_page` **不给**会话侧（会话的 `add_page` / `delete_page` 保持现状）。

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

### 3.7 附件与抽图

| 工具 | 参数 | 产物 |
|---|---|---|
| `extract_images` | `stored, name?` | 落池的 candidate 资产 + 候选清单文本 |

- `stored` 是附件在 `inbox/` 里的存储名，从会话材料段的「附件标识」行取，**不是文件名**。
- **不动源附件**：直接从 `inbox/` 读字节，只把候选图落池成 image 资产；源附件不进
  `person.assets`（一轮 8 个名额全给候选图）。所以候选图上 `extra.source` 记的是**源文件名**，
  不是源 PDF 的 assetId —— 那个 id 在 `person.assets` 里查不到，写了就是悬空引用。
- **本期不做「把源 PDF 登记成 `kind:"file"` 资产」**（幂等材料化）。原因：抽图只需要读 `inbox/`，
  不需要源 PDF 的 assetId；而一旦登记，`pool/` 里就多一份没有任何资产引用、`sync` 也永远不会
  回收的副本，每抽一次攒一份。
- 幂等：同一张图只落一份、id 恒等于池内存储名（`<sha16>.<ext>`），所以「每行开头反引号里的
  assetId」可以直接交给 `set_field`；已登记的按 id 跳过、且不占这 8 个名额 —— 再调一次就是拿下一批。
- 只对 PDF 生效，非 PDF 直接报「这不是 PDF」。
- 抽出的图**进 `person.assets`** 并带 `role:"candidate"`：模型能用它，渲染层不会拿它当页头照片。
- 系统**不判断**哪张是证件照 —— 只给尺寸与占比，选哪张由模型定。

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

| 工具 | 参数 | 产物 |
|---|---|---|
| `split_page` | `pageId?, afterBlockId` | `addPage` + `pageBlocks`×2 + `pageLayout`×2 |
| `delete_empty_page` | `pageId` | `removeEmptyPage` |

`split_page` 的语义（10 条）见技术方案 §4.1：新页插在源页后、最后一块报错、`parts` 随块走、
`preset` 继承、`layout.html` 存在则报错、`notes` 留源页、一轮 1 次、总页数上限 12、
返回值带新页 id。**一次 turn 只许 1 次**（`Outbox.splits` 计数，超了报错不是静默丢弃）。

`set_page_layout` 的返回在 freeform 档要追加（检测到绝对定位时）：

`注意：这一版有绝对定位（position:absolute / fixed）的内容，它不参与页高计算，实测页数可能比实际偏小。给根容器一个 min-height 能改善。`

排版 agent **不能**调内容写入工具 —— 它的职责边界是「不改内容」。`Scope` 表照旧：
`set_page_layout` / `read_layout_guide` 只有 `Layout`；内容工具只有 `Session`。

### 4.1 返回文案里的实测结论

`set_page_layout` 的返回值**必须**带一行实测结论，三态各一句（见 `agent/facts.rs`）：

- 达标：`实测 1.00 页，没有溢出（目标 1 页，达标）。`
- 未达标：`实测 1.12 页，超出 0.12 页（目标 1 页，仍未达标）。继续压，或停在这一档并说清差多少。`
- 量不到：`尚未实测（测量不可用：预览没打开）。这一轮不能确认是否在一页内 —— 不要写「已压到一页」。`

量不到时的措辞**必须**与「达标」明显不同：测量不可用时写「尚未实测」，不许写「没有溢出」。
`describe_page` 的正文末尾追加 `## 这一页的实测` 一段，与上面同源（`facts::measure_section()`）。

`达标` 只用于**用户说过的**目标（`target.source` 为 `UserThisTurn` / `UserBefore`）；
目标来自原件页数（`FromOrigin`：系统按原件页数给的默认值，不是用户的要求）时**不宣称达标**，
改说「按原件页数 {t} 页算」（事实块、实测段、`set_page_layout` 返回值三处同批）。

**文案只陈述系统状态，不替用户作证。** `facts.rs` 只能陈述「系统里记了什么」，
不能陈述「用户说过 / 没说过什么」—— 它看不到用户那句话，一旦模型漏调 `set_target_pages`，
替用户作证就成了伪证，而读者（用户与模型）都会信它。同理，事实块不陈述「用户是否说过」，
只陈述「系统里有没有记下目标页数」：`{target}` 的未设置档写
`未设置（系统里没有记下目标页数…）`，`FromOrigin` 的括注写 `系统里没有记下目标页数`。

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

### 5.0 页头照片的候选豁免

`photo_of()` 选页头照片时**跳过** `asset.extra.role == "candidate"` 的资产。

原因：`extract_images` 把 PDF 里的位图全部登记进 `person.assets`，而 `photo_of` 取的是
「第一张 `kind == "image"`」—— 不跳的话，PDF 里的第一个 logo / 水印 / 装饰条会立刻
变成页头的证件照，用户会看到一张完全无关的图被当成照片。

老数据没有 `role` 键 → 行为不变（仍然取第一张），升级后用户原有证件照不会消失。

### 5.1 块锚点

生成的 HTML 里，**每一个块的根元素**必须带 `data-block="<blockId>"`（手写 `parts` 无法保证，
所以只在结构化生成的分支加）。用途：
1. `note_on_page` 的批注要贴到具体块上；
2. 以后做「点纸上某段 → 跳到表单对应项」。

`data-block` 要加进 `GENERIC_ATTRS`，并确认 `ammonia` 没把它清掉（`data-*` 默认会被丢）。

**canvas 不支持**：`TAGS` 里没有、也不加。`GUIDE` 末尾补一句「矢量图、图表一律用
`<svg>`，canvas 会被清洗掉」。

### 5.2 三条不变量（跨两档）

**I1 禁清单只有一份**（`render::sanitize::FORBIDDEN_ALWAYS`，两档共用，只增不减）：
`script` / `on*` / `javascript:` / `vbscript:` / `iframe` `frame` `frameset` `object` `embed`
/ `base` / `data:text/html` / `@page` / `@import`。
**standard 档也不例外** —— 它现在走 schema 受限的白名单，但 `parts` 已允许局部 HTML，
将来放宽时必须已经受到同一份禁清单保护。

**I2 `@page` 只有生成器能写**：纸张与页边距只来自 `Resume.paper`；模型写的 `@page` 一律剥掉，
并与「`@page` 从静态 CSS 变注入」共用一个注入点（`src/styles/page-setup.ts`）。

**I3 入口唯一、管线可分歧**：`compile()` 是唯一分流点（按 `Resume.kind`）；
`sanitize()` 是唯一清洗入口但按 kind 取策略。**不许出现第二个 `sanitize` 调用点。**

### 5.3 freeform 档

`kind == "freeform"` 时，文档本体是 `layout.html`，按**黑名单**清洗（只剥 I1 与 I2），
不套 preset 白名单、不产结构化块；`preset` 恒为 `custom`；页边距仍由 `--pad-*` 给。
Word 导出禁用（`kind == "freeform"` 时按钮置灰并说明）。

### 5.4 file 档

`kind == "file"` 时允许**逐页预览全部页**、全部只读、导出逐页原样拷贝。
UI 常驻提示：`这是一份由 {n} 个原件组成的简历，不可编辑`。

---

## 6. 文件所有权（不许跨列改）

| owner | 文件 |
|---|---|
| **lead（我）** | `docs/CONTRACT.md`、最终集成与验证 |
| **BE-content** | `src-tauri/src/tools/edit.rs`(新)、`src-tauri/src/model.rs`、`src-tauri/src/agent/turn.rs`、`src-tauri/src/tools/read.rs` |
| **BE-domain** | `src-tauri/src/tools/mod.rs`、`src-tauri/src/tools/page.rs`、`src-tauri/src/tools/layout.rs`、`src-tauri/src/agent/layout.rs`、`prompts/*.md` |
| **BE-domain** | `src-tauri/src/pdfembed.rs`(新)、`src-tauri/src/agent/facts.rs`(新)、`src-tauri/src/tools/extract.rs`(新) |
| **FE** | `src/types/index.ts`、`src/store/workspace/*`、`src/lib/agent-actions.ts`、`src/components/ui/confirm*`、`src/components/chat/*`、`src/components/resume/paper.tsx`、`src/components/resume/panel.tsx` |
| **BE-domain** | `src-tauri/src/capability.rs`(新)、`src-tauri/src/render/sanitize.rs`(新) |
| **FE** | `src/styles/page-setup.ts`(新)、`src/lib/capability.ts`(新) |
| **Render** | `src-tauri/src/render/mod.rs`、`src-tauri/src/render/paper.css` |

`src-tauri/src/tools/edit.rs` 只被 BE-domain 在 `mod.rs` 里 `mod` + 注册，两边都别动对方的文件。

---

## 7. 模型能力（看图）

### 7.1 三态

`supported` / `unsupported` / `unknown`。**只有 `unknown` 与 `unsupported` 的区别是本质的**：
前者是「没测出来」，后者是「确认不行」。任何地方都不许把 unknown 当 unsupported 用。

### 7.2 存储

`settings` 表，key = `capability:v1`：

```json
{ "version": 1,
  "items": { "<fp>": { "state": "supported|unsupported|unknown",
                       "reason": "none"|"protocol-reject"|"auth"|"quota"|"bad-request"|"offline"|"timeout"|"no-probe",
                       "checkedAt": 1765000000, "model": "…", "baseUrl": "…" } } }
```

指纹：`hex(sha256("{baseUrl}\n{model}\n{apiKey}"))[..16]`。**不存密钥本体**。
换 key / 模型 / baseUrl → 不命中 → `unknown`（天然失效，无需清理）。

### 7.3 唯一入口

Rust：`capability::resolve(base_url, model, api_key) -> Capability`。
`attach::materials` / `tools::view` / UI 徽标三处**都只能调它**。
`SendArgs.vision` 字段**删除**；`Outbox.vision` 改为 `resolve(...) == Supported` 的派生值。

### 7.4 探测判定

全对 → `supported`；答错 / 半对 / 拒答 → **`unknown`**（不许判 unsupported）；
只有协议层明确拒图（`classify_error == ImageRejected`）→ `unsupported(protocol-reject)`。

### 7.5 改判必须可逆

`unsupported(protocol-reject)` 由 `net.rs` 在收到 ImageRejected 时就地写入；
同指纹探测得 `supported` → **无条件覆盖**（后写胜）。
400 但与图像无关、429 / 5xx / 超时 / 连接失败 → **不得改判**。
判 `ImageRejected` 还要求**这条请求真的带了图**（`net::has_images`）：纯文本请求被 400
拒绝跟能力无关。

### 7.6 自动探针（否则「没测过」等于永远不发图）

`unknown` 不发图，所以「用户没点过那个按钮」会让能看图的模型永远只收到元数据 ——
这正是这条链路坏掉时的症状。因此 `chat_send` 在**带附件**且 `should_auto_probe()`
为真时，当场探一次再决定。节流规则（三种，不许合并）：

| 上一次的结论 | 这一轮 |
|---|---|
| 从没测过（`checkedAt` 为 null） | **测** |
| `offline` / `timeout` 且已过 60 秒 | **测**（那是暂时的） |
| `supported` / `unsupported` / `auth` / `quota` / `bad-request` / `no-probe` | **不测**（配置问题重复测只是白花一次请求） |

### 7.7 两侧同源

`chat_send`（会话）与 `layout_submit`（排版）各自 `resolve` 一次，写入自己的
`Outbox.vision`，并把 `facts::capability_block(cap)` 拼进 system。排版侧不许省这一步：
它同样会在 `view` 里渲染页面给自己看。
