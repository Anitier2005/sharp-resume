# 批次三 · 三路并行契约（2026-10-28 晚）

主控已完成共享骨架：`lib.rs`（模块声明、命令注册、`AppInfo.ocr`、启动兜底）、
`knowledge.rs` / `extract.rs`（stub 命令，签名已定）、`Cargo.toml`（`ocr` feature 已声明、
reqwest 加了 `blocking`）、`knowledge/vectors.bin`（空文件，`include_bytes!` 用）、
`.tools/check.cmd`（同 build.bat 环境跑 `cargo check --lib`）。

## 全局红线

1. Rust 类型自检用 `cmd //c ".tools\\check.cmd"`（Git Bash 下）；多个 agent 并发时
   cargo 会等锁，看到 `Blocking waiting for file lock` 就让它等，别杀。
2. 前端类型自检用 `npx tsc --noEmit -p tsconfig.json`；**不要跑 `npm run build`**（写 dist）。
3. 不要碰：`src-tauri/src/lib.rs`、`src/types/index.ts`、`src/store/workspace/*`、
   `docs/`、`knowledge/`（除各自主张的文件）、`.tools/check.cmd`。
4. 注释写「为什么」，中文，不用 emoji；颜色/字号走令牌。
5. 交付时给主控 ≤15 行摘要：改了哪些文件、对外 API、没做到的部分。

## W1 · 模型三类统一管理（对话 / 嵌入 / OCR）

**拥有**：`src/lib/model-config.ts`、`src/lib/ai.ts`、`config/models.json`、
`src/components/settings/sheet.tsx`、`src/components/settings/sheet.module.css`

背景：设置页现在只有「对话模型」；要加**嵌入模型**（知识库向量检索用）和
**OCR 模型**（默认构建不带 tesseract 后，图片转文字的接口兜底），三类统一在
「模型」tab 里管理。

1. `model-config.ts`：
   - `export type ModelKind = "chat" | "embedding" | "ocr"`；`ModelConfig` 加
     `kind?: ModelKind`（缺省 = chat）；现有导出（MODELS/modelById/listModels/findModel/
     isCustomModel/saveCustomModel/removeCustomModel）保持兼容。
   - `listModels(kind?: ModelKind)`：只返回该类（缺省 chat）；自定义模型存取要带 kind
     （localStorage 里的旧数据读出来补 `kind: "chat"`）。
   - 内置清单 `config/models.json` 补两类预置（对话条目不动，缺省 kind=chat）：
     - embedding：`text-embedding-3-small`（OpenAI，dim 1536）、`text-embedding-v3`
       （通义 dashscope 兼容端点）、`bge-m3`（本地 ollama）——每条 `kind:"embedding"`；
       embedding 条目不需要 reasoning/vision 字段，`contextWindow` 填 8192（单次嵌入上限），
       `defaultTemperature: 0`。
     - ocr：`gpt-4o-mini`、`qwen-vl-max` 之类的多模态条目，`kind:"ocr"`，`vision: true`。
2. `ai.ts` 新增（别动 AiSettings/loadAi/saveAi 的既有签名）：
   ```ts
   export interface AuxModelChoice { model: string; baseUrl: string; apiKey: string }
   export interface AuxSettings { embedding: AuxModelChoice | null; ocr: AuxModelChoice | null }
   export function loadAux(): AuxSettings      // localStorage `sharp-resume:aux:v1`
   export function saveAux(patch: Partial<AuxSettings>): void
   ```
   `apiKey` 为空表示沿用对话模型那把 key（UI 上要写清）。
3. 设置页「模型」tab 改三段：**对话模型**（现状不动）／**嵌入模型**／**OCR 模型**。
   后两段各：select（`listModels(kind)`，自定义项后缀「（自定义）」）+「+」新建 +
   选中自定义时的删除（Confirm）；下面各两行小字段：Base URL、API Key（可空）。
   新建复用现有 `CreateModel` 小窗，按 kind 调字段：
   - embedding：显示名称 / 模型名 / Base URL / API Key / 维度（选填数字，仅存注）
   - ocr：显示名称 / 模型名 / Base URL / API Key（不问 reasoning / vision）
   - chat：维持现状
   保存后写入 `saveAux`（embedding/ocr 段），不切换对话模型。
4. 面板底部提示更新：嵌入模型给知识库向量检索用；OCR 模型给「模型看不了图且未内置
   OCR」时的图片转文字用。

## W2 · 知识库（预置只读 + 用户自建 + 面板 + 模型工具）

**拥有**：`src-tauri/src/knowledge.rs`、`src-tauri/src/db.rs`、`src-tauri/src/net.rs`、
`src-tauri/src/tools/mod.rs`、`src/lib/knowledge.ts`（新建）、
`src/components/knowledge/panel.tsx` + `panel.module.css`（新建）、`src/components/sidebar.tsx`

骨架 stub 已编好命令签名（`knowledge_list/get/upsert/delete/search/configure`、
`search_tool/read_tool`、`KnowledgeEntry/KnowledgeDoc/KnowledgeDraft/KnowledgeHit/
EmbeddingConfig`），lib.rs 已注册 —— **按 stub 的签名实现，别改签名**。

1. **存储**：预置 = `include_str!("../../knowledge/manifest.json")` +
   `include_bytes!("../../knowledge/vectors.bin")`（规范见 `knowledge/README.md`，
   manifest 可能是空 entries —— 所有解析都要容忍空库）。用户条目 = db.rs 新表
   `knowledge(id TEXT PRIMARY KEY, title TEXT NOT NULL, tags TEXT NOT NULL, body TEXT NOT NULL,
   updated_at INTEGER NOT NULL)`（tags 存 JSON 数组文本）。
2. **索引与检索**（Rust 内存，`OnceLock<RwLock<Index>>`，首次用时建，upsert/delete 后重建）：
   - 分词：中文相邻两字成二元组；英文/数字连成词；忽略标点与空白。
   - 打分：BM25（k1=1.2，b=0.75）+ 字段加权（title×3、tags×2、body×1）；
     用户条目正文 >400 字按段落切块（≤400 字，相邻留 1 句重叠），与预置 chunk 同一排序。
   - 向量通道：预置 chunk 带 vector 行号且 `knowledge_configure` 传入嵌入配置时才算
     query 向量（`net.rs` 新增 `pub fn embed_blocking(config:&EmbeddingConfig, text:&str)
     -> Result<Vec<f32>, String>`，用 reqwest blocking + `/embeddings`，OpenAI 兼容体）；
     与词项分做 RRF（k=60）融合。没配置 / 接口失败 / manifest 无 embedding 段 → 纯词项。
   - 命中返回 `KnowledgeHit`：snippet 取命中块前 120 字。
   - 排序参考 `priority`（预置条目 1–5，1 最高；用户条目视为 3）。
3. **工具**：`tools/mod.rs` 注册 `search_knowledge`（参数 `query` 必填、`tags` 可选、
   `limit` 可选默认 5 上限 10）和 `read_knowledge`（参数 `id`），`Scope::Session`，
   描述风格与现有条目一致（写清「写具体事实前先查，查不到就说不知道」）。
   工具的 query 向量：复用 `knowledge_configure` 存下的全局配置（blocking 调用）。
4. **面板**（`src/components/knowledge/`）：
   - 左栏底部加「知识库」入口按钮（`sidebar.tsx`，图标用 lucide 的 `BookOpen`/
     `Library`，风格与现有侧栏按钮一致），点开一个 764 宽的抽屉（portal 进 `.win`，
     参考 `ui/modal` 的令牌/滚轮约束：点遮罩不关、ESC=关闭捕获、Tab 关在窗内）。
   - 列表两段：「预置（只读）」「我的」；顶部搜索框调 `knowledge_search` 高亮命中。
   - 编辑：title / tags（逗号分隔）/ body（textarea）；新建与编辑走同一个小表单；
     删除走 `ui/confirm`；预置条目给「复制为我的条目」。
   - 全局配置：面板打开时与设置保存后，把 `loadAux().embedding` 推给
     `knowledge_configure`（没有就推 null）。
   - 面板里要能看到「向量检索未启用」的提示（当 embedding 未配置但预置带向量时）。
5. `src/lib/knowledge.ts`：invoke 包装 + 类型（与 Rust 结构对齐，camelCase）。

## W3 · OCR 可选构建 + OCR 接口兜底

**拥有**：`src-tauri/Cargo.toml`、`src-tauri/src/extract.rs`、`src-tauri/build.rs`、
`.tools/build.bat`、`.tools/build-ocr.bat`（新建）、`.tools/dev-build.cmd`、
`scripts/package.mjs`、`src/lib/attach.ts`

1. **feature 化**：`tesseract-sys` 改 `optional = true`；`ocr = ["dep:tesseract-sys"]`
   （feature 名已声明）。默认构建不编 tesseract。
   注意 `lib.rs` 顶部若有 `use tesseract_sys as _;` 之类的强制链接语句，按 feature cfg
   起来（在 extract.rs 内处理，别动 lib.rs）。
2. **extract.rs**：
   - `ocr_image`：`#[cfg(feature="ocr")]` 走现有 tesseract 实现；`#[cfg(not(feature="ocr"))]`
     返回 Err("此版本未内置 OCR。在 设置→模型→OCR 配置一个能看图的模型，或使用带 OCR 的构建")。
   - `ocr_api`（stub 已注册）：真实实现 —— 字节拼 `data:image/<ext>;base64,...`，
     构造 OpenAI 兼容的数组 content（text 段：「把图片里的可见文字逐行原样抄写，
     不要解释、不要总结、不要翻译」+ image_url 段），调 `net::chat_once`，返回正文。
     `chat_once` 是 async fn 而 tauri command 声明为 async —— 直接 await 即可。
   - `ocr_available()` 已有，别改签名。
3. **build.rs**：DLL/tessdata 拷贝与 `rerun-if-changed` 全部包进
   `if std::env::var_os("CARGO_FEATURE_OCR").is_some()`。
4. **构建脚本**：
   - `.tools/build.bat`：改为默认 lite —— `cargo build --release --features custom-protocol`
     （照旧，不再需要 OCR 环境变量也能过；环境变量留着无害）。说明文案补一句「OCR 版用 build-ocr.bat」。
   - `.tools/build-ocr.bat`：复制 build.bat，`--features ocr,custom-protocol`。
   - `.tools/dev-build.cmd`：默认 lite（不带 OCR 环境变量），加一个 `--ocr` 参数或
     `dev-build-ocr.cmd` 变体（bat 与命令同名会自递归，命名注意）。
   - `scripts/package.mjs`：DLL/tessdata 只在**实际存在**时才拷，自检相应放宽。
   - 两份 bat 必须 CRLF；文案禁 `->`（会被当重定向）；非 ASCII 文案只留在 echo 里，路径全 ASCII。
5. **attach.ts 兜底链**（`ensureOcrText`）：
   - 先试 `ocr_image`（内置可用就走它，现状不变）；
   - 报「未内置/找不到训练数据」类错误时，若 `loadAux().ocr` 已配置 →
     `invoke("ocr_api", { bytes, ext, baseUrl, apiKey, model })`；key 为空则沿用对话设置那把
     （`loadAi().apiKey`）；失败把两段原因都写进返回文案；
   - 都没有 → 现有「（OCR 失败：…）」文案 + 补一句「可在 设置→模型→OCR 配置接口模型」。

## 跨流依赖（主控收口）

- W3 的 `attach.ts` import W1 的 `loadAux`（`@/lib/ai`）—— 按上面签名写，W1 未完成时 tsc
  报错可以留给主控。
- W2 的面板 import W1 的 `loadAux` 同理。
- 主控负责最终 `tsc`、`cargo check`、`cargo test --lib`、双变体构建与提交。
