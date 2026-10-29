# 技术方案 · 文档形态（能力实测 + 页三形态）· P0

> 输入：team-lead 拍板的 10 条（B-P1…B-P10）、targetPages 路线二更正、A 块 F-A12、以及 `docs/PLAN-photo-fit.md`（仍在有效）。
> 本文件是**实施依据**。契约部分（§3）冻结后，任何变更必须先改 `docs/CONTRACT.md` 再改代码。
> 状态：**P0 定稿（B 块两节 = PENDING-研究，等选型结果）**。
> 作者：高见远（技术方案架构师）

---

## 0. 三条不变量（不依赖选型，现在就定；代码里要能 grep 到）

**I1 · 禁清单只有一份**（`render::sanitize::FORBIDDEN_ALWAYS`，两档共用，任何一档都不许私自放宽一条）：

```
script   on*（任何事件属性）   javascript:   vbscript:
iframe   frame   frameset   object   embed   base
data:text/html
@page     @import
```
`standard` 档**也不例外** —— 它现在只走 schema 受限的白名单，但 `parts` 已经允许局部 HTML 片段，将来标准简历迟早会放宽；那时如果它走的是另一条宽松路径，红线就破了。所以"共用一份禁清单"是给**未来**留的保险。

**I2 · `@page` 只有生成器能写**。预览与打印的纸张、页边距全部来自 `Resume.paper`；模型写的 `@page` 一律清掉。
且这条要与「`@page` 从静态 CSS 变注入」**共用一个注入点**（`src/styles/page-setup.ts`），否则就是两处各写一份 —— 一类是最常见的腐烂方式。

**I3 · 入口唯一，管线可分歧**。
- `compile()` 是**唯一分流点**（按 `Resume.kind`）；
- `sanitize()` 是**唯一清洗入口**，但**按 kind 取策略**（`standard_builder()` / `freeform_builder()`）；
- **不许出现第二个 `sanitize` 调用点** —— 有人图方便在旁边再写一个宽松版本，这条不变量就是为了挡那一手。review 时 grep `sanitize(` 应当只有一处定义 + 每个生成分支一次调用。

另两条数值层面的一处性（沿用上一轮，继续有效）：
- **能力只有一个 `resolve()`**：想知道「能不能看图」只能调它；不许读前端布尔、不许猜错误字符串。
- **页数判定只有一个 `fit_of()`**：阈值 `FIT_MAX / TOLERANCE_MAX` 只在 `agent/facts.rs:226-227`。

---

## 0.1 一页结论

| 块 | 要解决的真问题 | 落点 |
|---|---|---|
| A · 能力实测 | 「这个模型能不能看图」现在靠**用户在设置里手勾一个布尔**（`model-config.ts:47`）。勾错没人发现，勾对也没人验证；模型自己还会说「我大概能看图」 | 设置里那个布尔换成**三态实测**：`supported / unsupported / unknown`，按 `(baseUrl, model, key)` 指纹存 `settings.capability:v1`。Rust 侧**只留一个 `resolve()`**，`materials` / `view_asset` / UI 徽标三处都调它 |
| B · 页三形态 | 现在所有简历都当「我们排版 + 导出 Word」，但用户手里有两类完全不同的东西：参照 Word 排的标准简历，和「我要自己画」的版式稿 | `Resume.kind: standard / freeform / file`。`standard` 保 Word 可编辑；`freeform` 只出打印/PDF、放宽到正常 HTML；`file` 是单份只读原件 |
| 分页 · A＋ | 「实测页数」现在是**高度估算**，与真排版脱钩：字号/字体/表格边框一变，估算就和打印结果分叉 | 预览态改用 **Paged.js 物理页**（权威），编辑态保留估算并标 `estimate`；两者折成同一个 `contentPx = pages × onePage`，**`fit_of` 与全部文案不动** |
| 冲突 · targetPages | 目标挂在整份、判定却是逐页 → 多页简历要么每段都被压到 0.9 页，要么整份无约束 | **路线二：`targetPages` = 每一段的上限页数**。`fit_of` / 自动压版**不动**，只改文案与 `FromOrigin` 的条件；整份只做**展示值**（只统计已测段） |

三条不变量见上一节 §0（I1 禁清单 / I2 `@page` / I3 唯一入口），它们贯穿全文。

---

## 1. 改动地图

### 1.1 数据与迁移

| 文件 | 改什么 | 性质 | 为什么 |
|---|---|---|---|
| `src-tauri/src/model.rs` | `Resume` 加 `kind: String`、`paper: Paper`；新增 `struct Paper { margin: [f64;4] }`；`Workspace::normalize`（:405-416）里加 `resume.normalize()` | 改 | `Resume` 现在的 `extra` flatten 能兜住新键，但 `kind`/`paper` 是要被校验与 clamp 的**一等数据**，不能再当"未知字段"漂着 |
| `src-tauri/src/model.rs` | `impl Resume { fn normalize(&mut self) }`：推断 `kind`、clamp `paper.margin`、纠正不混用 | 新增 | 三个动作都要幂等（反复调用不变形），且**不许删数据** |
| `src/types/index.ts` | `Resume.kind?: ResumeKind`、`Resume.paper?: Paper`、`Resume.targetPagesNotice?: boolean` | 改 | `Resume` 没有索引签名（:78-84），新字段必须显式声明 |
| `src/store/workspace/types.ts` | `Action` 加 `pageBlocks`、`removeEmptyPage`、`resumeNotice` | 改 | 契约三处同改之一 |
| `src/store/workspace/reducer.ts` | 三个新 case，一律 `{...resume}` / `{...page}` 展开 | 改 | ⚠️ 手写重建对象会丢 `extra` 里的键 |
| `src/lib/agent-actions.ts` | `OP_SPEC` 加三条（:184） | 改 | 契约三处同改之二 |
| `src/components/resume/panel.tsx` | 形态切换入口（三个 tab）+ freeform 时 Word 项置灰 + 整份展示值落位 | 改 | B-P2 / B-P5 |
| `src/lib/blocks.ts` | `normalizeWorkspace` 加 `kind`/`paper` 兜底（后端 normalize 之后仍然要能读老数据） | 改 | 前端可能在 Rust normalize 之前就先渲染一帧 |

### 1.2 渲染与白名单

| 文件 | 改什么 | 性质 |
|---|---|---|
| `src-tauri/src/render/mod.rs` | `compile()`（:234）按 `page`/`resume.kind` 分流；两份清洗配置（standard 白名单 / freeform 黑名单）；`@page` 剥离；`shell()` 输出 `--pad-*` 变量 | 改 |
| `src-tauri/src/render/mod.rs` | 常量重组：`FORBIDDEN_ALWAYS`（两档共用的永远禁清单）+ `CSS_PROPS_STANDARD`（现 `CSS_PROPS` 改名）+ `SAFE_ATTRS_FREEFORM`（黑名单式，只列要剥的） | 改 |
| `src-tauri/src/render/paper.css` | `padding: var(--pad)` → `padding: var(--pad-t) var(--pad-r) var(--pad-b) var(--pad-l)`，并用 `--pad` 兜底 | 改 |
| `src/styles/print.css` | **删掉** `@page { margin:16mm }`（:57-59） | 改 |
| `src/styles/page-setup.ts` | 新增：`applyPageSetup(paper)` —— 管理 `<style id="page-setup">`，写 `@page { size:A4; margin: …mm }` | 新增 |
| `src-tauri/src/docx.rs` | :164 的硬编码 `pgMar 1134` 改为按 `paper.margin` 算 twips | 改 |

### 1.3 工具与事实

| 文件 | 改什么 | 性质 |
|---|---|---|
| `src-tauri/src/tools/page.rs` | 新增 `split_page`、`delete_empty_page`（都 Layout scope） | 改 |
| `src-tauri/src/tools/mod.rs` | 注册 2 个工具（Layout 9 → 11）；`assert_eq!(layout.len(), 11)`（:777） | 改 |
| `src-tauri/src/agent/facts.rs` | `target_of`（:376）加「只单段才 FromOrigin」；`target_line` 全部改「这一段最多 N 页」；新增【模型能力】块的产出函数 | 改 |
| `src-tauri/src/agent/context.rs` | 注入点从 1 个变 2 个：`facts::block()` + `facts::capability_block()` | 改 |
| `src-tauri/src/agent/layout.rs` | `describe_page` 首行加「简历形态」；`paper` 传入生成器 | 改 |
| `src-tauri/src/tools/target.rs` | 文案改「这一段最多 N 页」（工具描述 + 返回） | 改 |

### 1.4 前端 UI

| 文件 | 改什么 | 性质 |
|---|---|---|
| `src/components/settings/model-settings.tsx` | 「能看图 · 多模态」开关（:404-410）→ 三态徽标 + 「测一次」按钮 | 改 |
| `src/lib/measure.ts` | `measureTip`（:87-108）加「这一段」；新增 `summarize(pages, cache)` 整份展示值 | 改 |
| `src/store/workspace/provider.tsx` | `pageSetup` effect；`targetPagesNotice` 一次性提示；measure 缓存 Map | 改 |
| `src/components/status-bar.tsx` | 整份展示值（`已测 2/3 段 · 已测合计 2.4 张`） | 改 |
| `src/lib/capability.ts` | 新增加持：`capability_scan` / `capability_probe` 的调用与展示文案 | 新增 |

### 1.5 提示词

| 文件 | 改什么 | 性质 |
|---|---|---|
| `prompts/system.md` | 删 `:107` 前半句；新增「## 模型能力」一节 | 改 |
| `prompts/layout.md` | 加「标准 / 自由」两种口径的分段（按 `describe_page` 首行判定） | 改 |
| `src-tauri/src/render/mod.rs` `GUIDE` | 只加一句 freeform 才有的能力说明（**不许写阈值**，见 §9.4） | 改 |

### 1.6 契约 / 测试

| 文件 | 改什么 |
|---|---|
| `docs/CONTRACT.md` | §1.2 白名单 +3 op、§2 数据模型（kind/paper/targetPagesNotice）、§3 工具清单、§4 Layout 工具 +2、§5 渲染（两档清洗 + `@page` 不变量）、新增 §7 能力 |
| `src-tauri/src/tools/mod.rs` :776-777 | Session 40 不变、**Layout 9 → 11** |
| `src-tauri/src/render/mod.rs` tests | 白名单测试要拆成两份 |
| `src-tauri/src/agent/facts.rs` tests | `{target}` 文案断言全部要改（含「这一段」） |

**无表结构变更**：`Resume.paper` / `kind` 跟 `Resume` 一起 JSON 化落 `db::save`（`db.rs:399-413`），不需要迁移脚本。唯一新增的**表**用法是 `settings` 里多一个 key（`capability:v1`），沿用现成的 `settings_put/get`（`db.rs:596-620`）。

---

## 2. 数据与契约

### 2.1 `Resume.kind` 三态

```rust
// model.rs
/// 这份简历是哪一种文档。**三选一，不许混用**（见 normalize 的纠正规则）。
///
///   standard —— 参照 Word 排：段落/表格，导出 .docx 可继续编辑
///   freeform —— 自由版面：正常 HTML+CSS，只出打印/PDF，Word 置灰
///   file     —— 单份只读原件（1 个 file 页），不改内容不排版
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResumeKind {
    #[default]
    Standard,
    Freeform,
    File,
}

pub struct Resume {
    pub id: String,
    // …
    #[serde(default)]
    pub kind: ResumeKind,
    #[serde(default)]
    pub paper: Paper,
    // …
}
```

> 为什么用 enum 而不是 `String`：`kind` 决定**导出能力**（freeform 禁 Word）与**清洗策略**（黑名单 vs 白名单）。字符串错一个字，就会静默走进标准分支并把 freeform 页面清洗成空白 —— 那是"用户的心血变成一张白纸"。enum 让这个错误在反序列化时就失败。
> 代价：未知 kind 会让整份 Workspace 解析失败。所以 `normalize` 之前必须**先宽容读**：见 §2.4 的 `from_str` 兜底。

### 2.2 `Resume.paper`

```rust
/// 纸张与页边距。**毫米**，顺序 `[上, 右, 下, 左]`（与 CSS `margin` 简写同序）。
///
/// 为什么单独一个结构而不是塞 4 个 extra 键：页边距要三处同源
/// （预览 px / 打印 @page / docx twips），必须有**一处**能拿到完整四值，
/// 而不是在三个地方各自 `resume.marginTop ?? 20`。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paper {
    pub margin: [f64; 4],
}

impl Default for Paper {
    fn default() -> Self {
        // 20mm：与 Word 的默认页边距一致（docx.rs 的 1134 twips 就是 20mm）。
        // 16mm 只是当年 print.css 里打印用的，不是产品默认。
        // 最终形状见 §15.3 / 契约 §2：还多了 size / widthMm / heightMm。
        Self { margin: [20.0; 4] }
    }
}

pub const MARGIN_MIN_MM: f64 = 5.0;
pub const MARGIN_MAX_MM: f64 = 50.0;
pub const A4_W_MM: f64 = 210.0;
pub const A4_H_MM: f64 = 297.0;
```

```ts
// src/types/index.ts
export type ResumeKind = "standard" | "freeform" | "file";

export interface Paper {
  /** 毫米，`[上, 右, 下, 左]`，与 CSS margin 简写同序 */
  margin: [number, number, number, number];
}

export interface Resume {
  // …
  kind?: ResumeKind;
  paper?: Paper;
  /** 一次性提示已经看过：旧的多页简历带着「整份 1 页」的老目标 */
  targetPagesNotice?: boolean;
}
```

前端**用元组不用具名对象**：四值在打印、`--pad-*`、twips 三处都要按顺序用，具名字段会诱使人写成 `left: paper.margin.left`（不存在）而 Runtime undefined —— 元组配一个 `[T,R,B,L]` 注释更难出错。

### 2.3 迁移：`Workspace::normalize` 的位置与规则

**位置**：`model.rs:405-416`，在现有 `page.normalize()` 循环**之前**加一层简历级循环：

```rust
pub fn normalize(&mut self) {
    self.version = VERSION;
    for person in &mut self.people {
        for resume in &mut person.resumes {
            resume.normalize();            // ← 新增（kind 推断 + paper clamp）
            for page in &mut resume.pages {
                page.normalize();
            }
        }
    }
}
```

**`Resume::normalize` 的三条规则（顺序固定、单次幂等）**：

```
1) paper：每个值 clamp 到 [5, 50]（NaN/负数 → 20，0 → clamp 到 5，那是「我要贴边」的意图）
   然后若 (上+下) > 297-20 或 (左+右) > 210-20，按比例缩到上限（等比，不动比例关系）
   —— 缩完正好等于上限，第二次调用不再变 → 幂等

2) kind 缺失/不合法时推断（只在缺失时推断，已有值不覆盖）：
     pages.is_empty()                            → Standard
     pages.len() == 1 && pages[0].kind == "file" → File
     所有页 kind == "file" && len > 1            → File（多页原件：逐页可看、不可编辑）
     其它                                        → Standard

3) 纠正混用（**不删任何页**）：
     kind == File 且存在非 file 页   → kind = Standard
                                     （页留着；file 页在 standard 里合法，就是"材料页"）
     kind != File 且存在 file 页     → kind = Standard
                                     （freeform 是"自己画的版"，塞一张整页原件进去等于两张纸叠一起；
                                       降级到 standard 不会丢任何内容）
```

**`file` 档的多页原件（已拍板）**：
- **允许逐页预览全部页**（页面列表照常可切、每个 file 页各自渲染自己的 `<img>`）——
  一份 2 页扫描件只给看第 1 页，用户会以为文件坏了。
- **不可编辑**：页内容、页头字段、分区、`layout` 全部只读；UI 常驻提示 `这是一份由 {n} 个原件组成的简历，不可编辑`。
- **导出逐页原样拷贝**（现状 `exportResumeFolder` 已支持），Markdown 也照旧逐页转。
- **其余页不删、不合并**——没有"把两张原件合成一页"这种操作。

**「恰 1 个 file 页 → file」是显式规则**，不是推论：老数据里用户上传一整页扫描件当简历（`addFilePage`，`panel.tsx:481-506`）—— 那种简历只有一页且就是 file 页，它是 `file` 档，不是 `standard` 档。

> **必须注释的第 1 处**（`Resume::normalize`）：
> `// 纠正混用只降级 kind、不删页。删页是不可逆的：用户可能只是切错了档，回来的路上不该发现内容没了。`
> `// 反过来也不行 —— 不能因为发现一个 file 页就把整份升成 Freeform，那会顺手关掉用户的 Word 导出。`

**宽容读**：`ResumeKind` 用 `#[serde(other)]`？——不。serde 的 `other` 只对 unit variant 生效且会把未知值吞成默认。这里用显式两步：
```rust
#[derive(Deserialize)]
#[serde(untagged)]
enum KindRaw { Known(ResumeKind), Unknown(String), Missing }
```
实际落地更省：`kind` 先反序列化成 `String`（`#[serde(default)]`），`normalize` 里 `match s.as_str() { "standard"|"freeform"|"file" => …, _ => 推断 }`。**一个 String 字段 + 一次 match 比自定义 Deserialize 便宜**，且遇到的未知值会走"推断"这条安全路径。
→ 所以 §2.1 的 enum 只用于**渲染层的内部参数**（`fn compile(page,&person,kind:ResumeKind)`），存储仍是 String。这条要写清楚，否则开发会做成"存储 enum + 一键解析失败整库读不出"。

### 2.4 `settings.capability:v1` 形状与指纹

**指纹**（不存密钥本体）：
```
fp = hex(sha256(format!("{baseUrl}\n{model}\n{apiKey}")))[..16]
```
- 换 key → 指纹变 → 不命中 → `unknown`（自然失效，不需要清理逻辑）。
- 同 key 不同模型 → 不同指纹。baseUrl 也进指纹：同一个模型走不同网关，能看图与否可能不同。
- 沿用现成的 `pool::sha256_hex`（已在树里），不新增依赖。
- **只存前 16 位**：它不是密钥，但要避免"设置表泄露即密钥泄露"。16 hex = 64 bit，碰撞概率对本用途可忽略。

**形状**（`settings` 表，key = `capability:v1`）：
```json
{
  "version": 1,
  "items": {
    "3f2a9c1d4b5e6f70": {
      "state": "supported",          // supported | unsupported | unknown
      "reason": "",                  // "" | protocol-reject | bad-request | offline | timeout | no-probe
      "checkedAt": 1765000000,       // unix 秒；只用于界面显示「3 天前测过」
      "model": "qwen-vl-max",
      "baseUrl": "https://…/v1"
    }
  }
}
```
- `state` 与 `reason` 都存：`unsupported` 的原因分 `protocol-reject`（强证据）与人工判定的 `no-probe`（没测过）。
- **`unknown` 也存**：否则每次打开都显示"没测过"；存下来才能显示「上一次探测答错了，没得出结论」。
- 失效：**不设 TTL**。理由：能力是"这个模型这一版能不能收图"，不会自己变；真有变化（网关升级）时用户手点一次测试就覆盖。加 TTL 只会让状态在"测过"和"没测过"之间跳。
- 读不到 / 表空 / 解析失败 → **一律 `unknown`**，不当作 `unsupported`（见 §5.4 的代价不对称）。

### 2.5 三个新 op

```json
{ "type": "pageBlocks", "personId":"p1", "resumeId":"r1", "pageId":"pg2", "blocks":[ … ] }
{ "type": "removeEmptyPage", "personId":"p1", "resumeId":"r1", "pageId":"pg9" }
{ "type": "resumeNotice", "personId":"p1", "resumeId":"r1", "key":"targetPagesNotice" }
```

| op | 用途 | 确认门 | 说明 |
|---|---|---|---|
| `pageBlocks` | **整页 blocks 全量替换** | 否 | 现在只有 `sectionItems` 这类"容器级"替换，没有"页级"替换。`split_page` 与将来的 XML DSL 都需要它 |
| `removeEmptyPage` | 删空页 | **不进**确认门 | 判据是"不可撤销才问"：空页没有内容可丢。双保险 —— 工具校验 + reducer 再校验一次空 |
| `resumeNotice` | 记一个"已看过"的标记 | 否 | 只认白名单 key（当前仅 `targetPagesNotice`） |

> 为什么 `resumeNotice` 不塞进 `resumeTargetPages` 里当 `notice: true`：现有 `cleanOp` 只校验 `ids/arrays/objects`，不校验布尔。一旦复用，`{type:"resumeTargetPages", targetPages 缺省}` 的语义是**清除目标**（见 `target.ts` 的实现），ack 一条提示会顺手把目标页数清掉。**宁多一个 op，不要让 ack 有副作用。**

### 2.6 两档清洗的常量组织

```rust
// render/sanitize.rs（从 render/mod.rs 拆出来）

/// **I1：禁清单只有一份**，两档共用。写在这里就再也没有"某一档放开"的选项。
/// 只增不减。加一条要同时想清楚：它在 standard 的白名单里是不是也被挡住了？
pub const FORBIDDEN_ALWAYS: &[&str] = &[
    // 脚本
    "script",
    // 嵌套文档：绕过一切限制的后门
    "iframe", "frame", "frameset", "object", "embed",
    // 能改所有相对 URL 的指向
    "base",
];

/// 永远禁的属性形态：任何 `on*`（= script）、以及 style 里的 `url(`（PENDING 核实）。
pub const FORBIDDEN_ATTR_PREFIXES: &[&str] = &["on"];

/// 永远禁的 URL 协议。
pub const FORBIDDEN_SCHEMES: &[&str] = &["javascript", "vbscript"];

/// 永远禁的 data: 子类型（`data:image/*` 放行，`data:text/html` 是注入面）。
pub const FORBIDDEN_DATA_TYPES: &[&str] = &["text/html", "application/xhtml+xml"];

/// 永远禁的 at-rule（**I2**）。两档都要剥，不只 freeform。
pub const FORBIDDEN_AT_RULES: &[&str] = &["@page", "@import"];

/// **I3：唯一清洗入口**。`kind` 只决定"用什么策略"，不决定"洗不洗"。
pub fn sanitize(html: &str, kind: ResumeKind, warnings: &mut Vec<String>) -> String {
    match kind {
        ResumeKind::Standard => standard_builder(),
        // freeform / file 都走最宽的那档（file 的内容本来是 <img>，无需更严）
        _ => freeform_builder(),
    }
    .clean(html)
    .to_string()
    // 清洗后再跑一遍 I1/I2 的兜底扫（`<style>` 内容的 at-rule、`on*` 的变体）
    .pipe(|s| strip_forbidden_at_rules(&s, warnings))
    .pipe(|s| strip_event_attrs(&s))
}

/// standard：沿用现有白名单（render/mod.rs:40 的 TAGS / :82 的 CSS_PROPS 改名而来），
/// 但**先把 FORBIDDEN_ALWAYS 从 TAGS 里减掉**，保证"白名单里永不含禁清单"。
pub fn standard_builder() -> ammonia::Builder<'static> { … }

/// freeform：黑名单式 —— 只剥 I1 + I2，其余放行。
pub fn freeform_builder() -> ammonia::Builder<'static> { … }
```

**三条不变量在这里落地**：
1. `FORBIDDEN_ALWAYS` / `FORBIDDEN_SCHEMES` / `FORBIDDEN_DATA_TYPES` / `FORBIDDEN_AT_RULES` 只增不减，两档共用；`standard_builder()` 的白名单**由禁清单减去**而来，不允许"白名单里恰好也有一条禁项"。
2. `@page` / `@import` 剥离发生在 `sanitize()` 的**统一后置步**（`strip_forbidden_at_rules`），不只在 freeform 分支 —— 这样将来 standard 允许局部 CSS 时自动生效。
3. `sanitize()` 是唯一入口，`compile()` 是唯一分流点。**grep `sanitize(` 应当只有一处定义 + 每个生成分支一次调用**；出现第二个宽松版本就是破线。

> ⚠️ **本节的细节（freeform 精确禁清单、`<style>` 内容清洗实现、CSP 兜底）属 PENDING-研究**，见 §11。上面给的是**结构**，不是最终规则。

---

## 3. 页边距单一来源（三处同源）

**唯一源**：`Resume.paper.margin: [f64;4]`，单位 **mm**，顺序 `[上,右,下,左]`。

### 3.1 预览（px）

基准宽度 374px（`.wrap`，`paper.module.css:19`），A4 宽 210mm：
```
mm_to_px(v) = v * 374.0 / 210.0        // 16mm → 28.50px
```
机制：**生成器输出 CSS 变量，paper.css 消费它们**，而不是输出 `padding` 简写。

```rust
// render/mod.rs::shell()
let [t, r, b, l] = page_paper.margin;   // mm
let px = |v: f64| v * 374.0 / 210.0;
format!(
    r#"<article class="doc" data-preset="{preset}" style="--pad-t:{:.2}px;--pad-r:{:.2}px;--pad-b:{:.2}px;--pad-l:{:.2}px">{body}</article>"#,
    px(t), px(r), px(b), px(l)
)
```
```css
/* render/paper.css */
.doc {
  --pad: 34px;                                  /* 兜底：老数据/没传 paper 时 */
  --pad-t: var(--pad); --pad-r: var(--pad);
  --pad-b: var(--pad); --pad-l: var(--pad);
  padding: var(--pad-t) var(--pad-r) var(--pad-b) var(--pad-l);
}
@media print { .doc { padding: 0 } }            /* 打印留白交给 @page，保持现状 */
```

**为什么用变量而不是直接写 `padding`**：`@media print` 里的 `padding: 0` 必须继续生效（打印留白由 `@page` 提供，否则会两边叠加变成 32mm）。**内联 `padding` 的优先级高于样式表，会把打印规则顶掉**；内联自定义属性则不会 —— 打印规则设的是 `padding` 简写，与 `--pad-*` 无关。

**为什么基准是 374 而不是实际 offsetWidth**：`.wrap` 有 `zoom`（`panel.tsx` 预览分支），实际像素随缩放变；CSS px 在 zoom 容器内仍按 374 计算，所以用 374 是**唯一稳定值**。这条与 `facts.rs` 的 `MEASURE_RULER`（`正文宽 306px、页边距 34px、预览容器宽 374px`）同源。

### 3.2 打印（`@page`）

现状：`print.css:57-59` 静态 `@page { margin:16mm }`，且**没有 `size:A4`**。

改动：
1. **删掉** `print.css` 里那条 `@page`（留着会与注入的那条重复，谁生效取决于样式表顺序，不可控）。
2. 新增 `src/styles/page-setup.ts`：

```ts
const ID = "page-setup";

/** 纸张与页边距的唯一写入点。任何地方都不许再拼 @page。 */
export function applyPageSetup(paper?: Paper): void {
  const [t, r, b, l] = paper?.margin ?? [16, 16, 16, 16];
  let el = document.getElementById(ID) as HTMLStyleElement | null;
  if (!el) {
    el = document.createElement("style");
    el.id = ID;
    document.head.appendChild(el);
  }
  // size 必须显式写：没有它，Chrome 会按"用户上次选的纸型"打，
  // 用户在打印对话框里选 Letter，我们的 mm 边距就全错位了。
  // 四条边写 longhand 而不是 margin 简写：简写要按 [上,右,下,左] 排，
  // 排错的后果是"边距看着生效了但方向反了"——谁也不容易看出来；longhand 顺带让断言能直接 grep。
  el.textContent =
    `@page { size: A4;` +
    ` margin-top: ${t}mm; margin-right: ${r}mm; margin-bottom: ${b}mm; margin-left: ${l}mm; }`;
}
```

3. `provider.tsx` 里一个 effect：`useEffect(() => applyPageSetup(resume?.paper), [resume?.id, resume?.paper])`。**挂在 provider 而不是面板**：打印、导出、离屏测量三处都要用同一份，挂面板会在卸载时丢掉。
4. `printPage()`（`lib/export.ts:66-68`）不改 —— 它只 `window.print()`，样式已由 effect 常态维持。

**验收（可执行，team-lead 指定 + 我补两处）**：
```
① applyPageSetup({ margin:[20,16,16,16] }) → 生成的 style 文本
   同时含 "size: A4" 与 "margin-top: 20mm"（另三条同理）
② grep "@page" src/styles/print.css → 必须为空（旧那条删干净）
③ grep -rn "@page" src/ → 只允许命中 src/styles/page-setup.ts 一处（I2 的机械判据）
④ 打印预览实测：把 margin 设成 [40,40,40,40] 打一份 → 四边留白明显变大，
   且与预览里的 `--pad-*` 视觉一致（预览 40mm 会显得很空，这正是"同一份 paper"的证据）
```
第 ③ 条是关键：它把「`@page` 只有生成器能写」从文档约定变成一条能 grep 的红线。

### 3.3 docx（twips）

```
mm_to_twips(v) = round(v * 1440.0 / 25.4)     // 16mm → 907；20mm → 1134
```
`docx.rs:164` 现在是字面量 `w:pgMar w:top="1134" …`。改为从 `paper` 算：

```rust
let [t, r, b, l] = paper.margin;
let tw = |v: f64| (v * 1440.0 / 25.4).round() as i64;
format!(r#"…<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="{0}" w:right="{1}" w:bottom="{2}" w:left="{3}"/></w:sectPr></w:body></w:document>"#,
        tw(t), tw(r), tw(b), tw(l))
```
`pgSz` 保持 11906×16838（A4 20 半点）—— P0 不做纸型切换（`paper` 只有 margin）。

### 3.4 三处一致性断言（可执行）

```
assert_eq!(mm_to_px(16.0), 28.50);            // ±0.01
assert_eq!(mm_to_twips(16.0), 907);
applyPageSetup([16;4]) → style 文本含 "margin: 16mm 16mm 16mm 16mm" 且含 "size: A4"
```

---

## 4. `split_page` 与 `delete_empty_page`

### 4.1 `split_page`

```json
{ "pageId": "pg1", "afterBlockId": "sec-work" }
```
scope：**Layout only**（B-P3：排版 agent 才有增删页的授权；会话侧保持 `add_page`/`delete_page` 现状）。

**语义（逐条定死）**：

| # | 规则 |
|---|---|
| 1 | 新页插在**源页紧后面**（不是简历末尾）—— 拆页是"这一页太长"，拆出来的下半张当然紧邻 |
| 2 | `afterBlockId` 必须存在于源页；**是最后一块时报错**：`这一块后面没有内容，切出来会是空页。要给这一页开一张空白页，用 add_page`（切空页 = 变相 add_page，而 add_page 明确不做：空页 agent 填不了） |
| 3 | 块的归属：`afterBlockId` 及其**之前**留在源页；**之后**的块按原顺序进新页 |
| 4 | `layout.parts` 随块走：搬走的每个 `blockId`，把 `源页.layout.parts[blockId]` 移到 `新页.layout.parts[blockId]`，并从源页删除。**没搬的块的手写片段不许动** |
| 5 | `preset` 继承：新页 `layout.preset = 源页.layout.preset`（同档疏密，视觉上像一张纸断开而不是换了模板）。`layout.html` **不继承** |
| 6 | 源页有整页手写 `layout.html` → **报错**：`这一页是整页手写，切块没有意义。先 reset 掉手写再切，或者直接改 HTML` |
| 7 | `notes`（批注）**留在源页**：批注是过程记录、贴的是"当时那块"，跟着块跑到新页会让用户找不到 |
| 8 | 一次 turn 只许 1 次：`Outbox.splits: AtomicUsize`，超了报错（不是静默丢弃）。理由同 `MAX_NOTES_PER_TURN`：一轮里连切三刀，模型自己也算不清哪块在哪页 |
| 9 | 总页数上限 `MAX_PAGES = 12`（含 file 页）。超了报错：`页数已经 12，再切就管不动了。先删掉空页或合并内容` |
| 10 | 返回文案要能让模型下一轮引用新页 id：`已把第 2 块之后的 3 个块切到新页「第 2 页」（页 id: pg-new-1）` |
| 11 | **A＋ 之后返回值要带页数效果**（模型靠它验证这一刀有没用）：`切完这两段各占：第 1 段 1.00 页、第 2 段 0.32 页（按当前版面实测）`。数据来自 `segPages` + `fill`（§11.3）；**预览态才有真值**，编辑态只能写「切完再看预览确认」 |
| 12 | **为什么不改成"在物理页边界处切"**：① 物理页边界可能落在**块内部**，而数据模型没有"半个块"，切了就是丢内容；② `afterBlockId` 是**内容级**语义（换字体、换预设都不会变），物理页边界是**渲染级**结果 —— 拿渲染结果当编辑操作的输入，等于让排版去决定内容边界。所以参数不变，只把"切完占几页"报回去 |

**产物（三/四条 op，顺序即语义）**：
```
1. addPage   { personId, resumeId, id: <预生成>, label: <"第 N+1 页">, open: false }
2. pageBlocks{ …, pageId: <新页>, blocks: <搬走的块> }
3. pageLayout{ …, pageId: <新页>, layout: { preset, parts: <搬来的> } }
4. pageBlocks{ …, pageId: <源页>, blocks: <剩下的> }
5. pageLayout{ …, pageId: <源页>, layout: { preset, parts: <去掉搬走的> } }
```
`addPage` 必须排第一（否则 `pageBlocks` 的前端 reducer 找不到容器）。`open` 给 false：不抢用户当前页 —— 拆页后用户要看的还是原来那一页的下半张，不该被弹到新页。

### 4.2 `delete_empty_page`

```json
{ "pageId": "pg3" }
```
scope：Layout only。

**判据（两条都要空）**：`blocks.is_empty()` 且 `notes.is_empty()`。
- 不空时报错必须带数：`这一页还有 3 个块，删了内容就没了。要清空内容请让会话 agent 处理，或者手动删块`
- 成功：`已删掉空页「第 3 页」`。
- 最后一页不许删：`这是最后一段，删了这份简历就没有页了`（现有 `delete_page` 也有同样的保护思路，见 `panel.tsx:426` 的 `sole`）。
- 产物：`removeEmptyPage` op（不进确认门，理由见 §2.5）。

**双保险**：前端 reducer 收到 `removeEmptyPage` 时也要判一次空；不空则**整条丢弃并 console.warn**。理由：op 可以来自旧版本 agent、也可以来自确认窗重放，工具侧校验挡不住所有入口。

---

## 5. 能力探测（A 块）

### 5.1 唯一入口

```rust
// src-tauri/src/capability.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State { Supported, Unsupported, Unknown }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason { None, ProtocolReject, BadRequest, Offline, Timeout, NoProbe }

pub struct Capability { pub state: State, pub reason: Reason, pub checked_at: Option<i64> }

/// **唯一的**「这个模型能不能看图」判定入口。
///
/// 只读 settings，不发请求：探测是显式动作（设置页按钮 / 收图前的一次性探针），
/// 判定是纯函数。把探测塞进 resolve 会让每轮对话都可能多一次模型调用。
pub fn resolve(base_url: &str, model: &str, api_key: &str) -> Capability;

/// 探测：画一张色块图，问它颜色。写回 settings。
pub async fn probe(base_url: &str, model: &str, api_key: &str) -> Capability;

/// 协议层明确拒图时改判（F-A12）。**可逆**：后写胜。
pub fn mark_protocol_reject(base_url: &str, model: &str, api_key: &str) -> Capability;

pub fn fingerprint(base_url: &str, model: &str, api_key: &str) -> String;
```

**三处同源**（都调 `resolve`，不许各判各的）：

| 处 | 现在 | 之后 |
|---|---|---|
| `attach::materials`（`vision: bool` 入参，决定 PDF 要不要渲染、图片直传还是 OCR） | 前端 `findModel(ai.model)?.vision`（`chat/panel.tsx:324`）传上来 | `chat_send` 内部 `resolve(&args.base_url, &args.model, &args.api_key)`，**删 `SendArgs.vision`** |
| `tools::view`（`ctx.out.vision`，`view.rs:43` 拒绝非视觉模型） | 同上 | `Outbox.capability = resolve(...)`（在 `chat_send` / `run_turn` 各写一次，与 `vision` 同一个位置） |
| UI 徽标 | `settings/model-settings.tsx:406-410` 的手勾开关 | 新命令 `capability_of(conf)` → 读同一份 settings |

`Outbox.vision: bool` **保留但改为派生**：`out.vision = capability.state == Supported`。这样工具侧不用改一遍（`view.rs` 只读布尔），但真相只有一个。

### 5.2 探针图（不引新依赖）

用已有的 `image 0.25`（`Cargo.toml:67-71`，已开 png）+ `base64 0.22` 现成：画 **3×3 共 9 个色块**，每块 64×64，从 6 个高对比色里随机取（红/绿/蓝/黄/黑/白），白底黑框分隔。

```
img = RgbImage::new(192+4*4, 192+4*4)          // 每块 64 + 4px 间隙
for (i, color) in truth.iter().enumerate()     // truth: Vec<&'static str>，9 个色名
    fill_rect(at(i), color)
png = encode(img) → data URL
```
**ground truth 随机**：写死一张图会让"模型背答案"，也让"缓存了这张图的网关"显得能看图。

**请求形状**（与线上完全一致，这是重点 —— 探针必须走**真实的多模态消息形状**，否则测的不是同一条路）：
```json
{ "messages": [ { "role": "user", "content": [
    { "type": "text", "text": "这张图是 3 行 3 列的色块，按「第1行从左到右, 第2行…, 第3行…」的顺序，用 红/绿/蓝/黄/黑/白 六个词回答，逗号分隔，不要解释。" },
    { "type": "image_url", "image_url": { "url": "data:image/png;base64,…" } } ] } ],
  "max_tokens": 60, "temperature": 0 }
```
复用 `net::AiRequest` / `ChatMessage::user_with_images`（`net.rs:59`），不新写 HTTP。

### 5.3 判定伪代码

```
fn probe(...) -> Capability:
    (truth, data_url) = make_probe()                 // 9 个随机色名 + png data url
    resp = chat_once(req_with_image)                 // 不开 tools，不看正文以外的任何字段
    match resp:
      Ok(text) => {
          // 解析：按行切、按逗号切，取前 9 个词，映射到规范色名
          got = parse_colors(text)                   // 认不出来 → 空
          if got.len() == 9 && got == truth:
              Supported                                 // 答对了才算"能看图"
          else if got.len() >= 6 && overlap(got, truth) * 2 >= got.len():
              Unknown(NoProbe)                          // 看着像在描述图但数错了/顺序错了
          else:
              Unknown(NoProbe)                          // 答非所图、拒答、说了"我看不到图"
      }
      Err(e) if e.kind == ImageRejected  => Unsupported(ProtocolReject)   // 强证据
      Err(e) if e.kind == Offline/Timeout => Unknown(e.reason)            // 网络问题不判能力
      Err(e) if e.kind == Auth/Quota/NotFound => Unknown(e.reason)        // 配置错，与能力无关
      Err(e) => Unknown(BadRequest)                                        // 其余一律不判
    write_settings(fp, cap)                          // unknown 也写：界面要能显示"答错了没结论"
```

**为什么"答错"只能 `unknown`**（这段注释**逐字**照需求方原文落进 `probe()` 函数族，不许改写）：

```rust
// 探针答错 ≠ 不支持，必须保持 unknown。
// 三种情况在协议层长得一模一样：模型没有视觉 / 网关把 image_url 吞了 / 模型偶发降智数错了色块。
// 误判成 unsupported 的代价：materials 从此不再给图、view_asset 永久拒绝，
// 用户再也拿不回看图能力，而且界面上写的是"这个模型看不了图"——那是我们编的。
// 长期显示"没测出来"的代价：难看，最坏是用户多点一次测试。
// 两者代价不对称，所以宁可显示没测出来。
// 唯一的例外是协议层明确拒绝（classify_error 的 ImageRejected）：强证据，且必须可逆。
```

**为什么再加一条"半对也算 unknown"而不是直接 unknown**：其实两者结果相同（都是 `Unknown(NoProbe)`），那一支只用来**记一条日志**（`[capability] 半对：got=… truth=…`），方便判断"是不是模型能看图但数不清"。不引入新状态。

### 5.4 三态落库与失效

- 写入点只有两个：`probe()`（用户点测试）与 `mark_protocol_reject()`（F-A12）。
- 读点只有 `resolve()`。
- 失效：换 key / 换模型 / 换 baseUrl → 指纹不命中 → `unknown`；手工"重新测一次"→ 覆盖（**后写胜**）。
- `unknown` 与"没有记录"在界面上文案相同（`没测出来`），但在数据上区分（前者有 `checkedAt`，能显示"3 天前测过、答错了"）。

### 5.5 F-A12：协议层拒图的归因与改判

**错误分类**（新增纯函数，`net.rs`）：
```rust
pub enum ErrorKind {
    Auth,          // 401 / 403
    Quota,         // 402 / 429
    NotFound,      // 404（模型名错）
    Offline,       // 连接失败 / DNS
    Timeout,
    ImageRejected, // ← 新增：400 且错误体明确指向图像字段
    BadRequest,    // 400 其它
    Server,        // 5xx
}

pub fn classify_error(status: u16, body: &str) -> ErrorKind {
    // ImageRejected 判据（全部满足才算）：
    //   status == 400
    //   && body 命中 [image_url | "image" | "vision" | "multimodal" | "content[].type"] 之一
    //   && body 命中 [not support | unsupported | invalid | unknown | unrecognized] 之一
    // 只命中一半 → BadRequest（宁可不错判）
}
```

**为什么分类函数只在 `net.rs` 内部用、不把 `ErrorKind` 传遍调用栈**：只有 `ImageRejected` 一支需要「做一件事」（改判能力），其余六支只需要「说一句话」。为此改 `Result<String, String>` 为 `Result<_, TurnError>` 会波及 `session.rs` / `agent/layout.rs` / `agent/turn.rs` 三处签名与十几处 `?`。**在唯一知道 status+body 的地方（`net.rs` 发请求处）就地改判，是最小改动**。

**改判规则（逐条）**：

| 场景 | 分类 | 改判 vision | 用户可见错误必须含 |
|---|---|---|---|
| 400 + 图像相关词 + 不支持类词 | `ImageRejected` | `unknown → unsupported / protocol-reject` | 「这个模型不接受图像输入」+「去设置里测一次多模态」 |
| 400 但与图像无关（messages 格式 / base64 坏 / 请求体过大） | `BadRequest` | **不改** | 原始错误摘要 |
| 400 + 图像词 + 但不含"不支持"类词（例如 `image too large`） | `BadRequest` | **不改**（超限不是不支持，压缩后能过） | 原始错误摘要 |
| 429 / 5xx / 超时 / 连接失败 | 各自 | **不改**（服务端故障不是能力） | 原始错误摘要 + 重试提示 |

**可逆**：`probe()` 得到 `supported` 时**无条件覆盖**（后写胜）。`mark_protocol_reject` 只写 `unsupported`，不写"永久禁止"。

**收敛**：连续 3 轮都撞 `image-rejected` → 第 1 轮就改判 → 之后 `out.vision = false`（派生自 `resolve()`）→ 不再发图 → 不会再有第 2 次。**不需要额外计数器**：改判本身就让这条路径一次收敛。

### 5.6 【模型能力】事实块

**与【页面事实】同一注入点**（`agent/context.rs::locate`，紧接 `facts::block` 之后）：

```rust
pub fn capability_block(cap: &Capability) -> String   // agent/facts.rs
```

三态文案（逐字）：

```
【模型能力】下面这一行是程序实测的，不是猜的。关于"能不能看图"，只能照抄这一行。

支持图像输入。你可以直接看图；用户提到图里的内容时按图作答。
```
```
【模型能力】支持状态：这个模型这次实测**不支持**图像输入（{原因}）。不要声称能看图；需要图片内容时，请用户把内容写成文字，或者换一个能看图的模型。原因只有两种说法：「这个模型不接受图像输入」或「没测出来」——不要自己编第三种。
```
```
【模型能力】支持状态：**没测出来**。既不要声称能看图，也不要断言看不到 —— 直接说「这一项还没测出来」，并建议用户去 设置 → 模型 点一次「测一次多模态」。
```

`{原因}` 只在 `unsupported` 时出现，取值：
- `protocol-reject` → `模型在协议层明确拒收图像`
- `no-probe` → `还没测过`（理论上 unsupported 不会是这个原因，留作兜底）

> 为什么把"能不能看图"也做成事实块而不是塞进提示词：与【页面事实】同理 —— **能力是实测值，会变**（换 key、换模型、网关升级）。写进提示词就是写死一个可能过期的断言。

---

## 6. freeform 的渲染路径（结构部分；规则细则 PENDING）

### 6.1 `compile()` 的分流点（唯一）

```rust
pub fn compile(page: &ResumePage, person: &Person, kind: ResumeKind, paper: Paper) -> Compiled {
    match kind {
        ResumeKind::Freeform => compile_freeform(page, person, paper),
        ResumeKind::File      => compile_file(page, person, paper),
        ResumeKind::Standard  => compile_standard(page, person, paper),   // 现有逻辑原样搬进来
    }
}

fn compile_freeform(page, person, paper) -> Compiled {
    // 与现有「整页手写」同一条路：layout.html 就是 freeform 的文档本体
    let raw = page.layout.html.as_deref().unwrap_or("").trim();
    if raw.is_empty() {
        return Compiled { html: shell_with_margin(EMPTY_FREEFORM_HINT, paper), css: PRESET_CSS,
                          mode: "manual", preset: "custom",
                          warnings: vec!["这一页还没有版面：让排版 agent 写一版".into()] };
    }
    // 只剥黑名单 + @page；不套 preset 的白名单
    let cleaned = sanitize_freeform(raw, &mut warnings);
    Compiled { html: shell_manual(&cleaned, paper), css: PRESET_CSS, mode: "manual",
               preset: "custom", warnings }
}
```

三条要点：
1. **复用 `layout.html`，不新增字段**。freeform 的"文档本体"就是整页 HTML，现有 `PageLayout.html` 已经是这个语义（"存在时生成器完全不介入"）。新增 `freeform_html` 只会让"到底哪份生效"变成两处判断。
2. **`shell()` 不再强塞 `data-preset`**：freeform 的 `preset` 恒为 `custom`（`PRESETS` 里已有，`render/mod.rs:33`），且不套 `--fs/--lh` 的档位（用户的 CSS 说了算）。但 `--pad-*` 仍然给（页边距是设置层的，不是排版层的）。
3. **`paper.css` 的 `.doc` 基础样式仍然生效**（字体、颜色、`word-break`、`padding`）—— 这就是"正常 HTML"的合理默认，用户要覆盖就在自己的 CSS 里覆盖。

### 6.2 `layout.html` 复用的三个副作用（必须逐条处理）

| 能力 | 现状（standard） | freeform 下变成什么 | 处理 |
|---|---|---|---|
| `describe_page` | 输出块形状（页头/分区/条目/role）+ 实测段 | `layout.html` 存在时**块形状毫无意义**（生成器不介入）；但实测段仍然有用 | `describe_page` 检测 `kind==Freeform`：块形状段换成「整页手写 HTML，{n} 字；块结构不参与渲染」，实测段照旧 |
| 批注 `note_on_page` | 贴到 `data-block` 上 | 手写 HTML 里没有 `data-block`，`blockId` 校验会全部失败 | **freeform 下 `note_on_page` 只能不传 `blockId`**（页边批注）。传了就报错并解释：`这一页是自由版面，没有块锚点；批注会贴在页边` |
| 测量 | `--pad-*` + `.doc` 高度 | 一样（`.doc` 仍是容器） | 无需改。但**必须验证**：freeform 常用 `position:absolute`，绝对定位元素**不撑高父元素** → 溢出可能量不到。见 §10 风险表 |

### 6.3 `@page` 不变量（模型写的一律剥掉）

- `standard`：`style` 属性只能写 `CSS_PROPS_STANDARD` 里的属性，`@page` 是 at-rule、写不进属性值 → 天然不可能。
- `freeform`：`<style>` 内容要专门处理 —— 统一后置步 `strip_forbidden_at_rules()` 剥掉 `@page { … }` 与 `@import`，并记 warning：`已剥掉 @page：纸张和页边距由设置决定`。
- 生成器：`shell_manual()` **不输出** `@page`；`@page` 只由 `src/styles/page-setup.ts` 写。
- 这条写成不变量放进 `docs/CONTRACT.md` §5，并配一条 UT（§9.3）。

### 6.4 freeform 下 `set_page_layout` 的返回加成（已拍板）

**问题**：`position: absolute | fixed` 的元素**不撑高父元素**。freeform 的版式里这种写法很常见（角标、竖排装饰、页脚条），
于是 `.doc` 的高度可能远小于视觉内容 → 实测页数偏小 → 模型以为"一页放得下"，用户打印出来发现被截断。

**定**：P0 不做内容包围盒测量，但**把话说在工具返回值里**，不只写在提示词里 ——
写在提示词里模型会忘，写在工具返回里它每轮都看得见。

`tools/layout.rs::set_page_layout` 的返回值，在 freeform 档且这一版的 HTML（`layout.html` 或 `parts` 合并结果）里命中
`position\s*:\s*(absolute|fixed)` 时，追加一句（逐字）：

```
注意：这一版有绝对定位（position:absolute / fixed）的内容，它不参与页高计算，实测页数可能比实际偏小。给根容器一个 min-height 能改善。
```

判据实现：对**清洗后**的 HTML 做一次大小写不敏感检测（`html.to_ascii_lowercase().contains("position: absolute")` 与 `fixed` 两种写法都要覆盖；`position:absolute` 无空格也要覆盖）。**不做 CSS 解析** —— 这里要的是"提醒"，不是"判定"，误报（模型写了 `position: absolute` 但实际没内容）成本远低于漏报。

同一条信息也给排版侧的系统提示（`describe_page` 的实测段之后一行，只在 freeform 且检测到绝对定位时出现）：
```
这一版有绝对定位的内容，实测页数可能偏小；要看真实效果请打印预览。
```

---

## 7. targetPages 路线二（逐段上限）

### 7.1 为什么路线一（整份目标）不可行（结论与代价）

| 维度 | 路线一：整份目标 | 结论 |
|---|---|---|
| 与 `split_page` 的关系 | 整份目标下，**拆页 = 让情况变糟**：1.3 页的段拆成 1.0+0.3，整份从 1.3 张变 2 张。排版 agent 想救溢出反而违规 | **互斥**。而"允许排版模型增删页"是用户这次明确要的能力 |
| 可判定性 | 要算 Σ 就得渲染**全部段**；否则只能拿每段"最后一次缓存"，内容一改就 `stale` → 日常状态是 `unknown` | **长期判不出来**。一个平时永不生效的判定比没有更糟 |
| 实现成本 | 每页挂一个离屏宿主（每页一次 `compilePage` IPC + 一次布局）。10 段 = 10 次 IPC/每次内容变化 | 成本随页数线性涨，且与"内容一变就重排"的节拍叠加 |
| 用户预期 | 用户说"压到一页"时，99% 是一份单页简历 | 路线二在单段（绝大多数）下与路线一**完全等价** |

**定论：路线二**。`targetPages` = **每一段的上限页数**；P0 不做 Σ 判定，只做展示值。
**代价（明确接受）**：用户没法用"整份 3 页"来约束一份 5 段的简历。做到的办法是删内容，不是加页 —— 这句话要由一次性提示告知用户（§7.4），不许模型自己编。

### 7.2 `fit_of` 与自动压版：**不动**

`facts.rs:407 fit_of`、`provider.tsx` 的自动自查（同段 ≤3 轮）、导出门 `gatePage` —— 一行不改。逐段判、超出即未达标，正是路线二要的。

### 7.3 文案改造（这是本条的落地重点）

**`{target}` 槽位**（`facts.rs::target_line`）：

| 来源 | 新文案（逐字） |
|---|---|
| `UserThisTurn` | `{n} 页（用户这一轮明确要求：这一段最多 {n} 页）` |
| `UserBefore` | `{n} 页（用户之前要求过，仍然有效：这一段最多 {n} 页）` |
| `FromOrigin` | `{n} 页（沿用原件页数：原件只有 {n} 页，用户没有另说，整份只有这一段）` |
| `Unset` | 不变：`未设置（用户没有说过要几页，任何页数都合规；不要自行假定一页）`<br>`未设置（原件页数读不出，用户也没有说过要几页）` |

> 上一轮 `PLAN-photo-fit.md` §4.4 定稿的 `{target}` 槽位被本条**取代**（`压到一页` → `这一段最多 {n} 页`），以本文为准，`docs/CONTRACT.md` §2 同步。

**`measureTip`**（`lib/measure.ts:87-108`）：必须出现「这一段」三个字（验收断言）：

```
达标   ：实测 1.00 页（这一段最多 1 页，达标）。
溢出   ：实测 1.18 页（这一段最多 1 页，未达标）。超出 0.18 页 · 约 11 行正文。
容忍带 ：实测 1.01 页，在 2% 容忍内，仍算没有溢出（这一段最多 1 页）。
无目标 ：实测 1.18 页。没有目标页数，不讲达标与否。
量不到 ：尚未实测（预览没打开，量不到）。
```

**`set_target_pages`**（`tools/target.rs`）：
- 参数描述：`用户明确要求的页数上限（1–10），约束的是**当前这一段**`
- 返回：`已记下：这一段最多 {pages} 页。` / `已清除目标页数：不再按页数判达标。`
- 错误里的 `pages 只能是 1 到 10` 不变

### 7.4 `FromOrigin` 收紧 + 旧值语义债

**收紧**（`facts.rs:376 target_of`）：只有在 `resume.pages.len() == 1` 时才允许 `FromOrigin`：
```rust
// 「沿用原件页数」只在整份只有一段时成立。多段时"原件 3 页"跟"这一段该几页"
// 没有关系 —— 拿它当默认会把排版 agent 往"每段 3 页"推，等于取消约束。
let single = resume.get("pages").and_then(Value::as_array).map_or(false, |p| p.len() == 1);
match origin.and_then(|o| o.pages) {
    Some(n) if single && (1..=3).contains(&n) => Target { pages: Some(n), source: FromOrigin },
    _ => Target::default(),
}
```

**旧值语义债**：`pages.len() > 1 && targetPages` 有值 → 这个值在旧语义下是"整份"、在新语义下是"这一段"，含义变了。首次打开弹**一次性**提示：

```
标题：这份简历有 3 段，页数目标的含义变了
正文：你之前设的「1 页」现在只约束**当前这一段**。
     要让整份只占 1 张纸，得靠删内容，不是靠加页。
按钮：知道了
```
- 触发：provider 启动后，`ready && resume.pages.length > 1 && resume.targetPages && !resume.targetPagesNotice`
- 确认 → `dispatch({ type:"resumeNotice", key:"targetPagesNotice", … })` → 落库
- **幂等**：`targetPagesNotice` 一旦为 true 永不再弹；重放 op 不会重复弹（值是 true）

### 7.5 整份展示值（只统计已测段）

**数据**：provider 里一个 `Map<pageId, { content: string; onePage: number; contentPx: number }>`，每次 `usePageMeasure` 得到 `ok` 就写一条。**不落库**（与 measure 同一条原则：落库的数下次启动会变成假数据）。

**新鲜度**：渲染汇总时，对每个缓存项算当前 `contentVersion(person, page)`，与缓存里的 `content` 比对；不等 → 该段记为**未测**（内容改过，旧数作废）。

**展示**：
```
已测 2/3 段 · 已测合计 2.4 张
```
- 分母恒显示（`/3 段`），避免把部分当成整体；
- 有未测段时**只显示"已测合计"**，不显示"整份 X 张"，**也不按 1 张补齐**；
- 未设 `targetPages` 时也显示（它只是事实，不是判定）；
- freeform / file 同样适用（file 页只有 1 段且是整页原件，汇总显示 `已测 1/1 段 · 2.0 张` 很怪 → **file 档不显示汇总**）。

**明确不做**：汇总值**不参与** `gatePage` / `gateAll` 拦截，**不驱动**自动压版。它只是一个状态栏数字。

### 7.6 回归断言（必须写）

```
3 段、targetPages 未设置、第 1 段实测 1.18 页
  → facts::block 的 verdict 含「没有目标页数，不讲达标与否」
  → 不含「未达标」
  → gatePage() 返回空（不弹导出门）
  → 状态栏不出现「未达标」，只出现「已测 1/3 段 · 已测合计 1.18 张」
```

---

## 8. 提示词改动

### 8.1 `prompts/system.md`

**删**（`:107` 前半句）：
```
- 不要自称多模态、不要说「我能看图」，除非这一轮的材料段里确实附了图；看不到就直接说看不到，并说清原因（材料段里写了什么原因就说什么）。
```
**改为**：
```
- 能不能看图只看【模型能力】那一块，照抄它的状态。不要自己判断，也不要凭"这一轮有没有附图"反推。
```
理由：这句话是"能力实测"之前写的兜底（当时没有可靠的能力来源，只能让模型看材料段猜）。现在有了唯一来源，留着它会让模型**在 capability=supported 时仍然不敢说能看图**，两个指令打架。

**加**（放在【当前材料】之前，与【页面事实】的规矩并列）：
```markdown
## 能力与页数：都有实测，都照抄

- 【模型能力】说能看图 → 可以看图；说不能 → 不要声称能看；说没测出来 → 就说「这一项还没测出来」，并建议去设置里测一次。
- 【页面事实】的页数、实测、达标与否只能照抄；`目标页数` 说的是**这一段**最多几页，不是整份。
- 用户问「整份一共几页」时，如实说「这一份有 N 段，我可以逐段告诉你每一段是多少页」——不要拿单段实测当整份。
```

### 8.2 `prompts/layout.md`

**一份提示词服务两种形态**，判定方式：`describe_page` 输出的**首行**带上形态，提示词按首行分叉。

`describe_page` 首行改为：
```
第 2 页 / 共 3 页，页名「通用版」｜形态：standard（参照 Word 排，可导出 .docx）
第 2 页 / 共 3 页，页名「作品集」｜形态：freeform（自由版面，只出打印/PDF）
```

提示词加一段（放在「你的边界」之后）：
```markdown
## 你的形态：看首行的「形态：」

**形态：standard**（参照 Word）
- 版面靠 class 与 `parts`，不要写 `position` / `transform` / `animation` / `z-index` —— 这些在 Word 里表达不出来，导出的 .docx 会散架。
- 内容多了用 `split_page({ afterBlockId })` 把后半段切到下一段；不要试图用更小的字号硬塞。
- 删页只能删**空页**（`delete_empty_page`）；有内容的页不让删。

**形态：freeform**（自由版面）
- 这一页的文档本体是整页 HTML（`layout.html`）。你可以用正常的 HTML + CSS：`position`、`grid`、`transform`、`animation`、`<style>` 都行。
- **不能写** `<script>`、`on*` 事件属性、`<iframe>`、`<object>`、`<embed>`、`<base>`、`javascript:`；**`@page` 也不行**（纸张与页边距由设置决定，你写了会被剥掉）。
- 外链图片/字体可以写，但离线或打印时可能拉不到 —— 重要的图尽量用 `asset:<id>`。
- 这一页改内容**不会**改版面：内容变了之后，版式可能挤或空，需要你按新内容重排一遍。
```

### 8.3 `GUIDE`

只加一句（freeform 才有的能力，standard 看到也不会照做）：
```
【自由版面】形态是 freeform 时，上面这些 class 全都不必用 —— 你可以直接写 HTML + <style>。
唯一不能碰的是 <script>、on* 属性、iframe/object/embed/base、javascript:、@page。
```
**不许写任何数字阈值**（沿用上一轮 §10.4 的验收：提示词里不出现判定用数）。

---

## 9. 测试计划

### 9.1 A 块

| # | 用例 | 类型 | 断言 |
|---|---|---|---|
| A1 | `fingerprint` | Rust UT | 同参同值；换 key/模型/baseUrl 各变；输出 16 hex；**不含** key 明文 |
| A2 | `classify_error` | Rust UT | 400+`image_url`+`not support` → `ImageRejected`；400+`image`+`too large` → `BadRequest`；400+`messages` → `BadRequest`；401 → `Auth`；429 → `Quota`；503 → `Server`；连接失败 → `Offline` |
| A3 | 改判可逆 | Rust UT | 先 `mark_protocol_reject` → `unsupported/protocol-reject`；再写 `probe` 结果 `supported` → 读回 `supported`（后写胜） |
| A4 | 不该改判的三类 | Rust UT | `BadRequest` / `Quota` / `Timeout` 各跑一次改判路径 → state 仍 `unknown` |
| A5 | 探针图 | Rust UT | 9 个色块，尺寸 = 9*64+4*4=592? 断言 `width==height`、`truth.len()==9`、色名都在白名单内 |
| A6 | 答错保持 unknown | Rust UT | 给一个"回答 9 个乱面色"的假响应 → `Unknown`，**不是** `Unsupported`（这条是 A-P4 的机器化） |
| A7 | `capability_block` 三态 | Rust UT | 三态文案逐字；`unsupported` 含「这个模型不接受图像输入」；`unknown` 含「没测出来」且**不含**「看不了」 |
| A8 | 三处同源 | Rust UT | 假 settings 里标 `unsupported` → `resolve()` 返 unsupported；`materials` 不产图；`view_asset` 报错文本含「不接受图像输入」 |
| A9 | 前端徽标 | 人工 | 设置页显示「支持 / 不支持 / 没测出来」三态；点「测一次」后状态变化且重开应用仍在 |

### 9.2 B 块

| # | 用例 | 类型 | 断言 |
|---|---|---|---|
| B1 | kind 推断 | Rust UT | 0 页 → standard；1 页 file → file；2 页 file → file；1 页 native → standard；未知字符串 → standard |
| B2 | 混用纠正 | Rust UT | file+非 file 混 → standard 且**页数不变**；freeform+file → standard 且页数不变 |
| B3 | paper clamp | Rust UT | `[3,60,-1,16]` → `[5,50,16,16]`；`[100,100,100,100]` → 等比缩到上限；**再 normalize 一次结果不变**（幂等） |
| B4 | mm→px / mm→twips | Rust UT | 16mm → 28.50px（±0.01）/ 907 twips；20mm → 1134 |
| B5 | `@page` 唯一写入点 | Rust UT | `compile()` 对三档的输出**都不含** `@page`；freeform 手写里带 `@page{size:A4}` → 输出被剥掉 + 一条 warning |
| B6 | `shell` 输出 `--pad-*` | Rust UT | `paper.margin=[10,20,30,40]` → html 含 `--pad-t:17.81px`（10*374/210）等四值；**不含** `padding:` 内联 |
| B7 | split_page 语义 | Rust UT | 3 块、切在第 1 块后 → 源页 1 块 / 新页 2 块，顺序不变；`parts` 跟着走；`preset` 继承；`html` 存在时报错；最后一块时报错 |
| B8 | split 限次与上限 | Rust UT | 一轮两次 → 第二次报错；12 页时 split → 报错 |
| B9 | 删空页 | Rust UT | 空页成功；有 1 个块 → 报错且消息含「1 个块」；有批注 → 报错；最后一页 → 报错 |
| B10 | reducer 双保险 | 前端 UT | 手工 dispatch `removeEmptyPage` 到非空页 → 页还在 |
| B11 | targetPages 逐段 | Rust UT | 3 段 + targetPages=1 → `{target}` 含「这一段最多 1 页」；`FromOrigin` 只在单段出现（3 段 + origin 3 页 → Unset） |
| B12 | measureTip 含「这一段」 | 前端 UT（vitest） | 三态各断言含 `这一段` |
| B13 | 整份展示值 | 前端 UT（vitest） | 3 段、2 段有缓存且内容未变 → `已测 2/3 段`；第 3 段缓存内容版本过期 → 仍 `已测 2/3`；未测段**不**按 1 张补齐 |
| B14 | 无目标不判未达标 | 前端 UT + Rust UT | 见 §7.6 |
| B15 | freeform 禁 Word | 前端 UT | `kind==="freeform"` → 菜单项 disabled 且带说明 |
| B16 | 打印边距 | 人工 | 改 `paper.margin` → 预览留白变、打印预览留白变、两者 mm 值一致；docx 的 `pgMar` 与之一致（解压 `document.xml` 看） |
| B17 | 三形态渲染 | 人工 | standard 出结构化版；freeform 出 `<style>` + 绝对定位且**脚本被剥**；file 只读 |
| B18 | freeform 绝对定位提示进**工具返回** | Rust UT | `set_page_layout` 在 freeform 档、HTML 含 `position: absolute`（三种写法：有空格 / 无空格 / `fixed`）→ 返回值含「不参与页高计算」；standard 档同输入 → **不含**该句；freeform 无绝对定位 → 不含 |
| B19 | 多页 file 档可逐页预览 | 人工 | 上传 2 页扫描 PDF 作为简历（`kind=file`、2 个 file 页）→ 页列表可切、两页各自显示自己的原件、全部只读、常驻提示含「由 2 个原件组成」；导出文件夹得到 2 个文件 |

### 9.3 会被打破的现有测试（必须同批修）

| 位置 | 为什么红 | 修法 |
|---|---|---|
| `tools/mod.rs:777` `assert_eq!(layout.len(), 9)` | 新增 2 个 Layout 工具 | 改 11 |
| `tools/mod.rs:776` `session.len()==40` | split/删空页**不进** Session | 不变（要盯住别顺手加进去） |
| `tools/mod.rs` 角色断言 | 新增 | 加 `layout.contains(&"split_page")`、`!session.contains(&"split_page")` |
| `render/mod.rs` 白名单测试（`CSS_PROPS` 相关、`svg_in_hand_written_html_survives`、`data_block_survives`） | 常量拆分 + 两份 builder | 每份 builder 各跑一遍同一组断言 |
| `render/mod.rs` `compile` 的所有调用点（tests 里十几处） | 签名加 `kind, paper` | 加一个 `compile_default(page, person)` 测试辅助，避免逐处改 |
| **`docx.rs` 的全部测试**（`docx.rs` 内 `mod tests`） | 整文件换成 `docx-rs`，手写 XML 的断言全部失效 | 删掉按 XML 字符串断言的用例，改成"能打包 + 解包回来能读到 N 个段落 + `sectPr` 的 `pgMar` 与 `paper.margin` 一致"（用 `read_docx` 回读，`docx-rs` 自带） |
| **`export.rs` 的「每页一个 docx」manifest 测试** | 导出模型改成「整份一个 docx」 | 断言从 `files.len() == pages.len()` 改成 `files.len() == 1 + 旁挂图片数`，manifest 里记页边界 |
| `agent/facts.rs` 的 `fit_of` 用例 | 输入语义变（`contentPx` 由物理页折出），**边界值不变** | 数值断言不用改（阈值没动）；但要补一条"`source: "paged"` 与 `"estimate"` 同输入 → 同判定"（防有人给 paged 单独开阈值） |
| `lib/measure.ts` 的 `PageMeasure` 构造点 | 新增必填感字段 `source` | 给默认值 `"estimate"`，老构造点不报错；`measureTip` 文案不变，测试不用改 |
| `paper.tsx` 的预览分支（若有 DOM 结构断言） | 预览 DOM 从 `.doc` 变成 `.pagedjs_pages > .pagedjs_page` | 断言改成"存在 N 个 `.pagedjs_page`"；编辑态的断言不动 |
| `agent/facts.rs` `{target}` 文案断言 | 文案改「这一段最多 N 页」 | 全部同步 |
| `agent/facts.rs` `target_of` 用例 | `FromOrigin` 收紧 | 补「多段 → Unset」用例 |
| `lib/measure.ts` 相关（若有） | `measureTip` 文案变 | 同步 |
| `db.rs` 的 workspace 往返测试 | `Resume` 多两个字段 | 断言往返后 `kind`/`paper` 不丢 |

---

## 10. 风险与取舍

| 风险 | 影响 | 应对 |
|---|---|---|
| **迁移误判**（老数据 kind 推错） | 用户打开发现 Word 导出灰了，或者自由版面被当标准版清洗成空白 | ① 推断规则只读不删；②「混用一律降 standard」是**最保守**的方向（standard 允许 file 材料页，不会丢任何页）；③ 三形态都能在 UI 手动切换，误判可自救；④ B1/B2 用例覆盖 |
| **freeform 渲染在主 WebView 的安全边界** | 只要脚本进得来，就能调全部 Tauri 命令。这不是样式问题，是"用户机器被远程代码执行" | **禁 script/on* 是硬红线**，且不只靠 ammonia：`<style>` 内容单独扫 + 拦截 `will-navigate`/新窗口。⚠️ **不能靠 CSP**：CSP 是文档级的，同文档里跑着 Paged.js，`script-src 'none'` 会把它一起挡掉 → **P0 不加 CSP**（见下一条） |
| **⚠️ Paged.js 会把 app 的样式摘走** | `Previewer.removeStyles()` 从 document 收集并**移除**所有 `<style>`/`<link>`（只排除 `data-pagedjs-ignore` 或 `media~=screen`）。Vite 的 CSS 就是注入 `<style>` → **不处理则预览一打开，整个界面样式消失** | 每次 `preview()` 前扫一遍给 app 的 style 打 `data-pagedjs-ignore`（`page-setup` 那条**不**打，它要给 Paged.js 用）；这条写进 §11.5-2，并进 B3b-1 的验收（"预览态下界面样式不丢"） |
| **CSP 与 Paged.js 互斥** | `tauri.conf.json:30` 现在是 `csp: null`；将来若加 `script-src 'none'`，同文档的 Paged.js 会被一起挡掉 → 预览直接白屏 | **P0 不加 CSP**（保持 `null`）。CSP 只有走"第二 WebView 承载 freeform"方案时才成立（那时各文档各 CSP）；这条与 §11.6 的 F3/F4 绑定，不要在 A＋ 落地期间顺手加 |
| **docx 分页与屏幕不一致** | 用户看到 3 页、Word 打开是 4 页 → 觉得"你骗我" | §11.4 已写死契约：**同内容 + 同断页意图，不做像素级复刻**；UI 只说"尽量对齐"，**不许承诺页数**；`cantSplit` 处理跨页块；不采用"每页转图"（会失去可编辑性） |
| **Paged.js 分页耗时** | 每次内容变化都重排整条流；10 段 + 大量图片时可能上百毫秒，用户看到闪 | ① 只在预览态跑；② 300ms 防抖；③ 分页期间保留旧页并加 `aria-busy`，不闪空白；④ 用 `flow.performance` 记一次基线（B3b-1 验收里记数值） |
| **`@page` 从静态变注入** | 时序问题：打印时样式还没写进去 → 打出默认边距；或注入两条 `@page` 互相覆盖 | ① provider 常驻 effect（不是打印时才算）；② 删掉 print.css 那条，**全仓只有一处写 `@page`**；③ 打印前 `waitFor` 一次（`document.getElementById("page-setup")` 存在即可） |
| **双档白名单维护成本** | 两份规则，改一份忘另一份 | 用 `FORBIDDEN_ALWAYS` 单一来源兜"永远禁"，允许集本来就该分档；`standard` 那份是现有白名单改名，不新写 |
| **探测消耗真实 token** | 每次点"测一次"= 一次带图请求（约 1.2k prompt tokens） | 只在用户显式点击、或首次配置模型时跑一次；**不在每轮对话里跑**；结果落库不再重复 |
| **standard 里 file 材料页与页数的关系** | standard 简历里放一张整页扫描件（证书），它的"页数"是 1 还是 0？汇总里算不算？ | 定：**file 页不参与 `fit_of`**（`facts.rs` 已对 file 页只给 verdict、不判页数）；整份汇总里 file 页记 `1.0 张`（它就是一张纸）。⚠️ 这条需要在 B13 的用例里显式覆盖，别留成"想当然" |
| **freeform 绝对定位不撑高父元素** | 溢出量不到 → 实测恒为 1.00 页 → 用户以为没溢出，打印出来才发现被截断 | ① 实测段照报（不撒谎）；② **提示进工具返回值**（§6.4：`set_page_layout` 检测到 `position:absolute/fixed` 追加一句）—— 写在提示词里模型会忘，写在返回里每轮都看得见；③ `describe_page` 的实测段后再补一行；④ P1 才做"内容包围盒"测量（`getBoundingClientRect` 并集），P0 不做 |
| **`pageBlocks` 是"整页替换"** | 同轮两次 split 会互相覆盖（与 `mutate` 的旧快照问题同源） | 已用"一轮只许 1 次 split"限制；将来放开要先做向量时钟 |
| **`RemoveEmptyPage` 绕过确认门** | 万一 reducer 判空有 bug，会删掉有内容的页 | 工具校验 + reducer 校验双保险；且 `removeEmptyPage` 只按 pageId 删、不批量 |
| **`resolve()` 依赖 settings 可读** | settings 损坏/被清 → 全部退 `unknown` → 用户以为模型不能看图 | `unknown` 的文案是"没测出来"，不是"看不了"；且 `materials` 在 unknown 时的行为要定：**照旧给图**（`unknown` 不等于 no）。这条与"答错→unknown"的取舍一致 |

---

## 11. A＋ 定稿（选型已定：Paged.js 分页 + docx-rs 导出）

> 用户定 **方案 A＋**。本节把原「PENDING-研究」两节收口成定稿；freeform 的安全兜底仍有 4 个未定项，移到 §11.5。

### 11.1 选型（只加两个依赖，参数按用户给定）

| 用途 | 选型 | 许可 / 体积 | 关键约束 |
|---|---|---|---|
| 屏幕与打印分页 | `pagedjs@0.4.3` | MIT / 93KB gzip | **只用 ESM 的 `Previewer`**，绝不用 `paged.polyfill.js` —— polyfill 会把整个 body 包进 template 重建，在 React SPA 里会炸树 |
| docx 导出 | `docx-rs@0.4.22` | MIT | 替换现在手写 XML 的 `docx.rs` |

`Previewer` 的用法（已核 docs.pagedjs.org / devdocs）：`new Previewer().preview(content, stylesheets, renderTo) -> Promise<flow>`，`flow = { total, pages, performance, size }`。
`stylesheets` 传**内联样式对象**（`{ "paper.css": cssText }`）而不是 URL —— 避免 `tauri://` 下由库去 `fetch`。**落地核对点（15 分钟可验）**：若 0.4.3 的该形参只吃 href 数组，退化为"先把 CSS 文本注入一个临时 `<style>`（不加 `data-pagedjs-ignore`），再让 Paged.js 从 document 收走" —— 两条路都不改架构。

### 11.2 结构结论：一条流 + `break-before: page` —— **可行**

**做法**：把 N 段编译成**一条 HTML 流**，每段包一层，第一段不加断页：

```html
<div class="docs-flow">
  <section data-seg="pg1" class="doc" data-preset="classic" style="--pad-t:…">…</section>
  <section data-seg="pg2" class="doc" data-preset="compact" style="break-before: page">…</section>
</div>
```

**依据（已核官方文档，不是推测）**：
- `break-before: page` 是 Paged.js 官方支持的属性（`pagedjs.org/en/documentation/5-web-design-for-print`：「Paged.js automatically creates a page break when your content doesn't fit… you may need to control this fragmentation… `break-before: page`」）。它由 `Breaks` handler 落成 `data-break-before` 属性，`Chunker.handleBreaks` 据此插页。
- 段内溢出由 Paged.js 自然分页；段间由 `break-before` 强制断页。**两级分页同时成立，不冲突**。

**反查「物理页边界落在哪个段/哪个块之后」—— 可行，且不依赖库的内部状态。** 三样东西足够：

| 来源 | 用途 |
|---|---|
| 我们自己的 `data-seg` / `data-block` | 段与块的锚点（`data-block` 已有，`data-seg` 新增） |
| `.pagedjs_page` 的 DOM 顺序 | 物理页序（下标即页号 − 1） |
| Paged.js 的 `data-ref` / `data-split-from` / `data-split-to` | 判断"某个块是否被切开"（分裂片段带 `data-split-from`，指向上一片段） |

```
pages   = [...root.querySelectorAll(".pagedjs_page")]      // 顺序 = 物理页序
for (i, page) of pages:
    segs   = [...page.querySelectorAll("[data-seg]")]      // 该页涉及的段
    blocks = [...page.querySelectorAll("[data-block]")]    // 该页涉及的块（document order）
    // 页 i 的断点 = blocks 最后一个；它若同时出现在页 i-1，说明这个块被切开
    boundary[i] = { afterBlock: blocks.at(-1)?.dataset.block ?? null,
                    splitInside: blocks.at(-1)?.dataset.splitFrom != null }
segPages[seg] = pages.filter(p => p.querySelector(`[data-seg="${seg}"]`)).length
```

`segPages` 与 `boundary` 就是「这一段实测几页」和「导出时该在哪断」的全部输入。

### 11.3 页数来源（口径改一次，`fit_of` **不用改**）

**权威顺序**：
1. **预览态（Paged.js 已渲染）→ 物理页数，权威**；
2. 编辑态（没有真纸面，这是既有事实）→ 保留现有 `offsetHeight / onePage` 估算，`source = "estimate"`。

**两者换算成同一把尺子**（这是关键设计，否则 `fit_of` 与已冻结文案全要推翻）：

```
onePage = 物理页的内容区高度 px（页高 − 上下 margin，来自 .pagedjs_page_content 的 rect）
pages   = (该段占的物理页数 − 1) + fill(最后一页)
fill(p) = clamp((最后一个块的底边 − 内容区顶边) / 内容区高度, 0, 1)
measure.contentPx = pages * onePage          // ← 仍然喂给 fit_of
measure.onePage   = onePage
measure.source    = "paged" | "estimate"     // 新增，只用于诚实标注
```

于是：
- **`fit_of` / `FIT_MAX` / `TOLERANCE_MAX` / 全部已冻结文案一行不改** —— 输入还是 `contentPx` 与 `onePage`，只是数从"高度比"换成"真分页折出来的等价高度"。
- 「实测 1.18 页」这个粒度保住了（不是变成"2 页"），因为 `fill` 把最后一页的充实度折成小数。**保留小数的理由是它指导"还要压多少"**：说"超出 0.18 页"比说"超出 1 页"更能让排版 agent 决定下一步。
- `PageMeasure` 加一个 `source` 字段（默认 `"estimate"`，老前端不传也能解析），**文案不变**，只在 Tooltip 里能区分「精确实测 / 估算」。

**这条同时解决 `targetPages` 的口径**：仍按「每一段」（已定稿），但"这一段几页"从"高度估算"变成"该段占的物理页数折算"—— 更准，且能回答"这一段跨了几张纸"。

### 11.4 导出：一份 docx 多页 —— 契约要说实话

**导出模型变更**：manifest 从「每页一个 docx」改成「整份一个 docx」。

**断页规则（与屏幕同源，但不可能逐页一致）**：

| 位置 | 做法 |
|---|---|
| 段间 | **强制分页**（段的第一段除外）。屏幕上是 `break-before: page`，docx 上是段落分页符 |
| 段内 | 按 `boundary[i].afterBlock` 在该块之后插分页符；**但 `splitInside == true` 的块不切** —— 块被页边界切开时，docx 里用 `cantSplit` 让它整体移到下一页 |
| 表格 | `cantSplit` 行不跨页；跨页表格由 docx 自己重排 |

**必须写死的限制（不许开发自己猜，也不许 UI 撒谎）**：
> docx 是另一个排版引擎。字体度量、行高、表格边框、字号舍入都不同，**分页位置不会与屏幕逐页一致**。
> 「一份 docx 多页」给的是**同样的内容 + 同样的断页意图**（段间必断、段内尽量在块边界断），**不是一个像素级复刻**。
> 因此 UI 文案只能说「内容与分页位置尽量对齐」，**不许承诺「Word 里也是 3 页」**。
> 要更强一致性只有把每页转成图贴进 docx（那就不可编辑了）——**明确不做**。

### 11.5 集成约束（Paged.js 落地必须遵守的 6 条）

1. **`renderTo` 是专用容器，React 不管它的子树**。Paged.js 自己 append `.pagedjs_page`；React 若同时渲染 children 会被清空/打架。用 `useEffect` + `ref`，容器内**不写任何 JSX**。
2. **⚠️ 必须给 app 自己的 `<style>` 打 `data-pagedjs-ignore`**。Paged.js 的 `Previewer.removeStyles()` 会从 document 里**收走并移除**所有 `<style>` / `<link>`（只排除带 `data-pagedjs-ignore` 或 `media ~= 'screen'` 的）。Vite 的 CSS 是注入 `<style>` 的 → **不处理就会把整个界面样式摘走，界面当场崩**。
   ```ts
   /** Paged.js 会把 document 里的 <style> 全收走，app 的样式必须打标豁免。每次 preview 前扫一遍（dev 的 HMR 会插新 style）。 */
   document.querySelectorAll("style:not([data-pagedjs-ignore])").forEach((el) => {
     if (el.id === "page-setup") return;      // 这一条要给 Paged.js 用，不能豁免
     (el as HTMLElement).dataset.pagedjsIgnore = "";
   });
   ```
3. **只在「预览」视图跑，300ms 防抖**；编辑视图继续用现有的连续纸面（编辑态本来没有真纸面）。
4. **`@page` 一份喂两处**：`page-setup.ts` 生成的 `@page{size:A4;margin-*:…}` 既给 Paged.js 算页，也是打印用的那条。**`print.css` 里那条静态 `@page{margin:16mm}` 必须删**，否则与页容器的边距叠加成双倍。
5. **纸面只读**：Paged.js 输出是 `cloneNode` 的副本，编辑它不会回到数据。UI 要写明「纸面只读，改内容请在左侧表单/对话里改」，别让用户双击纸面试图打字。
6. **`asset:` 由浏览器解析，Paged.js 不碰它**（它只处理 CSS/HTML 里的 `src`/`url()`，解析器仍是 WebView）。但 Paged.js 自己不会 `fetch` 我们传进去的 CSS 文本（走内联对象），这条与 §11.1 的"不传 URL"配合。

### 11.6 仍 PENDING 的小项（不影响 A＋ 落地）

freeform 的安全兜底还有 4 个未定项，原样保留：**F1** ammonia 能否黑名单模式；**F2** `<style>` 里 at-rule 剥离用正则还是 CSS 解析器；**F3** CSP 注入方式；**F4** 是否要第二 WebView 隔离。
其中 **F3 与 A＋ 直接冲突，见 §10 风险表新增那条**：CSP 是**文档级**的，`script-src 'none'` 会把同文档里的 Paged.js 一起挡掉 → **P0 不加 CSP**，CSP 只有走第二 WebView 方案时才有意义。

## 12. 批次建议

> **开工闸门**：B0–B6 的顺序与归属只是建议，**必须等 team-lead 发话并出了任务卡才动手** ——
> 同一段文案（`{target}` / `measureTip` / 工具返回）这轮已经因为两份版本打过一次架，
> 不允许开发照本文件自行开工。B7 不动，等选型。

| 批次 | 内容 | 并行 | 串行约束 |
|---|---|---|---|
| **B0** | `docs/CONTRACT.md` 全部 diff；`model.rs` 的 `kind`/`paper`/normalize；三个新 op（三处同改）；`types/index.ts` | 契约先落 | **`model.rs` 只一个人改** |
| **B1** | A 块后端：`capability.rs`（指纹/resolve/probe/改判）、`classify_error`、`facts::capability_block`、`Outbox.capability`、删 `SendArgs.vision` | 与 B3 并行 | 与 B2 有 `net.rs` 交集 → **`net.rs` 只归 B1** |
| **B2** | A 块前端 + 提示词 `system.md`：设置页三态 + 测一次、徽标、错误文案 | 与 B1 同一人更省（口径要一致） | 依赖 B1 的 `capability_of` 命令 |
| **B3a**（已批，**先做**；DSL 无关） | 文件集合固定 6 个：① `src/styles/page-setup.ts`(新)；② `src/store/workspace/provider.tsx`（只加常驻 effect）；③ `src/styles/print.css`（删静态 `@page`）；④ `src-tauri/src/render/paper.css`（`padding` 改四变量）；⑤ `src-tauri/src/docx.rs`（`pgMar` 由 `paper.margin` 派生）；⑥ `src-tauri/src/model.rs` 的 `Paper` 与 clamp（与 B0 同一人，避免抢文件） | 与 B1 并行 | **等 `dev-measure` 交出 `measureTip`/汇总值之后再派**（两边都碰 `provider.tsx`，同一人做同一文件才安全） |
| **B3b-1**（A＋ 接入 · Paged.js） | 新增 `src/lib/paged.ts`（`Previewer` 封装 + `data-pagedjs-ignore` 打标 + 反查 `segPages`/`boundary`）；`paper.tsx` 预览分支换成 Paged.js 容器（**只读副本 + React 不管子树**）；`provider.tsx` 加 300ms 防抖 + 预览态专用 effect；`render/mod.rs::shell()` 输出 `--pad-*` 与 `data-seg`；`compile()` 签名加 `kind, paper` | 与 B1 并行 | 依赖 **B3a**（`page-setup.ts` 与 CSS 文本要能拿到）；**`render/mod.rs` 归本批** | 
| **B3b-2**（A＋ · 口径） | `measure.ts`：`pages = (segPages-1)+fill`、`contentPx = pages*onePage`、新增 `source: "paged"\|"estimate"`；`facts.rs` 只加字段不做判定（`fit_of` 不动） | 否 | 依赖 B3b-1 的 `segPages` |
| **B3b-3**（A＋ · 导出） | `docx-rs@0.4.22` 替换 `docx.rs`；manifest 从「每页一个 docx」改「整份一个 docx」；按 `boundary` 插分页符、`splitInside` 的块用 `cantSplit`；`PageMargin` 吃 `paper.margin`（twips） | 与 B3b-1 并行（不同文件） | **`docx.rs` 整文件归本批**，与 B3a 的 twips 改动合并到本批（避免两批改同一处） |
| **B4** | 工具：`split_page`、`delete_empty_page`、`pageBlocks`/`removeEmptyPage` reducer、Layout 工具数 9→11 | 依赖 B0 | **`tools/mod.rs` 与 `render/mod.rs` 是热点**：`tools/mod.rs` 归 B4（一次加完两条），`render/mod.rs` 的 `compile` 分流归 PENDING 落地批 |
| **B5** | targetPages 路线二：`target_of` 收紧、`target_line`/`measureTip`/`target.rs` 文案、整份展示值、一次性提示 | 依赖 B0 | 与 B4 无文件交集，可并行 |
| **B6** | 提示词 `layout.md`/`GUIDE` + 全部 UT + 前端 vitest + 人工冒烟 | 最后一个 | 依赖 B1–B5 |
| **B7（等 A＋ 合入）** | freeform 黑名单与安全兜底（F1/F2/F4）+ standard 的 XML DSL | — | A＋ 先落地：**freeform 的 `<style>` 与 Paged.js 的样式注入会互相影响**，同一批做才看得清 |

**热点文件的归属（不许两人同时改）**：
- `src-tauri/src/tools/mod.rs` → **B4**
- `src-tauri/src/render/mod.rs` → **B3b-1**（`shell` + `compile`）；B3a **不碰**这个文件
- `src-tauri/src/render/paper.css` / `src/styles/print.css` / `src/styles/page-setup.ts` → **B3a**
- `src-tauri/src/docx.rs` → **B3b-3**（整文件换成 `docx-rs`；B3a 不再动它）
- `src/lib/paged.ts`(新) / `src/components/resume/paper.tsx` → **B3b-1**
- `src/lib/measure.ts` → **B3b-2**
- `src/lib/export.ts`（manifest）→ **B3b-3**
- `package.json` / `Cargo.toml`（两个新依赖）→ **B3b-1 / B3b-3 各自加**（git 冲突热点：同一行 `dependencies`，谁先合谁加，后合 rebase）
- `src-tauri/src/net.rs` → **B1**
- `src-tauri/src/agent/facts.rs` → **B1（能力块）+ B5（文案）**，同一人
- `src/store/workspace/provider.tsx` → **B5**（measure 缓存 + 提示）；B3 的 `pageSetup` effect 也放这里 → 两批要排先后，建议 B3 先

---

## 13. 必须写注释的 3 处（中文，说为什么）

1. **`model.rs::Resume::normalize`**（纠正混用）
   `// 纠正混用只降级 kind、不删页。删页是不可逆的：用户可能只是切错了档，回来的路上不该发现内容没了。反过来也不行 —— 不能因为发现一个 file 页就把整份升成 freeform，那会顺手关掉用户的 Word 导出。`
2. **`net.rs` 的 image-rejected 改判点**
   `// 只有这一支改判，因为它是协议层的强证据；其余 400 一律不改判（400 的分辨率太高：messages 格式、base64、body 过大都会是 400）。改判必须可逆：probe 得到 supported 就覆盖回来。`
3. **`render/mod.rs` 的 `@page` 剥离**
   `// 模型写的 @page 一律剥掉：它是唯一能把设置层的纸张/页边距架空的东西。全仓只有 src/styles/page-setup.ts 一处写 @page。`

> 外加 **A-P4 那段注释原文**（§5.3）**逐字**放进 `capability::probe` 函数族，不许改写、不许精简。

---

## 14. 已拍板（原「待拍板」5 条，team-lead 已全部拍定）

| # | 问题 | 结论 | 相对我推荐的改动 |
|---|---|---|---|
| 1 | `@import` 是否禁 | **禁**（与 `@page` 同类：外部 CSS 是注入面，离线必坏）→ 已进 I1 清单 | 无 |
| 2 | 多 file 页（>1）怎么归 | 判 `file`；**允许逐页预览全部页**（页列表可切，各自渲染自己的 `<img>`）；**不可编辑**；UI 常驻提示 `这是一份由 {n} 个原件组成的简历，不可编辑`；导出逐页原样拷贝；其余页不删不合并 | ⚠️ **改了**：原推荐"只展示第一页"被否 —— 一份 2 页扫描件只给看第 1 页，用户会以为文件坏了。已按本行改 §2.3 |
| 3 | freeform 绝对定位撑不高 | **P0 只提示**，但**提示必须进工具返回值**（`set_page_layout` 检测到 `position:absolute/fixed` 时追加一句），不只写提示词 —— 写在提示词里模型会忘，写在返回里每轮都看得见 | ⚠️ **加强**：原推荐只写提示词。已按本行新增 §6.4 |
| 4 | `unsupported` 时 `materials` 还给不给图 | **不给**（与现行为一致） | 无 |
| 5 | 整份汇总给不给排版 agent 看 | **不给**（它只管这一段） | 无 |

**另三条直接确认（不用再问）**：
- 删掉 `SendArgs.vision`，`Outbox.vision` 改成派生值（`resolve() == Supported`）→ `view.rs` 一行不改。**这是这轮最省的做法**。
- `resumeNotice` 单开一个 op，不塞进 `resumeTargetPages`（ack 会顺手清掉 targetPages）。
- `compile()` 加 `compile_default()` 测试辅助，不逐处改十几处调用。

---

## 15. `docs/CONTRACT.md` 的 diff（可直接粘贴；由开发在 B0 落）

> 老规矩：**先合这份 diff，再改代码**。三处同改（§1.2 白名单 + `lib/agent-actions.ts::OP_SPEC` + `store/workspace/types.ts::Action`）以本节为准。

### 15.1 §1.2 白名单与载荷

```
 personFields  libraryItems  personAssets  itemAssets
 addPage  removePage  pageLabel  pageHeader  pageStyle  pageLayout  blockRole
 addSection  removeSection  blockFields  removeBlock  sectionTitle  sectionItems  entryPatch
 createResume  renameResume  deleteResume  deletePerson
 resumeTargetPages
+pageBlocks          ← 新增：页级 blocks 全量替换
+removeEmptyPage     ← 新增：删空页（不进确认门）
+resumeNotice        ← 新增：记一个"已看过"的标记（不是内容）
```

| op | 载荷 |
|---|---|
| `pageBlocks` | `personId, resumeId, pageId, blocks: Block[]` —— **整页全量替换**。现在只有容器级替换（`sectionItems` 等），没有页级 |
| `removeEmptyPage` | `personId, resumeId, pageId` —— **不进确认门**（空页没有内容可丢）；reducer 必须再判一次空，非空则整条丢弃 |
| `resumeNotice` | `personId, resumeId, key` —— `key` 白名单：`targetPagesNotice`。**不改内容，不进确认门** |

### 15.2 §1.3 确认门

```
 不进确认门（新增）：removeEmptyPage、resumeNotice
+理由：`removeEmptyPage` 的判据是"不可撤销才问" —— 空页没有内容可丢，问一次只是噪音。
+双保险：工具侧校验 + reducer 侧再校验（防旧版本 agent、防确认窗重放）。
```

### 15.3 §2 数据模型

```
+`Resume` 新增两个一等字段（不再是 extra 里的未知键）：
+
+```rust
+/// standard / freeform / file。**存储用 String**（未知值走推断，不整库读不出），
+/// 渲染层内部才换成 enum。三选一，不许混用（normalize 会纠正，只降级不删页）。
+pub kind: String,
+
+/// 纸张与页边距。毫米，顺序 `[上,右,下,左]`，与 CSS margin 简写同序。
+/// 三处同源的唯一来源：预览 --pad-*、打印 @page、docx twips。
+pub paper: Paper,          // Paper { size, width_mm, height_mm, margin: [f64; 4] }，默认 A4 + [20.0; 4]
+```
+
+`Resume.extra` 上再记一个"已看过"标记：
+
+```rust
+targetPagesNotice: Option<bool>,   // 旧值语义债的一次性提示（多段 + 有 targetPages 时弹一次）
+```
+
+**`{target}` 槽位文案改写**（路线二：`targetPages` = 每段上限，取代 photo-fit 那版）：
+
+| 来源 | 文案 |
+|---|---|
| `UserThisTurn` | `{n} 页（用户这一轮明确要求：这一段最多 {n} 页）` |
| `UserBefore` | `{n} 页（用户之前要求过，仍然有效：这一段最多 {n} 页）` |
| `FromOrigin` | `{n} 页（沿用原件页数：原件只有 {n} 页，用户没有另说，整份只有这一段）` —— **只在 `pages.len()==1` 时出现** |
| `Unset` | 不变 |
```

### 15.4 §3 会话工具

```
+| `set_target_pages` | `pages?` | `resumeTargetPages`（省略 = 清除） |
+
+语义修正：`targetPages` 约束的是**当前这一段**，不是整份。工具描述与返回文案里的
+「这份简历压到 N 页」全部改为「这一段最多 N 页」。
+`split_page` / `delete_empty_page` **不给**会话侧（会话的 `add_page` / `delete_page` 保持现状）。
```

### 15.5 §4 排版工具

```
+| 工具 | 参数 | 产物 |
+|---|---|---|
+| `split_page` | `pageId?, afterBlockId` | `addPage` + `pageBlocks`×2 + `pageLayout`×2 |
+| `delete_empty_page` | `pageId` | `removeEmptyPage` |
+
+`split_page` 的语义（10 条）见技术方案 §4.1：新页插在源页后、最后一块报错、`parts` 随块走、
+`preset` 继承、`layout.html` 存在则报错、`notes` 留源页、一轮 1 次、总页数上限 12、
+返回值带新页 id。**一次 turn 只许 1 次**（`Outbox.splits` 计数，超了报错不是静默丢弃）。
+
+`set_page_layout` 的返回在 freeform 档要追加（检测到绝对定位时）：
+`注意：这一版有绝对定位（position:absolute / fixed）的内容，它不参与页高计算，实测页数可能比实际偏小。给根容器一个 min-height 能改善。`
```

### 15.6 §5 渲染增量

```
+### 5.0 三条不变量（跨两档）
+
+I1 禁清单只有一份（`render::sanitize::FORBIDDEN_ALWAYS`，两档共用，只增不减）：
+   `script` / `on*` / `javascript:` / `vbscript:` / `iframe` `frame` `frameset` `object` `embed`
+   / `base` / `data:text/html` / `@page` / `@import`
+   **standard 档也不例外** —— 它现在走 schema 受限的白名单，但 `parts` 已允许局部 HTML，
+   将来放宽时必须已经受到同一份禁清单保护。
+
+I2 `@page` 只有生成器能写：纸张与页边距只来自 `Resume.paper`；模型写的 `@page` 一律剥掉，
+   并与「@page 从静态 CSS 变注入」共用一个注入点（`src/styles/page-setup.ts`）。
+
+I3 入口唯一、管线可分歧：`compile()` 是唯一分流点（按 `Resume.kind`）；
+   `sanitize()` 是唯一清洗入口但按 kind 取策略。**不许出现第二个 `sanitize` 调用点。**
+
+### 5.1 freeform 档
+
+`kind == "freeform"` 时，文档本体是 `layout.html`，按**黑名单**清洗（只剥 I1 与 I2），
+不套 preset 白名单、不产结构化块；`preset` 恒为 `custom`；页边距仍由 `--pad-*` 给。
+Word 导出禁用（`kind == "freeform"` 时按钮置灰并说明）。
+
+### 5.2 file 档
+
+`kind == "file"` 时允许**逐页预览全部页**、全部只读、导出逐页原样拷贝。
+UI 常驻提示：`这是一份由 {n} 个原件组成的简历，不可编辑`。
```

### 15.7 新增 §7 能力（A 块）

```
+## 7. 模型能力（看图）
+
+### 7.1 三态
+
+`supported` / `unsupported` / `unknown`。**只有 `unknown` 与 `unsupported` 的区别是本质的**：
+前者是"没测出来"，后者是"确认不行"。任何地方都不许把 unknown 当 unsupported 用。
+
+### 7.2 存储
+
+`settings` 表，key = `capability:v1`：
+
+```json
+{ "version": 1,
+  "items": { "<fp>": { "state": "supported|unsupported|unknown",
+                       "reason": ""|"protocol-reject"|"bad-request"|"offline"|"timeout"|"no-probe",
+                       "checkedAt": 1765000000, "model": "…", "baseUrl": "…" } } }
+```
+
+指纹：`hex(sha256("{baseUrl}\n{model}\n{apiKey}"))[..16]`。**不存密钥本体**。
+换 key / 模型 / baseUrl → 不命中 → `unknown`（天然失效，无需清理）。
+
+### 7.3 唯一入口
+
+Rust：`capability::resolve(base_url, model, api_key) -> Capability`。
+`attach::materials` / `tools::view` / UI 徽标三处**都只能调它**。
+`SendArgs.vision` 字段**删除**；`Outbox.vision` 改为 `resolve(...) == Supported` 的派生值。
+
+### 7.4 探测判定
+
+全对 → `supported`；答错 / 半对 / 拒答 → **`unknown`**（不许判 unsupported）；
+只有协议层明确拒图（`classify_error == ImageRejected`）→ `unsupported(protocol-reject)`。
+
+### 7.5 改判必须可逆
+
+`unsupported(protocol-reject)` 由 `net.rs` 在收到 ImageRejected 时就地写入；
+同指纹探测得 `supported` → **无条件覆盖**（后写胜）。
+400 但与图像无关、429 / 5xx / 超时 / 连接失败 → **不得改判**。
```

### 15.9 A＋ 增量（Paged.js 分页 + docx-rs 导出）

```
+## 1.5 实测上送（补充）
+
+`PageMeasure` 新增一个字段（**不改任何文案、不改 `fit_of`**）：
+
+```rust
+/// 这一版实测是怎么来的：paged = Paged.js 的物理页折出来的（权威）；
+/// estimate = 编辑态按纸面高度估的（没有真纸面时的兜底）。
+/// 只用于诚实标注（Tooltip），判定不看它。
+#[serde(default)]
+pub source: String,     // "paged" | "estimate"，缺省 "estimate"
+```
+
+权威顺序：预览态（`paged`）> 编辑态（`estimate`）。两者都换算成
+`contentPx = pages * onePage` 之后喂给同一个 `fit_of`。
```

```
+### 5.3 预览渲染（A＋）
+
+预览视图由 Paged.js 分页：N 段编译成一条流，每段一层 `<section data-seg="…">`，
+第一段不断页、其余 `break-before: page`。Paged.js 产出的 `.pagedjs_page` 是**物理页**。
+
+- 纸面**只读**：Paged.js 的产物是 clone，编辑不回数据；改内容仍走表单/对话。
+- `renderTo` 容器由 Paged.js 独占，React 不渲染它的 children。
+- 打印打印的就是这些页容器，因此 `print.css` 里**不许**再有静态 `@page`（与页容器边距叠加）。
+- `@page` 仍只由 `src/styles/page-setup.ts` 生成，同时喂给 Paged.js 与打印（I2 不变）。
```

```
+### 5.4 导出（A＋）
+
+导出模型：**整份一个 .docx**（不再是每页一个）。
+分页：段间强制分页；段内在"物理页边界所在的块"之后插分页符，
+块被页边界切开（`data-split-from`）时用 `cantSplit` 让整块移到下一页。
+**契约限制**：docx 与浏览器是两套排版引擎，分页位置不保证逐页一致 ——
+对外只说"内容与分页位置尽量对齐"，不承诺页数。
```

### 15.8 §6 文件所有权

```
+| **BE-domain** | `src-tauri/src/capability.rs`(新)、`src-tauri/src/render/sanitize.rs`(新) |
+| **FE** | `src/styles/page-setup.ts`(新)、`src/lib/capability.ts`(新) |
```

---

## 16. 方案冻结声明

- **冻结范围**：`Resume.kind` / `Resume.paper` / 三个新 op / `split_page` 与 `delete_empty_page` 的语义与参数 / `capability:v1` 的指纹与形状 / 能力三态文案 / `{target}` 与 `measureTip` 新文案 / §0 的三条不变量 / **§11 的 A＋ 定稿（一条流 + `break-before`、`pages = 页数−1+fill`、`docx` 一份多页且不承诺逐页一致）** / §15 的 CONTRACT diff。
- **不冻结**：freeform 的精确禁清单实现与兜底（§11.6 F1/F2/F4）。
- **变更流程**：先改 `docs/CONTRACT.md`（用 §15 的文本），再改本文，再通知受影响开发。**跨成员的口径变更一律由 team-lead 中转**，我不直接发。
- **交付**：本文件（P0 可实施部分）。代码由 B0–B6 落地；B7 等 A＋ 合入。
