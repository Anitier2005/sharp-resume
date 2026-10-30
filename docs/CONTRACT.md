# 接口契约 · 会话 agent 写入通道 / 排版委派 / 确认门

> 这份文件是**唯一事实源**。前端、后端内容域、后端 agent 域、渲染域分别实现，
> 任何一方想改形状，先改这里。写完再改代码，不要反过来。

## 0. 一句话

会话 agent 在**当前候选人**范围内拿到与用户同等的读写权限；它的每一次写入都不是
「后端直接改库」，而是**产出一条和用户点击完全相同的 reducer action**，
交给前端走同一条通道落地 —— 因此校验、确认弹窗、撤销、持久化全部复用同一套。

排版不是会话 agent 的活。会话 agent 只负责**描述与转达**：
`request_layout` 把 brief 投进该页信箱，排版 agent 自己改；排版 agent 有疑问时，
反过来用 `note_on_page` 在纸面上留注给用户看。

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

## 3. 会话工具

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

## 4. 排版工具（与会话工具同在一套里）

**没有第二个 agent。** 曾经有过「页排版 agent」：独立的一轮循环、每页一份会话、靠信箱与
`layout:applied` 事件和会话互相喊话。它被取消掉了 —— 代价是两边互相看不见（内容改完排版不知道、
排版改完会话不知道），换来的是两个模型各说各话、每次内容变更都要多烧一轮。

合并后的形态：**一套工具集，一个 `agent::turn::run`**。排版三个工具与内容工具并列：

| 工具 | 参数 | 产物 |
|---|---|---|
| `set_page_layout` | `preset? / head? / parts? / html? / margin?` | `{type:"patchPageLayout", pageId, layout}` |
| `read_layout_guide` | 无 | 组件手册原文（`render::GUIDE`） |
| `note_on_page` | `text, blockId?` | `{type:"pageNote", pageId, note:{id,blockId?,text,at}}` |

- `note_on_page`：纸面上一个批注点（有 `blockId` 贴在该块右侧，否则贴页边）。用户可点开、可关闭，
  **不进导出**。**每轮最多 2 条**（`Outbox.notes` 计数，超了直接拒绝，不是静默丢弃）。
- 三个工具都在 `specs()` 的全量清单里，**没有 `Scope` 这层抽象**：内容与版面都是同一个模型在决定。
- 曾经存在的 `ask_session`（排版 agent 向会话提问）与 `request_layout`（会话把意图转给排版 agent）
  随机制一起删除。历史会话里模型仍可能硬调 `ask_session`，`call()` 会返回一句人话说明它不存在，
  不能落成「没有这个工具」这种读起来像配置错了的报错。

### 4.1 返回文案里的实测结论

`set_page_layout` 的返回值**必须**带一行实测结论，三态各一句（见 `agent/facts.rs`）：
达标 / 未达标 / 量不到。量不到时的措辞**必须**与达标明显不同：写「尚未实测」，不许写「没有溢出」。

**量不到时还要追加一句「实测滞后一轮」**：页数是上一轮纸面量出来的，这一轮改完不会变。
不写这句，模型会连改两次、看到数字没动，然后推出「版面无效、瓶颈在内容」，转头去劝用户删内容 ——
这个推论我们已经在真机上见过，代价是把「忠实还原」这个目标本身丢了。

**有数字时同样要说时态**：那行是**这次改动之前**量的。真机出过：模型把「实测 1.22 页」当成
「刚返回的结果」转述给用户，而改完其实是 1.15 页 —— 数字是真的，时态是假的，用户据此做的取舍跟着错。
所以三态各有各的尾巴：量不到 → 「等这一轮结束」；有数 → 「这是改动之前的，改动后的要等重排」。

`达标` 只用于**用户说过的**目标（`target.source` 为 `UserThisTurn` / `UserBefore`）；
目标来自原件页数（`FromOrigin`）时**不宣称达标**，改说「按原件页数 {t} 页算」。

**文案只陈述系统状态，不替用户作证。** `facts.rs` 只能陈述「系统里记了什么」，
不能陈述「用户说过 / 没说过什么」—— 它看不到用户那句话，一旦模型漏调 `set_target_pages`，
替用户作证就成了伪证，而读者（用户与模型）都会信它。

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

### 5.00 条目头是一行三段

默认档的条目头**必须**是一行：`<div class="erow"><b>名称</b><span class="role">中间字段</span><span class="time">时间</span></div>`。

- 中间字段**去掉「标签：」前缀**（原件印的就是 `教育部直属 211` / `行政管理` / `产品助理`）。
- 时间字段：标签命中 `时间/日期/起止…`，或值的形态像日期区间 → `<span class="time">` 靠右；认不出来就留在中间，**不许硬推到右边**。
- 只有一个字段且值为空、但有要点列表的条目（真机上的「技能与兴趣」）→ **整行头都不出**，否则纸上会把分区标题重复一遍。

为什么值得单独立一条：早先每个字段独占一行、还带标签，一个条目就多占 2 行；一份 2402 字的简历因此
多出 11–14 行 —— 正好是「实测 1.15 页、怎么调版面都压不回一页」的全部来源。改完之后同一份内容
**1.00 页**收进来，且和原件长得一样。**排版压不下去时，先怀疑生成器在浪费行，不要先怀疑字号。**

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

---

## 8. 会话轮次的终止判定

**唯一可靠信号是 `finish_reason`**（OpenAI 口径：`stop` / `length` / `tool_calls` / `content_filter`；
Anthropic 兼容端点给 `stop_reason`，`finish_reason` 优先，都缺则退回旧判据并打日志）。
按「正文有没有内容」判结束是生产反模式——真机上就是这么把一个"说到一半"的回合当成完成落库的。

`net::StreamOutcome` 必须收下三样：`content`、`tool_calls`、`finish_reason`；另收 `reasoning_chars`（只累计字数，不存文本）。

### 8.1 五态

| `classify(reason, reply, has_calls)` | 处理 |
|---|---|
| `has_calls` 为真（无论正文） | `Continue`，执行工具后继续 |
| `length` / `max_tokens` | `Truncated` → 追加「继续」再请求**一次** |
| `content_filter` | `Filtered` → **不重试**，如实告知（再问还是被掐） |
| 正文非空 | `Done` |
| 正文为空 | `EmptyReply` → 追加收尾提示**一次** |

收尾提示占一个轮次名额（总请求数上限必须硬，不许偷偷 +1）。

### 8.2 未完成必须可诊断

`TurnResult.unfinished` 四态，缺一不可：

- `EmptyAfterNudge` —— **真的发出过**收尾提示且仍空。没发过就不许叫这个名字。
- `Truncated` —— 输出被长度截断。
- `Filtered` —— 被安全过滤拦截。
- `RoundLimit { repeat }` —— `repeat=false`：用满轮次仍无正文（含"最后一轮白卷"）；`repeat=true`：同一工具同参数连调 3 次。
  文案必须对两种现场都成立：`模型用满了 {MAX_ROUNDS} 轮仍然没有收尾。已执行 {n} 次工具操作，改动已生效。`

### 8.3 两条护栏

同一工具同参数**连续 3 次** → 停；连续 **2 轮**空正文 → 停。
工具已经有了副作用（`add_field` / `convert_image`），所以「没写完」**不许当 Err**：
本轮返回 Ok，但带 `unfinished`，前端在消息下方显示一行灰色说明。

### 8.4 请求日志

`chat_send` 组装完 system（含 digest）后写 `<数据目录>/logs/context/{unix秒}-{personId}.md`。
- 必须在请求**前**落地：它记的是「模型将看到什么」。
- 思考字数只在请求结束才有，所以是**尾部追加** `reasoning=N 字`，不写回同一行。
- `personId` 进文件名前把非 `[A-Za-z0-9_-]` 换成 `_`（防路径穿越）。
- 任何一步失败都 `let _ =`，不影响这一轮。

## 9. 纸面尺寸与分页

**屏幕上那张纸必须是 1:1 的 A4**：`210mm × 297mm`（96dpi 下 793.7 × 1122.5px）。
曾经的 374px「缩略纸 + 绝对 px 字号」让测量出的页数系统性偏大 **2.5 倍**，排版模型被一个假高度逼着压字号、改栏数、改写文字。

- 预览缩放只走 CSS zoom，默认 47%（视觉宽 ≈373px，与旧版观感一致）；菜单里的 **100% = 真实 A4**。
- 离屏测量宿主与纸同尺寸、同口径（`.paper` 给 16mm 页边距、`.doc` padding 0）。
- **一页可用高度**：`onePage = paper.offsetWidth × (297/210) − padY`，`contentPx = paper.offsetHeight − padY`，`padY` 现场读 `getComputedStyle` 的上下内边距。写死 265mm 就是第二个漂移源。
- `.paper` 用 `min-height` 而非 `height`：写死会让 `offsetHeight` 恒等于 297mm，等于把测量废掉。

### 9.1 真分页（Paged.js）

只走 ESM `Previewer`，不进主 chunk（动态 import）。三条硬约束：

1. **必须外面包一层 shell 再传**：Paged.js 会丢掉传入的根元素、只从 `firstChild` 开始搬；直接传 `.doc` 会让纸上的内容失去 `.doc` 作用域，按 16px 默认字号排，半页的东西被切成两页。
2. 显式传样式表数组（对象形式 `{ [href]: cssText }`，传字符串会被当 URL 去 fetch，静默 0 页），并给所有 `<style>` 打 `data-pagedjs-ignore`。
3. 离屏预排不能 `display:none`（高度为 0，一页也切不出）；用 `position:absolute; left:-20000px; visibility:hidden`。

分页失败 → 退回单张纸 + 虚线（`console.warn`，不许白屏）。head 里 Paged.js 插入的样式必须按「开跑前快照」回收，退出时 `resetPagination()` 全清。

## 10. 排版文字锁

**任何一个入口都不许让文字变。** 排版模型拥有完整版面能力（尺寸、边距、分栏、字体字号、定位、增删页），内容权限为零。

两个入口都要锁：`parts`（逐块）与 `html`（整页接管）。

- **归一化链**（两侧同一个函数、各一次）：去零宽/软连字符 → NFKC → **抹标点邻接空白** → 压空白。
  标点邻接空白一律视为排版噪声：`A B:C` / `A B : C` / `A B：C` 归一后全等；字母之间的空格不动（`Java Script` ≠ `JavaScript`）。
- **比对用序列比对**（`blocks` 按顺序拼接）。因此：重排区块顺序、把两块并成一块都会被拒——**这是有意的**，调区块顺序是内容决定，要用内容工具改，不是在 HTML 里搬。
- **报错两侧口径不同**：比对走归一文本，**报错预览走不归一的那份**。否则 NFKC 把全角「：」压成半角，模型认不出自己写的是哪句。
- `kind == "media"` 整块豁免；未知块报「块 {id} 不存在」并列出这一页的块，不许静默放行。
- 整页接管的拒绝文案不报块号：`整页接管不能改字：这一页原文共 {n} 字，你写的是 {m} 字。要改字请用内容工具（set_field / update_entry）。`

`position` / `top/left/right/bottom` / `z-index` 由 `ALLOW_ABSOLUTE` 把着（默认 false），且必须排在真分页之后才能打开——绝对定位元素跨页只能由分页引擎兜底。
`transform` 只放行 `translate*`：`scale/rotate/skew/matrix` 一律剔除（Word 表达不了缩放，放开了就是「屏幕上对、导出去错」）。

**页头 `aside` 已可用**（`HEAD_ASIDE_ABSOLUTE = true`）：`set_page_layout { head: "aside" }` 产出
`<header class="head aside" style="text-align:center">` + `<div class="txt">…</div>` +
`<img class="photo abs" style="top:0;right:0">` —— 姓名与联系方式居中，照片绝对定位到版心右上角。
三条前提都已满足：Paged.js（`Previewer`）按页切分、`paper.css` 的 `.doc{position:relative}` 让 `.abs`
以版心为原点、docx 侧 `w:framePr` 承接同一个坐标（`w:x` 已加页边距）。

**这两件事必须分开**，所以是两个常量而不是一个：
- 页头照片只是生成器写死坐标的一个 `<img>` 盒子，模型既写不到它、也改不了它的 `top/right`，跨页风险可控 —— 开放。
- 模型手写的 `position` 仍在白名单外（`style_props()` 不含 `ABSOLUTE_PROPS`）：它能把整段正文抽出文档流，
  屏幕上看还在，纸上的页数与 Word 的 `w:framePr` 同时失真，而模型看不见渲染结果、发现不了 —— 不开放。

放开「让照片落在右上角」不等于放开「让模型自己排版任意定位」。

## 11. 附件正文：Fact 层

**附件正文是事实，不是每轮的投影。** 曾经把它整段塞进 system 用完即弃，一轮中断就永久丢失（真机：2404 字正文蒸发，后续轮次只能说「系统里只有你的照片」）。

- 表 `source(stored PK, name, ext, pages, chars, text, added_at)` 建在 `ensure_additive`，不升 schema 版本；同一 `stored` 重复上传仍是 1 行。
- 投影只给**指针 + 摘要**：正文 ≤800 字照旧全文；>800 字只给前 120 字 + `（原件正文共 {chars} 字…要全文调 read_source，参数 stored = {stored}）`。
- 工具 `read_source { stored, offset?, limit? }`（缺省 4000 / 上限 20000），越界报「offset 超过正文长度」；事实层缺失但文件还在 → 现场重解析并补写。
- **投影永不反写事实**：摘要与报告的摘要用同一个 `digest_text`，不许各处各写一遍。

## 12. Word 导出

- **绝对定位 → `w:framePr`**：`w:hAnchor="page" w:vAnchor="page"`，坐标口径是**版心左上角 + 页边距**（与屏幕 `.abs` 一致：`.doc` 带 `position:relative`，绝对定位的包含块就是版心）。px→twips = `px / 96 × 1440`。
- `framePr` 是**段落级**：一个 `.abs` 容器的多个子元素会被拼成一段纯文本，`z-index` 不存在（重叠变成文字让位），`transform`/圆角/阴影/背景色全部丢失。这些是 Word 的表达边界，不是 bug。
- **照片必须真内嵌**：`word/media/photo{N}.jpg` + `rId` 关系 + `<wp:extent>`。显示 62×82px（EMU 9525/px），**源图压到 160px 宽 / JPEG 0.8**（62÷96×300≈194 设备像素，取 160 在 A4 上约 245dpi）。取不到图 → 退回「［照片］」占位框，版面不重排，导出里不写报错文字。
- `pgMar` 从 `paper.margin`（mm→twips）派生、`pgSz` 从 `paper.size_mm()` 派生，字号/行距按预设派生（classic 10 / compact 9.4 / roomy 10.6 / dense 9.6）。
- **已知缺口**：HEIC/AVIF 等没有解码器的格式进图片字段时，Word 里会静默变成占位框。正确解法是资产入池时用 `convert_image` 转码，不在导出层加解码器。

### 12.1 网格：role → `w:tbl`

屏幕上 `grid-2 / grid-3 / table / cols-2 / cols-3` 是容器差异，Word 里统一落成**表格**（无边框均分列，`tblLayout=fixed`）：

| role | Word 里长什么样 |
|---|---|
| `grid-2` / `grid-3` | 无边框表格，一格一条 entry |
| `table` | 表格，`insideH` 带线（颜色取 `--line`），一格一条 entry |
| `cols-2` / `cols-3` | 无边框表格，**按列优先切分**：一条 entry 时切要点（`per = ceil(n/cols)`），多条 entry 时切条目 |

- 列宽 = 版心宽 / 列数，除不尽的零头归最后一列。
- 分区标题永远在表格**外面**：合并进表格会让标题跟着整表被推到下一页。
- `cols-N` 的切分必须与屏幕的 `columns` 顺序一致（列优先），否则「左栏第一条」在 Word 里会跑到右边。
- **仍不同构的一处**，写在这是为了别再被当成 bug：栏高（屏幕按内容高度自动平衡，表格按条数均分）。栏间距已对齐 —— 表格 `cellMar` 左右各 `COLUMN_GAP_PX / 2`，与屏幕的 18px `column-gap` 同值。

### 12.2 块级样式：对齐 / 字号 / 加粗

`Block.style { align, size, bold }`（`set_block_style` 写，`blockStyle` op 落地），屏幕与 Word **必须同时生效**：

- 屏幕：块根元素的内联 `text-align` / `font-size: {n}pt` / `font-weight: 700`；
- Word：同块的段落带 `w:jc` / `w:sz`（half-points，即 pt×2）/ `w:b`；
- 值域：align ∈ left/center/right，size 8–24 的整数，bold 布尔。非法值**报错**，不静默丢弃 —— 模型据此判断「设上了没有」。
- `bold: false` 的语义是「不再额外加粗」，撤不掉条目名/分区标题**结构性**的粗体。文案必须这么写，否则模型会以为能反向操作。
- 这是纯排版：**一个字都不改**。`parts` / `html` 路径不受影响，也不该用它们绕开 —— 写在 HTML 里的内联样式在 Word 里会丢。

## 13. PDF 导出

**走 WebView2 的 `PrintToPdf`**（`src-tauri/src/topdf.rs::export_pdf`），不走 `window.print()`。

- 「点一下 → 得到一个文件 → 告诉用户它在哪」。`print()` 打开系统对话框，用户在里面选打印机还是选存 PDF、最后有没有真存下来，应用一概不知道 —— 而 PDF 是成品交付物，不能是「用户自己想办法」。
- **保真的唯一解**：屏幕上的纸面就是 Chromium 排的。换 pdfium / mupdf / 自己写排版器等于把同一份简历实现两遍，两遍必然漂移（Word 那条路就是这么产生的）。打印管线与屏幕同一个排版引擎。
- 三条不可省的参数：A4（210×297mm → 英寸）、零/16mm 边距按来源分档、`ShouldPrintBackgrounds = true`（分区线与表头底色都是「背景」，不打就是一份缺线的简历）。
- **边距由前端给**（`marginMm`），因为纸有两种口径：Paged.js 分出来的真 A4 已经把页边距画进纸里（传 0，再给一次就是双重边距）；单张预览靠 `@page { margin: 16mm }`（传 `PAGE_MARGIN_MM`）。`PrintSettings` 的边距是「自定义边距」，一旦写死就会覆盖 CSS 的 `@page`。
- **命令必须 async**：同步命令跑在主线程，而 `with_webview` 的闭包要回主线程才能跑 —— 主线程等自己，窗口白屏死锁（设置窗踩过一次）。
- 失败必须说话：运行时不支持（cast 不到 `ICoreWebView2_7`）、写不出来、`completed` 报成功但文件不存在，三种都返回具体原因，不返回 `Ok(())`。
- 菜单里「打印…」仍然保留（真的去打纸时用它），与「PDF · 当前页」是两个入口，不是一件事的两种叫法。
- **整份 PDF = 每页一个文件，落在 `<数据目录>/导出 PDF/<人 - 简历>/`**，文件名带 `01-` 前缀（资源管理器按名字排就是页顺序）。不拼成一份：整份的版式来自 Paged.js，而它只排「当前这一页」，合并要么重排一遍、要么在 PDF 层合并资源，两条都会和屏幕漂移。也不和 Word 挤同一个目录 —— 那个目录归 `export_resume` 的 manifest 管，多出几个不是它写的文件，下次整份导出会判「这里有别的东西」而拒绝执行。

### 13.1 逐页导出的两个必答问题

「等纸面换到这一页」和「`data-mode` 是不是这一页的」必须分开答，否则导出会把上一页的内容写进这一页的文件（实测两页 PDF 内容一模一样）：

1. **纸面归属**：`.wrap` 挂 `data-page={page.id}`。判据必须先看它，不能只看「有纸、有 `data-mode`」—— 切页的第一帧这些都还是上一页的。
2. **编译完成**：`Paper` 在 `page.id` 变化时**清空 `doc`**。不清的话旧 `doc` 会让 `data-mode` 一直为真，等的人永远立刻通过；顺带还修掉「切页后一百多毫秒仍显示上一页纸面」这个观感 bug。只认 `page.id`，改内容不清（否则每敲一个字纸面闪一次白）。
3. 空白页分页得到 0 张纸，所以「已换页 + 已编译 + 安静 20 帧」也算完成，否则最后一页会一直等到超时。

## 14. 提示词 ↔ 工具表一致性（护栏）

排版/会话的提示词里只要写了「调 `X`」，就必须有一条测试去**工具注册表**里核对 `X` 真的存在、参数名真的在该工具的 `required` 里。

为什么单独立一节：这个错我们犯过三次——早期的 `set_field`、样张段的 `view`（真实工具是 `view_asset`）、以及把池内字段名 `stored` 当成对外参数名（真实参数是 `assetId`）。三次都是**单测全绿、真机第一步就断**，因为断言写成了 `text.contains("view")` —— 它只能证明"文案里有这句话"，不能证明"这句话说的是真的"。

三条硬要求：

1. **存在性**：`specs()` 的全量清单里必须含提示词点名的每个工具；`ask_session`、`request_layout` 必须**根本不存在**（不是「某角色不能用」）。
2. **参数名**：文案里凡出现「参数 `Y`」，断言 `Y` 在该工具 `params()` 的 `required` 中；且那句提示里不得出现 `stored =`（池内寻址名不是对外参数名）。
3. **反空转**：断言本轮**实际核对过的次数**（工具名条数、参数核对次数）不为零。一条从没红过的护栏，和没有护栏是一样的；一个"抽取器坏了所以一条都没抽到"的护栏，会静默变绿。

已知限制（写进测试注释，不是免责）：抽取器只认「调 `X`」句式，散落在普通句子里的工具名抓不到。因此提示词统一用「调 `X`」「参数 `Y`」是**可执行约定**，不是风格偏好。

## 15. 一轮之内的可见性（写入、实测、进度）

工具层每轮开始时只拿到一份**静态快照** `ToolCtx.workspace`。凡是「模型看不见自己刚做的事」的 bug，根都在这里。三条已经落地的不变量：

**14.1 本轮写入必须能被本轮读到。** `EditState.live` 是「快照 + 本轮已暂存写入按序应用后」的工作副本；`tools::call` 在分发**之前**把 ctx 换成它（`ToolCtx::view()`），所以工具内部照常读 `ctx.workspace`，拿到的就是副本。镜像只做在两个入队口（`mark` / `mark_op`），不靠调用点自觉 —— 漏一个出口，就又会出现「读回页面看不到刚加的字段，于是以为没写进去、再写一遍」。

线上代价：还原一份 PDF 简历时，模型把页头六项和证件照写了两遍还在问「为什么页头是空的」；同一轮里 `add_field(A)` 之后 `set_field(B)` 会把 A 冲掉（两次都基于不含对方的旧数组算整份 fields）。

**14.2 写入要边写边落，不要攒到轮末。** 每批工具执行完立刻 `flush_all` 并推一条 `mutate` 信封（`StreamChunk::envelope`）；轮末 `drain_actions` 只收尾剩余部分，不会重复。会话侧自己就把它落进 store，纸面边改边变。

**14.3 页头块的 id 由 Rust 定。** 页头 = 这一页第一个 `kind=="fields"` 的块。块不存在时前端原本会自己 `uid("b")` 现造一个，Rust 猜不到 ⇒ 同轮后续写入没有 id 可打。现在 `pageHeader` 信封带 `blockId`（同一页同一轮只会发一个 id），前端 `withHeader(page, fields, blockId)` 照它建块。

**14.4 每一页都有自己的离屏实测。** 离屏宿主是最大 8 份 `.wrap[data-page]`（宽度 793.7px 的真 A4，`visibility:hidden`，不能用 `display:none`，否则量不出高度）。曾经只有「当前选中页」这一份，于是非当前页永远「量不到」；更要命的是**内容变更触发的那条 `submitLayout` 根本不带 `measure`**，排版 agent 只能在看不见页数的情况下凭字数猜。

**14.5 实测永远是上一轮的数，同一轮里不会变。** 这句话必须出现在提示词里：模型连改两次、看到页数没动，会推出「版面无效、瓶颈在内容」，然后转头劝用户删内容——而我们付的代价是「忠实还原」这个目标本身。`set_page_layout` 在量不到时主动把这句话写进返回值。

**14.6 长时间无输出要有心跳。** 一轮可能跑 30–90 秒。工具事件（`StreamChunk::tool_event`）会一路推到会话气泡的 tool 行，那是用户判断「它还活着」的唯一依据 —— 所以 `turn::describe()` 必须给每个工具一句人话，退化成「调用 set_page_layout」就等于没有信号。
