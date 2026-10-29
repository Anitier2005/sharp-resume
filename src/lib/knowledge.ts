import { invoke } from "@tauri-apps/api/core";
import { loadAux, type AuxModelChoice } from "@/lib/ai";

/**
 * 知识库的前端接口。
 *
 * 三层：库 → 条目 → chunk。
 *   - 库与条目由用户维护（增删改都在这边发起）
 *   - chunk 由后端按固定规则切，摘要与关联问题由模型生成
 *   - 模型只能读：`search_knowledge` / `read_knowledge` 两个工具，没有写入口
 */

export interface KnowledgeLibrary {
  id: string;
  name: string;
  note: string;
  /** 预置库：随程序分发，只能看 */
  builtin: boolean;
  entries: number;
  updatedAt: number;
  /** 向量化时用的嵌入模型（空 = 没向量化过） */
  embeddingModel: string;
  embeddingDim: number;
  /** 已带向量的块数 */
  vectorized: number;
}

export interface KnowledgeEntry {
  id: string;
  libraryId: string;
  title: string;
  tags: string[];
  chunks: number;
  /** 每一块都有摘要 = true；界面据此提示「待生成摘要」 */
  enriched: boolean;
  updatedAt: number;
  preset: boolean;
}

export interface KnowledgeChunk {
  /** `entryId#序号`：检索命中的是它，读原文也认它 */
  id: string;
  ord: number;
  heading: string;
  text: string;
  summary: string;
  questions: string[];
  /** text / image */
  kind: string;
  /** 图片块：材料池存储名，显示用 `asset:<asset>` */
  asset: string;
  assetName: string;
  /** 图片摘要来源：model / ocr / 空 */
  annotation: string;
}

export interface KnowledgeDoc {
  id: string;
  libraryId: string;
  title: string;
  tags: string[];
  body: string;
  preset: boolean;
  updatedAt: number;
  chunks: KnowledgeChunk[];
}

export interface KnowledgeLibraryDraft {
  id?: string;
  name: string;
  note?: string;
}

export interface KnowledgeEntryDraft {
  id?: string;
  libraryId: string;
  title: string;
  tags: string[];
  body: string;
}

export interface KnowledgeHit {
  chunkId: string;
  entryId: string;
  libraryId: string;
  library: string;
  title: string;
  tags: string[];
  heading: string;
  snippet: string;
  /** 图片块：`asset:<stored>` */
  image: string | null;
  score: number;
  preset: boolean;
}

/** 生成摘要用的对话模型配置（与设置里的对话模型一致） */
export interface EnrichConfig {
  baseUrl: string;
  apiKey: string;
  model: string;
}

export const knowledgeLibraries = () => invoke<KnowledgeLibrary[]>("knowledge_libraries");

export const knowledgeLibrarySave = (draft: KnowledgeLibraryDraft) =>
  invoke<KnowledgeLibrary>("knowledge_library_save", { draft: { ...draft, id: draft.id ?? "" } });

export const knowledgeLibraryDelete = (id: string) =>
  invoke<void>("knowledge_library_delete", { id });

export const knowledgeEntries = (libraryId: string) =>
  invoke<KnowledgeEntry[]>("knowledge_entries", { libraryId });

export const knowledgeEntryGet = (id: string) =>
  invoke<KnowledgeDoc | null>("knowledge_entry_get", { id });

export const knowledgeEntrySave = (draft: KnowledgeEntryDraft) =>
  invoke<KnowledgeDoc>("knowledge_entry_save", { draft: { ...draft, id: draft.id ?? "" } });

export const knowledgeEntryDelete = (id: string) =>
  invoke<void>("knowledge_entry_delete", { id });

/**
 * 给这一条的每个块生成摘要与关联问题。
 *
 * `force` = 连已有摘要的块也重写（改了正文但块文本没变时用得上）。
 * 模型没配就由调用方跳过 —— 这个接口自己不做降级判断。
 */
export const knowledgeEntryEnrich = (id: string, ai: EnrichConfig, force = false) =>
  invoke<KnowledgeDoc>("knowledge_entry_enrich", { id, ai, force });

/** 手动向量化一个库：只有用户点了才算，导入不自动算 */
export const knowledgeVectorize = (libraryId: string, embedding: EnrichConfig) =>
  invoke<KnowledgeLibrary>("knowledge_vectorize", { libraryId, embedding });

/** 解析文件：返回文本与（可能的）图片字节 */
export interface Ingested {
  title: string;
  text: string;
  images: { name: string; ext: string; bytes: number[] }[];
}

export const ingestFile = (name: string, bytes: number[]) =>
  invoke<Ingested>("ingest_file", { name, bytes });

export interface ImportTextDraft {
  libraryId: string;
  title: string;
  tags: string[];
  text: string;
}

/** 导入文本（后端用模型分片，模型挂了会退回机械切块） */
export const knowledgeImportText = (draft: ImportTextDraft, ai: EnrichConfig) =>
  invoke<KnowledgeDoc>("knowledge_import_text", { draft, ai });

export interface ImportImageDraft {
  libraryId: string;
  title: string;
  asset: string;
  assetName: string;
  summary: string;
  annotation: string;
}

export const knowledgeImportImage = (draft: ImportImageDraft) =>
  invoke<KnowledgeDoc>("knowledge_import_image", { draft });

/** 给图片写一句话说明：model / ocr / none */
export const knowledgeImageAnnotate = (args: {
  bytes: number[];
  ext: string;
  mode: "model" | "ocr" | "none";
  ai: EnrichConfig | null;
  ocr: EnrichConfig | null;
}) => invoke<{ summary: string; annotation: string }>("knowledge_image_annotate", args);

/** 重写一个图片块的说明：只换 summary / annotation，图本身还在材料池里 */
export const knowledgeImageUpdate = (chunkId: string, summary: string, annotation: string) =>
  invoke<KnowledgeDoc>("knowledge_image_update", { chunkId, summary, annotation });

/** 删掉一个块；图片块会顺手清掉正文里对应的 `![名](asset:…)` 行 */
export const knowledgeChunkDelete = (entryId: string, chunkId: string) =>
  invoke<KnowledgeDoc>("knowledge_chunk_delete", { entryId, chunkId });

export const knowledgeSearch = (query: string, libraryId?: string, limit = 8) =>
  invoke<KnowledgeHit[]>("knowledge_search", { query, libraryId: libraryId ?? "", limit });

/**
 * 把设置里的嵌入模型同步给后端。
 *
 * 调用点只有两处：应用启动（面板挂载）与设置页保存后 —— 后端拿它决定
 * 「要不要算 query 向量」，没配就只用词项，索引不用重算。
 */
export function configureKnowledge(choice: AuxModelChoice | null = loadAux().embedding): void {
  const payload = choice && choice.model.trim()
    ? {
        model: choice.model.trim(),
        baseUrl: choice.baseUrl.trim(),
        apiKey: choice.apiKey,
      }
    : null;
  void invoke("knowledge_configure", { embedding: payload }).catch(() => {
    // 浏览器调试模式没有后端：静默跳过，面板照样能用（只是没有检索预览）
  });
}
