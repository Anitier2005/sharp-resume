import { invoke } from "@tauri-apps/api/core";
import type { Attachment } from "@/types";

/**
 * 附件：这里只做「上传」和「取回显示」，**不做任何解析**。
 *
 * 取字、PDF 拆页、图片 OCR 全在 Rust 侧（`src-tauri/src/attach.rs`）：
 * 前端只该管怎么显示，把文件塞进 WebView 里解析会让刷新一次就丢缓存，
 * 而且浏览器调试模式与桌面模式必然各长一套。
 */

/** 上传回执：后端已经收下并解析过这份文件 */
export interface AttachInfo {
  stored: string;
  name: string;
  ext: string;
  kind: string;
  chars: number;
  scanned: boolean;
}

const IMAGE_EXT = new Set(["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"]);

/** 扩展名（小写、不带点；没有扩展名就是空串） */
export const extOf = (name: string) =>
  name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";

export const isImage = (ext: string) => IMAGE_EXT.has(ext.toLowerCase());

/** 单份附件上限 12MB：再大就不是简历或证件照了 */
const MAX_BYTES = 12 * 1024 * 1024;

/** 已解析成 blob URL 的图片，按 stored 缓存，避免历史消息反复读盘 */
const srcCache = new Map<string, string>();

/** 落盘前剥掉临时预览，避免把 object URL 写进会话文件 */
export const stripPreview = (file: Attachment): Attachment => {
  const { preview, ...rest } = file;
  void preview;
  return rest;
};

/** 浏览器 File → 附件。桌面端顺手把字节交给后端解析 */
export async function toAttachment(file: File): Promise<Attachment> {
  const ext = extOf(file.name);
  const id = `f-${crypto.randomUUID().slice(0, 8)}`;
  // 待发送阶段用 object URL 做缩略图；发出去之后靠 stored 从 inbox 取回
  const preview = isImage(ext) ? URL.createObjectURL(file) : undefined;

  if (file.size > MAX_BYTES) {
    throw new Error(`「${file.name}」超过 12MB，先压缩再放进来`);
  }
  // base64 而不是数字数组：12MB 的文件序列化成数字要 40MB 的 JSON
  const info = await invoke<AttachInfo>("attach_put", {
    name: file.name,
    dataBase64: toBase64(await file.arrayBuffer()),
  });
  return { id, name: file.name, size: file.size, ext, preview, stored: info.stored };
}

/**
 * 附件 → 发给后端的引用。
 *
 * 只有名字：正文、OCR、PDF 页面都在 Rust 组上下文时现取，
 * 前端既拿不到也不该拿这些字节。
 */
export const attachRefs = (list: Attachment[]) =>
  list.map((file) => ({ stored: file.stored ?? "", name: file.name, ext: file.ext }));

/** 缩略图地址：待发送的用 object URL，落盘后的从 inbox 取回 */
export async function attachmentSrc(file: Attachment): Promise<string | null> {
  if (file.preview) return file.preview;
  const stored = file.stored;
  if (!stored) return null;
  const cached = srcCache.get(stored);
  if (cached) return cached;
  try {
    // 后端用 ipc::Response 发原始字节，这里拿到的是 ArrayBuffer
    const bytes = await invoke<ArrayBuffer>("attach_get", { stored });
    const url = URL.createObjectURL(new Blob([bytes]));
    srcCache.set(stored, url);
    return url;
  } catch {
    return null;
  }
}

/** ArrayBuffer → base64。分块拼，别把几 MB 一次性摊进函数参数 */
function toBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  const chunk = 0x8000;
  let binary = "";
  for (let index = 0; index < bytes.length; index += chunk) {
    binary += String.fromCharCode(...bytes.subarray(index, index + chunk));
  }
  return btoa(binary);
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
