# prompts/ — 模型提示词的唯一存放处

**规则：凡是发给模型的静态提示词，都放这个目录。** 代码里只许 `include_str!`，不许在 `.rs` 里内联字符串。

## 为什么

提示词是产品行为，不是实现细节。内联在 Rust 源码里时：

- 改一个字要动 `.rs`，`git diff` 里混着引号、`\n`、续行反斜杠，review 时看不出到底改了什么；
- 一个 `\n` 少写就是把两段话粘成一句，编译器不会吭声；
- 想请不写代码的人校一遍措辞，得先让他读 Rust。

放进 `.md` 之后，提示词能单独 diff、单独 review、单独改写，代码里只剩「哪份提示词在哪用」。

## 目录里的文件

| 文件 | 谁在用 | 什么时候读 | 代码位置 |
|---|---|---|---|
| `system.md` | 会话 agent（唯一的 agent） | 每轮请求的 system | `session.rs` → `SYSTEM_TEMPLATE` |
| `layout-guide.md` | 组件手册 | 模型调 `read_layout_guide` 时原样返回 | `render/mod.rs` → `GUIDE` |
| `chunk.md` | 分片模型 | 导入长文、把它切成检索块时 | `knowledge.rs` → `CHUNK_PROMPT` |
| `enrich.md` | 摘要模型 | 给块补 summary 与关联问题 | `knowledge.rs` → `ENRICH_PROMPT` |
| `capability-probe.md` | 多模态探针 | 设置里点「测一次多模态」 | `capability.rs` → `PROBE_PROMPT` |
| `compact.md` | 压缩模型 | 对话超出上下文预算、把旧对话压成要点时 | `session.rs` → `COMPACT_PROMPT` |

## 载入方式是编译期

`include_str!` 在**编译**时把文件内容打进二进制。改完 `.md` 必须重新构建：

- 开发模式：`cargo build`（或让 tauri dev 自己重启）；
- 发行包：走 `.tools/build.bat`。

**只改 `.md` 不重编，运行时用的还是旧文本** —— 这是这套约定唯一的坑，改完顺手 build 一次。

## 这里不放什么

**运行时拼出来的文本不是提示词。** 这些留在代码里，因为它们带参数、要跟着数据变：

- 定位头（`agent/context.rs::locate`）—— 候选人名、第几页、版面现状，每轮都不同；
- 页数事实（`agent/facts.rs`）—— 实测页数、目标页数、结论三态；
- 工具返回值与报错文案 —— 它们描述的是「刚才发生了什么」，不是「你该怎么做」。

判断标准：**能原样抄进文件的，放这里；要拼参数的，留在代码里。**

## 改写时注意

1. **一份文件只服务一个调用点。** 同一段话别让两个模型共用 —— 各自的读者和场景不同，早晚要分叉。
2. **占位符用 `{{name}}`**（`system.md` 里的 `{{context}}`）。占位符名字改了要同步改代码里的替换点。
3. **只写「怎么做事」**，不写代码路径、不写内部实现理由。模型不需要知道文件在哪，它需要知道这一步该干什么。
4. **改完跑 `cargo test --lib`**：`tools/mod.rs` 有一条护栏会核对 `system.md` 里「调 `X`」点到的工具是否真的存在 —— 提示词与工具表漂移过三次，这是唯一拦住它的地方。
5. `layout-guide.md` 的内容与 `render/mod.rs` 的 `TAGS` / `CSS_PROPS` / `paper.css` 是一套：**加了 class 或放宽了白名单，这份手册要手动跟上**，否则模型照着手册写、样式却被清洗掉。
