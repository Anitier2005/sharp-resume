# 功能单元任务卡 · 照片入简历 + 单页约束 + 检验机制

> 依据：`docs/PLAN-photo-fit.md` v2（已冻结）+ `docs/CONTRACT.md`（B0-1 落地后的版本）。
> 拆卡人：拆得细 · 状态：待开发认领
> **凡涉及文案 / 阈值 / 排序的，一律以方案 §13「口径已定」为准，本文件不重述口径。**

## 0. 读卡须知（每人开工前读一遍）

1. **契约冻结**：任何接口形状要先改 `docs/CONTRACT.md`，再改代码。B0-1 不合入，后面三批不许开工。
2. **阈值与文案只有一处**：`src-tauri/src/agent/facts.rs`。前端 / 渲染层 / 提示词**只引用，不复刻**。
3. **行号以方案 § 为准**，本文件写的行号是核对源码后的实际值，标「约」的允许 ±10 行。
4. 每张卡都有「不许做的事」——那是防止你顺手把下一批的活干了（会造成合并冲突）。
5. 一个卡 = 一次可提交的完整改动。卡内不做需求扩展；发现卡写得不够，先问主理人再动。

---

## 1. 批次 / 依赖总览

| 卡号 | 标题 | 批次 | 依赖 | 改动文件数 | 可并行组 |
|---|---|---|---|---|---|
| B0-1 | `CONTRACT.md` 六节 diff 落地（**闸门**） | 0 | — | 1 | G0·先 |
| B0-2 | `Cargo.toml` 显式声明 `lopdf` | 0 | B0-1 | 1 | G0 |
| B0-3 | `agent/facts.rs` 骨架：类型 + 常量 + 空实现 | 0 | B0-1 | 2 | G0 |
| B0-4 | TS 类型：`Resume` / `Asset` / `PageMeasure` / `Origin` | 0 | B0-1 | 1 | G0 |
| B0-5 | `resumeTargetPages` 三处同改（Action / reducer / OP_SPEC） | 0 | B0-1、B0-4 | 3 | G0 |
| B0-6 | `lib/measure.ts` 纯函数 + vitest 接线 + UT | 0 | B0-1、B0-4 | 5 | G0 |
| B1-1 | `pdfembed.rs`：XObject 递归遍历 + 版心 + CTM | 1 | B0-2、B0-3 | 2 | G1 |
| B1-2 | `pdfembed`：图像解码矩阵（Flate / DCT / bpc / 色彩 / SMask） | 1 | B1-1 | 1 | G1 |
| B1-3 | `attach.rs`：`read` 上提 + 页数 + `Materials` + 附件标识行 | 1 | B0-3 | 1 | G1 |
| B1-4 | `tools/extract.rs`：`extract_images` + 8 张规则 + 三失败文案 | 1 | B1-2、B1-3 | 2 | G1 |
| B1-5 | `render::photo_of` 候选豁免 | 1 | B0-4 | 1 | G1（任意时点） |
| B1-6 | 注册 `extract_images` + 工具数断言 39 + 角色断言 | 1 | B1-4 | 1 | G1·末 |
| B2-1 | `facts::fit_of` + `measure_line` 四态文案 | 2 | B0-3 | 1 | **可与 G1 并行** |
| B2-2 | `facts::target_of` + `{target}` 五槽 + `{verdict}` + `block()` | 2 | B2-1 | 1 | **可与 G1 并行** |
| B2-3 | `facts::measure_section` + `layout_result_line` | 2 | B2-1 | 1 | **可与 G1 并行** |
| B2-4 | `ToolCtx.facts` + `session.rs` 组 facts + `context::locate` 注入 | 2 | **B1-6**、B2-2 | 7 | G2 |
| B2-5 | 排版侧：`LayoutArgs.measure/origin` + `describe_page` 实测段 + `set_page_layout` 追加行 | 2 | B2-3、B1-3 | 2 | G2 |
| B2-6 | `set_target_pages` 工具 + 注册（数 40）+ `turn::describe` | 2 | **B1-6**、B0-5 | 4 | G2·末 |
| B3-1 | `usePageMeasure`（离屏测量 hook） | 3 | B0-6 | 1 | G3 |
| B3-2 | `provider.tsx` 离屏测量宿主 + `measure` 进 context | 3 | B3-1 | 2 | G3 |
| B3-3 | `provider.tsx` 轮末自动自查（F-010） | 3 | B3-2、B0-5 | 1 | G3 |
| B3-4 | `status-bar` 实测徽标 + `.dotWarn` + `measureTip` 用例 | 3 | B3-2 | 3 | G3 |
| B3-5 | `panel.tsx` 三项导出前置门（F-011） | 3 | B3-2 | 1 | G3 |
| B3-6 | `panel.tsx` 预览虚线改用 `measure.onePage` | 3 | B3-2 | 1 | G3 |
| B3-7 | `chat/panel.tsx` 上送 `measure` + 存 `origin` | 3 | B3-2、B2-4 | 1 | G3·末 |
| B4-1 | `prompts/system.md` 两节（页数来源 / 照片） | 4 | B0-1 | 1 | 任意 |
| B4-2 | `prompts/layout.md` 容量表降级 + `render::GUIDE` 同步 | 4 | B0-1 | 2 | 任意 |
| B4-3 | 全量验证 + 冒烟 6 条收口 | 4 | 全部 | 0 | 最后 |

**卡数 28**：Batch 0 = 6、Batch 1 = 6、Batch 2 = 6、Batch 3 = 7、Batch 4 = 3。

---

## 2. 依赖关系图

```
B0-1(CONTRACT 闸门)
 ├─ B0-2 lopdf ──┐
 ├─ B0-3 facts骨架 ┼─→ B1-1 → B1-2 ─┐
 │                 │                 ├─→ B1-4 → B1-6 ─┬─→ B2-4 ─┐
 ├─ B0-4 TS 类型 ──┼─→ B1-3 ─────────┘                 └─→ B2-6 ─┤
 │                 │      └──────────────→ B2-5（还需 B2-3）      │
 │                 ├─→ B1-5 photo_of（自由时点）                  │
 │                 ├─→ B3-1 → B3-2 ┬─→ B3-3                      │
 │                 │               ├─→ B3-4                      │
 ├─ B0-5 op 三处 ──┼───────────────┼─→ B3-3                      │
 │                 │               ├─→ B3-5                      │
 │                 │               ├─→ B3-6                      │
 │                 │               └─→ B3-7（还需 B2-4）───────────┘
 └─ B0-6 measure 纯函数 ─→ B3-1

B2-1 → B2-2 ─┐
B2-1 → B2-3 ─┴─→（B2-4 / B2-5 / B2-6）

B4-1 / B4-2 随时可写；B4-3 等全部合入
```

**三条硬串行线**（违反必冲突）：
1. `docs/CONTRACT.md` 只有 B0-1 动。
2. `src-tauri/src/tools/mod.rs` 只有 B1-6 → B2-4 → B2-6 这一个顺序动（`TOOLS` 数组与 `ToolCtx` 结构体在同一个文件里）。
3. `src/store/workspace/provider.tsx` 只有 B3-2 / B3-3 动，且同一个人。

---

## 3. 任务卡

### 【B0-1】`docs/CONTRACT.md` 六节 diff 落地 —— 全局闸门

- **批次 / 依赖**：Batch 0；无前置；**被全部卡依赖**。
- **改的文件**：`docs/CONTRACT.md`（唯一）。
- **做什么**：按方案 §4.5 逐节照抄，六处一并落地：
  1. §1.2 白名单（约 `:47-52`）加一行 `resumeTargetPages`；
  2. §1.2 载荷表（约 `:54-71`）末尾加一行（`targetPages` 省略 = 清除）；
  3. §1.2 之后的说明段（约 `:73-79`）追加两条（不进确认门 / 必须 `{...resume}` 展开）；
  4. **新增 §1.5「实测上送（不落库）」**（插在约 `:110` 之后、"## 2. 数据模型增量" 之前）：三条通道表格 + 三态说明 + 阈值只在 `facts.rs`；
  5. §2 数据模型增量（约 `:114` 之后）追加 `Resume.extra` 两键 + `Asset.extra` 三键；
  6. §3.3 表加 `set_target_pages` 一行 + 新增 §3.7「附件与抽图」；§4 之后新增 §4.1「返回文案里的实测结论」；§5 新增 §5.0「页头照片的候选豁免」；§6 文件所有权表加 `BE-domain` 一行（三个新文件）。
- **输入 → 输出**：无代码；产出是冻结后的 `CONTRACT.md`（其后一切代码照它写）。
- **契约引用**：方案 §4.5 全文（含每节的插入锚点字符串）。
- **验收标准**：
  - `grep -c resumeTargetPages docs/CONTRACT.md` ≥ 3（白名单、载荷表、§3.3 表）；
  - 文件里存在 `### 1.5`、`### 3.7`、`### 4.1`、`### 5.0` 四个新标题；
  - §1.5 里出现 `chat_send` / `layout_submit` / `PageMeasure` 三个词，且**不出现** `1.02`（阈值不许写进契约的下游说明，避免两处口径）；
  - §6 表格里出现 `pdfembed.rs`、`agent/facts.rs`、`tools/extract.rs`。
- **自测命令**：`grep -n "### 1.5\|### 3.7\|### 4.1\|### 5.0\|resumeTargetPages" docs/CONTRACT.md`（人工核对行号与锚点位置）
- **已知会打破的**：无（纯文档）。
- **不许做的事**：不许改方案 §4 已定文字（一个字都不改，包括「更大的没列」）；不许把 `MEASURE_RULER` 抄进契约；不许在契约里写实现步骤。

---

### 【B0-2】`Cargo.toml` 显式声明 `lopdf`

- **批次 / 依赖**：Batch 0；前置 B0-1（形式上的顺序，可并行）。
- **改的文件**：`src-tauri/Cargo.toml`（`[dependencies]` 段，约 `:47` 之后任意位置）。
- **输入 → 输出**：加 `lopdf = "0.34"`；不写 features、不写 default-features。
- **契约引用**：方案 §1.1 第一行、§2.2。
- **验收标准**：
  - `Cargo.lock` **不发生版本变化**（必须仍锁 `lopdf 0.34.0`，`Cargo.lock:1946-1948`）；
  - `cargo check` 通过；`Cargo.lock` 的 diff 只有 `sharp-resume` 自身依赖列表新增一行。
- **自测命令**：`cmd //c ".tools\\check.cmd"` → `git diff --stat src-tauri/Cargo.lock`（应为 +1 行）
- **已知会打破的**：无。若出现「lock 文件重排几十行」，说明 `cargo` 顺手升级了别的包 —— 回滚重来，不要带着版本漂移合入。
- **不许做的事**：**不许**加 `flate2`（已在树里但不是我们的直接依赖）；不许加 `lopdf` 的任何 feature；不许改 `[features]` 段。

---

### 【B0-3】`agent/facts.rs` 骨架：类型 + 常量 + 空实现

- **批次 / 依赖**：Batch 0；前置 B0-1；被 B2-x 全部、B1-3、B2-4 依赖。
- **改/新增的文件**：
  - 新增 `src-tauri/src/agent/facts.rs`；
  - `src-tauri/src/agent/mod.rs`（4 行 → 5 行）加 `pub mod facts;`。
- **输入 → 输出**（照方案 §3.1 一字不差地定义，**不写逻辑**）：

```rust
pub struct Origin { pub name: String, pub pages: Option<u32> }         // camelCase 序列化
pub enum   TargetSource { UserThisTurn, UserBefore, FromOrigin, Unset }
pub struct Target { pub pages: Option<u32>, pub source: TargetSource }
pub struct Previous { pub content: String, pub one_page: f64, pub content_px: f64 }
pub struct PageMeasure {   // 全字段 #[serde(default)]，前端可以不填全
    pub page_id: String, pub status: String, pub version: u32, pub content: String,
    pub one_page: Option<f64>, pub content_px: Option<f64>, pub line_height: Option<f64>,
    pub previous: Option<Previous>, pub reason: Option<String> }
pub struct PageFacts { pub origin: Option<Origin>, pub target: Target, pub measure: Option<PageMeasure>,
                       pub page_label: String, pub page_kind: String, pub turn: Option<u32> }  // derive(Clone, Default)
pub enum Fit { Fit(f64), Tolerance(f64), Overflow { pages: f64, over: f64, lines: u32 }, Unknown(Reason), Stale { .. } }
pub const FIT_MAX: f64 = 1.00;   pub const TOLERANCE_MAX: f64 = 1.02;
pub const MEASURE_RULER: &str = "A4 210×297mm、正文宽 306px、页边距 34px、预览容器宽 374px";
pub fn block / measure_section / measure_line / layout_result_line(f: &PageFacts) -> String  // 四个都先 String::new()
pub fn target_of(resume: &Value, origin: Option<&Origin>, turn: Option<u32>) -> Target      // 先返回 Unset
pub fn fit_of(m: Option<&PageMeasure>) -> Fit                                               // 先返回 Unknown(NoPreview)
```

  另需 `Reason` 枚举（`NoPreview` / `Fonts` / `Timeout` / `Empty`）+ `impl Reason { fn text(&self) -> &'static str }`，四句文案照方案 §4.4 `{原因}` 四选一。
- **契约引用**：方案 §3.1（结构体逐字）、§4.4（`{口径}` 常量）、§13（`MEASURE_RULER` 定稿）。
- **验收标准**（只断言常量，不断言文案 —— 文案是 B2 的活）：
  - `assert!(MEASURE_RULER.contains("预览容器宽 374px"))`；
  - `assert!(!MEASURE_RULER.contains("860") && !MEASURE_RULER.contains("16mm"))`（防回退，方案 §11.1 最后一行）；
  - `assert_eq!(FIT_MAX, 1.00); assert_eq!(TOLERANCE_MAX, 1.02)`；
  - `serde_json::from_value::<PageMeasure>(json!({"pageId":"p","status":"ok"}))` 成功（证明全字段有默认值，前端可以不填全）。
- **自测命令**：`cmd //c ".tools\\test-quiet.cmd facts"` + `cmd //c ".tools\\check.cmd"`
- **已知会打破的**：无（新模块，无人引用）。
- **不许做的事**：不许在这一卡里写任何一句文案（写了就会与 B2 撞车，且同一句会存在两份）；不许改 `model.rs`；不许把阈值定义在前端或其他 Rust 文件。

---

### 【B0-4】TS 类型：`Resume` / `Asset` / `PageMeasure` / `Origin`

- **批次 / 依赖**：Batch 0；前置 B0-1；被 B0-5、B0-6、B1-5、B3-x 依赖。
- **改的文件**：`src/types/index.ts`。
- **输入 → 输出**（三处追加，形状照方案 §3.2 逐字）：
  - `Resume`（约 `:78-84`）加 `targetPages?: number`（注释写「没有 = 未设置，不要自行假定一页」）与 `targetPagesTurn?: number`；
  - `Asset` 加 `role?: "candidate"` / `source?: string` / `pageIndex?: number`（渲染层据此跳过候选图）；
  - 新增 `export interface PageMeasure {...}`（`status: "ok" | "stale" | "unknown"`、`previous?`、`reason?: "no-preview" | "fonts" | "timeout" | "empty"`）；
  - 新增 `export interface Origin { name: string; pages?: number }`（`pages` 缺失 = 读不出，**不是 0**）。
- **契约引用**：`CONTRACT.md` §2（数据模型增量）、§1.5（`PageMeasure` 形状）；方案 §3.2。
- **验收标准**：
  - `npx tsc --noEmit -p tsconfig.app.json` 零错；
  - 新增处全部是可选字段（`?`），**没有一个既有字段被改名或改类型**（否则 `migrate.ts` 与存档都不兼容）。
- **自测命令**：`npx tsc --noEmit -p tsconfig.app.json`
- **已知会打破的**：无（只加可选字段；`Resume` 没有索引签名，所以必须显式声明 —— 这正是这卡存在的原因）。
- **不许做的事**：不许动 `Field` / `Block` / `Workspace`；不许把 `PageMeasure` 放进 `workspace` state（它**不落库**）；不许改 `migrate.ts`。

---

### 【B0-5】`resumeTargetPages` 三处同改（Action / reducer / OP_SPEC）

- **批次 / 依赖**：Batch 0；前置 B0-4；被 B2-6、B3-3 依赖。
- **改的文件**（三处必须同一提交，否则前端会静默丢这个 op）：
  1. `src/store/workspace/types.ts`（`Action` 联合，约 `:60` 页控制段附近）加：
     `| { type: "resumeTargetPages"; personId: string; resumeId: string; targetPages?: number; turn?: number }`
  2. `src/store/workspace/reducer.ts` 加 `case "resumeTargetPages"`：用 `mapResume(state, personId, resumeId, fn)`，内部 `const next = { ...resume }` 再 `delete`/赋值（**必须展开**，手写重建会丢 `extra` 里的键）；
  3. `src/lib/agent-actions.ts` 的 `OP_SPEC`（约 `:184-212`）加：
     `resumeTargetPages: { ids: ["personId", "resumeId"] }`（`targetPages`/`turn` 是数字，不进 `ids`）。
- **输入 → 输出**：约 18 行代码，无新依赖。
- **契约引用**：`CONTRACT.md` §1.2（白名单 + 载荷 + 两句说明）、§1.3（不进确认门）；方案 §3.3。
- **验收标准**：
  - `dispatch({type:"resumeTargetPages", personId, resumeId, targetPages: 1, turn: 12})` 后 `resume.targetPages === 1 && targetPagesTurn === 12`；
  - `dispatch({type:"resumeTargetPages", personId, resumeId})`（省略）后两个键**都被删掉**（`!("targetPages" in resume)`）；
  - `cleanOps([{type:"resumeTargetPages", personId:"p", resumeId:"r", targetPages:1}]).length === 1`；
  - `needsConfirm([该 op]) === false`。
- **自测命令**：`npx tsc --noEmit -p tsconfig.app.json` + `npm run lint`
- **已知会打破的**：`Action` 联合加成员后，任何对 `Action["type"]` 做穷尽 switch 的地方会报 `never` 不匹配 —— **本仓当前没有穷尽断言**（已核对），所以预期零红。若报错，说明你多加了一个 `default` 之外的收窄，去掉它。
- **不许做的事**：不许把这个 op 塞进 `ui`（它是简历级事实）；不许给 `resumeTargetPages` 加确认门；不许在 `OP_SPEC` 里把 `targetPages` 写进 `ids`（清除时它就是缺省的）。

---

### 【B0-6】`lib/measure.ts` 纯函数 + vitest 接线 + UT

- **批次 / 依赖**：Batch 0；前置 B0-4；被 B3-1、B3-4、B3-7 依赖。
- **改/新增的文件**：
  - 新增 `src/lib/measure.ts`：`export const A4_RATIO = 297 / 210`、`contentVersion(person, page): string`、`measureTip(measure, targetPages?): string`、`putOrigin(resumeId, origin)` / `originOf(resumeId): Origin | null` / `clearOrigin(resumeId)`（模块级 `Map`）；
  - 新增 `src/lib/measure.test.ts`；
  - `package.json`：`devDependencies.vitest`（与 vite 7 兼容档）+ `"test": "vitest run"`；
  - `vite.config.ts`：`defineConfig` 返回对象里加 `test: { environment: "node" }`；
  - `tsconfig.app.json`：若 `include` 不含 `*.test.ts` 也无需改（vitest 自己走 vite），只在 `tsc` 报错时才需处理。
- **输入 → 输出**：
  - `contentVersion(person | null, page | null): string` —— 空入参返回 `""`；序列化 `{kind, assetId, layout, blocks(canon 后), fields:[label,value,assetId], assets:[id,stored]}` 后 FNV-1a 32 → 8 位 hex。`canonBlock` 必须 `Object.keys().sort()`（Record key 顺序不稳定）。
  - `measureTip(m, target?)` —— 五句照方案 §9.2：量到（含「目标 X 页，未达标…超出 N 页 · 约 K 行正文」）/ 容忍带 / 无目标 / 量不到（按 `reason` 四选一）/ 过期。
  - `originOf` 是**只读窥视**（自动自查与 `submitLayout` 都要读同一份，取走会让第二个读到 null）。
- **契约引用**：`CONTRACT.md` §1.5；方案 §7.2（哈希算法）、§9.2（五句文案）、§11.2（vitest 决定）。
- **验收标准**（`src/lib/measure.test.ts` 至少 6 个 case）：
  - 同内容的**两个不同对象** → `contentVersion` 相等；
  - 改一个 bullet 里的一个字 → 不相等；
  - `layout.parts` 的 key 顺序不同 → 相等；
  - 只改 `page.notes` → 相等（`notes` 不计入）；
  - `measureTip` 三态各一条（`toContain("超出 0.18")` / `toContain("2% 容忍内")` / `toContain("尚未实测")`）；
  - `putOrigin("r1", {name:"a.pdf", pages:3})` → `originOf("r1")!.pages === 3`，`clearOrigin` 后为 `null`。
- **自测命令**：`npm run test`（首次需 `npm i -D vitest`）+ `npx tsc --noEmit -p tsconfig.app.json` + `npm run build`
- **已知会打破的**：`vite.config.ts` 加 `test` 字段后 `tsc -b` 可能报「`test` 不存在于 UserConfig」—— 用 `/// <reference types="vitest/config" />` 或 `import { defineConfig } from "vitest/config"` 解决（二选一，别两个都加）。
- **不许做的事**：**不许引 jsdom / @testing-library**；不许在 `measure.test.ts` 里测 `usePageMeasure`（它要真实布局，jsdom 只会给假绿）；`measureTip` **必须**留在 `measure.ts`，**不许**搬回 `status-bar.tsx`（node 环境的测试文件不能 import 引了 `.module.css` 的 `.tsx`）。

---

### 【B1-1】`pdfembed.rs`：XObject 递归遍历 + 版心面积 + CTM 显示尺寸

- **批次 / 依赖**：Batch 1；前置 B0-2（`lopdf`）、B0-3（`Origin` 只用于文档注释，可选）；被 B1-2、B1-4 依赖。
- **改/新增的文件**：
  - 新增 `src-tauri/src/pdfembed.rs`；
  - `src-tauri/src/lib.rs`（约 `:12` `mod pdfimage;` 附近）加 `mod pdfembed;`。
- **输入 → 输出**：

```rust
pub struct Embedded { pub width: u32, pub height: u32, pub rgba: Vec<u8>,
                      pub page: u32, pub disp: Option<(f64, f64)>,
                      pub alpha: bool, pub codec: &'static str }
pub struct Report { pub images: Vec<Embedded>, pub unsupported: Vec<&'static str>,
                    pub pages: usize, pub inner_area: f64 }
pub fn extract(bytes: &[u8]) -> Result<Report, String>;   // Err("open") = 打不开
```
  内部：`page_media_box(&doc, page_id)`（沿 `/Parent` 上溯，读不出退 A4 595.28×841.89）、`inner_area_of(media)`（版心口径照方案 §2.3 的常量，A4 → 357,332pt²）、`display_sizes(&doc, page_id)`（`Content::decode` → `q/Q/cm/Do` 状态机，`disp = (hypot(a,b), hypot(c,d))`）、`walk(...)`（`depth > 8` 停手、`seen: HashSet<ObjectId>` 去重、`get_page_resources` 已沿 Parent 上溯）。
  这一卡里 `Image` 分支先只填 `width/height/page/disp/codec`，`rgba` 留空、`codec = "raw"`（B1-2 接手解码）。
- **契约引用**：方案 §3.1（两个结构体）、§5.1（遍历伪码）、§2.3（版心与 CTM 口径，含 `PAD_X_RATIO`/`PAD_Y_RATIO` 的推导）。
- **验收标准**（in-file `#[cfg(test)]` 用 `lopdf` 现场造 PDF）：
  - 一页、一个 `Do` 引用图像 XObject，`cm` 为 `0.3749 0 0 0.3695 e f`，图像 413×531 → `disp` ≈ (154.8, 196.2)、`pct ≈ 8`、`near_page = false`；
  - `cm` 铺满版心 → `pct ≈ 100`、`near = true`；
  - 没有 `Do` 对应的 XObject（只写在 `Resources` 里没画）→ `disp = None`，**不 panic**、`pct` 记「不可测」；
  - `extract(b"not a pdf")` → `Err("open")`；截断的 PDF → 不 panic；
  - 加密文档 `decrypt("")` 失败 → `Err("open")`。
- **自测命令**：`cmd //c ".tools\\test-quiet.cmd pdfembed"` + `cmd //c ".tools\\check.cmd"`
- **已知会打破的**：无（新模块）。若把 `mod pdfembed;` 加进 `lib.rs` 但文件没建，`cargo check` 会报「file not found for module」—— 同一提交里建文件。
- **不许做的事**：**不许**用 `Document::get_page_images`（方案 §2.1 已列五处不能用）；不许追 Form 内部的 `cm`（沿用调用方 CTM，方案 §5.1 取舍）；不许处理内联图（`BI…ID…EI` 直接跳过并记 `unsupported["inline"]`）；不许拿渲染宽 1240px 当版心标尺。

---

### 【B1-2】`pdfembed`：图像解码矩阵（Flate / DCT / bpc / 色彩空间 / SMask）

- **批次 / 依赖**：Batch 1；前置 B1-1；被 B1-4 依赖。
- **改的文件**：`src-tauri/src/pdfembed.rs`（新增 `decode_image(&doc, &stream) -> Result<Embedded, &'static str>` + `probe_decompress(&stream) -> Result<Vec<u8>, &'static str>` + `expand_bpc` / `to_rgba` 私有函数）。
- **输入 → 输出**：`Err` 的原因是 `&'static str`，取值只能是 `"JPX"` / `"CCITT"` / `"JBIG2"` / `"bpc"` / `"colorspace"` / `"too-large"` / `"filters"`（多 filter）；`Ok` 时 `rgba` 是解码后的 RGBA8，`alpha = smask 合成成功`。
- **关键实现（唯一允许的写法）**：

```rust
let mut probe = stream.clone();            // Stream { dict, content }，局部克隆
probe.dict.remove(b"Subtype");             // 绕过 lopdf 对图像流的拒解
let raw = probe.decompressed_content()?;   // 走 lopdf 自己的 zlib + 5 种 PNG predictor
```
  这一行**必须**带方案 §15 第 1 处的注释原文（中文，说清「probe 是局部克隆，文档对象没被改；不这么写就得自己实现 5 种 predictor，那是静默花屏的高发区」）。
- **契约引用**：方案 §2.2（解码路径与代价）、§5.2（解码矩阵表，逐行照做）、§15 第 1 处注释。
- **验收标准**（in-file UT，`lopdf` 造流）：
  - 1 张 `DCTDecode` + 1 张 `FlateDecode`（8bpc RGB）+ 1 张 `JPXDecode` 的 PDF → `images.len() == 2`、`unsupported == ["JPX"]`、两张的 `page` 都是 1；
  - `Predictor 12` 的 Flate 流解码成功且**首行像素正确**（防 predictor 走错路：用 2 行 × 3 通道的已知数据断言首字节）；
  - `BitsPerComponent = 16` → 取高字节，`images[0].width == 声明宽`；
  - `DeviceCMYK` → 输出 4 通道合成后的 RGB（取一个已知 CMYK 值断言数值，公式为乘积式 `C'=(255-C)*(255-K)/255`）；
  - `/Indexed` 调色板映射正确（2 色调色板，断言两个像素）；
  - 宽或高 > 4000 → `unsupported` 含 `"too-large"`，`images` 为空，**不 panic**；
  - `/SMask` bpc=8 → `alpha == true` 且 RGBA 的 A 通道等于 SMask 值；SMask 解不出 → `alpha == false`、图仍然产出。
- **自测命令**：`cmd //c ".tools\\test-quiet.cmd pdfembed"`
- **已知会打破的**：B1-1 里「Image 分支只填元数据」的断言如果写死了 `codec == "raw"`，这里会红 —— B1-1 的断言要写成「不关心 codec」，或本卡同批改掉。
- **不许做的事**：**不许**引 `flate2` 直接依赖；**不许**自己写 predictor；不许为 JPEG 做二次编码（`content` 就是完整 JPEG，需要时直接当字节用）；任何一条异常路径都返回 `Err(原因)`，**不许 `unwrap`/`panic`**。

---

### 【B1-3】`attach.rs`：`read` 上提 + PDF 页数 + `Materials` 结构 + 附件标识行

- **批次 / 依赖**：Batch 1；前置 B0-3（`facts::Origin`）；被 B1-4、B2-4、B2-5 依赖。
- **改的文件**：`src-tauri/src/attach.rs`（`fn read` 约 `:160`；`struct Parsed` 约 `:56`；`fn parse` 约 `:167`；`pub async fn materials` 约 `:214`）。
- **输入 → 输出**：

```rust
fn read(root: &Path, stored: &str) -> Result<Vec<u8>, String>          // 由私有改 pub(crate)
struct Parsed { text: String, scanned: bool, note: String, pages: Option<u32> }
pub struct Materials { pub section: String, pub images: Vec<String>,
                       pub origin: Option<crate::agent::facts::Origin> }
pub async fn materials(root, refs, vision, ocr) -> Materials            // 原来是 (String, Vec<String>)
```
  - `pages` 在 `parse("pdf", data)` 里算：`lopdf::Document::load_mem(bytes).and_then(|d| Ok(d.get_pages().len()))`，失败 `None`（**绝不写 0**）；并跟着 `Parsed` 进缓存（同一份内容只解析一次）。
  - `origin`：取**本轮第一个 `ext == "pdf"` 的附件** → `Origin { name, pages }`；没有 PDF 附件 → `None`。
  - 附件段每个 PDF 块里补一行「附件标识」：`（附件标识：{stored}）`，让模型能把它传给 `extract_images`。
- **契约引用**：`CONTRACT.md` §3.7（`stored` 是存储名、不是文件名）；方案 §1.1 attach 行、§2.8（页数算法与缓存键）。
- **验收标准**：
  - 造一份 2 页 PDF 写进 temp `inbox/` → `materials(...).origin == Some(Origin{name:"x.pdf", pages:Some(2)})`；
  - 传 `b"not a pdf"` 的 `.pdf` 附件 → `origin.pages == None`（且 `section` 里仍有那句「读不到内容」，行为不回退）；
  - `section` 里出现 `（附件标识：<stored>）`；
  - 纯 `.md` 附件 → `origin == None`；
  - 同一份文件两次 → `parse` 只跑一次（用缓存命中断言，或在 `Parsed` 上加计数器只在 test 里用）。
- **自测命令**：`cmd //c ".tools\\test-quiet.cmd attach"`
- **已知会打破的（必改）**：
  - `attach.rs:437` `let (section, images) = materials(...)` → 改 `.section` / `.images`；
  - `attach.rs:455` `let (section, _) = materials(...)` → 改 `.section`；
  - `session.rs:502` 调用点仍在解构元组（B2-4 才改）→ **本卡必须同时把它改成 `let materials = ...` 并只用 `.section`/`.images`**，否则编译不过。这两个文件的这一处改动属于本卡（B2-4 只负责加 `measure`/`origin` 字段）。
- **不许做的事**：不许把页数写进 `workspace`／数据库（它不落库，方案 §2.8）；不许改 `PDF → 图片` 的既有渲染逻辑；不许在附件段里列出「有几张内嵌图」（那是 `extract_images` 的活）。

---

### 【B1-4】`tools/extract.rs`：`extract_images` + 8 张规则 + 三失败文案

- **批次 / 依赖**：Batch 1；前置 B1-2（`pdfembed::extract`）、B1-3（`attach::read`）；被 B1-6 依赖。
- **改/新增的文件**：
  - 新增 `src-tauri/src/tools/extract.rs`：`pub fn extract_images_params() -> Value`、`pub fn extract_images(args: &Value, ctx: &ToolCtx) -> Result<String, String>`、`fn ensure_source_asset(ctx) -> Result<String /*stored*/, String>`（幂等材料化）；
  - `src-tauri/src/tools/mod.rs`：**只加 `pub mod extract;` 这一行**（注册留给 B1-6）。
- **输入 → 输出**：
  - 参数 `{ stored: String (必填), name?: String }`，`additionalProperties: false`；
  - 产物：`push_asset` 落池的 `Asset`（`extra: {role:"candidate", source:<stored>, pageIndex:<页序>}`）+ 候选清单文本。
  - `assetId == stored 名`（`<sha 前16>.<ext>`）→ 用 `pool::put_at(root, id, name, &png_bytes)`，`id` 直接用 `snapshot::pool_name(&sha, Some("png"))` 的结果，**不要** `uuid_like()`。
  - PNG 编码：`image::RgbaImage::from_raw(...)` → `write_to(Cursor, ImageFormat::Png)`。
- **8 张上限规则（方案 §13 第 7 条，逐条实现）**：
  1. 候选按 `pct` **升序**（小的先），`pct` 不可测的排最后并按像素面积升序兜底；
  2. 落池最多 8 张，从上往下取；
  3. 清单只列这 8 张，超过时末尾追加 `还有 {k} 张更大的没列（一次最多列 8 张），需要的话说「把剩下的也抽出来」。`；
  4. **无分页参数**：已登记的（`workspace` 里 `person.assets` 按 id 找到 **或** `ctx.out.assets` 里已有）按 id 跳过、不重复 `push`、**不占这 8 个名额**，所以第二次调用天然拿到下一批。
- **契约引用**：`CONTRACT.md` §3.7、§1.2（写 op 走 `mutate`，本工具无 op，只 `push_asset`）；方案 §3.4（schema）、§4.4（清单与三失败文案逐字）、§2.9（assetId = stored 名）。
- **验收标准**（`tools/extract.rs` in-file UT，多数不需要真 PDF）：
  - **无内嵌位图** → 文案含「没有内嵌位图」且含「照片没有放上去」；
  - **有图读不出**（造 `unsupported == ["JPX","JPX"]`）→ 含「2 张都读不出来」+「编码 JPX 暂不支持」+「照片没有放上去」；
  - **打不开**（`Err("open")`）→ 含「打不开（文件损坏或加密）」+「照片没有放上去」；
  - 三类文案**互不相同**（`assert_ne!` 两两比）；
  - **材料登记失败**（`ctx.root == None`）→ 含「【材料登记】」+「照片没有放上去」；
  - **11 张图的 PDF**：第一次 `ctx.out.assets` 增 8 条、文案含 `还有 3 张更大的没列`；把首批 8 个 id 塞进 `workspace.people[0].assets` 再调一次 → 只剩 3 条、**id 与首批零重复**；
  - **幂等**：同一 `stored` 连调两次（不预置已登记）→ `out.assets` 只多 8 条，第二次返回既有 assetId 的清单；
  - 清单每行形如 `` `a3f19c04b2e6.png`｜第 2 页｜413×531 px｜宽高比 0.78｜不透明｜不接近整页（占版心 8%）｜21.4 KB ``（有 `pct` 与无 `pct` 两种都要有用例）。
- **自测命令**：`cmd //c ".tools\\test-quiet.cmd extract"`
- **已知会打破的**：`tools/mod.rs:718` 的 `session.len() == 38` **本卡不改**（`pub mod extract;` 不增工具数，注册在 B1-6）—— 若你顺手注册了，这卡就已经越界，推翻重做。
- **不许做的事**：不许加 `skip` / `offset` / `page` 参数；不许依赖 `push_asset` 的静默截断（上限 8 在 `tools/mod.rs:122-132`，自己控数量）；不许判断「哪张是证件照」；不许把图写进 `person.assets`（只经 `push_asset` 出站，由前端 `addAssets` 落库）。

---

### 【B1-5】`render::photo_of` 候选豁免

- **批次 / 依赖**：Batch 1；前置 B0-4（`Asset.role` 类型）；无需等其它 B1 卡；被 B0-4 之后的全部渲染行为依赖。
- **改的文件**：`src-tauri/src/render/mod.rs`（`fn photo_of` 约 `:817-823`；测试约 `:1012`、`:1279`、`:1292`）。
- **输入 → 输出**：`fn photo_of(person: &Person) -> Option<String>` 逻辑改为：先找 `kind == "image"` **且** `extra.role != "candidate"` 的第一张；找不到再退回「第一张 `kind == "image"`」（老数据没有 `role` 键，行为不变）。
- **契约引用**：`CONTRACT.md` §5.0（逐字原因说明）；方案 §2.9、§15 第 3 处注释。
- **验收标准**（in-file UT）：
  - `person.assets = [candidate 图, 真照片]` → 返回**真照片** id；
  - 老数据（两张图都没有 `role`）→ 返回第一张（向后兼容，升级后用户的证件照不会消失）；
  - 全是 candidate → 返回 `None`（`head` 里不出 `<img>`），不 panic。
- **自测命令**：`cmd //c ".tools\\test-quiet.cmd render"`
- **已知会打破的**：`render/mod.rs` 既有断言 `src="asset:a-photo"`（约 `:1017`、`:1131`、`:1280-1293`）—— 它们的 fixture 没有 `role` 键，**预期不红**；若红了，说明你把「退回第一张」这一步漏了。
- **不许做的事**：不许改 `find_asset`（候选图仍要能被 `render_media` / `parts` 引用）；不许在 `render` 里过滤掉 candidate（模型写 HTML 引用它是合法的）；不许动 `GUIDE`（那是 B4-2）。

---

### 【B1-6】注册 `extract_images` + 工具数断言 39 + 角色断言

- **批次 / 依赖**：Batch 1；前置 B1-4；**Batch 2 的 B2-4 必须等这一卡合并**。
- **改的文件**：`src-tauri/src/tools/mod.rs`（`TOOLS` 数组、`capability_map_is_one_line_per_tool` 测试约 `:711-763`）。
- **输入 → 输出**：`TOOLS` 里加一项：`name: "extract_images"`、`description`（写「什么时候该用」：用户上传的 PDF 里可能带证件照；`stored` 从材料段的「附件标识」行取，**不是文件名**）、`params: extract::extract_images_params`、`run: extract::extract_images`、`scopes: &[Scope::Session]`、`writes: true`。**放在「页面控制」段之前的内容写入段末尾**，别插到 `Layout` 工具里。
- **契约引用**：`CONTRACT.md` §3.7；方案 §4.3（Session 38 → 40 的中间态是 39）、§14 硬约束 2（`TOOLS` 一次只许一个人动）。
- **验收标准**：
  - `assert_eq!(session.len(), 39)`（本卡只加一个），`assert_eq!(layout.len(), 9)` 不变；
  - `assert!(session.contains(&"extract_images"))`、`assert!(!layout.contains(&"extract_images"))`；
  - `assert!(!session.contains(&"set_target_pages"))`（那是 B2-6）；
  - `specs(Scope::Session)` 里该项 `parameters.required == ["stored"]`。
- **自测命令**：`cmd //c ".tools\\test-quiet.cmd tools"`
- **已知会打破的（必改）**：`tools/mod.rs:718` `assert_eq!(session.len(), 38)` → 39（这条现在是**必然红**，属于本卡验收范围）。
- **不许做的事**：**不许**在这一卡加 `set_target_pages`（B2-6 才加，两人同时动 `TOOLS` 数组必冲突）；不许给 Layout 角色开口子；不许改 `push_asset` 的上限。

---

### 【B2-1】`facts::fit_of` + `measure_line` 四态文案

- **批次 / 依赖**：Batch 2；前置 B0-3；**可与 Batch 1 并行**（只碰 `facts.rs`）。
- **改的文件**：`src-tauri/src/agent/facts.rs`（填 `fit_of` 与 `measure_line` 两个空实现 + UT）。
- **输入 → 输出**：
  - `fit_of(Option<&PageMeasure>) -> Fit`：`status != "ok"` → `Unknown(Reason)`（`reason` 缺失退 `NoPreview`）/ `stale` → `Stale{..}`；`pages = content_px / one_page`；`pages <= 1.00` → `Fit`；`<= 1.02` → `Tolerance`；否则 `Overflow { pages, over: pages - 1.0, lines }`，`lines = round(over_px / line_height)`，`line_height` 缺失退 `17`。
  - `measure_line(f: &PageFacts) -> String`：四态逐字取方案 §4.4 的 `{measure}` 槽（fit / overflow / 容忍带 / 不可用 / 过期），`{口径}` 用 `MEASURE_RULER` 常量拼（**不许**在函数里再写一遍字符串）。
- **契约引用**：`CONTRACT.md` §1.5（三态）、§4.1（措辞必须与「达标」明显不同）；方案 §4.4 `{measure}` 五条、§5.3（行数折算公式）、§11.1 的 F-007/8 行。
- **验收标准**（逐字 `assert_eq!` / `assert!`）：
  - `pages = 1.00` → `Fit`；`1.005` → `Tolerance` 且文案含「2% 容忍内」；`1.02` → `Tolerance`；`1.021` → `Overflow`；
  - `Overflow.over` 保留 2 位（`1.18 → 0.18`）；
  - `line_height = 17.0`、`content_px - one_page = 187` → `lines == 11`；
  - `line_height` 为 `None` → 用 17 折算，不 panic；
  - unknown 时文案含「尚未实测」且**不含**「没有溢出」（这条是 F-007 的核心）；
  - 所有 ok 态文案都含 `MEASURE_RULER` 全文；`assert!(!text.contains("860") && !text.contains("16mm"))`。
- **自测命令**：`cmd //c ".tools\\test-quiet.cmd facts"`
- **已知会打破的**：无（只填空实现）。
- **不许做的事**：不许在这一卡写 `block()` / `target_of` / `verdict`（B2-2）；不许把阈值改成字面量 1.0/1.02（用 `FIT_MAX` / `TOLERANCE_MAX`）；不许在 `measure_line` 里判「有没有设目标」（那是 verdict 的事）。

---

### 【B2-2】`facts::target_of` + `{target}` 五槽 + `{verdict}` + `block()`

- **批次 / 依赖**：Batch 2；前置 B2-1；可与 Batch 1 并行。
- **改的文件**：`src-tauri/src/agent/facts.rs`（`target_of` / `target_line`（私有）/ `verdict_line`（私有）/ `block`）。
- **输入 → 输出**：
  - `target_of(resume: &Value, origin: Option<&Origin>, turn: Option<u32>) -> Target`：`targetPages` 存在时，`targetPagesTurn + 1 >= current_turn` → `UserThisTurn`，否则 `UserBefore`（排版侧 `turn == None` 一律 `UserBefore`）；否则 `origin.pages == Some(n) && n <= 3` → `FromOrigin(n)`；否则 `Unset`。
  - `block(f: &PageFacts) -> String`：照 §4.4 的六行骨架，槽位**按状态取一条，不拼接不省略**。
  - **file 页短路**：`f.page_kind == "file"` → 只返回 `结论：这一页是用户上传的整页原件，没有可排的空间，不做页数判定。`（方案 §13 第 5 条）。
- **契约引用**：`CONTRACT.md` §1.5；方案 §4.4（`{n}` / `{target}` 五条 / `{verdict}` 五条，全部逐字）、§2.7（三态推导）、§13 第 3、4、5 条。
- **验收标准**（逐字 `assert_eq!`，`target_of` 四种 + 槽位五条 + verdict 五条）：
  - `targetPages=Some(1), targetPagesTurn=Some(11), turn=Some(12)` → `UserThisTurn`；
  - 同上前提 `turn=Some(13)` → `UserBefore`；
  - `origin.pages=Some(3)`、无 targetPages → `FromOrigin(3)`；`origin.pages=Some(4)` → `Unset`；
  - `{target}` 五条逐字：`1 页（用户这一轮明确要求：压到一页）` / `1 页（用户之前要求过，仍然有效）` / `3 页（沿用原件页数：原件只有 3 页，用户没有另说）` / `未设置（用户没有说过要几页，任何页数都合规；不要自行假定一页）` / `未设置（原件页数读不出，用户也没有说过要几页）`；
  - **n > 1 的括注**：`targetPages=Some(2), UserThisTurn` → 含 `压到 2 页`（方案 §13 第 3 条）；
  - `{n}` 三分支：正常 `页数 3 页（来源：PDF 页树）`；读不出 = 整行 `原件：{原名}，页数读不出（文件损坏或不是 PDF）。不要按 1 页算，也不要猜。`；无附件 = 整行 `原件：这一轮没有上传原件。`；
  - `{verdict}`：未设目标 + overflow 含「实测 1.18 页这件事仍然要说准」；未设目标 + fit 含「实测 1.00 页，仍然要照报。」；unknown 含「不要说「一页」」；
  - file 页：`block()` 的输出 `assert!(!text.contains("实测"))` 且只含那一条 verdict。
- **自测命令**：`cmd //c ".tools\\test-quiet.cmd facts"`
- **已知会打破的**：无。注意 `Target.pages` 是 `Option<u32>` 而 `FromOrigin` 的 `pages` 来自 `Option<u32>` —— 别写出 `pages: 0` 的分支（读不出**绝不**写 0，方案 §2.8）。
- **不许做的事**：不许在这一卡改 `fit_of` / `measure_line`（B2-1 已定）；不许把 file 页做成「只跳过 verdict 但还实测」（§13 第 5 条是整块短路）；不许在 `block()` 里数 user 消息条数（`turn` 由调用方给）。

---

### 【B2-3】`facts::measure_section` + `layout_result_line`

- **批次 / 依赖**：Batch 2；前置 B2-1；被 B2-5 依赖。
- **改的文件**：`src-tauri/src/agent/facts.rs`。
- **输入 → 输出**：
  - `measure_section(f) -> String`：带 `## 这一页的实测` 标题的三行块（fit / overflow 插一行「超出 0.18 页（11 行正文）。」/ unknown / 未设目标），照方案 §4.4「describe_page 的实测段」逐字。
  - `layout_result_line(f) -> String`：三态逐字 —— 达标 `实测 1.00 页，没有溢出（目标 1 页，达标）。` / 未达标 `实测 1.12 页，超出 0.12 页（目标 1 页，仍未达标）。继续压，或停在这一档并说清差多少。` / 量不到 `尚未实测（测量不可用：预览没打开）。这一轮不能确认是否在一页内 —— 不要写「已压到一页」。`
- **契约引用**：`CONTRACT.md` §4.1（三态 + 「措辞必须与达标明显不同」）；方案 §4.4 两段、§11.1 的 F-008/F-009 行。
- **验收标准**：
  - `measure_section` 输出以 `## 这一页的实测` 开头；unknown 时含「尚未实测」且**不含**「没有溢出」；overflow 时含「超出 0.18 页（11 行正文）。」；
  - 未设目标时第二行含「未设置（用户没有说过要几页）」、第三行含「没有目标，不讲达标与否；实测值仍然要照报。」；
  - `layout_result_line` 三态逐字 `assert_eq!`；
  - 两者对同一 `PageFacts` 的「实测 1.00 页，没有溢出」措辞**完全一致**（用 `measure_line` 的返回值做 `contains` 断言，证明是同一来源）。
- **自测命令**：`cmd //c ".tools\\test-quiet.cmd facts"`
- **已知会打破的**：无。
- **不许做的事**：不许在 `measure_section` 里自己拼 `{口径}` 字符串（用 `MEASURE_RULER`）；不许把 `##` 标题换成 `###`（提示词按 `##` 找）；不许改 `layout.md` / `GUIDE`（B4-2）。

---

### 【B2-4】`ToolCtx.facts` + `session.rs` 组 facts + `context::locate` 注入

- **批次 / 依赖**：Batch 2；前置 **B1-6**（`tools/mod.rs` 独占）、B2-2；被 B3-7 依赖。
- **改的文件（7 个，含 10 处字面量）**：
  1. `src-tauri/src/tools/mod.rs`：`pub struct ToolCtx`（约 `:63-72`）加 `pub facts: Option<&'a crate::agent::facts::PageFacts>`；`ToolCtx::of`（约 `:76-91`）里 `facts: None`；
  2. `ToolCtx { … }` 字面量补 `facts: None`（**全部 10 处**，已核对）：`tools/mod.rs:808`、`tools/mod.rs:845`、`tools/image.rs:314`、`tools/image.rs:347`、`tools/read.rs:577`、`tools/read.rs:694`、`tools/read.rs:733`、`tools/edit.rs:2274`、`net.rs:533`、`agent/layout.rs:520`（后者由 B2-5 给真值，本卡先 `None`）；
  3. `src-tauri/src/session.rs`：`SendArgs`（约 `:430-471`）加 `#[serde(default)] pub measure: Option<crate::agent::facts::PageMeasure>`；`SendResult`（约 `:421-426`）加 `pub origin: Option<crate::agent::facts::Origin>`；`chat_send` 里组 `PageFacts`；
  4. `src-tauri/src/agent/context.rs`：`locate()`（`:32-87`）加第 6 个参数 `facts: &PageFacts`，在 `:84`（视图行）与 `:85`（收尾句）之间插入 `out.push('\n'); out.push_str(&facts::block(facts));`，并在调用点 `session.rs:515-521` 传参。
- **组装点**（方案 §6 的表，逐格填）：
  `origin` = `materials.origin`（B1-3）；`target` = `target_of(&resume_json, origin.as_ref(), Some(user_count))`；`measure` = `args.measure`；`turn` = `Some(user_count)`；`page_label` / `page_kind` 从 `resume.pages` 里按 `args.page_id` 取（`..unwrap_or("")`）；`user_count` = `state.list(&args.key)` 里 `role == "user"` 的条数。
- **契约引用**：`CONTRACT.md` §1.5、§2；方案 §2.5（不落库、不做全局缓存）、§2.7（`current_turn` 定义）、§6（组装点表格 + 注入位置）。
- **验收标准**：
  - `cargo check` 零错（10 处字面量全补上）；
  - 单测：`locate(..., &facts)` 的输出里 `【页面事实】` 出现在「- 视图：」之后、「- 你可以用工具读取」之前；
  - 单测：`measure = None` + `origin = None` 时 `locate()` 仍含「原件：这一轮没有上传原件。」，且不 panic；
  - 单测：同一 `SendArgs` 连跑两次（第二次 `measure` 不同）→ 第二次输出用的是新值（证明**没有跨请求缓存**）。
- **自测命令**：`cmd //c ".tools\\check.cmd"` + `cmd //c ".tools\\test-quiet.cmd session"`
- **已知会打破的（必改）**：
  - 10 处 `ToolCtx` 字面量（方案 §11.4 只列了 5 处，实际 10 处 —— 少一个都编不过）；
  - `session.rs:502` 若 B1-3 已改则不动；`session.rs:515-521` 的 `locate()` 调用要加参数；
  - `agent/context.rs` 没有测试，若你加了测试，注意新参数。
- **不许做的事**：不许做 `OnceLock<Mutex<HashMap>>` 之类的进程级缓存（方案 §2.5 明确禁止）；不许把 `measure` / `origin` 写进 `workspace` 或落库；不许改 `SendResult` 里既有的 `id/actions/assets`；不许在 `locate()` 里再拼事实文案（只调 `facts::block`）。

---

### 【B2-5】排版侧：`LayoutArgs.measure/origin` + `describe_page` 实测段 + `set_page_layout` 追加行

- **批次 / 依赖**：Batch 2；前置 B2-3、B1-3；可与 B2-4 并行（不同文件）。
- **改的文件**：
  1. `src-tauri/src/agent/layout.rs`：`LayoutArgs`（`:42-53`）加 `#[serde(default)] pub measure: Option<PageMeasure>` 与 `#[serde(default)] pub origin: Option<Origin>`；`describe_page(args)`（`:234`）签名改 `describe_page(args: &LayoutArgs, facts: &PageFacts)`；在 `:408`（`正文合计约 {chars} 字` 之后、`:411`「当前版面」之前）追加 `out.push_str(&facts::measure_section(facts));`；`run_turn`（`:477`）在 `:483` 之前组 `facts`（`turn: None`），并把它同时传给 `describe_page` 与 `ToolCtx.facts`（`:520`）；
  2. `src-tauri/src/tools/layout.rs`：`set_page_layout`（`:168`）返回值（`:285`）改为 `format!("这一页的版面已更新：{}\n{}", notes.join("、"), facts::layout_result_line(facts))`，`facts` 从 `ctx.facts` 取，`ctx.facts == None` 时**不追加**（老调用点/测试友好）。
- **输入 → 输出**：`facts` 组装表格照方案 §6 第二行：`origin = args.origin`、`target = target_of(&resume_json, origin, None)`、`measure = args.measure`、`turn = None`。`resume_json` 按 `args.person_id` / `args.resume_id` 从 `args.workspace` 取（`describe_page` 里已有同款查法）。
- **契约引用**：`CONTRACT.md` §4.1、§1.5（`layout_submit` 两个新字段）；方案 §1.2（三行改动）、§4.4（describe_page 实测段 + 追加行）、§6（注入位置与理由）。
- **验收标准**：
  - `describe_page` 输出里 `## 这一页的实测` 出现在「正文合计约 N 字」之后、「当前版面：」之前（顺序错则红）；
  - `args.measure = None` → 该段出现「尚未实测（预览没打开，量不到）」；
  - `set_page_layout` 的返回在 fit 时含「（目标 1 页，达标）」；`ctx.facts = None` 时返回与今天完全相同（`assert!(!text.contains("实测"))`）；
  - 排版侧 `target` 只可能是 `UserBefore` / `FromOrigin` / `Unset`（`turn = None`，用一个 `UserThisTurn` 前置数据断言不会出现）。
- **自测命令**：`cmd //c ".tools\\test-quiet.cmd layout"`
- **已知会打破的（必改）**：
  - `agent/layout.rs:668-669` 的 `fn sample() -> LayoutArgs` 字面量 —— 新字段必须补（`measure: None, origin: None`）；
  - `agent/layout.rs` 里所有 `describe_page(&args)` 调用点（至少 `:483`）；
  - `tools/layout.rs` 里 `set_page_layout` 的既有测试若断言返回值全文，会因新增行而红 —— 改成 `contains` 关键片段；
  - `net.rs:533` / `agent/layout.rs:520` 的 `ToolCtx` 字面量已在 B2-4 补 `None`，本卡只把 `:520` 换成真值。
- **不许做的事**：不许在排版侧传 `current_turn`（方案 §2.7：排版侧一律 `None`）；不许把实测段插到 `describe_page` 开头（顺序有理由，见方案 §6）；不许在 `set_page_layout` 里重新算页数（用 `layout_result_line`）；不许碰 `provider.tsx`。

---

### 【B2-6】`set_target_pages` 工具 + 注册（数 40）+ `turn::describe`

- **批次 / 依赖**：Batch 2；前置 **B1-6**、B0-5；**最后一个动 `tools/mod.rs` 的卡**。
- **改的文件**：
  1. 新增 `src-tauri/src/tools/target.rs`：`pub fn set_target_pages_params() -> Value`（`pages` 整数、1–10、非必填、`additionalProperties: false`，描述照方案 §3.4 逐字）、`pub fn set_target_pages(args: &Value, ctx: &ToolCtx) -> Result<String, String>`；
  2. `src-tauri/src/tools/edit.rs`：**只加一个 5 行 `pub fn push_op(ctx: &ToolCtx, op: Value)`**（内部 `with_state(ctx.out, |st| st.mark_op(op))`）—— 目的：让本工具产出的 op 与同轮内容改动走同一条 `mutate` 信封，`Ordering` 天然正确；
  3. `src-tauri/src/tools/mod.rs`：`pub mod target;` + `TOOLS` 加 `set_target_pages`（`scopes: &[Scope::Session]`、`writes: true`）+ 断言 39 → 40；
  4. `src-tauri/src/agent/turn.rs`：`describe()`（`:58-82`）的 `match` 加两条人话 —— `"extract_images" => "在原件里找内嵌图片".to_string()`、`"set_target_pages" => 按 args["pages"] 给「把目标定成 N 页」/「清掉目标页数」`。
- **输入 → 输出**：`pages` 有值 → `push_op({type:"resumeTargetPages", personId: ctx.person_id, resumeId: ctx.resume_id, targetPages: n, turn: ctx.out.turn})`；省略 → `push_op({… 不含 targetPages, turn …})`（= 清除）。返回文案两句：`已记下：这份简历压到 {n} 页。` / `已清除目标页数：不再按页数判达标。`
  - **`turn` 字段**：`Outbox` 加 `pub turn: u32`（默认 0），`chat_send` 里写一次 `out.turn = user_count`（这是 B2-4 已算出来的值，本卡补一行赋值）。
- **契约引用**：`CONTRACT.md` §1.2（op 白名单 + 载荷）、§1.3（不进确认门）、§3.3（工具表）；方案 §3.4（schema）、§2.7（`Outbox.turn`）。
- **验收标准**：
  - `call("set_target_pages", {"pages":1}, &ctx, Scope::Session)` 后 `flush_all(out)` 的 `ops[0]` == `{"type":"resumeTargetPages","personId":"p1","resumeId":"r1","targetPages":1,"turn":0}`；
  - 不传 `pages` → `ops[0]` 里**没有** `targetPages` 键；
  - `pages: 0` / `pages: 11` / `pages: "1"` → 报错且 `out.edits` 里无残留（用一次 `flush_all` 返回 `None` 断言）；
  - `assert_eq!(session.len(), 40)`、`assert_eq!(layout.len(), 9)`、`assert!(!layout.contains(&"set_target_pages"))`；
  - `turn::describe("set_target_pages", &json!({"pages":1}))` 含「1 页」。
- **自测命令**：`cmd //c ".tools\\test-quiet.cmd tools"` + `cmd //c ".tools\\check.cmd"`
- **已知会打破的（必改）**：
  - `tools/mod.rs:718` `assert_eq!(session.len(), 39)` → 40（B1-6 已推到 39，本卡推到 40）；
  - `edit.rs` 里若有对 `EditState` 私有方法可见性的测试假设，补 `push_op` 不影响它们；但 `edit.rs` 的 `OP_LIMIT = 200` 逻辑别动；
  - `turn.rs` 的 `describe` 是纯函数，无既有测试断言 fallback 全文（已核对），预期零红。
- **不许做的事**：不许改 `EditState` 的其它逻辑（`push_op` 只是把已有私有 `mark_op` 暴露出来）；不许把 `resumeTargetPages` 做成 `patchUi` 之类的界面指令；不许给这个工具加 `resumeId` 参数（来自 `ToolCtx`）；不许同时再改 `TOOLS` 里别的项。

---

### 【B3-1】`usePageMeasure`（离屏测量 hook）

- **批次 / 依赖**：Batch 3；前置 B0-6；被 B3-2 依赖。
- **改的文件**：`src/lib/measure.ts`（追加 hook，不动已有的纯函数）。
- **输入 → 输出**：

```ts
export function usePageMeasure(input: {
  hostRef: React.RefObject<HTMLDivElement | null>;
  person: Person | null;
  page: ResumePage | null;
}): PageMeasure | null;
```
  内部按方案 §7.1 的 7 步：`content = useMemo(contentVersion)`；`read()` 取 `hostRef.current.firstElementChild`，`w = offsetWidth`（0 → `unknown("no-preview")`）、`h = offsetHeight`（`<= 0` → `unknown("empty")`）、`onePage = w * A4_RATIO`、`lineHeight = parseFloat(getComputedStyle(paper).lineHeight) || 17`；`!fontsReady` → `unknown("fonts")`；`document.fonts.ready` 到了再 `read()`；`last.content !== content` → `stale{previous: last}`；每次 ok `version += 1`（**模块级** `let`）；1.5s 未拿到 ok → `unknown("timeout")`；`useLayoutEffect` 建 `ResizeObserver(paper)`，deps 为 `[page?.id, content]`。
- **契约引用**：`CONTRACT.md` §1.5（三态与 `reason` 四值）；方案 §7.1、§7.4（字体与超时）、§7.3（离屏无 zoom，天然 1:1）。
- **验收标准**（代码可读性级，UT 不做 —— 方案 §11.2 明确）：
  - deps **不含 `mode`**（`grep -n "mode" src/lib/measure.ts` → 无命中）；
  - `stale` 时返回对象带 `previous.content` / `previous.onePage` / `previous.contentPx` 三项；
  - `version` 在模块级（两个组件同时用不会互相覆盖计数）；
  - `onePage` 用的是 `A4_RATIO` 常量，**没有**第二个 `297 / 210` 字面量。
- **自测命令**：`npx tsc --noEmit -p tsconfig.app.json` + `npm run lint`
- **已知会打破的**：无（新 hook；`measure.test.ts` 不 import 它）。
- **不许做的事**：不许给 `mode` 加依赖；不许在字体未就绪时给 `ok`（宁可 `unknown`）；不许把测量结果写进 store / 落库；不许在这一卡挂 DOM 节点（宿主是 B3-2）。

---

### 【B3-2】`provider.tsx` 离屏测量宿主 + `measure` 进 context

- **批次 / 依赖**：Batch 3；前置 B3-1；被 B3-3/4/5/6/7 依赖。
- **改的文件**：
  1. `src/store/workspace/provider.tsx`：`hostRef` + `usePageMeasure({hostRef, person, page})` + 返回值挂进 context 的 `value`；`return` 里（`<WorkspaceContext.Provider>` 内、`{children}` 之后、`<Confirm>` 之前）挂离屏宿主，结构照方案 §2.4 的 JSX（`aria-hidden` + `data-measure="host"` + `position:absolute; left:-9999; width:374; visibility:hidden; pointerEvents:none; zIndex:-1`，内层 `<div className={paperStyles.wrap} ref={hostRef}><Paper person={person} page={page} /></div>`，条件 `person && page`）；
  2. `src/store/workspace/context.ts`：`WorkspaceContextValue` 加 `measure: PageMeasure | null`。
- **输入 → 输出**：`measure` 是**唯一真值源**；`provider.tsx` 需要 import `Paper` 与 `paper.module.css`（`@/components/resume/paper`）。
- **契约引用**：`CONTRACT.md` §1.5（`chat_send` / `layout_submit` 的上送来源）；方案 §2.4（宿主为什么常驻、为什么宽 374）、§15 第 2 处注释。
- **验收标准**：
  - 编辑态（默认视图，没有 `.paper`）下 `measure?.status === "ok"` 且 `contentPx > 0`（DevTools 打断点或临时 `console.log` 验一次，验完删）；
  - 切到预览再切回来，`measure.version` **不跳变**（证明没有随视图重算）；
  - 宿主 DOM 不出现在可见区域（`document.querySelector('[data-measure="host"]').getBoundingClientRect().left < -1000`）；
  - 宿主节点**带方案 §15 第 2 处的中文注释**（说清「编辑态没有纸面 DOM；预览那份只管显示，不许另起一套公式」）。
- **自测命令**：`npx tsc --noEmit -p tsconfig.app.json` + `npm run build` + `npm run dev`（人工看两项）
- **已知会打破的**：`context.ts` 加必填字段后，任何手写 `WorkspaceContextValue` 字面量的地方会红 —— 只有 `provider.tsx` 一处（已核对），同一提交里补 `measure`。
- **不许做的事**：不许改 `panel.tsx`（B3-5/B3-6 才动）；不许删预览那份 `.paper`（用户要看）；不许把宿主宽写成 860 / 百分比 / `100%`；不许在宿主上挂 `key` 让它随页重建（会导致每页一次 `compilePage`）。

---

### 【B3-3】`provider.tsx` 轮末自动自查（F-010）

- **批次 / 依赖**：Batch 3；前置 B3-2、B0-5；可与 B3-4/5/6 并行。
- **改的文件**：`src/store/workspace/provider.tsx`（新增一个 `useEffect` + `const autoRounds = useRef(new Map<string, number>())`）。
- **输入 → 输出**：照方案 §8 的代码，五个前置条件**都要**：无 `measure` / `status !== "ok"` / 无 `onePage|contentPx` / `layoutStatus?.pageId === page.id` / `!resume.targetPages` → **直接 return（不投也不清零）**。`pages <= 1.02` → `autoRounds.delete(page.id)` 并 return。`round >= 3` → 只在 `round === 3` 时置 4 并 `dispatch pageNote`（id `n-auto-{pageId}-4`）。否则 `round+1` 并 300ms 防抖后 `submitLayout({kind:"request", text: 自动自查…, measure, origin: originOf(resume.id)})`。
- **契约引用**：方案 §8（逐行）、§9.1（`overflowedPages` 用的同一判据 1.02）、`CONTRACT.md` §1.5。
- **验收标准**：
  - 连续 3 轮溢出 → 第 4 次只出现**一条**批注（`pageNote` 按 id 去重，重放不叠）；
  - 压到 `<= 1.02` → `autoRounds` 该键被删（再次溢出从 1 重新计数）；
  - `measure.status === "stale"` → 不投递（拿假数指挥排版是负收益）；
  - 同一帧多次测量只投一次（300ms 防抖的 `clearTimeout` 生效）。
- **自测命令**：`npx tsc --noEmit -p tsconfig.app.json` + `npm run dev`（冒烟第 3、4 条）
- **已知会打破的**：`loadWorkspace` 后的初次渲染会立刻满足「溢出」条件 → 首次打开一份已溢出的简历会立刻投一轮。**这是预期行为**（方案 §8 未排除冷启动），但在冒烟时要确认「不会一次投三份」。
- **不许做的事**：不许在 `stale`/`unknown` 时投递；不许把计数放进 store/落盘；不许改 `submitLayout` 的签名（B3-7 负责）；不许在 Rust 侧加定时/轮询（方案 §8 末条）。

---

### 【B3-4】`status-bar` 实测徽标 + `.dotWarn` + `measureTip` 用例

- **批次 / 依赖**：Batch 3；前置 B3-2；可与 B3-3/5/6 并行。
- **改的文件**：
  1. `src/components/status-bar.tsx`：`useWorkspace()` 解构出 `measure`；页码后、字数前插入实测徽标（方案 §9.2 的 JSX）；`pagesOk = status==="ok" && pages <= 1.02`；`pagesText` 三档 `1.00 页内` / `1.01 页` / `1.18 页 · 超 0.18`；`title` 用 `measureTip(measure, resume?.targetPages)`（**从 `@/lib/measure` import，不在本文件重写**）；
  2. `src/components/status-bar.module.css`：加 `.dotWarn`（照 `.dotBusy` 的形状，**颜色用 `var(--danger)`** —— `--warn` 已被「保存中」占用，两个琥珀点会分不清）+ `.dotIdleDash`（未知态用虚线点：`border: 1px dashed var(--fg-3); background: transparent`）；
  3. `src/lib/measure.test.ts`：补 `measureTip` 的五条用例（若 B0-6 已写三条，这里补「无目标」「过期」两条）。
- **契约引用**：方案 §9.2（徽标三档文案 + `measureTip` 五句）、§13 第 4 条（未设目标的措辞）。
- **验收标准**：
  - 溢出时徽标文字含 `超 0.18`，点是 danger 色；
  - `measure == null` → 徽标整块不渲染（不显示「页数未知」占位除非 `measure` 存在但 `status !== "ok"`）；
  - `status === "unknown"` → 显示「页数未知」+ 虚线点；
  - `title` 含目标页数信息（`hover` 看得到「目标 1 页，未达标」）。
- **自测命令**：`npm run test` + `npx tsc --noEmit -p tsconfig.app.json`
- **已知会打破的**：`measureTip` 若在 B0-6 里没导出或签名不同 —— 以 B0-6 的签名为准（`(measure, targetPages?)`），不要在本卡改它的签名。
- **不许做的事**：不许在前端复刻 `FIT_MAX` / `TOLERANCE_MAX` 常量（前端只能用 `1.02` 这一个「容忍带」判据，且方案里它出现在 §8/§9.1/§9.2 三处，**写成 `measure.ts` 里一个 `TOLERANCE = 1.02` 常量**引用，不许各写一份字面量）；不许改 `status-bar.tsx` 的布局 grid。

---

### 【B3-5】`panel.tsx` 三项导出前置门（F-011）

- **批次 / 依赖**：Batch 3；前置 B3-2；可与 B3-3/4/6 并行。
- **改的文件**：`src/components/resume/panel.tsx`（导出下拉 `:658-686`；新增 `pendingExport` state + `gate()` + `overflowedPages()` + 一个 `<Confirm>`）。
- **输入 → 输出**：`gate(run)` 包住三项：打印（`:667` → `handlePrint`）、Word 当前页（`:669`）、Word 整份（`:681`）。**Markdown 两项（`:670`、`:672`）不过门**。门条件：`resume.targetPages` 存在 **且** `overflowedPages().length > 0`；否则直接 `run()`。确认窗文案照方案 §9.1（`title` = `这一页还没压到 ${target} 页内`、`confirmLabel="照这样导出"`、`cancelLabel="先去压一页"`）。
- **契约引用**：方案 §9.1（逐行，含「不放 provider」的理由）；`components/ui/confirm` 的 props。
- **验收标准**：
  - 溢出 + 已设目标 → 点「Word · 当前页」弹窗；「先去压一页」→ 不导出；「照这样导出」→ 导出。
  - 「Markdown · 当前页」任何情况都不弹。
  - 未设目标 / 未溢出 → 三项都直接导出（不弹窗）。
  - 量不到（`measure` 为 `null` / `status !== "ok"`）→ **不拦**（`overflowedPages()` 返回 `[]`，符合 F-011「量不到的页不拦」）。
- **自测命令**：`npx tsc --noEmit -p tsconfig.app.json` + `npm run dev`（冒烟第 5 条）
- **已知会打破的**：`exportFolder`（整份 Word，`:684`）改为 `() => gate(exportFolder)` 后，`onSelect` 的返回类型仍是 `void` —— 别让它返回 Promise（`Dropdown` 的 `onSelect` 签名）。
- **不许做的事**：不许把 `pendingExport` 提到 provider（方案 §9.1 明说不放）；不许给 Markdown 加门；不许在这一卡改虚线（B3-6）。

---

### 【B3-6】`panel.tsx` 预览虚线改用 `measure.onePage`

- **批次 / 依赖**：Batch 3；前置 B3-2；**必须在 B3-5 之后**（同一文件，避免冲突）。
- **改的文件**：`src/components/resume/panel.tsx`。
- **输入 → 输出**：
  - 删掉本地的 `const [a4Height, setA4Height] = useState(529)` 与 `const [sheets, setSheets] = useState(1)`，以及 `:325-339` 那个自己算 `onePage` 的 `useEffect`（含 `ResizeObserver`）；
  - 预览分支（`:689-699`）改用 `const breakTop = measure?.onePage ? measure.onePage * (zoom / 100) : 0;`，条件 `measure?.status === "ok" && pages > 1.02` 画 `.break`（`top: breakTop`）；
  - `PAPER_W = 374` 保留（它给「适应宽度」算倍率），但**不再**参与分页计算。
- **契约引用**：方案 §7.3（zoom 归一化，同时修掉老隐患）、§2.4（「预览那份只管显示」）。
- **验收标准**：
  - zoom 从 100% 调到 50% → 虚线的 `top` 减半（`measure.onePage` 不随 zoom 变，这是关键）；
  - 编辑态没有任何 `.paper` 依赖：`grep -n "a4Height\|setSheets" src/components/resume/panel.tsx` → 无命中；
  - 全文只有一处 `297 / 210`（在 `src/lib/measure.ts` 的 `A4_RATIO` 里）。
- **自测命令**：`npx tsc --noEmit -p tsconfig.app.json` + `npm run build`
- **已知会打破的**：删掉 `useState` 后若有 import 变成未使用，`npm run lint` 会报（`no-unused-vars`）—— 顺手删 import。
- **不许做的事**：不许在 `panel.tsx` 里再写一遍 `onePage = width * 297/210`；不许改 `Paper` 组件（它只管渲染）；不许动 B3-5 的门。

---

### 【B3-7】`chat/panel.tsx` 上送 `measure` + 存 `origin`

- **批次 / 依赖**：Batch 3；前置 B3-2、**B2-4**（`SendArgs.measure` / `SendResult.origin`）；Batch 3 最后一个。
- **改的文件**：`src/components/chat/panel.tsx`（`invoke("chat_send", …)` 约 `:389-420`；`const sent = …` 之后）。
- **输入 → 输出**：
  - `args` 里加 `measure: measure ?? null`（`measure` 从 `useWorkspace()` 取）；
  - `sent.origin` 有值 → `putOrigin(resume.id, sent.origin)`；
  - `submitLayout` 的调用点（若在 `chat/panel.tsx` 有通过 `requestLayout` 的桥）不需要改 —— `origin` 由 `provider.tsx` 的自动自查与 `provider.tsx` 的内容变更 effect 传（`originOf(resume.id)`），本卡只需保证 `putOrigin` 发生过；
  - `src/lib/layout.ts` 的 `submitLayout` 入参加 `measure?: PageMeasure` 与 `origin?: Origin`，透传进 `invoke("layout_submit", { args: { …, measure, origin } })`。
- **契约引用**：`CONTRACT.md` §1.5（三条通道）；方案 §2.5、§2.8（origin 链路）、§1.3（`src/lib/layout.ts` 与 `measure.ts` 的暂存）。
- **验收标准**：
  - 发送一轮后 `originOf(resume.id)` 有值（本轮上传了 PDF 的前提下）；
  - 刷新页面后 `originOf` 为 `null`（内存暂存，诚实退化）；
  - `layout_submit` 的 payload 里出现 `measure` 与 `origin`（DevTools Network / `console.log` 验一次）。
- **自测命令**：`npx tsc --noEmit -p tsconfig.app.json` + `npm run dev`（冒烟第 2 条）
- **已知会打破的**：`submitLayout` 的新参数是**可选**，所以 `provider.tsx` 的两个既有调用点（内容变更 effect、B3-3 的自动自查）不传也不会红 —— 但 B3-3 传 `origin` 后才真正生效，别把 `origin` 设成必填。
- **不许做的事**：不许把 `measure` 存进 store / localStorage；不许在 `chat/panel.tsx` 里自己算 `pages`；不许改 `chat_send` 的既有字段名。

---

### 【B4-1】`prompts/system.md` 两节（页数来源 / 照片）

- **批次 / 依赖**：Batch 4；前置 B0-1；任意时点可写。
- **改的文件**：`prompts/system.md`（在 `:92` 「## 当前材料」之前插两节）。
- **输入 → 输出**：照方案 §10.1 逐字插入 `## 页数只有一处来源` 与 `## 照片` 两节（短句、可 grep，别改写）。
- **契约引用**：方案 §10.1；`CONTRACT.md` §3.7（`extract_images` 的用法）、§4.1（不许谎报）。
- **验收标准**：
  - `grep -c "页数只有一处来源" prompts/system.md` == 1，`grep -c "## 照片" == 1`；
  - 两节里出现 `set_target_pages`、`extract_images`、`set_field` 三个工具名；
  - 出现「照片没有放上去」这句（与 `tools/extract.rs` 的失败文案对齐）。
- **自测命令**：`cmd //c ".tools\\check.cmd"`（`include_str!` 编译期嵌入，改错会直接编不过）+ 人工通读一遍两节
- **已知会打破的**：无（纯文本，但 `include_str!` 意味着**格式错误在编译期就会暴露**）。
- **不许做的事**：不许把阈值（1.00 / 1.02）写进提示词（一处来源在 `facts.rs`）；不许把 `MEASURE_RULER` 抄进来；不许改既有的其它小节。

---

### 【B4-2】`prompts/layout.md` 容量表降级 + `render::GUIDE` 同步

- **批次 / 依赖**：Batch 4；前置 B0-1；任意时点可写。
- **改的文件**：
  1. `prompts/layout.md`：`:87-108`（`## 容量参考（用它判断，不要凭感觉）` 整段）替换为方案 §10.2 的 `## 容量参考只是量级参考` 段；
  2. `src-tauri/src/render/mod.rs`：`GUIDE` 里 `:192-194`（`【收尾前自查】` 两条）替换为方案 §10.3 的两条。
- **输入 → 输出**：两处**同一口径**：容量表只估量级，判据是 `## 这一页的实测`。
- **契约引用**：方案 §10.2、§10.3（含「必须同步改」的理由）；`CONTRACT.md` §4.1。
- **验收标准**：
  - `layout.md` 里出现 `## 这一页的实测`、`≤ 1.00`、`1.00–1.02`、`超过 1.02`；
  - `GUIDE` 里出现 `≤1.00 达标、≤1.02 容忍内、>1.02 溢出` 与「只是量级参考」；
  - 两处都不再出现「用它判断」这种绝对口气（`grep -n "用它判断" prompts/layout.md src-tauri/src/render/mod.rs` → 无命中）。
- **自测命令**：`cmd //c ".tools\\check.cmd"` + `grep -n "实测" prompts/layout.md src-tauri/src/render/mod.rs`
- **已知会打破的**：`render/mod.rs` 的 `GUIDE` 有测试断言其内容吗？—— 已核对：`GUIDE` 无逐字断言（只有 `read_layout_guide` 返回它），预期零红；若红，改断言不要改口径。
- **不许做的事**：不许只改一处（方案 §10.3：两处口径打架时模型会选它先读到的那个）；不许把容量数字删掉（它仍是量级参考）；不许碰 `render/mod.rs` 的其它代码（B1-5 也动这个文件，只在自己的 hunk 里改）。

---

### 【B4-3】全量验证 + 冒烟 6 条收口

- **批次 / 依赖**：Batch 4；前置**全部卡**；最后一个。
- **改的文件**：无业务代码（只允许补漏的断言；有漏就回对应卡补，不在这里发明新行为）。
- **做什么**：按下面四条命令全绿 + 逐条跑 §6 冒烟清单并把结果记进交付说明（哪一条过、哪一条不过、不过的原因）。
- **契约引用**：`CONTRACT.md` §1.5 / §2 / §3.7 / §4.1 / §5.0 / §6（收口时逐节对照代码，任何一节对不上就是没过）；方案 §11.3（冒烟 6 条）、§11.4（已知会打破的测试清单）。
- **验收标准**：
  - `cmd //c ".tools\\check.cmd"` 无 warning 级以上的新告警；
  - `cmd //c ".tools\\test-quiet.cmd"` 全绿，且 `session.len()==40` / `layout.len()==9`；
  - `npx tsc --noEmit -p tsconfig.app.json` 零错；`npm run test` 全绿；`npm run build` 成功；
  - 冒烟 6 条逐条有结论（不许写「基本通过」）。
- **自测命令**：四条命令 + 人工冒烟（见 §6）
- **已知会打破的**：无（这是收口卡）。若某条冒烟不过，**不许**靠改文案让它「看起来过了」。
- **不许做的事**：不许在这里改契约 / 改阈值 / 补新功能；不许把冒烟里发现的问题静默忽略（写进结论）。

---

## 4. 并行排期建议

| 开发 | 负责卡 | 说明 |
|---|---|---|
| **开发甲（Batch 0 单人）** | B0-1 → B0-2 → B0-3 → B0-4 → B0-5 → B0-6 | 闸门。**B0-1 先单独提交并合并**，之后 2~6 可连做。这一批不合入，其余四批不许开工。 |
| **开发乙（抽图）** | B1-1 → B1-2 → B1-3 → B1-4 → B1-6（B1-5 任意时点插入） | 一条链，都碰 `src-tauri/src/pdfembed.rs` / `attach.rs` / `tools/*`。 |
| **开发丙（事实与工具）** | 先做 B2-1 → B2-2 → B2-3（可与乙并行）；乙合并 B1-6 后再做 B2-4 → B2-5 → B2-6 | B2-1~3 只碰 `facts.rs`，与乙零冲突 —— 这一条比方案 §14 的「Batch 2 整体等 Batch 1」多回收一天，前提是**只碰 `facts.rs`**。 |
| **开发丁（前端）** | B3-1 → B3-2 → B3-3 → B3-4 → B3-5 → B3-6 → B3-7 | 前六张只依赖 B0，可全程与后端并行；B3-7 等 B2-4 的 `SendResult.origin`（可以先写，最后联调）。 |
| **开发戊（提示词与收口）** | B4-1 → B4-2（随时）→ B4-3（最后） | B4-2 与 B1-5 都动 `render/mod.rs`：不同 hunk，合并即可，但同一天内不要两人同时提交这个文件。 |

**回收的并行度**：方案 §14 说「Batch 1 与 Batch 2 并行」，但 Batch 2 里有 3 张卡要动 `tools/mod.rs`（`ToolCtx` 结构体 + `TOOLS` 数组）。本文件把它们单独标成「B1-6 之后」，其余（B2-1~3）**提前**与 Batch 1 并行 —— 净效果比原计划更快，且不违反「`TOOLS` 一次只许一个人动」。

---

## 5. 跨卡风险与协调点

| 文件 | 谁改 | 约定 |
|---|---|---|
| `docs/CONTRACT.md` | **只有 B0-1** | 冻结后谁都不许动。要改先回主理人。 |
| `src-tauri/src/tools/mod.rs` | B1-6 → B2-4 → B2-6（**严格串行**） | 三处在同一文件：`TOOLS` 数组、`ToolCtx` 结构体、测试断言。`TOOLS` 数组**一次只许一个人动**：B1-6 推到 39，B2-6 推到 40。 |
| `src-tauri/src/agent/facts.rs` | B0-3（骨架）→ B2-1/2/3（文案，同一个人） | 骨架先合并；三张文案卡由同一人连做，避免同函数两处各写一半。 |
| `src/store/workspace/provider.tsx` | **只有 B3-2 / B3-3**（同一人） | 离屏宿主、`measure`、自动自查都在这一个文件。别人一律不许碰。 |
| `src/lib/measure.ts` | B0-6（纯函数）→ B3-1（hook）→ B3-4（`measureTip` 用例） | 纯函数与 hook 分开提交，hook 不改纯函数签名。 |
| `src/components/resume/panel.tsx` | B3-5 → B3-6（串行） | 同一文件两次改动，串行最安全。 |
| `src-tauri/src/render/mod.rs` | B1-5（`:817` 附近） / B4-2（`:192-194`） | 不同 hunk。合并无冲突，但**同日不并行提交**。 |
| `package.json` + `vite.config.ts` | **只有 B0-6** | 加 `vitest` 与 `test` 脚本；别批不许加任何依赖。 |
| `src-tauri/src/attach.rs` | B1-3（含 `session.rs:502` 调用点） | `materials()` 返回类型变更会波及 `session.rs` 一处，B1-3 一并修。 |
| `src-tauri/src/tools/edit.rs` | B2-6（只加 `pub fn push_op`，5 行） | 该文件属 BE-content；只允许这一处新增，不许改 `EditState` 逻辑。若评审不同意，退路是让 `set_target_pages` 直接 `push_action` 一条裸 action，但那要再改 `agent-actions.ts` 的 switch + `provider.tsx` 的 hooks（跨 B0/B3 两个所有者），代价更大。 |
| `src-tauri/Cargo.toml` / `Cargo.lock` | **只有 B0-2** | `Cargo.lock` 只许 +1 行（`lopdf`），出现版本漂移就回滚。 |

**三处「看起来能顺手做，但会打架」的地方，明确禁止**：
1. **阈值常量**：任何卡都不许在 `facts.rs` 之外写 `1.00` / `1.02` 作为判定（前端只许有一个 `TOLERANCE = 1.02` 常量在 `measure.ts`，且它只用于「是否画虚线 / 是否投自查 / 是否弹门」，**不产生任何文案**）。
2. **`{口径}` 字符串**：只有 `MEASURE_RULER` 一处。B2-x / B3-x / B4-x 全部引用或 import，不许复制。
3. **`session.len()` 断言**：只有 B1-6（→39）与 B2-6（→40）改。谁先合谁推，别在中间态写 `>= 39` 这种模糊断言（那会让「偷偷给某角色开口子」失去防线）。

---

## 6. 冒烟清单（人工，JS 单测证明不了真实布局）

> 为什么必须人工：`usePageMeasure`、自动自查、导出门都依赖 `ResizeObserver` / `document.fonts` / 真实排版。jsdom 测出来的是假绿（方案 §11.2 明确禁止）。
> 每条写清「跑什么 / 看什么 / 什么算过」。**结论写进 B4-3 的交付说明，不许写「基本通过」。**

| # | 跑什么 | 看什么 | 什么算过 |
|---|---|---|---|
| 1 | 上传一份单页带证件照的**扫描 PDF**，让会话 agent「把照片放到页头」 | `extract_images` 返回的候选清单；随后预览里的页头 | 清单里有一张 `占版心 5–12%`、宽高比 `0.7–0.8`；`set_field` 写进页头后预览能看到照片，且**不是**那张整页扫描件（`占版心 90%+` 的那张） |
| 2 | **停在编辑态**（不切预览）发一句「看看现在排版怎么样」 | 事实块里 `这一页「…」：实测 X 页…` | 出现「实测 1.00 页」这种真数，**不出现**「尚未实测」。这是本次最关键回归点（编辑态没有 `.paper`，靠离屏宿主） |
| 3 | 把内容加到溢出，再设目标 1 页，等一轮 | 状态栏徽标 → 自动投递 → 排版跑完 → 徽标再变 | 先出现 `1.18 页 · 超 0.18`（琥珀点）；自动投递让排版压到 `≤ 1.02`；徽标转「1.00 页内」（正常色） |
| 4 | 故意造「压不动」的页（连续 3 轮仍溢出） | 第 4 轮的批注 | 第 4 次出现**一条**批注「自动自查已投 3 轮，仍实测 X 页…」，且**只出现一次**（刷新/重放不叠） |
| 5 | 溢出 + 已设目标 → 点「Word · 当前页」；再点「Markdown · 当前页」 | 确认窗 | `Word · 当前页` 弹窗；点「先去压一页」→ 不导出；点「照这样导出」→ 导出成功；`Markdown · 当前页` **不弹窗**直接导出 |
| 6 | 上传一份含 **11 张**内嵌位图的 PDF → 调 `extract_images` → 再说「把剩下的也抽出来」 | 两次的清单行数与 `person.assets` | 第一次清单 **8 行** + 末尾「还有 3 张更大的没列」；第二次清单 **3 行**；两次之后 `person.assets` 累计 **11 条** candidate、**无重复 id** |

**冒烟环境备注**：
- 走**桌面版**（`npm run tauri:dev` 或 `npm run tauri:build` 后运行），不要用纯浏览器 `npm run dev` —— 附件、`compilePage` IPC、字体都只在 Tauri 里真实。
- 冒烟第 2 条若失败（编辑态显示「尚未实测」），先查三处：宿主是否真的挂上（`[data-measure="host"]` 存在）、`hostRef` 是否指向 `.wrap` 而不是宿主外层、`provider` 的 `page` 是否为 `null`（无页时不渲染宿主，这是正确行为）。

---

## 7. 附：与方案的三处偏差（已定，需要主理人知情）

1. **`measureTip` 放在 `src/lib/measure.ts`，不放 `status-bar.tsx`**（方案 §9.2 说「同文件」）。理由：B0-6 的 vitest 在 `environment: "node"` 下跑，测试文件不能 import 引了 `*.module.css` 的 `.tsx`。`status-bar.tsx` 只 `import { measureTip }` 并渲染。
2. **`origin` 暂存用 `originOf()`（只读窥视）+ `clearOrigin()`**，方案写的 `takeOrigin`（取走）会让第二个读取方拿到 `null` —— 而自动自查（provider）与 `submitLayout` 都要读同一份。
3. **`set_target_pages` 走 `mutate` 信封**（在 `edit.rs` 加 5 行 `pub fn push_op`），而不是推一条裸 action。理由：裸 action 要在 `agent-actions.ts` 的 `switch` 加 case + 在 `provider.tsx` 加 hook —— 那会把 B2-6 的手伸进 B0（`agent-actions.ts`）与 B3（`provider.tsx`）的所有者手里，冲突面积大得多。
