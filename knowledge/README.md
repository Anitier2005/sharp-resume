# 知识库规范 v1

一份**随程序分发、只读、界面上不可见**的知识，供 agent 检索事实。
本文件是契约：应用按它读取，外部工作流按它产出。改格式先改这里。

## 1. 定位与边界

- 用途：给模型提供「工作上的事实底座」（项目细节、行业口径、术语、模板规则…），
  让它在写简历、回答问题时**查到**而不是**编出来**。
- 不属于本库的东西：候选人字段与经历（在工作区数据里）、材料图片（在材料池里）、
  版式规则（在 `prompts/` 与 `render/paper.css` 里）。
- 两个来源，一个检索入口：
  - **预置（只读）**：本目录的 `manifest.json`，编译期嵌进二进制，安装目录里看不到、
    界面里改不了。由人收集清洗，经外部工作流产出。
  - **用户自建（可改）**：应用内「知识库」窗口里维护，落在 app.db 的三张表
    （`knowledge_library` / `knowledge_entry` / `knowledge_chunk`），跟着数据目录走。
- 模型只能读：`search_knowledge(query, tags?, limit?)`、`read_knowledge(id)`。
  写入只发生在用户自己操作界面时，模型没有写工具 —— 知识库里放的是**客观求职知识**，
  不该被模型在对话中悄悄改写。

## 2. 目录结构

```
knowledge/
├── README.md           本规范
├── manifest.json       唯一入口（构建期打进二进制）
├── vectors.bin         可选：向量表，行主序、int8 量化
└── entries/            可选：清洗后的原文，供人追溯（不参与运行时）
    ├── _template.md    模板（外部工作流不处理下划线开头的文件）
    └── <slug>.md
```

`entries/` 只是给人看的留档；**运行时只认 `manifest.json` + `vectors.bin`**。
两者不一致时以 manifest 为准（原文缺失不影响检索，只是没法点回原文看）。

## 3. manifest.json

```jsonc
{
  "version": 1,                       // 必填，当前为 1
  "generatedAt": "2026-10-28T09:00:00Z",
  "embedding": {                      // 可选：没有这个段 = 纯词项检索
    "model": "bge-m3",                // 产出向量的模型名（查询时必须同一个）
    "dim": 1024,                      // 向量维度
    "dtype": "int8",                  // 当前只支持 int8
    "normalized": true,               // 是否已 L2 归一化（必须为 true）
    "scale": 127                      // 量化系数：int8 = round(x * scale)
  },
  "entries": [
    {
      "id": "k-ecommerce-001",        // 必填、全局唯一、稳定（改了就是另一条）
      "title": "电商中台项目背景",      // 必填，检索时权重 ×3
      "tags": ["项目", "电商"],        // 可选，权重 ×2，也可作为过滤条件
      "agents": ["session"],          // 可选：可调用的 agent，缺省 ["session"]
      "priority": 3,                  // 可选 1–5，1 最高，同分时优先
      "source": "entries/ecommerce.md",
      "chars": 820,                   // 可选：正文字数，仅用于体积统计
      "chunks": [                     // 必填，至少 1 条
        {
          "id": "k-ecommerce-001#1",  // 必填，建议 <entryId>#<序号>
          "heading": "背景",           // 可选：这一块在原文里的小标题
          "text": "……",               // 必填：≤ 400 字，一段讲完一件事
          "vector": 0                 // 可选：在 vectors.bin 里的行号（0 起）；没有向量填 null
        }
      ]
    }
  ]
}
```

### 切块规则（外部工作流必须遵守）

1. 一个 chunk **≤ 400 个汉字**（英文按 2 字符≈1 汉字折算）；超过就按小标题 / 段落切开。
2. 切开的相邻块保留 **1 句重叠**，避免答案正好落在切口上。
3. `heading` 写原段落的小标题（没有就不填），检索时随正文一起参与匹配。
4. 一条 entry 尽量单一主题：一个项目、一个术语、一条规则。混主题的条目检索命中率会掉。

### 向量规则

1. 先 L2 归一化，再 `round(x * scale)` 取整并夹到 `[-128, 127]`，按行主序写进 `vectors.bin`（每行 `dim` 字节，无头部、无分隔）。
2. `vector` 字段是行号；`vectors.bin` 行数必须 ≥ 所有行号的最大值 + 1。
3. **查询时必须用同一个模型和同一套归一化**：模型不一致时分数量纲不同，
   应用会直接丢弃向量通道、退回词项检索（不会报错）。
4. 允许只有一部分 chunk 带向量（其余填 `null`）：命中的块仍有词项分数。

### id 命名

- entry：`k-<英文 slug>-<三位序号>`，例如 `k-jd-template-001`。
- chunk：`<entryId>#<序号>`，从 1 开始。
- 规则只有一条：**同一份内容重跑工作流，id 不变**（这样引用、日志、缓存都还指得对）。

### agents 字段

- `["session"]`：只有会话 agent 能检索到（缺省）。
- `["layout"]`：只有排版 agent。
- `["session", "layout"]`：两者都能。**谨慎使用**——排版 agent 每轮都跑，
  给它检索能力会明显增加延迟与花费。

## 4. 用户自建：库 → chunk（条目是切块的中间层）

界面上只有两层：库与块。库是主题边界（删库=整块不要），块是检索与管理的单位。
`knowledge_entry` 仍然存在，但它只是「一次输入被切成这组块」的载体：
用户输入一段文字或导入一个文件 → 后端立即切块 → 之后就是逐块改、逐块删。
块删光时那条 entry 会被级联删掉（否则它会变成界面里点不到、只让计数变大的幽灵）。

- 表：
  - `knowledge_library(id, name, note, ord, updated_at)`
  - `knowledge_entry(id, library_id, title, tags, body, ord, updated_at)`
  - `knowledge_chunk(id, entry_id, ord, heading, text, summary, questions, updated_at)`
- 切块：保存时**后端立即切**（`split_chunks`，与第 3 节同口径），
  块的 id = `<entryId>#<序号>`。切块规则内置、不可配 —— 召回率依赖它稳定。
- 摘要与关联问题：切完由模型生成（`knowledge_entry_enrich`，按 entry 发一次请求），
  `summary` ≤60 字，`questions` 3 条「别人可能怎么问」。它们参与词项打分（权重 ×2），
  这样「并发能力」也能命中正文只写「QPS」的块。
- 单块改正文走 `knowledge_chunk_save`：改完这块的 `summary`/`vector` 一并清空，
  因为它们是照着旧正文算的 —— 留着就会拿旧结论骗检索。
- 文本没变的块在重存时保留原摘要，避免「改一个标点全库重算」。
- id：条目 `k-<毫秒时间戳>`；带这个形态的 id 不会与预置冲突（预置形如 `k-<slug>-001`）。
- 向量：用户条目**没有**向量（写入时不算），只参与词项通道；预置块的向量仍来自 `vectors.bin`。

## 5. 应用侧怎么用（实现契约）

1. **构建期**：`build.rs` 读 `knowledge/manifest.json`（缺失或 `entries` 为空时按「知识库为空」处理），
   校验结构后 `include_str!` 进二进制；`vectors.bin` 用 `include_bytes!`。
   - 校验失败：`dev` 打印警告并当作空库；`release` 直接构建失败（避免带着坏库发版）。
2. **启动**：解析 manifest → 建 BM25 索引（中文二元组 + 英文/数字按词；`k1=1.2, b=0.75`，
   标题 ×3、tags ×2、小标题/摘要/关联问题 ×2、正文 ×1）→ 若存在 `embedding` 段则把向量表读进内存。
3. **检索**：`search_knowledge` 先按 `agents` 过滤 → 词项召回 top 20；
   有向量且当前配置了 embedding 接口时再算 query 向量、取 top 20；
   两路用 **RRF（k=60）** 融合 → 取 top `limit`（默认 5，上限 10）。
   返回：`chunkId`、`entryId`、`library`、`title`、`tags`、`heading`、
   摘要（模型写过就用它，否则截正文 120 字）、命中分数。
4. **读取**：`read_knowledge(id)` —— 传 `chunkId` 只读那一块，传 `entryId` 读整篇。
5. **注入策略**：不把知识库内容自动塞进上下文；系统提示里只加一句
   「写具体事实前先用 search_knowledge 查；查不到就说不知道，不要编」。
6. **降级**：没有 `embedding` 段、或运行时没有配置 embedding 接口、或模型名不一致 →
   只用词项通道，不报错、不影响其他工具。

## 6. 外部工作流约定（在另一个会话里跑）

产出物只有两样：`manifest.json` + `vectors.bin`（+ 可选 `entries/*.md` 留档）。流程：

1. 收集与清洗资料 → 写成 `entries/<slug>.md`（一个小标题一段）；
2. 按第 3 节的切块规则切块（≤400 字、1 句重叠、保留 heading）；
3. 用选定的 embedding 模型逐块算向量，L2 归一化 + int8 量化，写 `vectors.bin`；
4. 生成 `manifest.json`；
5. 跑 `node scripts/knowledge-check.mjs` 自检（校验 schema、id 唯一、字数上限、
   向量行号与 `vectors.bin` 大小是否对得上）；
6. 提交这两个文件，构建后即生效。

## 7. 规模与性能（当前约束）

- 目标规模：**约 1000 条、3000–5000 个 chunk**。
- 向量表：`3000 × 1024 × 1B ≈ 3MB`，随 exe 一起分发，可接受。
- 检索：BM25 全量打分 + 向量暴力点积（`3000 × 1024` 次乘加）都在 10ms 级，
  **不引入 ANN 索引**（HNSW 这类依赖不值得）。
- 超过 1 万 chunk 再考虑分片/缓存，届时先改本规范。

## 8. 向量化（v5 起）

- **默认不向量化**：导入与保存只分片，不调嵌入接口 —— 避免"先存着"也要付一次调用费。
- **每个库一个嵌入模型**：`knowledge_library.embedding_model / embedding_dim` 记录当初用的是谁；
  用户点「向量化」时全库重算并覆盖。
- **模型不一致就退回词频检索**：运行时配置的嵌入模型与库上记的不一致时，
  向量通道整段失效，只用 BM25（hash）——不拿两套模型的向量算余弦。
- **硬编码（预置）知识不写向量**：`manifest.vectors` 字段已不再读取；预置库按词频检索。
- **图片块**：`kind = 'image'`，`asset` 存材料池存储名，正文是一句
  `![名称](asset:<stored>)`；摘要来源记在 `annotation`（model / ocr / 空）。
