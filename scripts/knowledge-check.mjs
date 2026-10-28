#!/usr/bin/env node
/**
 * 知识库自检：外部工作流产出的 manifest.json / vectors.bin 是否符合规范。
 *
 * 为什么要有这个脚本：知识库是「编进二进制」的，格式错了要重编一次才能发现，
 * 而且坏库在 release 里会直接让构建失败。在这里先拦住，比在 cargo 里看一屏
 * serde 报错便宜得多。
 *
 * 用法：node scripts/knowledge-check.mjs [knowledge 目录]
 * 退出码 0 通过 / 1 有问题（错误逐条打印）。
 */
import { readFileSync, existsSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

const dir = resolve(process.argv[2] ?? "knowledge");
const errors = [];
const notes = [];

const fail = (message) => errors.push(message);

/** 汉字按 1、其余按 0.5 折算 —— 与规范里的「400 字」口径一致 */
function weight(text) {
  let total = 0;
  for (const ch of text) total += /[\u3400-\u9fff\uf900-\ufaff]/.test(ch) ? 1 : 0.5;
  return total;
}

function readManifest() {
  const file = join(dir, "manifest.json");
  if (!existsSync(file)) {
    fail(`找不到 ${file}`);
    return null;
  }
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    fail(`manifest.json 不是合法 JSON：${error.message}`);
    return null;
  }
}

const manifest = readManifest();
if (manifest) {
  if (manifest.version !== 1) fail(`version 必须是 1，实际是 ${JSON.stringify(manifest.version)}`);

  const entries = Array.isArray(manifest.entries) ? manifest.entries : null;
  if (!entries) fail("entries 必须是数组");

  /** 向量表尺寸：没有文件就是 0 行 */
  const vectorFile = join(dir, "vectors.bin");
  let rows = 0;
  const embedding = manifest.embedding ?? null;
  if (existsSync(vectorFile)) {
    if (!embedding) {
      fail("存在 vectors.bin 但 manifest 里没有 embedding 段（向量通道会被忽略，白占体积）");
    } else if (!Number.isInteger(embedding.dim) || embedding.dim <= 0) {
      fail(`embedding.dim 必须是正整数，实际是 ${JSON.stringify(embedding.dim)}`);
    } else {
      const bytes = statSync(vectorFile).size;
      if (bytes % embedding.dim !== 0) {
        fail(`vectors.bin 大小 ${bytes} 不是 dim(${embedding.dim}) 的整数倍`);
      } else {
        rows = bytes / embedding.dim;
      }
    }
  } else if (embedding) {
    notes.push("manifest 声明了 embedding，但没有 vectors.bin —— 会按纯词项检索");
  }
  if (embedding) {
    if (embedding.dtype !== "int8") fail(`embedding.dtype 目前只支持 int8，实际是 ${embedding.dtype}`);
    if (embedding.normalized !== true) fail("embedding.normalized 必须为 true（向量必须先 L2 归一化）");
    if (!Number.isFinite(embedding.scale) || embedding.scale <= 0) fail("embedding.scale 必须是正数");
    if (!embedding.model) fail("embedding.model 不能为空（查询时靠它判断模型是否一致）");
  }

  const ids = new Set();
  const chunkIds = new Set();
  let chunks = 0;
  let vectors = 0;
  let chars = 0;

  for (const [index, entry] of (entries ?? []).entries()) {
    const where = `entries[${index}]`;
    const id = typeof entry?.id === "string" ? entry.id.trim() : "";
    if (!id) {
      fail(`${where}.id 缺失`);
      continue;
    }
    if (ids.has(id)) fail(`entry id 重复：${id}`);
    ids.add(id);

    if (typeof entry.title !== "string" || !entry.title.trim()) fail(`${id}.title 不能为空`);
    if (entry.tags !== undefined && !Array.isArray(entry.tags)) fail(`${id}.tags 必须是数组`);
    if (entry.agents !== undefined) {
      const allowed = new Set(["session", "layout"]);
      for (const agent of entry.agents ?? []) {
        if (!allowed.has(agent)) fail(`${id}.agents 里有未知的 agent：${agent}`);
      }
    }
    if (entry.priority !== undefined && !(entry.priority >= 1 && entry.priority <= 5)) {
      fail(`${id}.priority 必须在 1–5`);
    }

    const list = Array.isArray(entry.chunks) ? entry.chunks : [];
    if (!list.length) fail(`${id}.chunks 至少要有一块`);
    for (const [ci, chunk] of list.entries()) {
      chunks += 1;
      const cid = typeof chunk?.id === "string" ? chunk.id.trim() : "";
      if (!cid) fail(`${id}.chunks[${ci}].id 缺失`);
      else if (chunkIds.has(cid)) fail(`chunk id 重复：${cid}`);
      else chunkIds.add(cid);

      const text = typeof chunk?.text === "string" ? chunk.text : "";
      if (!text.trim()) fail(`${cid || `${id}#${ci + 1}`} 的 text 为空`);
      const size = weight(text);
      chars += size;
      if (size > 400) fail(`${cid} 有 ${size.toFixed(0)} 字，超过 400 —— 请按小标题切开`);

      if (chunk?.vector !== undefined && chunk.vector !== null) {
        if (!Number.isInteger(chunk.vector) || chunk.vector < 0) {
          fail(`${cid}.vector 必须是非负整数行号`);
        } else if (!embedding) {
          fail(`${cid}.vector 给了行号，但 manifest 没有 embedding 段`);
        } else if (chunk.vector >= rows) {
          fail(`${cid}.vector=${chunk.vector} 超出 vectors.bin 的行数（${rows}）`);
        } else {
          vectors += 1;
        }
      }
    }
  }

  notes.push(`条目 ${ids.size} 个 · 块 ${chunks} 个 · 带向量的块 ${vectors} 个 · 正文约 ${chars.toFixed(0)} 字`);
  if (embedding) {
    notes.push(`向量表 ${rows} 行 × ${embedding.dim} 维（模型 ${embedding.model}）`);
    if (rows > vectors) notes.push(`有 ${rows - vectors} 行向量没有被任何块引用（检查行号是否错位）`);
  }
}

for (const note of notes) console.log(`  ${note}`);
if (errors.length) {
  console.error(`\n知识库自检未通过（${errors.length} 处）：`);
  for (const error of errors) console.error(`  ✗ ${error}`);
  process.exit(1);
}
console.log("\n知识库自检通过。");
