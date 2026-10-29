# 技术方案 · 照片入简历 + 单页约束 + 检验机制

> 输入：《需求说明书》F-001…F-014、《产品设计说明》定稿文案、调研结论（带 file:line）。
> 本文件是**实施依据**。契约部分（第 4 章）冻结后，任何变更必须先改 `docs/CONTRACT.md` 再改代码。
> 作者：高见远（技术方案架构师）· 状态：**已定稿（v2，主理人 7 条口径已拍板并入，见 §13）**

> **v2 变更摘要**（相对初稿，只有这几处，架构未动；全部已定稿，见 §13）：
> ① `{口径}` 改为 `A4 210×297mm、正文宽 306px、页边距 34px、预览容器宽 374px`（文案里删掉 860px 与 16mm）；
> ② `PAGE_PAD_MM = 16.0` **保留**，只服务「接近整页」的面积换算（打印口径），并在注释里写清它与预览实测的 34px 不是一回事；
> ③ 抽图 >8 张的规则定死（`pct` 升序保留小的 8 张、已登记按 id 跳过、重跑拿下一批、**幂等靠内容寻址，不靠分页参数**）；
> ④ 前端测试定为 vitest（只测两个纯函数，不引 jsdom）；
> ⑤ `{target}` n>1 / `{verdict}` 未设目标+fit / file 页三条分支已定稿；
> ⑥ Batch 0 增两条硬约束（CONTRACT.md diff 先合并、`TOOLS` 数组一人一条）。

---

## 0. 一页结论

三件事，各有落点：

| 问题 | 落点 | 一句话 |
|---|---|---|
| 照片进不来 | 新模块 `pdfembed.rs` + 新工具 `extract_images` | 自己遍历 XObject，抽出位图落池成 image 资产；**不依赖视觉模型、不渲染整页** |
| 单页保不住 | `agent/facts.rs` 一处产出事实块 + 前端离屏测量宿主 | 页数/实测/达标**只有一个来源**，会话与排版两侧都照抄它 |
| 没有检验 | `PageMeasure` 随请求上送（不落库）+ 轮末自动自查 | 工具返回值带实测结论；overflow 自动投 brief，同页 ≤3 轮 |

三条硬边界：
- **不新造 Rust↔前端往返通道**。所有实测值随已有的 `chat_send` / `layout_submit` payload 上送。
- **不给 workspace 加持久字段**（除 `resume.targetPages`，那是用户说过的事实，必须跨会话有效）。
- **判定阈值只有一处**：`facts.rs` 的 `FIT_MAX` / `TOLERANCE_MAX`。前端不判。

---

## 1. 改动地图

### 1.1 后端 · 抽图

| 文件 | 改什么 | 性质 | 为什么 |
|---|---|---|---|
| `src-tauri/Cargo.toml` | `[dependencies]` 加 `lopdf = "0.34"` | 改 | `lopdf` 已是传递依赖（`Cargo.lock:1946-1948`，0.34.0），显式声明**不增编译量**；`use lopdf::` 必须显式依赖 |
| `src-tauri/src/pdfembed.rs` | 新建：XObject 递归遍历 + 图像解码 + 显示尺寸 | 新增 | 抽图与渲染页解耦；纯 Rust，跨平台一致（不像 `pdfimage.rs` 要分 Windows/CLI 两条路） |
| `src-tauri/src/lib.rs` | 注册 `mod pdfembed;` | 改 | 模块入口 |
| `src-tauri/src/attach.rs` | `fn read` → `pub(crate) fn read`（:160）；`materials()` 返回结构化 `Materials`（含 `origin`）；`Parsed` 加 `pages: Option<u32>`；PDF 附件段补一行「附件标识」 | 改 | ① 工具拿不到 `AppHandle`，只能按数据目录读；② 原件页数（F-005）只有这里知道；③ 模型要能拿到 `stored` 才能调 `extract_images` |
| `src-tauri/src/tools/extract.rs` | 新建：`extract_images` 工具 + `ensure_source_asset()`（幂等材料化） | 新增 | F-001 + F-002 + F-003 + F-013 |
| `src-tauri/src/tools/mod.rs` | `pub mod extract;`；`TOOLS` 注册 2 个工具（:167+）；`ToolCtx` 加 `facts` 字段 | 改 | 注册点唯一 |
| `src-tauri/src/model.rs` | 不改 | — | `Asset.extra` / `Resume.extra` 已有 `#[serde(flatten)]`（:86-87、:295-296），新键天然不丢 |

### 1.2 后端 · 事实与工具

| 文件 | 改什么 | 性质 | 为什么 |
|---|---|---|---|
| `src-tauri/src/agent/facts.rs` | 新建：`Origin` / `Target` / `PageFacts` / `PageMeasure` / 全部文案槽位 / 阈值常量 | 新增 | F-012「页数类陈述唯一来源」，两侧注入同一函数 |
| `src-tauri/src/agent/context.rs` | `locate()` 加 `facts: &PageFacts` 参数，在 :84（视图行）与 :85（收尾句）之间插入事实块 | 改 | 这是全仓最自然的插入点：定位头末尾、收尾叮嘱之前 |
| `src-tauri/src/agent/layout.rs` | `LayoutArgs` 加 `measure` / `origin`；`describe_page()` 末尾（:408 之后）追加实测段；`ToolCtx` 构造（:520）带 facts | 改 | F-008 |
| `src-tauri/src/session.rs` | `SendArgs` 加 `measure`；`SendResult` 加 `origin`；`chat_send` 组 facts 并传给 `locate()` / `ToolCtx` | 改 | 实测上送通道 + F-005 |
| `src-tauri/src/tools/layout.rs` | `set_page_layout` 返回值（:285）追加 fit 结论行 | 改 | F-009 |
| `src-tauri/src/agent/turn.rs` | `describe()`（:58-82）加两条人话 | 改 | 界面上「调用 extract_images」比「调用 调用」有用 |
| `src-tauri/src/render/mod.rs` | `photo_of()`（:817-823）跳过 `extra.role == "candidate"` | 改 | ⚠️ 不跳的话，PDF 里第一张 logo 会立刻变成页头证件照 |

### 1.3 前端 · 测量

| 文件 | 改什么 | 性质 | 为什么 |
|---|---|---|---|
| `src/lib/measure.ts` | 新建：`PageMeasure` 类型、`contentVersion()`、`usePageMeasure()` | 新增 | 测量只有一套公式 |
| `src/store/workspace/provider.tsx` | 挂**离屏测量宿主**（常驻，两态都在）；`measure` 进 context；轮末自动自查 effect | 改 | 编辑态没有 `.paper`（`panel.tsx:700-723`），必须自己造一个；测量要跨组件共享（排版自动提交在 provider、徽标在 status-bar） |
| `src/lib/layout.ts` | `submitLayout` 带 `measure` + `origin` | 改 | 排版侧要实测（F-008/009） |
| `src/lib/measure.ts` | 另建 `origin` 内存暂存（`putOrigin` / `takeOrigin`） | 新增 | 原件页数不落库，随 payload 走 |

### 1.4 前端 · UI / 类型

| 文件 | 改什么 | 性质 | 为什么 |
|---|---|---|---|
| `src/types/index.ts` | `Resume.targetPages?` / `targetPagesTurn?`；`Asset.role?` / `source?` / `pageIndex?`；`PageMeasure` 类型 | 改 | TS 的 `Resume`（:78-84）没有索引签名，新字段必须显式声明 |
| `src/store/workspace/types.ts` | `Action` 加 `resumeTargetPages`（:18 联合里） | 改 | 契约三处同改之一 |
| `src/store/workspace/reducer.ts` | 实现 `resumeTargetPages`，用 `mapResume` + `{...resume}` | 改 | ⚠️ 手写重建对象会丢未知字段，必须展开 |
| `src/lib/agent-actions.ts` | `OP_SPEC` 加 `resumeTargetPages`（:184） | 改 | 契约三处同改之二 |
| `src/components/resume/panel.tsx` | 预览虚线改用 `measure.onePage`（:325-339、:693）；三项导出加前置门 | 改 | 一套公式；F-011 |
| `src/components/status-bar.tsx` | 页码后加实测徽标 | 改 | 状态可见性 |
| `src/components/status-bar.module.css` | 加 `.dotWarn` | 改 | 溢出时点是琥珀色 |
| `src/components/chat/panel.tsx` | `chat_send` 带 `measure`；收到 `origin` 存起来；本轮结束后触发自查 | 改 | 上送通道唯一出口 |

### 1.5 提示词

| 文件 | 改什么 | 性质 | 为什么 |
|---|---|---|---|
| `prompts/system.md` | 加「页数只有一处来源」「照片」两节（第 9 章给原文） | 改 | 编译期 `include_str!`（`session.rs:28`），重打包生效 |
| `prompts/layout.md` | 容量表（:87-95）降级为量级参考，判据改为实测段 | 改 | 现在只有一个测点 |
| `src-tauri/src/render/mod.rs` | `GUIDE`（:193 的字数容量表）同步降级 | 改 | `read_layout_guide` 只喂排版 agent（`tools/layout.rs:116-118`），两处口径不能打架 |

### 1.6 契约文档

| 文件 | 改什么 | 性质 |
|---|---|---|
| `docs/CONTRACT.md` | §1.2 白名单 + 载荷；新增 §1.5 实测上送；§2 两个 Resume 键；§3 两个新工具；§4 返回文案；§5 photo_of 豁免；§6 文件所有权 | 改（第 4 章给逐节 diff） |

### 1.7 测试

| 文件 | 改什么 | 性质 |
|---|---|---|
| `src-tauri/src/tools/mod.rs`（:718、:719 等） | 工具数 38→40；补 `extract_images` / `set_target_pages` 断言 | 改（**现在必然红**） |
| `src-tauri/src/agent/facts.rs` | 文案逐字断言 | 新增 |
| `src-tauri/src/pdfembed.rs` | 用 lopdf 现场造 PDF 的三类编码用例 | 新增 |
| `src-tauri/src/render/mod.rs` | `photo_of` 豁免用例 | 改 |
| `src/lib/measure.test.ts` | `contentVersion` 稳定性 | 新增（见 §10.2 决策点） |

**无表结构变更**：`Resume` 落库是 `db::save` 把整份 resume JSON 化（`db.rs:399-413`），新键走 `extra` 的 `flatten`，读写都不需要迁移。故本方案**不产出建表 SQL**（本仓无 per-field 表）。

---

## 2. 关键技术决定（含理由）

### 2.1 抽图不用 `lopdf::Document::get_page_images`

调研已确认它五处不能用（`document.rs:514-587`）：页无自身 `/Resources` 时整页 Err、无 XObject 时 Err、XObject 内联字典也 Err、不递归 Form、不处理内联图、`content` 未解码、无 SMask。

自己写遍历，**用 `get_page_resources`（`document.rs:422`，会沿 `/Parent` 上溯）**。这是唯一能拿到"继承后的 Resources"的现成 API。

**结论：内联图（`BI … ID … EI`）本期不支持**，计入 `unsupported`（原因「内联图」）。理由：真实简历 PDF 的证件照几乎都是 XObject（内联图只出现在极简生成器里），而解析 `ID` 后的二进制要按 `EI` 扫描，容易把数据里的 `EI` 当结束符 —— 收益与风险不成比例。

### 2.2 Flate 解码路径：**clone 流、删掉 `Subtype`、调 `decompressed_content()`**

理由（对比自己写 flate2 + predictor）：

1. `decompressed_content()`（`object.rs:642`）已经覆盖 `FlateDecode / LZWDecode / ASCII85Decode`（:653-660），并且 `decompress_zlib`（:758-771）**已经处理 PNG predictor**。predictor 有 5 种类型（None/Sub/Up/Average/Paeth），自己写一遍是最容易出静默错的地方（不报错、像素错位、图能开但是花的）。
2. 唯一的阻碍是 `Subtype == "Image"` 时它直接 Err（:646-648），而 `decompress()` 对图像流是 no-op（:771）。绕过方式是一行：
   ```rust
   let mut probe = stream.clone();          // Stream { dict, content }
   probe.dict.remove(b"Subtype");           // 只影响这次解码，不改文档
   let raw = probe.decompressed_content()?; // 走 lopdf 自己的 zlib + predictor
   ```
3. 不引 `flate2` 直接依赖。虽然它在树里（`Cargo.lock:877-879`，1.1.10），但引了就要自己写 predictor 或拒绝带 predictor 的流 —— 那会把最常见的一类扫描件挡在门外。

**代价**：`decompressed_content` 不认 `DCTDecode`（JPEG 我们直取 `content`，反正不需要解）、不认 `JPX/CCITT/JBIG2`（本来就没有解码器）。这两类都明确计入 `unsupported`，不 panic。

（主理人已确认此路径，不再摇摆：不引 `flate2` 直接依赖、不自己写 predictor。）

> **将来必须注释的第 1 处**：`pdfembed.rs` 里 `probe.dict.remove(b"Subtype")` 这一段。必须写清「这是绕过 lopdf 对图像流的拒解，不是数据修正；`probe` 是局部克隆，文档对象没被改」。

### 2.3 「接近整页」的版心口径 —— 必须解析 CTM

产品给的示例是 `413×531 px｜宽高比 0.78｜不接近整页（占版心 8%）`。若按「1px = 1pt」估算，413×531 相对 A4 版心是 **58%**，不是 8%。8% 只能来自**图片在页面上的实际显示尺寸**（≈155×196pt ≈ 5.5×6.9cm，正是一张证件照的尺寸）。

所以必须拿到 CTM。做最小的图形状态机，用 `lopdf::content::Content::decode()`（现成 tokenizer）遍历操作符：

```
q            → stack.push(ctm)
Q            → ctm = stack.pop() (空了就不动)
cm a b c d e f → ctm = ctm * Matrix{a,b,c,d,e,f}
Do /Name     → 记下 (Name, ctm)          // 只记，不在这里解码
BI … EI      → 跳过整段，计入 unsupported["内联图"]
```

显示尺寸（单位 pt，忽略斜切）：

```
disp_w = hypot(ctm.a, ctm.b)
disp_h = hypot(ctm.c, ctm.d)
```

**版心面积口径（已定稿，写进常量）**：用**打印 `@page` 的 16mm**，只服务「接近整页」这一个换算。

```
PAGE_PAD_MM = 16.0                        // 打印 @page 的 16mm，只用于「接近整页」的面积换算，与预览实测的 34px 不是一回事
MM_TO_PT    = 72.0 / 25.4                 // = 2.834646
inner_w_pt  = media_w_pt - 2*PAGE_PAD_MM*MM_TO_PT   // A4 → 504.57pt
inner_h_pt  = media_h_pt - 2*PAGE_PAD_MM*MM_TO_PT   // A4 → 751.18pt
pct         = round(100 * disp_w * disp_h / (inner_w_pt * inner_h_pt))
near_page   = pct >= 80                   // F-003：NEAR_PAGE = 面积比 ≥ 0.8
```

**为什么这里可以跟实测用两把尺子（别"顺手统一"）**：两者量的不是同一块地方 ——
`pct` 问的是「这张图在**原件 PDF 的页面**上占了多少可打印区」，那是 PDF 侧的概念，用打印边距（16mm）才对应"原件印出来什么样"；
实测问的是「这一页在**应用预览**里有多高」，那是 WebView 里的 DOM，只有 34px 这一个真值。
一个是原件的，一个是预览的，各自用自己那把尺子才对。**唯一不许的是把打印口径写进事实块文案**（那会让人以为我们量的是打印结果）。

验算：413×531 的证件照（`cm` 缩放 0.3749 / 0.3695）→ 154.83×196.20pt → 30,378 / 379,055 = **8.0% → 占版心 8%**，与产品示例一致。

`MediaBox` 从页 dict 取（沿 Parent 上溯）；读不出退 A4（595.28×841.89pt）。**不用渲染页宽 1240px**：1240 是给视觉模型的位图宽度（`pdfimage.rs:35`），与"图片在 PDF 里占多大"没有关系，用它会让比例随渲染参数漂移。（主理人已确认此口径，不再摇摆。）

退化：`Do` 找不到对应的 XObject（被 Form 间接引用、或 content 解不开）→ 该图记 `pct: None`，文案写「占版心 不可测」，`near_page = false`（不拦模型选它）。**这是罕见路径**，因为 `Content::decode` 覆盖绝大多数线性 content stream。

### 2.4 测量宿主：常驻离屏，唯一真值源

现状：纸面只在预览分支挂载（`panel.tsx:689-699`），编辑分支（:700-723）没有 `.paper`；而默认视图是编辑，排版自动提交不看视图（`provider.tsx:443-470`）→ **排版 agent 拿到的几乎必然是空值**。

决定（照主理人定的做）：`provider.tsx` 里挂一个**常驻离屏宿主**，编辑/预览都挂：

```tsx
{person && page && (
  <div
    aria-hidden
    data-measure="host"
    style={{
      position: "absolute", left: -9999, top: 0, width: 374,
      visibility: "hidden", pointerEvents: "none", zIndex: -1,
    }}
  >
    <div className={paperStyles.wrap} ref={hostRef}>
      <Paper person={person} page={page} />
    </div>
  </div>
)}
```

- 宽度 374 与 `.wrap`（`paper.module.css`）一致 —— 纸是 A4 等比缩略，宽度一变字号与分页估算就失真，所以宽度必须是固定值，不能跟随面板。
- 预览那份（`panel.tsx:691-698`）**保留**（用户要看），但它不再负责测量：虚线位置改由同一份 `measure` 换算（`measure.onePage * zoom/100`）。这样**只有一套公式**，顺带修掉「zoom 后 offsetHeight 被放大」的老隐患。
- 成本：编辑态多一次 `compilePage` IPC + 一次 HTML 解析/布局。可接受（写进风险表）。

> **将来必须注释的第 2 处**：`provider.tsx` 离屏宿主。必须写清「编辑态没有纸面 DOM，实测只能从离屏实例取；预览那份只管显示，不许另起一套公式」。

### 2.5 实测值传输：随 payload 走，不落库

```
chat_send  ←  args.measure : Option<PageMeasure>
layout_submit ← args.measure : Option<PageMeasure>   +   args.origin : Option<Origin>
```

Rust 侧**不做全局缓存，也不落库**：`chat_send` / `layout_run` 各自把 `args.measure` 组装成 `PageFacts` 塞进 `ToolCtx`。工具（事实块 / describe_page / set_page_layout）从这一次请求的上下文读。
理由：全局 `OnceLock<Mutex<HashMap>>` 会带来"跨请求残留"（上一次测量的数被当成本次），而这正是 F-014 要防的错。请求上下文天然无残留。

### 2.6 内容版本（F-014）由**前端**算并判定过期

- `contentVersion(person, page)`：对「参与排版的内容」做稳定序列化 + FNV-1a 32 → 8 位 hex。输入 = `page.{id,kind,assetId,layout}` + 递归规范化 `blocks`（key 排序）+ `person.fields` + `person.assets` 的 `id/stored`。`notes` 不计（批注绝对定位，不影响高度）。
- **为什么前端算**：测量对象是前端渲染的那份 DOM，只有前端知道"我量的是哪一版内容"。Rust 用 serde_json 复算同一份哈希需要两侧 key 顺序一致 —— 那是个必然会腐烂的约定。
- **过期判定也在前端**：`usePageMeasure` 保留上一次成功测量；`contentVersion` 变了就把它降级成 `status: "stale"` 并带 `previous`。F-014 的「内容变则旧结果作废」在这里落地，且文案里 `{c0}` / `{c}` 两个版本号都是真值。
- Rust 只做两件事：把 `status` 翻译成定稿文案；判 fit/overflow（阈值唯一）。

### 2.7 目标页数三态：落库两个键 + 一个 turn 号

```
resume.extra.targetPages      : u32   // 用户明确说过的目标页数
resume.extra.targetPagesTurn  : u32   // 写入时对应「第几条用户消息」
```

推导（`facts::target_of`，Rust 一处）：

```
if targetPages 存在:
    if targetPagesTurn + 1 >= current_turn  → UserThisTurn   「这一轮明确要求」
    else                                    → UserBefore     「之前要求过，仍然有效」
elif origin.pages == Some(n) && n <= 3:      → FromOrigin(n)  「沿用原件页数」
else:                                        → Unset
```

`current_turn` = 该会话里 user 消息条数（`chat_send` 里 `state.list(key)` 数一次即可）。`targetPagesTurn` 由工具写入时取 `Outbox.turn`（每轮新建的 `Outbox`，字段默认 0，`chat_send` 写一次）。差 1 判"这一轮"：用户这一轮说"压到一页"→ 模型调 `set_target_pages(1)` → 下一轮（也是用户看到效果的那一轮）事实块显示"这一轮明确要求"。差 2 起降为"之前要求过"。

排版侧不传 `current_turn`（`None`）→ 一律归 `UserBefore`。这是准确的：对排版 agent 来说，那个页数确实是"用户之前要求过"。

### 2.8 原件页数（F-005）不落库，随 payload 传给排版侧

`origin = { name, pages: Option<u32> }`（`None` = 读不出，**绝不用 0**）。

链路：
```
chat_send → attach::materials() 顺带算 PDF 页数 → facts + SendResult.origin
         → 前端 putOrigin(resumeId, origin)（模块级内存）
         → submitLayout 带上 origin → LayoutArgs.origin → 排版侧事实块
```
刷新/重启后丢失 → "这一轮没有上传原件"（诚实的退化，不假装知道）。

`pages` 算法：`lopdf::Document::load_mem(bytes).and_then(|d| d.get_pages().len())`。失败 → `None`。结果进 `attach` 的 `Parsed` 缓存（键是内容寻址的 `stored`），同一份文件只解析一次。

### 2.9 抽图资产与照片位

- 抽出的图 **进 `person.assets`**（模型要能用 assetId），`extra` 打标：`{"role":"candidate","source":"<源 PDF assetId>","pageIndex":2}`。
- `render::photo_of()` 跳过 `role == "candidate"`。老数据没这个键 → 仍然取第一张 `image`（**向后兼容**，升级后用户原有证件照不会消失）。
- **assetId = stored 名**（`<sha 前16 位>.<ext>`，如 `a3f19c04b2e6.png`）。理由三条：① 产品文案里行首反引号就是这一串，模型直接照抄给 `set_field` 不会错位；② 同内容同 id，天然幂等（同一张图在 PDF 里出现两次只登记一次）；③ 省掉一次 id 生成与一次去重表。

---

## 3. 数据结构与契约

### 3.1 Rust

```rust
// src-tauri/src/agent/facts.rs

/// 本轮原件。`pages: None` = 读不出（文件损坏或不是 PDF）—— 绝不写成 0。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    pub name: String,
    pub pages: Option<u32>,
}

/// 目标页数的三态 + 来源。`pages: None` 就是 Unset。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetSource { UserThisTurn, UserBefore, FromOrigin, Unset }

#[derive(Debug, Clone, Copy)]
pub struct Target { pub pages: Option<u32>, pub source: TargetSource }

/// 前端量的数。只有它知道 DOM，所以判定过期也在前端。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageMeasure {
    pub page_id: String,
    /// ok | stale | unknown
    pub status: String,
    /// 测量版本号，每次成功测量 +1（前端模块级计数器）
    #[serde(default)]
    pub version: u32,
    /// 内容版本（8 位 hex，前端算）
    #[serde(default)]
    pub content: String,
    /// A4 一页在这个容器里的高度（px）= 纸宽 * 297/210
    #[serde(default)]
    pub one_page: Option<f64>,
    /// 纸面实际高度（px）
    #[serde(default)]
    pub content_px: Option<f64>,
    /// 正文行高（px），用来把溢出量折算成「多少行」
    #[serde(default)]
    pub line_height: Option<f64>,
    /// stale 时带上一次的数
    #[serde(default)]
    pub previous: Option<Previous>,
    /// unknown 的原因：no-preview | fonts | timeout | empty
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Previous {
    pub content: String,
    pub one_page: f64,
    pub content_px: f64,
}

/// 一次请求里工具能看到的全部页面事实。
#[derive(Debug, Clone, Default)]
pub struct PageFacts {
    pub origin: Option<Origin>,
    pub target: Target,
    pub measure: Option<PageMeasure>,
    /// 当前页名与 kind（file 页不做页数判定）
    pub page_label: String,
    pub page_kind: String,
    /// 第几条用户消息（排版侧为 None）
    pub turn: Option<u32>,
}

// 阈值只有这一处。前端不许复刻。
pub const FIT_MAX: f64 = 1.00;
pub const TOLERANCE_MAX: f64 = 1.02;

/// 把测量折算成判定。这是全仓唯一的 pages 计算。
pub enum Fit { Fit(f64), Tolerance(f64), Overflow { pages: f64, over: f64, lines: u32 }, Unknown(Reason), Stale { .. } }
pub fn fit_of(m: Option<&PageMeasure>) -> Fit;
```

```rust
// src-tauri/src/pdfembed.rs

pub struct Embedded {
    /// 已解码的位图（可直接写 PNG）
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// 来源页序，1 基
    pub page: u32,
    /// 页面上的显示尺寸（pt）。None = content stream 里对不上，不可测
    pub disp: Option<(f64, f64)>,
    pub alpha: bool,
    /// 原始编码，仅供文案（jpg / png / raw）
    pub codec: &'static str,
}

pub struct Report {
    /// 按来源页序、页内出现顺序
    pub images: Vec<Embedded>,
    /// 解不出的张数（编码不支持 / 内联图 / 尺寸过大）
    pub unsupported: Vec<&'static str>,   // 每张一条原因
    pub pages: usize,
    /// 版心面积（pt²），供算占比
    pub inner_area: f64,
}

/// 打不开的三种情况用 Result 的 Err 区分（文案在 tools/extract.rs 里拼）：
///   Err("open")      → 文件打不开（损坏 / 加密 / 不是 PDF）
///   Ok(report)       → report.images 空 + unsupported 空 = 没有内嵌位图
pub fn extract(bytes: &[u8]) -> Result<Report, String>;
```

### 3.2 TS

```ts
// src/types/index.ts 追加

export interface Resume {
  id: string;
  personId: string;
  title: string;
  updatedAt: number;
  pages: ResumePage[];
  /** 用户明确说过的目标页数。没有 = 未设置（不要自行假定一页） */
  targetPages?: number;
  /** 写入时对应「第几条用户消息」，用来区分「这一轮」与「之前」 */
  targetPagesTurn?: number;
}

export interface Asset {
  // …既有字段
  /** candidate = 从 PDF 里抽出来、等着被选中的图。渲染层不会拿它当页头照片 */
  role?: "candidate";
  /** candidate 的来源 PDF 资产 id */
  source?: string;
  /** candidate 在来源 PDF 里的页序（1 基） */
  pageIndex?: number;
}

export interface PageMeasure {
  pageId: string;
  status: "ok" | "stale" | "unknown";
  version: number;
  content: string;
  onePage?: number;
  contentPx?: number;
  lineHeight?: number;
  previous?: { content: string; onePage: number; contentPx: number };
  reason?: "no-preview" | "fonts" | "timeout" | "empty";
}
```

### 3.3 新 op `resumeTargetPages`

```json
{ "type": "resumeTargetPages", "personId": "p1", "resumeId": "r1", "targetPages": 1, "turn": 12 }
```
`targetPages` 省略 = 清除（回到"未设置"）。不进确认门。前端 reducer：

```ts
case "resumeTargetPages":
  return mapResume(state, action.personId, action.resumeId, (resume) => {
    const next = { ...resume };
    if (action.targetPages === undefined) {
      delete next.targetPages;
      delete next.targetPagesTurn;
    } else {
      next.targetPages = action.targetPages;
      next.targetPagesTurn = action.turn ?? 0;
    }
    return next;
  });
```
> `{...resume}` 必须展开：`createResume` 那类手写重建的 action 会丢 `extra` 里的键（调研已点名）。这条 op 不犯那个错。

### 3.4 两个新工具

| 工具 | scope | writes | 参数 | 产物 |
|---|---|---|---|---|
| `extract_images` | Session | true | `stored`(必填)、`name?` | 落池的 candidate 资产（`push_asset`）+ 候选清单文本 |
| `set_target_pages` | Session | true | `pages?`(整数或省略) | `resumeTargetPages` op |

`extract_images` 参数 schema：

```json
{
  "type": "object",
  "properties": {
    "stored": { "type": "string", "description": "附件的存储名（会话材料段里那一行「附件标识」）。不是文件名" },
    "name":   { "type": "string", "description": "用户看到的原文件名，只用于显示与新资产命名" }
  },
  "required": ["stored"],
  "additionalProperties": false
}
```

`set_target_pages`：

```json
{
  "type": "object",
  "properties": {
    "pages": { "type": "integer", "description": "用户明确要求的目标页数，1–10。用户没说过页数就不要调这个工具；要清除用户之前的要求就不传这个参数" }
  },
  "additionalProperties": false
}
```

---

## 4. 接口契约（冻结内容）

### 4.1 通用约定

- 统一响应体：本方案**不新增 HTTP 接口**。所有新数据走已有的 Tauri command（`chat_send` / `layout_submit`）与已有的 action 通道。
- 鉴权：无（本地应用）。
- 错误码：无数字码，沿用 `Result<_, String>` 的中文消息。文案要求见 §4.4。
- 幂等：`extract_images` 幂等（同内容 → 同 assetId，不重复落池）；`resumeTargetPages` 幂等（同载荷重复 dispatch 结果相同）。

### 4.2 传输结构变更

```
chat_send.args    += measure?: PageMeasure          // 不落库
chat_send.result  += origin?: { name, pages? }      // 不落库
layout_submit.args += measure?: PageMeasure         // 不落库
layout_submit.args += origin?: { name, pages? }     // 不落库
```

### 4.3 工具清单变化

| scope | 之前 | 之后 |
|---|---|---|
| Session | 38 | **40** |
| Layout | 9 | 9（不变） |

### 4.4 文案（逐字，代码里就是这些字符串）

**事实块**（`facts::block()`，一处产出，两处注入）：

```
【页面事实】下面是程序实测的当前值。页数只能照抄这几个数：不要改写、不要四舍五入、不要按比例推算。

原件：{原名}，页数 {n} 页（来源：PDF 页树）
目标页数：{target}
这一页「{页名}」：{measure}
结论：{verdict}
```

槽位（按状态取一条，不拼接不省略）：

- `{n}`：
  - 正常 `3`
  - 读不出 → 整行 `原件：{原名}，页数读不出（文件损坏或不是 PDF）。不要按 1 页算，也不要猜。`
  - 无附件 → 整行 `原件：这一轮没有上传原件。`
- `{target}`：
  - `1 页（用户这一轮明确要求：压到一页）`
  - `1 页（用户之前要求过，仍然有效）`
  - `3 页（沿用原件页数：原件只有 3 页，用户没有另说）`
  - `未设置（用户没有说过要几页，任何页数都合规；不要自行假定一页）`
  - `未设置（原件页数读不出，用户也没有说过要几页）`
  - ✅ 已定稿：n>1 时第一、二条的括注补成 `压到 {n} 页`（定稿只给了 n=1 的写法；n=1 与定稿逐字一致）
- `{measure}`：
  - fit → `实测 1.00 页，没有溢出（{口径}）。测量版本 #{v}，内容版本 {c}`
  - overflow → `实测 1.18 页，超出 0.18 页（{k} 行正文）。（{口径}）测量版本 #{v}，内容版本 {c}`
  - 容忍带 → `实测 1.01 页，在 2% 容忍内，仍算没有溢出。（{口径}）测量版本 #{v}，内容版本 {c}`
  - 不可用 → `尚未实测（{原因}）。量不到就不能说这一页「在一页内」。测量版本 无`
  - 过期 → `尚未实测（内容在上次测量之后改过，旧测量作废：上次 1.00 页 / 内容版本 {c0}，现在 {c}）。测量版本 无`
- `{口径}` 常量（逐字，代码里就是这一串，只此一处）：
  ```
  A4 210×297mm、正文宽 306px、页边距 34px、预览容器宽 374px
  ```
  （= 测量发生在预览 DOM 上：374 容器 − 两侧各 34px = 正文 306px。不写 16mm，那是打印口径。）
- `{原因}` 四选一：`预览没打开，量不到` / `字体还没加载完` / `测量超时` / `这一页是空的，没有可量的内容`
- `{verdict}`：
  - 未设目标 → `没有目标页数，不讲达标与否。实测 1.18 页这件事仍然要说准：它已经排到第二页上。`
  - fit → `达标：这一页在 {t} 页内。`
  - overflow → `未达标：超出 {t} 页。不要说「已经压到一页」，也不要说「差不多」。`
  - unknown → `不知道：这一页量不到。不要说「一页」「单页」「已经放得下」。`
  - file 页 → `这一页是用户上传的整页原件，没有可排的空间，不做页数判定。`
  - ✅ 已定稿：未设目标 + fit / 容忍带时，第二句改成 `实测 1.00 页，仍然要照报。`（定稿那句"已经排到第二页上"只在 overflow 时成立，overflow 时逐字用原句）
- ✅ 已定稿：**file 页整块只注入 verdict 那一句**（不实测、不判目标），避免新造槽位文案。

**describe_page 的实测段**（`facts::measure_section()`，追加在 `agent/layout.rs:408` 之后）：

```
## 这一页的实测
实测 1.00 页，没有溢出（A4 210×297mm、正文宽 306px、页边距 34px、预览容器宽 374px）。
目标页数 1 页（用户这一轮明确要求）。达标。
测量版本 #12，内容版本 v37。内容一改，这些数字作废，要重新量。
```
- overflow：第 1、3 行同上（第 3 行结论换 `未达标。`），第 2 行后插一行 `超出 0.18 页（11 行正文）。`
- unknown：`尚未实测（预览没打开，量不到）。` + `不知道有没有达标。这一轮不要说「已经压到一页」「放在一页内」。`
- 未设目标：第二行 `目标页数 未设置（用户没有说过要几页）。`、第三行 `没有目标，不讲达标与否；实测值仍然要照报。`

**抽图候选清单**（`tools/extract.rs`）：

```
【抽图结果】`扫描件.pdf` 里共 3 张内嵌位图，按来源页序排列。这里只列尺寸，不判断哪张是证件照 —— 选哪张由你定。
- `a3f19c04b2e6.png`｜第 2 页｜413×531 px｜宽高比 0.78｜不透明｜不接近整页（占版心 8%）｜21.4 KB
- `7c2f0e1a9b4d3f58.png`｜第 3 页｜1240×1754 px｜宽高比 0.71｜不透明｜接近整页（占版心 92%）｜318.6 KB
放照片只有一条路：把每行开头反引号里的 `assetId` 传给 `set_field` 的 `assetId` 参数，指向页头那个图片字段。
挑不出哪张是人像时，问用户一句「用第几张」，不要猜。
```
- 有解不出的：在列表与「放照片」之间插一行 `还有 2 张没抽出（编码 JPX 不支持）。`
- **超过 8 张（已定稿，`push_asset` 上限 `tools/mod.rs:122-132`）**：
  1. 排序键 = **「接近整页 pct」升序**（小的先留）。理由：证件照通常比整页扫描图小，整页图对"放照片"是无用候选。`pct` 不可测的排最后，按像素面积升序兜底。
  2. 落池**最多 8 张**，从上往下取。
  3. 候选清单只列这 8 张，并在末尾追加一行：
     ```
     还有 {k} 张没列（一次最多列 8 张），需要的话说「把剩下的也抽出来」。
     ```
  4. **不加 `skip` / `offset` 参数，也不做分页**。「把剩下的也抽出来」= 再调一次 `extract_images({stored})`：
     - **幂等靠内容寻址（asset id = sha 前 16 位），不靠分页参数**：已登记的 candidate 按 `id` 识别出来，**跳过、不重复落池、也不占这 8 个名额**；
     - 于是第二次天然返回接下来的 8 张（同样按 `pct` 升序取前 8），第三次再往后；
     - 实现点：`已登记` = `workspace.people[].assets` 里能按 id 找到，**或**本轮 `ctx.out.assets` 里已有（同轮调两次也不会重复登记）。
     - 给开发的硬话：**如果发现"第二批拿不到"，去查 id 生成是否真的等于 stored 名，不要加参数。**

**抽图失败三文案**（每条都含「照片没有放上去」）：

① 无内嵌位图
```
【抽图结果】`扫描件.pdf` 里没有内嵌位图 —— 文字是矢量的，没有可抽的图。
照片没有放上去：页头仍是空的。要放照片，让用户发一张图片，或者从其它材料里找现成的图片。
```
② 有图读不出
```
【抽图结果】`扫描件.pdf` 里有 2 张内嵌位图，2 张都读不出来（编码 JPX 暂不支持）。
照片没有放上去：页头仍是空的。不要按文件名或页面样子断定哪张是照片。
```
③ 文件打不开
```
【抽图结果】`扫描件.pdf` 打不开（文件损坏或加密），读不出里面的图。
照片没有放上去：页头仍是空的。让用户重新发一份，或者直接把照片发进来。
```
附 材料登记失败
```
【材料登记】`扫描件.pdf` 没能登记成材料，拿不到 assetId。
照片没有放上去：没有 assetId，`set_field` 写不进图片位。先把文件重新发一次。
```

**set_page_layout 追加行**：
- fit → `实测 1.00 页，没有溢出（目标 1 页，达标）。`
- overflow → `实测 1.12 页，超出 0.12 页（目标 1 页，仍未达标）。继续压，或停在这一档并说清差多少。`
- unknown → `尚未实测（测量不可用：预览没打开）。这一轮不能确认是否在一页内 —— 不要写「已压到一页」。`

### 4.5 `docs/CONTRACT.md` 逐节 diff

**§1.2 白名单**（`docs/CONTRACT.md:47-52`）—— 加一个名字：

```
 personFields  libraryItems  personAssets  itemAssets
 addPage  removePage  pageLabel  pageHeader  pageStyle  pageLayout  blockRole
 addSection  removeSection  blockFields  removeBlock  sectionTitle  sectionItems  entryPatch
 createResume  renameResume  deleteResume  deletePerson
+resumeTargetPages
```

**§1.2 载荷表**（:54-71）末尾加一行：

```
+| `resumeTargetPages` | `targetPages?, turn?` —— `targetPages` **省略 = 清除**（回到「未设置」），不要传 `null`；`turn` 是写入时对应的用户消息序号，前端缺省 0 |
```

**§1.2 之后的说明段**（:73-79 之后）追加：

```
+`resumeTargetPages` 不进确认门：它不改内容，改的是「这一份简历要压到几页」这个约定。
+两个键落进 `Resume.extra`（`targetPages` / `targetPagesTurn`）：reducer 里必须 `{...resume}`
+展开再改，手写重建对象会把它们丢掉。
```

**新增 §1.5**（插在 :110 之后，"## 2. 数据模型增量"之前）：

```
+### 1.5 实测上送（不落库）
+
+页面实测值只有一个来源：前端。它随请求上送，**不进 workspace、不进数据库** ——
+落库的测量值下次启动会变成假数据。
+
+| 通道 | 字段 | 形状 |
+|---|---|---|
+| `chat_send` | `args.measure` | `PageMeasure?` |
+| `layout_submit` | `args.measure` | `PageMeasure?` |
+| `layout_submit` | `args.origin` | `{ name, pages? }?` —— 原件页数，只在本轮有效 |
+
+`PageMeasure.status` 只有三个：`ok`（量到了）/ `unknown`（量不到，带 `reason`）/
+`stale`（量到过，但内容之后改过 —— 旧值作废，带 `previous`）。
+
+阈值（1.00 fit / 1.02 容忍 / 1.02 以上 overflow）与全部文案只在
+`src-tauri/src/agent/facts.rs` 实现，前端与渲染层不许复刻。
```

**§2 数据模型增量**（:114 之后）追加：

```
+`Resume.extra` 上新增两个键（走 `#[serde(flatten)]`，不改 struct 形状）：
+
+```rust
+/// 用户明确说过的目标页数。没有 = 未设置。
+/// 前端 `src/types/index.ts` 必须显式声明 `targetPages?: number`（`Resume` 没有索引签名）。
+pub target_pages: Option<u32>,       // JSON: targetPages
+pub target_pages_turn: Option<u32>,  // JSON: targetPagesTurn —— 第几条用户消息写入的
+```
+
+`Asset.extra` 上新增三个键，用于「从 PDF 抽出来的候选图」：
+
+```rust
+pub role: Option<String>,       // "candidate" —— 渲染层不会拿它当页头照片
+pub source: Option<String>,     // 来源 PDF 的 assetId
+pub page_index: Option<u32>,    // 在来源 PDF 里的页序（1 基）
+```
```

**§3 会话 agent 工具**（:152 之后，3.3「页与简历」表里）加两行：

```
+| `set_target_pages` | `pages?` | `resumeTargetPages`（省略 = 清除） |
```
并在 §3 末尾（:213 之前）新增一小节：

```
+### 3.7 附件与抽图
+
+| 工具 | 参数 | 产物 |
+|---|---|---|
+| `extract_images` | `stored, name?` | 落池的 candidate 资产 + 候选清单文本 |
+
+- `stored` 是附件在 `inbox/` 里的存储名，从会话材料段的「附件标识」行取，**不是文件名**。
+- 幂等：同一份内容只登记一次源资产，同一张图只落一份、id 恒等于池内存储名（`<sha16>.<ext>`），
+  所以「每行开头反引号里的 assetId」可以直接交给 `set_field`。
+- 只对 PDF 生效，非 PDF 直接报「这不是 PDF」。
+- 抽出的图**进 `person.assets`** 并带 `role:"candidate"`：模型能用它，渲染层不会拿它当页头照片。
+- 系统**不判断**哪张是证件照 —— 只给尺寸与占比，选哪张由模型定。
```

**§4 排版工具**（:216 之后）追加返回文案约定：

```
+### 4.1 返回文案里的实测结论
+
+`set_page_layout` 的返回值**必须**带一行实测结论，三态各一句（见 `agent/facts.rs`）：
+
+- 达标：`实测 1.00 页，没有溢出（目标 1 页，达标）。`
+- 未达标：`实测 1.12 页，超出 0.12 页（目标 1 页，仍未达标）。继续压，或停在这一档并说清差多少。`
+- 量不到：`尚未实测（测量不可用：预览没打开）。这一轮不能确认是否在一页内 —— 不要写「已压到一页」。`
+
+量不到时的措辞**必须**与「达标」明显不同：测量不可用时写「尚未实测」，不许写「没有溢出」。
+`describe_page` 的正文末尾追加 `## 这一页的实测` 一段，与上面同源（`facts::measure_section()`）。
```

**§5 渲染增量**（:237 之后）追加：

```
+### 5.0 页头照片的候选豁免
+
+`photo_of()` 选页头照片时**跳过** `asset.extra.role == "candidate"` 的资产。
+
+原因：`extract_images` 把 PDF 里的位图全部登记进 `person.assets`，而 `photo_of` 取的是
+「第一张 `kind == "image"`」—— 不跳的话，PDF 里的第一个 logo / 水印 / 装饰条会立刻
+变成页头的证件照，用户会看到一张完全无关的图被当成照片。
+
+老数据没有 `role` 键 → 行为不变（仍然取第一张），升级后用户原有证件照不会消失。
```

**§6 文件所有权**（:270-280 的表）加行：

```
+| **BE-domain** | `src-tauri/src/pdfembed.rs`(新)、`src-tauri/src/agent/facts.rs`(新)、`src-tauri/src/tools/extract.rs`(新) |
```

---

## 5. 关键算法

### 5.1 XObject 递归遍历（`pdfembed.rs`）

```
fn extract(bytes) -> Result<Report, String>:
    doc = Document::load_mem(bytes).map_err(|_| "open")?
    if doc.is_encrypted() && doc.decrypt("").is_err(): return Err("open")
    pages = doc.get_pages()                       // BTreeMap<u32, ObjectId>
    report.pages = pages.len()
    for (page_no, page_id) in pages:
        media = page_media_box(&doc, page_id)     // 沿 Parent 上溯；读不出用 A4
        report.inner_area = inner_area_of(media)  // 见 2.3
        contexts = display_sizes(&doc, page_id)   // name → (w_pt, h_pt)；见 2.3 的 cm 状态机
        (resources, _) = doc.get_page_resources(page_id)   // 已沿 Parent 上溯
        walk(doc, resources, contexts, page_no, &mut report, seen: &mut HashSet<ObjectId>, depth=0)
    Ok(report)

fn walk(doc, resources: Option<&Dictionary>, contexts, page_no, report, seen, depth):
    if depth > 8: return                      // Form 自引用/深嵌套：停手，不 panic
    xobjects = resources?.get(b"XObject")?.as_dict()
    for (name, value) in xobjects:
        id = value.as_reference()?            // 内联字典（非引用）→ 记 unsupported["inline"]
        if !seen.insert(id): continue         // 同一 XObject 复用（水印）只算一次
        stream = doc.get_object(id)?.as_stream()?
        subtype = stream.dict.get(b"Subtype")?.as_name_str()?
        match subtype:
            "Form" => {
                // Form 的 /Resources 可能为空 → 继承调用方的
                let sub_res = stream.dict.get(b"Resources").ok().and_then(|o| o.as_dict().ok())
                                  .or(Some(resources));
                let content = probe_decompress(&stream)?;      // 同 2.2
                let inner_ctx = contexts.clone();              // 不追 Form 内的 cm（见取舍）
                walk(doc, sub_res, inner_ctx, page_no, report, seen, depth+1)
            }
            "Image" => match decode_image(&doc, &stream):
                Ok(mut img) => { img.page = page_no; img.disp = contexts.get(name); report.images.push(img) }
                Err(reason) => report.unsupported.push(reason),
            _ => {}
    // 内联图：content stream 里遇 BI…EI 直接跳过并记 unsupported["inline"]
```

**取舍**：Form 内部的 `cm` 不参与显示尺寸累积（沿用调用方的 CTM）。Form 里画证件照的 PDF 极少；做对了收益小而实现风险大（要把 content 解成操作符再喂回状态机）。这条写进风险表。

### 5.2 图像解码矩阵

| `/Filter` | 处理 | 备注 |
|---|---|---|
| `DCTDecode` | `content` 就是完整 JPEG → `image::load_from_memory` | 不解码，直接当字节用 |
| `FlateDecode` / `Fl` | `probe.dict.remove(Subtype)` + `decompressed_content()` → 原始像素 | 见 2.2 |
| `LZWDecode` / `ASCII85Decode` | 同上（lopdf 已支持） | 少见但免费 |
| `JPXDecode` / `CCITTFaxDecode` / `JBIG2Decode` | `unsupported.push("JPX")` 等 | `image` 没开 jpx（`Cargo.toml:67-71`），也没有 fax/jbig2 解码器 |
| 多个 filter（如 `[FlateDecode DCTDecode]`） | `unsupported` | 罕见，不猜 |

像素 → PNG：

```
bpc 展开（`/BitsPerComponent`）：
  8  → 原样
  1 / 2 / 4 → 每字节拆成 8/bpc 个样本，按位宽左移到 8 位（值 × 255/(2^bpc-1)）
  16 → 取每个样本的高字节（PDF 里 16 位图像实际有效位数通常 = 8）
  其它 → unsupported["bpc"]

色彩空间（`/ColorSpace`）：
  DeviceGray / CalGray / ICCBased(N=1) → 灰度直接复制到 RGB
  DeviceRGB  / CalRGB  / ICCBased(N=3) → 原样
  DeviceCMYK / ICCBased(N=4)          → C'=(255-C)*(255-K)/255，逐通道（乘积式，比减法式更接近印刷观感）
  [/Indexed base hival lookup]        → 按 bpc 位宽取索引，查调色板（string 或 stream），映射到 base
  Separation / DeviceN / Lab          → unsupported["colorspace"]

透明：`/SMask`（引用或内联流）→ 解出灰度 alpha（只支持 bpc=8）→ 合成为 RGBA；解不出就丢弃 alpha（不报错）
`/Decode [1 0]`：只处理灰度反相，其它忽略（反色不致命，缺图才致命）
尺寸上限：宽或高 > 4000px → unsupported["too-large"]（防止一张 20000px 的图把内存打爆）
```

任何一条路径都**返回 Err 原因，不 panic**：这是工具链，一张怪图不该毁掉整轮对话。

### 5.3 溢出量折算成「行」

```
lineHeightPx = 前端从 getComputedStyle(paper).lineHeight 读出（px），随 measure 上传
lines        = round(overflowPx / lineHeightPx)
overflowPx   = contentPx - onePage
```
`lineHeightPx` 缺失时退 `17`（paper.css 默认 `--fs:10px × --lh:1.7`）。preset 会改 `--fs/--lh`（`paper.css:46-56`），所以行数只是量级参考 —— 但它是**实测行高**折出来的，不是猜的。

---

## 6. 事实块实现

**模块**：`src-tauri/src/agent/facts.rs`（`agent/mod.rs` 加 `pub mod facts;`）

**函数签名**：

```rust
/// 完整事实块。会话侧 system 注入用它。
pub fn block(f: &PageFacts) -> String;

/// fit 段（带 `## 这一页的实测` 标题）。排版侧 describe_page 末尾追加用它。
pub fn measure_section(f: &PageFacts) -> String;

/// 只要 {measure} 那一句。两处共用，保证「实测 1.00 页，没有溢出（口径）」一字不差。
pub fn measure_line(f: &PageFacts) -> String;

/// set_page_layout 返回值末尾追加的那一行。三态。
pub fn layout_result_line(f: &PageFacts) -> String;

/// 目标页数三态的推导（§2.7）。
pub fn target_of(resume: &Value, origin: Option<&Origin>, turn: Option<u32>) -> Target;
```

**输入从哪来**（`PageFacts` 组装点只有两个）：

| 组装点 | origin | target | measure | turn |
|---|---|---|---|---|
| `session.rs::chat_send`（`locate()` 之前） | `attach::materials()` 返回 | `target_of(&resume_json, origin, Some(user_count))` | `args.measure` | `Some(user_count)` |
| `agent/layout.rs::run_turn`（`:483` 之前） | `args.origin` | `target_of(&resume_json, origin, None)` | `args.measure` | `None` |

`resume_json` 从 `args.workspace` 里按 `person_id`/`resume_id` 取（`describe_page` 已有同款查法，`agent/layout.rs:234-275`）。

**两侧注入的具体位置**：

1. 会话侧 —— `src-tauri/src/agent/context.rs`：
   ```rust
   pub fn locate(workspace:&Value, person_id:&str, resume_id:&str, page_id:&str, view:&str,
                 facts:&crate::agent::facts::PageFacts) -> String {
       // …原样到 :84 的「- 视图：…」
       out.push_str(&format!("- 视图：{}\n", …));
       out.push('\n');
       out.push_str(&crate::agent::facts::block(facts));      // ← 插在这里
       out.push_str("\n- 你可以用工具读取真实内容；…");        // :85 原句
   }
   ```
   调用点 `session.rs:515-520` 多传一个参数。

2. 排版侧 —— `src-tauri/src/agent/layout.rs:408`（`正文合计约 {chars} 字\n` 之后、`:411` 的"当前版面"之前）：
   ```rust
   out.push_str(&crate::agent::facts::measure_section(&facts));
   ```
   需要把 `facts` 传进 `describe_page(args, &facts)`。
   > 放在这里（形状描述之后、版面描述之前）的理由：容量判断紧跟"这页有多少字"，读者顺序是「多少内容 → 实测多少页 → 现在怎么排的」。

---

## 7. 测量实现

### 7.1 `usePageMeasure` 签名

```ts
// src/lib/measure.ts
export interface Measured { measure: PageMeasure; }
export function usePageMeasure(input: {
  /** 离屏宿主的 .wrap 元素 */
  hostRef: React.RefObject<HTMLDivElement | null>;
  person: Person | null;
  page: ResumePage | null;
}): PageMeasure | null;
```

内部：

```
1. content = useMemo(() => contentVersion(person, page), [person, page])
2. state: last = { content, version, onePage, contentPx, lineHeight } | null
         fontsReady: boolean
3. read():
     paper = hostRef.current?.firstElementChild as HTMLElement  // .paper
     if (!paper) → unknown("no-preview")
     w = paper.offsetWidth; h = paper.offsetHeight
     if (!w) → unknown("no-preview")
     if (h <= 0) → unknown("empty")
     onePage = w * (297/210)                    // 与 panel.tsx:331 同一个字面量，抽成常量 A4_RATIO
     lineHeight = parseFloat(getComputedStyle(paper).lineHeight) || 17
     if (!fontsReady) → 先给 unknown("fonts")，document.fonts.ready 到了再 read()
     → ok{ onePage, contentPx: h, lineHeight }
4. 过期：last.content !== content && last 存在 → stale{ content, previous: last }
5. 版本号：每次 ok 成功 version += 1（模块级 let）
6. 超时：读过一次后起 setTimeout(1500)，若字体仍未 ready 且没有 ok → unknown("timeout")
7. useLayoutEffect 建 ResizeObserver(observe paper)，回调 read()；deps: [mode 无关, page?.id, content]
```

**不给 `mode` 加依赖**（现在 `panel.tsx:339` 的 deps 是 `[mode, page?.id]`）—— 离屏宿主两态都在，测量不该随视图切换重算。

### 7.2 `contentVersion`

```ts
const A4_RATIO = 297 / 210;

export function contentVersion(person: Person | null, page: ResumePage | null): string {
  if (!person || !page) return "";
  const shape = {
    kind: page.kind ?? "native",
    assetId: page.assetId ?? "",
    layout: page.layout ?? null,          // preset/html/parts 都改高度
    blocks: (page.blocks ?? []).map(canonBlock),
    fields: (person.fields ?? []).map((f) => [f.label, f.value, f.assetId ?? ""]),
    assets: (person.assets ?? []).map((a) => [a.id, a.stored]),
  };
  return fnv1a(JSON.stringify(shape)).toString(16).padStart(8, "0");
}
// canonBlock：{...block, parts? 排序} —— Record 的 key 顺序不稳定，必须先 Object.keys().sort()
```
FNV-1a 32 位。碰撞概率对"一页几百次编辑"足够（2^32）。**不需要跨语言一致**：Rust 不复算。

### 7.3 `overflowPx` 的 zoom 归一化

离屏宿主无 `zoom`（`zoom` 只加在预览 `.wrap` 上，`panel.tsx:691`），所以 `measure.onePage` / `measure.contentPx` **天然是 1:1 的**。预览侧唯一的换算：

```ts
// panel.tsx 预览分支
const breakTop = measure?.onePage ? measure.onePage * (zoom / 100) : 0;
{measure && measureStatus === "overflow" && <div className={paperStyles.break} style={{ top: breakTop }} />}
```
这一改同时修掉老隐患：原来 `a4Height` 来自 `paper.offsetHeight / offsetWidth`，在 zoom 下两者都被放大，比值恰好不变 —— 但 `sheets` 的分子分母同乘 zoom 后仍正确，纯属运气。现在不靠运气。

### 7.4 字体未就绪与超时

- `document.fonts.ready`（Promise）→ 到货后重跑 `read()`。
- 1.5s 上限：到点还没有一次 `ok` → 出 `unknown("timeout")`，不再等。
- 字体没就绪时的临时值用 `unknown("fonts")` 而不是 `ok`：**宁可说量不到，也不给一个会变的数** —— 这正是 F-007「未 fit 不得宣称一页」的前提。

---

## 8. 自动自查与投递（F-010）

**位置**：`provider.tsx`（它有 `page` / `resume` / `measure` / `layoutStatus` / `submitLayout`）。

```tsx
const autoRounds = useRef(new Map<string, number>());   // pageId → 已投递轮数

useEffect(() => {
  if (!person || !resume || !page || !measure) return;
  // 量不到就不投：拿一个假数去指挥排版，比不投更糟
  if (measure.status !== "ok" || !measure.onePage || !measure.contentPx) return;
  // 排版正在跑这一页：不叠加，等它跑完自然会再看一眼
  if (layoutStatus?.pageId === page.id) return;
  const target = resume.targetPages;
  if (!target) return;                                  // 没有目标，不讲达标与否

  const pages = measure.contentPx / measure.onePage;
  if (pages <= 1.02) { autoRounds.current.delete(page.id); return; }   // 收敛了就清零

  const round = autoRounds.current.get(page.id) ?? 0;
  if (round >= 3) {
    if (round === 3) {
      autoRounds.current.set(page.id, 4);               // 只留一次，别每帧都写
      dispatch({ type: "pageNote", personId: person.id, resumeId: resume.id, pageId: page.id,
        note: { id: `n-auto-${page.id}-4`, text: `自动自查已投 3 轮，仍实测 ${pages.toFixed(2)} 页（目标 ${target} 页）。差 ${(pages - 1).toFixed(2)} 页，需要人决定：减内容还是放宽页数。`, at: Math.floor(Date.now()/1000) } });
    }
    return;
  }
  autoRounds.current.set(page.id, round + 1);
  const timer = window.setTimeout(() => {              // 300ms 防抖：一帧内的多次测量只投一次
    void submitLayout({
      person, resume, page, state, kind: "request",
      text: `自动自查：这一页实测 ${pages.toFixed(2)} 页，超出 ${(pages - 1).toFixed(2)} 页，目标是 ${target} 页。请继续压到目标页数内；压不动就说清差多少，不要谎报。`,
      measure, origin: originOf(resume.id),
    });
  }, 300);
  return () => window.clearTimeout(timer);
}, [measure, person, resume, page, state, layoutStatus, dispatch]);
```

- **计数键**：`pageId`（同一个 brief 文本是常量，所以页 + 固定 brief 天然是同一个键）。
- **上限**：3 轮；第 4 次只留一条批注（id `n-auto-{pageId}-4`，`pageNote` reducer 按 id 去重，重放不会叠）。
- **清零**：fit / 容忍带（`<= 1.02`）时删键；内容变化后 `measure` 变 `stale`/`unknown` → 提前 return，不投也不清零（等用户改完再量一次）。
- **失败**：测量 unknown → 不投（拿假数指挥排版是负收益）。
- **不做什么**：不在 Rust 里定时/轮询。Rust 没有测量能力（DOM 在 WebView 里），做定时只会产生假数据。

---

## 9. 导出前置门（F-011）与状态栏徽标

### 9.1 前置门

**挂载点**：`panel.tsx` 导出下拉的三项（`:667` 打印 / `:669` Word 当前页 / `:681` Word 整份）。Markdown 三项（`:670`、`:672`）**不过门**。

```tsx
const [pendingExport, setPendingExport] = useState<null | { run: () => void; items: string[] }>(null);

/** 已设目标页数 + 这一页实测溢出 → 先问一句，不硬拦 */
const gate = (run: () => void) => {
  const list = overflowedPages();          // 只算「有测量且 overflow」的页
  if (!resume.targetPages || !list.length) { run(); return; }
  setPendingExport({ run, items: list });
};

const overflowedPages = () => {
  if (!page || !measure || measure.status !== "ok" || !measure.onePage) return [];
  const pages = measure.contentPx! / measure.onePage;
  if (pages <= 1.02) return [];
  return [`第 ${pageIndex(resume!, page.id) + 1} 页「${page.label || "未命名页"}」实测 ${pages.toFixed(2)} 页`];
};
```
整份 Word：只有当前页有测量（离屏宿主只渲染当前页），所以 `overflowedPages()` 天然只可能列当前页 —— **量不到的页不拦**，符合 F-011。

确认窗用 `components/ui/confirm`（`ui/confirm.tsx:19-31` 的 props）：

```tsx
<Confirm
  open={!!pendingExport}
  danger={false}
  title={`这一页还没压到 ${resume?.targetPages ?? "目标"} 页内`}
  description="导出的文件和你在屏幕上看的一样，会排到第二页上。要不要先让排版继续压？"
  items={pendingExport?.items}
  confirmLabel="照这样导出"
  cancelLabel="先去压一页"
  onConfirm={() => { const job = pendingExport; setPendingExport(null); job?.run(); }}
  onCancel={() => setPendingExport(null)}
/>
```
不放 provider：它是导出的局部状态，挂全局会让"一次只允许一个确认窗"的假设变复杂。

### 9.2 状态栏徽标

`status-bar.tsx`，页码后、字数前插一档：

```tsx
{measure && <span className={styles.measure} title={measureTip(measure, resume?.targetPages)}>
  {measure.status === "ok"
    ? <><i className={pagesOk ? styles.dotIdle : styles.dotWarn} />{pagesText}</>
    : <><i className={styles.dotIdleDash} />页数未知</>}
</span>}
```
- `pagesText`：`1.00 页内` / `1.01 页` / `1.18 页 · 超 0.18`
- `.dotWarn` 加在 `status-bar.module.css`（照 `.dotBusy` 的形状，颜色用 `--warn`，没有就用 `--danger` 的 60% 透明）。

`measureTip()`（同文件，纯函数，10 行内）：

```
量到了：`实测 1.18 页（目标 1 页，未达标）。超出 0.18 页 · 约 11 行正文。`
量到了但宽容忍带：`实测 1.01 页，在 2% 容忍内，仍算没有溢出。`
量到但没设目标：`实测 1.18 页。没有目标页数，不讲达标与否。`
量不到：`尚未实测（预览没打开，量不到）。` → 按 reason 四选一
过期：`尚未实测（内容在上次测量之后改过，旧测量作废）。`
```

---

## 10. 提示词改动

### 10.1 `prompts/system.md`

在「## 当前材料」之前（`:91` 前）插两节。**短句、可 grep**：

```markdown
## 页数只有一处来源

页数、实测、达标与否，**只能照抄【页面事实】那一段**。不要自己数、不要按比例推算、不要把「大概一页」说成「已经压到一页」。

- `未达标`：说清超出多少（「超出 0.18 页」），不要说「差不多了」「基本在一页内」。
- `尚未实测`：说「量不到」，并且**不要**说这一页「在一页内」「已经压到一页」。
- `没有目标页数`：不讲达标与否，但实测值仍然要照报。
- `页数读不出`：就说「页数读不出」，**不要按 1 页算**，也不要猜。

用户说「压到一页」「控制在一页」时，先调 `set_target_pages({ pages: 1 })` 把这件事定下来，再 `request_layout` 把要求转给那一页的排版 agent。只转达、不自己排。

## 照片

用户上传的 PDF 里可能带证件照。要放照片只有一条路：

1. 调 `extract_images({ stored })` 拿候选清单（`stored` 从材料段的「附件标识」行取）。
2. 把清单里某一行的 assetId 传给 `set_field({ fieldId, assetId })`，指向页头那个图片字段。
3. 页头没有图片字段时，先 `add_field({ ownerId: 页头块 id, label: "照片" })`，再写图。

- 系统**不判断**哪张是证件照，由你选；挑不出就问用户一句「用第几张」，**不要猜**。
- 抽图失败时，回答里**必须**有一句「照片没有放上去」，并说清原因（没有内嵌位图 / 编码不支持 / 文件打不开）。
- 不要把「占版心 90% 以上」的那张（那是整页扫描件）当成证件照。
```

### 10.2 `prompts/layout.md`（:87-108 整段替换）

```markdown
## 容量参考只是量级参考

| 档位 | 大致容量 |
|---|---|
| `roomy` | 550–700 字 |
| `compact` | 950–1150 字 |
| `classic` | 700–900 字 |

这张表**只用来估量级**（「这一页是 600 字还是 1400 字」）。它不告诉你这一页装不装得下 ——
字号、边距、分栏、图片都会改实际容量，靠字数推算误差能到三成。

**唯一判据是页面上方的「## 这一页的实测」**：实测 ≤ 1.00 页 = 达标；1.00–1.02 = 在容忍内；
超过 1.02 = 溢出，要写清超出多少。

调一次不等于排好。交版前看一遍实测数：还在溢就继续压（换档、收边距、降字号、改两栏）；
压过头就放回来。已经在 `compact` 且实测达标，就**停在这里**，不要在两个档位之间来回横跳。

若实测显示「尚未实测」：这一轮**不能**说「已经压到一页」，也不要说「放在一页内」。
```

### 10.3 `render::GUIDE`（`render/mod.rs:193`）

**结论：同步改**。理由：`read_layout_guide` 只喂排版 agent（`tools/layout.rs:116-118`），如果它继续说「用字数估容量」，就会和 `describe_page` 的实测段打架 —— 模型会选它先读到的那个。

`【收尾前自查】` 两条替换为：

```
- 这一页装不装得下，只看「## 这一页的实测」那个数：≤1.00 达标、≤1.02 容忍内、>1.02 溢出
- 字数（compact 950–1150 / normal 700–900 / roomy 550–700）只是量级参考，不要用它判达标
```

---

## 11. 测试计划

### 11.1 Rust UT（`cargo test`，全部可执行）

| F | 用例 | 断言 |
|---|---|---|
| F-005 | `facts::block` 三分支 | 全文 `assert_eq!`：正常 `页数 3 页（来源：PDF 页树）`；读不出 = 那整行；无附件 = 那整行 |
| F-006 | `target_of` 四种 | `UserThisTurn` / `UserBefore`（turn 差 2）/ `FromOrigin(3)` / `Unset`；`origin.pages = Some(4)` 时不产生 `FromOrigin` |
| F-006 | 五条 `{target}` 槽位 | 逐条 `assert_eq!` 文案 |
| F-007/8 | `fit_of` 边界 | 1.00→fit、1.005→容忍（文案含「2% 容忍内」）、1.02→容忍、1.021→overflow；overflow 的 `over` = `pages-1` 保留 2 位 |
| F-008 | `measure_section` 三态 | 含 `## 这一页的实测`；unknown 时含「尚未实测」且**不含**「没有溢出」 |
| F-009 | `layout_result_line` 三态 | 逐字 |
| F-013 | `extract_images` 三失败文案 | 每条都 `assert!(text.contains("照片没有放上去"))`；三类互不相同 |
| F-002 | `pdfembed::extract` 三类编码 | 用 `lopdf` 现场构造 PDF：1 张 `DCTDecode` + 1 张 `FlateDecode`（8bpc RGB）+ 1 张 `JPXDecode` → `images.len()==2`、`unsupported==["JPX"]`、页序 = [1,1] |
| F-002 | Flate predictor 用例 | 造 `Predictor 12` 的流 → 解码成功且首行像素正确（防 predictor 走错路） |
| F-002 | 不 panic | 给 `extract(b"not a pdf")` → `Err("open")`；给截断的 PDF → 不 panic |
| F-003 | 占比 | `cm` 设为 0.3749/0.3695 → `pct ≈ 8`、`near=false`；`cm` 铺满版心 → `pct ≈ 100`、`near=true` |
| F-001 | 幂等 | 同一 `stored` 调两次 → `out.assets` 只多 1 条；第二次返回既有 assetId |
| F-001 | 8 张上限与「再抽一次」 | 造 11 张图的 PDF：第一次 `images=8`、文案含 `还有 3 张没列`；把第一批假装已登记后第二次的 8 张里不含第一批任何 id、只剩 3 张 |
| F-003 | 口径常量 | `assert!(block(...).contains(MEASURE_RULER))`，且 `MEASURE_RULER` 含 `预览容器宽 374px`、**不含** `860` / `16mm`（防文案回退） |
| F-004 | 照片位 | `photo_of`：person.assets 里有 `role:"candidate"` 的图在前、真照片在后 → 返回真照片 id；老数据（无 role）→ 返回第一张 |
| — | 工具清单 | `session.len()==40`、`layout.len()==9`（`tools/mod.rs:718-719`） |

### 11.2 前端测试（已定稿：**加 vitest，只测纯函数**）

现状：`package.json` 无测试框架、无 `*.test.ts`；`scripts` 只有 dev/build/lint。

**决定（已定稿）**：

- `package.json` 加 `vitest` **一个** devDependency（版本跟 vite 7 的兼容档）+ `"test": "vitest run"`。
- `vite.config.ts` 里加 `test: { environment: "node" }`。
- **不加 jsdom、不做组件测试、不加 `@testing-library/*`。**
- 测试放 `src/lib/*.test.ts`（本期只有 `src/lib/measure.test.ts`），只测两个纯函数：
  - `contentVersion`：同内容不同对象 → 同值；改一个 bullet 的一个字 → 不同值；`layout.parts` 的 key 顺序不影响结果；`notes` 变化**不**影响结果。
  - `measureTip`：三态（达标 / 溢出 / 量不到）文案各一条。

其余前端逻辑（`usePageMeasure`、自动自查 effect、导出门）**只能真实渲染冒烟**（§11.3），不写 UT：它们依赖 ResizeObserver / `document.fonts` / 真实布局，jsdom 测出来的是假绿 —— **不许用假绿糊过去**。

### 11.3 真实渲染冒烟（人工，6 条）

1. 上传单页带证件照的扫描 PDF → `extract_images` → 候选清单里有一张 `占版心 5–12%`、宽高比 0.7–0.8 → `set_field` 写进页头 → 预览里看到照片、且**不是**那张整页扫描件。
2. 编辑态（不切预览）发一句话 → 事实块里 `这一页「…」：实测 1.00 页…`（不出现「尚未实测」）。这是本次最关键的回归点。
3. 内容加到溢出 + 设目标 1 页 → 一轮之后状态栏出现 `1.18 页 · 超 0.18`；自动投递让排版压到 ≤1.02；徽标转正常。
4. 同页模拟压不动（连续 3 轮溢出）→ 第 4 次出现批注 `自动自查已投 3 轮…`，且只出现一次。
5. 溢出 + 已设目标 → 点「Word · 当前页」→ 弹确认窗；点「先去压一页」→ 不导出；点「照这样导出」→ 导出。「Markdown · 当前页」不弹窗。
6. 一份含 11 张内嵌位图的 PDF → 第一次清单 **8 行** + 末尾「还有 3 张没列」→ 再说「把剩下的也抽出来」→ 清单 **3 行**；两次之后 `person.assets` 里累计 **11 条** candidate、无重复 id（验证 §4.4 的 8 张规则与「内容寻址即幂等」）。

### 11.4 会被本次改动打破的现有测试（必须同批修）

| 位置 | 为什么红 | 修法 |
|---|---|---|
| `tools/mod.rs:718` `assert_eq!(session.len(), 38)` | 新增 2 个工具 | 改 40 |
| `tools/mod.rs:719` `assert_eq!(layout.len(), 9)` | 不变 | 不改（但要确认没顺手加进 Layout） |
| `tools/mod.rs:722-742` 角色断言 | 新增断言 | 加 `session.contains(&"extract_images")`、`!layout.contains(&"extract_images")`、`!layout.contains(&"set_target_pages")` |
| `tools/mod.rs`、`image.rs`、`session.rs` 里 `ToolCtx { … }` 字面量（mod.rs:808、image.rs:314、mod.rs:844 等） | 新增 `facts` 字段 | 全部补 `facts: None`（或用 `ToolCtx::of`） |
| `attach.rs` 的 `materials()` 三处测试（:425/:447） | 返回类型变 struct | 解构改用 `.section` / `.images` |
| `agent/context.rs` 无测试 | `locate()` 签名变 | 若加测试，注意新参数 |
| `render/mod.rs` photo_of | 新增豁免 | 补两条断言（见 §11.1 的 F-004 行） |

---

## 12. 风险与取舍

| 风险 | 影响 | 应对 |
|---|---|---|
| **编码覆盖不全**（JPX / CCITT / JBIG2 无解码器） | 部分扫描件抽不出图 | 明确计入 `unsupported` 并在文案里说「2 张都读不出来（编码 JPX 暂不支持）」；不假装成功。`image` 加 `jpx` 特性会带进 openjpeg 的 C 依赖，不做 |
| **Flate 走 lopdf 的 `decompressed_content`** | 极端 PDF 上可能失败 | 失败即 `unsupported`，不 panic；用 predictor 用例覆盖最常见路径。不引 `flate2` 自己写 predictor（5 种类型，错了是静默花屏） |
| **Form 内部的 cm 不累积** | Form 里画证件照时占比算不准 | 罕见；占比只是"是否接近整页"的提示，算偏了最坏是模型多问一句用户 |
| **预览与打印边距不一致**（预览正文宽 **171.9mm**、打印 `@page margin:16mm` → 正文 **178mm**、Word 20mm，`docx.rs:164`） | 预览比打印**窄 6.1mm**（纸张同宽 210mm，预览两侧各 19.05mm 而打印各 16mm）→ **预览偏保守**：会把「打印其实装得下」误判成溢出 | ✅ **已接受这个方向**（宁可误报装不下，也不能让用户导出后才发现超页）。本方案**不统一三套边距**（那是 out of scope 的排版重构）。事实块文案改成实测真实的三个数（正文宽 306px / 页边距 34px / 容器 374px），**文案里不许再出现 16mm** |
| **`{口径}` 文案与实现** | — | ✅ 已定稿为 `A4 210×297mm、正文宽 306px、页边距 34px、预览容器宽 374px`；常量 `MEASURE_RULER` 一处定义，另有 UT 断言它含 `374px` 且不含 `860`/`16mm`（防回退） |
| **离屏宿主双渲染成本** | 编辑态多一次 `compilePage` IPC + 一次布局 | 只渲染当前页（不预渲染全简历）；`Paper` 内部已有 300ms 防抖（`paper.tsx:63`）；可接受。实测影响在 Batch 3 验收时用 DevTools 记一次 |
| **抽图耗时（目标 ≤3s）** | 大图 + 多图可能超 | 单张边长 > 4000px 直接 `unsupported["too-large"]`；单次最多 8 张进池；Flate 解码是内存操作（1240×1754 约 100–300ms），8 张约 1–2.4s。Batch 1 用真实 PDF 计一次时 |
| **`photo_of` 语义变更** | 老数据兼容 | 只在有 `role:"candidate"` 时跳过；没这个键 = 老行为。加一条专门的 UT |
| **`push_asset` 上限 8（`tools/mod.rs:122-132`）** | 一次抽 >8 张会静默丢 | 自己控数量：按 `pct` 升序取前 8 张（小的先留），文案明说「还有 k 张没列」；不依赖 `push_asset` 静默截断。已登记的按 id 跳过、不占名额 → 重跑一次天然拿到下一批，**无分页参数** |
| **Windows / non-Windows 差异** | 无 | `pdfembed` 纯 Rust，与 `pdfimage` 的 WinRT/CLI 双路无关。这是本方案相对"渲染整页再切"的主要优势 |
| **`ToolCtx` 加字段** | 5 处构造点 + 测试 | 已有 `ToolCtx::of` helper；测试里全部走它 |
| **系统文本缩放（`--text-scale`）改变纸面字号** | 实测值随系统设置变 | 这正是"实测"的应有行为（它就是渲染出来的）。但会让同一份简历在两台机器上页数不同 → 文案里写明"正文宽 306px / 页边距 34px"，不承诺跨机一致 |

---

## 13. 口径已定稿（7 条，全部生效，不再讨论；本章即最终值）

| # | 项 | 定稿 | 影响面 |
|---|---|---|---|
| 1 | 容器宽 | `预览容器宽 374px`（= `.wrap`），**不是 860** | `MEASURE_RULER` 字符串 |
| 2 | 边距 | 文案里**删掉 16mm**，改成实测真实的 `正文宽 306px、页边距 34px`；`PAGE_PAD_MM = 16.0` **只留在「接近整页」的面积换算里**（打印口径，与 34px 不是一回事，§2.3） | `MEASURE_RULER` + `pdfembed` 常量注释 |
| 3 | `{target}` n>1 | `压到 {n} 页`（n=1 逐字同定稿） | `facts::target_line` 的一个 `match` 分支 |
| 4 | `{verdict}` 未设目标 + fit | `没有目标页数，不讲达标与否。实测 1.00 页，仍然要照报。`；overflow 逐字用原定稿 | `facts::verdict_line` |
| 5 | file 页 | 整块只注入 verdict 一句，不实测不判目标，**不新造槽位** | `facts::block` 提前 return |
| 6 | 前端测试 | 加 `vitest` **一个** devDependency（**不加 jsdom**），只测 `contentVersion` 与 `measureTip` 两个纯函数；`package.json` 加 `"test": "vitest run"`；测试放 `src/lib/*.test.ts` | `package.json`、`vite.config.ts`、`src/lib/measure.test.ts` |
| 7 | 抽图 >8 张 | `pct` **升序**保留小的 8 张；已登记按 id 跳过不占名额；重跑一次拿下一批；**幂等靠内容寻址，不靠分页参数** | `tools/extract.rs` 的选择与文案 |

常量落一处（改一个字全链生效）：

```rust
// src-tauri/src/agent/facts.rs
pub const MEASURE_RULER: &str =
    "A4 210×297mm、正文宽 306px、页边距 34px、预览容器宽 374px";
```

第 7 条追加行的定稿文案（不再有"更大/更小"，避免与排序键打架）：

```
还有 {k} 张没列（一次最多列 8 张），需要的话说「把剩下的也抽出来」。
```

**已确认不再摇摆的两处技术判断**：
- 「接近整页」用 **CTM 显示尺寸**；版心分母用打印 16mm（`PAGE_PAD_MM`），**不用** 1240px 渲染宽当标尺（§2.3）。
- Flate 走 `clone 流 → 删 Subtype → decompressed_content()`，**不引 flate2、不自己写 predictor**（§2.2）。

---

## 14. 实施顺序（批次）

| 批次 | 内容 | 可并行 | 依赖 |
|---|---|---|---|
| **Batch 0**（无行为变化的地基） | ① `Cargo.toml` 加 `lopdf`；② `agent/facts.rs` 骨架（`Origin`/`Target`/`PageMeasure`/阈值/`MEASURE_RULER`/空实现）；③ `types/index.ts` 新字段与 `PageMeasure`；④ `lib/measure.ts` 的 `contentVersion`；⑤ **`docs/CONTRACT.md` 全部 diff**；⑥ `vitest` + `src/lib/measure.test.ts` | ①③④⑥ 可并行；**⑤ 是其余一切的闸门** | 无。**契约先落地并合并**，后面所有人照着写 |
| **Batch 1**（抽图） | `pdfembed.rs` + `tools/extract.rs` + `attach.rs` 改造 + `render::photo_of` 豁免 + 工具注册（**只加 `extract_images`**） | 可与 Batch 2 并行 | Batch 0（依赖 `lopdf`、`Origin`、`Asset.role` 类型） |
| **Batch 2**（事实与工具） | `facts.rs` 全部文案 + `context::locate` 注入 + `describe_page` 实测段 + `set_page_layout` 追加行 + `set_target_pages` 工具（**只加 `set_target_pages`**）+ `ToolCtx.facts` | 可与 Batch 1 并行 | Batch 0；`attach::Origin` 由 Batch 1 提供（可先按结构体写，编译期等它） |
| **Batch 3**（前端测量与门） | provider 离屏宿主 + `usePageMeasure` + 自动自查 + 状态栏徽标 + 导出门 + `panel.tsx` 虚线归一 | 否（都在 provider/panel 一带动，串行更安全） | Batch 0；Batch 2 的 `measure` 字段形状 |
| **Batch 4**（提示词与测试补齐） | `system.md` / `layout.md` / `GUIDE` + 全部 UT + 冒烟 6 条 | 提示词部分可并行 | 前三批 |

**并行建议**：Batch 1 与 Batch 2 交给两个开发（互不碰同一文件：一个动 `pdfembed/extract/attach/render`，一个动 `agent/*` + `tools/layout.rs`）。Batch 3 与 Batch 1/2 也能并行 —— 只要 Batch 0 的契约与类型先冻结，前端可以对着 `PageMeasure` 的接口写，不用等后端。

**必须串行的三个点（硬约束）**：

1. **`docs/CONTRACT.md` 的 diff 必须先合并**（Batch 0），之后 Batch 1/2/3 才能对着它写。否则 §1.2 白名单 / `OP_SPEC` / `Action` 三处会各写各的。
2. **`tools/mod.rs` 的 `TOOLS` 数组只归两个人、各加一个**：Batch 1 只加 `extract_images`（连同 `pub mod extract;`），Batch 2 只加 `set_target_pages`（连同 `ToolCtx.facts`）。禁止一人同时改两条注册 —— `TOOLS` 是同一个数组字面量，两边同时动必冲突。谁先合谁把 `session.len()` 断言推到 39，后合的人推到 40，最终 40。
3. **`provider.tsx` 只归 Batch 3**（离屏宿主、measure、自动自查都在它一个文件里）。

---

## 15. 必须写注释的 3 处（中文，说"为什么"）

1. **`pdfembed.rs`：`probe.dict.remove(b"Subtype")`**
   `// 绕过 lopdf 对图像流的拒解（decompressed_content 见到 Subtype=="Image" 直接 Err）。probe 是局部克隆，文档对象没被改。不这么写就得自己实现 5 种 PNG predictor，那是静默花屏的高发区。`
2. **`provider.tsx`：离屏测量宿主**
   `// 编辑态没有纸面 DOM（面板只在预览分支挂 .paper），而默认视图就是编辑。实测只能从这份离屏实例取；预览那份只管显示，不许另起一套公式。`
3. **`render/mod.rs::photo_of` 的候选豁免**
   `// 跳过 role=="candidate"：PDF 抽出来的图全部登记进 person.assets，不跳的话第一张 logo/水印会立刻变成页头证件照。老数据没有这个键，行为不变。`

---

## 16. 方案冻结声明

- **冻结范围**：§3 数据结构与契约 + §4 全部契约内容（含逐字文案）+ §2.7 目标页数三态 + §2.3 版心口径。冻结后，前端 / 后端内容域 / 后端 agent 域 / 渲染域按此各自实现。
- **不冻结**：`pdfembed` 的解码覆盖范围（按 Batch 1 实测逐类扩）、离屏宿主的样式细节、徽标的具体像素。
- **变更流程**：任何契约变更必须先改 `docs/CONTRACT.md`、再改本文件 §4、然后通知全部受影响开发（会话层 / 排版层 / 渲染层 / 前端），禁止口头约定。
- **交付**：本文件 + `docs/CONTRACT.md` 的 diff 文本（§4.5）。代码由 Batch 0–4 落地。
