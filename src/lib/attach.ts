import type { Attachment } from "@/types";

function extOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

/** 纯文本类附件可以直接读进上下文；其余只给模型文件名 */
const TEXT_EXT = new Set([
  "txt", "text", "md", "markdown", "csv", "tsv", "json", "log",
  "yml", "yaml", "html", "htm", "xml", "css", "ts", "tsx", "js",
  "jsx", "py", "java", "go", "rs", "sql",
]);

export const isReadableText = (ext: string) => TEXT_EXT.has(ext.toLowerCase());

/** 单份上限 200KB，超出截断，避免把上下文撑爆 */
const MAX_CHARS = 200 * 1024;

/** 选完文件就把正文读进内存，发送时不必再等 IO */
const bodyCache = new Map<string, string>();

async function cacheBody(id: string, file: File): Promise<void> {
  if (!isReadableText(extOf(file.name)) || file.size > MAX_CHARS) return;
  try {
    const text = await file.text();
    bodyCache.set(id, text.slice(0, MAX_CHARS));
  } catch {
    /* 读不出来就只留文件名 */
  }
}

/** 浏览器 File → 附件元数据（同时后台读取可读文本） */
export function toAttachment(file: File): Attachment {
  const id = `f-${crypto.randomUUID().slice(0, 8)}`;
  void cacheBody(id, file);
  return {
    id,
    name: file.name,
    size: file.size,
    ext: extOf(file.name),
  };
}

/** 取已缓存的正文；非文本类型或还没读完返回 undefined */
export const attachmentText = (id: string): string | undefined => bodyCache.get(id);

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
