import { invoke } from "@tauri-apps/api/core";
import { loadAux, type AuxModelChoice } from "@/lib/ai";

/**
 * 知识库的前端接口。
 *
 * 真相全在 Rust：预置条目来自编译期嵌入的 manifest，用户条目在 app.db。
 * 前端只负责「列出来、读一条、写一条、删一条」和把嵌入模型配置推给它 ——
 * 索引与打分都在后端，面板里的预览检索和模型调工具走的是同一条路。
 */

export interface KnowledgeEntry {
  id: string;
  title: string;
  tags: string[];
  /** 预置 = 只读 */
  preset: boolean;
  updatedAt: number;
}

export interface KnowledgeDoc {
  id: string;
  title: string;
  tags: string[];
  body: string;
  preset: boolean;
  updatedAt: number;
}

export interface KnowledgeDraft {
  id?: string;
  title: string;
  tags: string[];
  body: string;
}

export interface KnowledgeHit {
  id: string;
  title: string;
  tags: string[];
  heading: string;
  snippet: string;
  score: number;
  preset: boolean;
}

export const knowledgeList = () => invoke<KnowledgeEntry[]>("knowledge_list");

export const knowledgeGet = (id: string) => invoke<KnowledgeDoc | null>("knowledge_get", { id });

export const knowledgeUpsert = (draft: KnowledgeDraft) =>
  invoke<KnowledgeDoc>("knowledge_upsert", { draft: { ...draft, id: draft.id ?? "" } });

export const knowledgeDelete = (id: string) => invoke<void>("knowledge_delete", { id });

export const knowledgeSearch = (query: string, limit = 8) =>
  invoke<KnowledgeHit[]>("knowledge_search", { query, limit });

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
