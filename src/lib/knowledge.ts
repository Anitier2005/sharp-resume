import { invoke } from "@tauri-apps/api/core";
import { loadAux, type AuxModelChoice } from "@/lib/ai";

/**
 * 知识库的前端接口。
 *
 * 界面只有两层：库 → chunk。条目是切块的中间产物（一篇长文进来被切成若干块），
 * 用户看不见也不需要维护它 —— 他要管理的就是「这个库里有哪些块」。
 *   - 库由用户维护；块可以逐块改正文、改图片说明、删
 *   - 块的摘要与关联问题由模型生成，改过正文就作废
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

/**
 * 平铺的块：一列跨条目的块，界面第二列直接用这个。
 *
 * `entryId` 由后端带出来只是为了删除时按归属校验（后端会拦「这块不属于这条」），
 * 界面不展示它 —— 条目在用户眼里不存在。
 */
export interface KnowledgeChunkItem extends KnowledgeChunk {
  entryId: string;
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

/** 一个库里的全部块，平铺。界面第二列就是它 */
export const knowledgeChunks = (libraryId: string) =>
  invoke<KnowledgeChunkItem[]>("knowledge_chunks", { libraryId });

/**
 * 改一块的正文。后端会连带清掉这块的摘要与向量（照旧正文算的，改完就是错的），
 * 返回整库的块列表 —— 少一次往返，也不会出现「列表里的字和详情里的字不一样」。
 */
export const knowledgeChunkSave = (chunkId: string, text: string) =>
  invoke<KnowledgeChunkItem[]>("knowledge_chunk_save", { chunkId, text });

/**
 * 存一段文字：后端按空行分段、每段不超过 400 字切成块。
 *
 * `title` 只用于检索结果里露一面的「这是哪来的」，界面上没有这一栏 ——
 * 用户给的是正文，标题由第一行截断得来（见 panel 的 deriveTitle）。
 */
export const knowledgeEntrySave = (draft: KnowledgeEntryDraft) =>
  invoke<KnowledgeDoc>("knowledge_entry_save", { draft: { ...draft, id: draft.id ?? "" } });

/**
 * 给这些块生成摘要与关联问题（按条目发一次请求，条目就是一篇文章切出来的那组块）。
 *
 * `force` = 连已有摘要的块也重写。
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

/** 删掉一个块；图片块会顺手清掉正文里对应的 `![名](asset:…)` 行，块删光则整条消失 */
export const knowledgeChunkDelete = (entryId: string, chunkId: string) =>
  invoke<void>("knowledge_chunk_delete", { entryId, chunkId });

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
    // 配置失败不影响面板本身的可用性（只是没有检索预览），不打扰用户
  });
}
