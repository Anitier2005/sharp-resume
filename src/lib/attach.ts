import { invoke } from "@tauri-apps/api/core";
import type { Attachment } from "@/types";
import { loadAi, loadAux } from "@/lib/ai";
import { isTauri } from "@/lib/storage";
import { renderPdfPages } from "@/lib/pdf-render";

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
/** PDF 提取的原始字节上限：超过它解析又慢又容易把内存吃爆 */
const MAX_PDF_BYTES = 20 * 1024 * 1024;
/** OCR 的原图上限：OCR 只认图，超大图先让 convert_image 那条路压一压 */
const MAX_OCR_BYTES = 10 * 1024 * 1024;

/** 选完文件就把正文读进内存，发送时不必再等 IO */
const bodyCache = new Map<string, string>();
/** 读取任务：选完文件就启动，发送时 await 它，抢在读取完成前发送也不会丢正文 */
const pending = new Map<string, Promise<void>>();
/** 原始 File：OCR 按「当前模型有没有视觉」决定跑不跑，跑的时候要字节 */
const fileCache = new Map<string, File>();
/** 图片的 data URL —— 模型支持视觉时 base64 直传，不再走 OCR */
const imageCache = new Map<string, string>();
/** OCR 结果。懒执行：OCR 一张要几秒，不在选文件时抢跑 */
const ocrCache = new Map<string, string>();

async function cacheBody(id: string, file: File): Promise<void> {
  if (!isReadableText(extOf(file.name)) || file.size > MAX_CHARS) return;
  try {
    const text = await file.text();
    bodyCache.set(id, text.slice(0, MAX_CHARS));
  } catch {
    /* 读不出来就只留文件名 */
  }
}

/**
 * PDF 的文字在 Rust 侧提（pdf-extract，纯 Rust）。
 * 浏览器调试模式没有通讯层，保持「只给文件名」的旧行为。
 */
async function extractPdf(id: string, file: File): Promise<void> {
  if (!isTauri() || file.size > MAX_PDF_BYTES) return;
  try {
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
    const text = await invoke<string>("extract_pdf", { bytes });
    bodyCache.set(id, text.slice(0, MAX_CHARS));
  } catch (error) {
    // 提取失败也要说清原因 —— 模型读到「只有文件名」时，用户收到的回答
    // 会是「请把内容贴给我」，把真实原因留在这他才知道该换扫描件还是修 PDF
    bodyCache.set(id, `（这份 PDF 读不出来：${errorText(error)}）`);
  }
}

/** 图片：存一份 data URL 给视觉模型直传；OCR 懒到发送时按需跑 */
async function cacheImage(id: string, file: File): Promise<void> {
  if (file.size > MAX_OCR_BYTES) return;
  try {
    const dataUrl = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result));
      reader.onerror = () => reject(new Error("读不出图片内容"));
      reader.readAsDataURL(file);
    });
    imageCache.set(id, dataUrl);
  } catch {
    /* 读不出就只留文件名 */
  }
}

/**
 * 把一张图的字节读成文字：内置 tesseract → 设置里的 OCR 接口 → 明确失败文案。
 *
 * 两条腿：内置 OCR（只有带 ocr feature 的构建才有）→ 设置里配的 OCR 接口。
 * lite 构建第一条腿必然报「这一版没有内置 OCR」，这时要**自动**走第二条，
 * 而不是把错误原样扔给用户 —— 否则功能等于凭空消失。
 *
 * 不缓存：图片附件与 PDF 渲染出来的页面各自在外面决定缓存键。
 */
async function ocrBytes(bytes: number[], ext: string): Promise<string> {
  let failure = "";
  try {
    return (await invoke<string>("ocr_image", { bytes, ext })).slice(0, MAX_CHARS);
  } catch (error) {
    failure = errorText(error);
  }

  const aux = loadAux().ocr;
  if (aux && aux.model.trim()) {
    try {
      const ai = loadAi();
      return (
        await invoke<string>("ocr_api", {
          bytes,
          ext,
          // 接口地址/密钥留空表示沿用对话模型那把，与设置页的说明一致
          baseUrl: aux.baseUrl.trim() || ai.baseUrl,
          apiKey: aux.apiKey.trim() || ai.apiKey,
          model: aux.model,
        })
      ).slice(0, MAX_CHARS);
    } catch (error) {
      failure = `${failure}；OCR 接口：${errorText(error)}`;
    }
  } else if (!failure.includes("没有内置 OCR")) {
    failure = `${failure}；也可在 设置 → 模型 → OCR 配置一个能看图的模型`;
  }
  return `（OCR 失败：${failure}）`;
}

/** OCR 是兜底通道，只在「模型看不了图」时才花那几秒。同一次会话里一张图只跑一次 */
export async function ensureOcrText(id: string): Promise<string | undefined> {
  const cached = ocrCache.get(id);
  if (cached) return cached;
  const file = fileCache.get(id);
  if (!file || !isImage(extOf(file.name)) || !isTauri() || file.size > MAX_OCR_BYTES) return undefined;

  const text = await ocrBytes(Array.from(new Uint8Array(await file.arrayBuffer())), extOf(file.name));
  ocrCache.set(id, text);
  return text;
}

/** data URL → 字节。PDF 页面只在内存里，OCR 接口要的是原始字节 */
function dataUrlBytes(url: string): number[] {
  const base64 = url.slice(url.indexOf(",") + 1);
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return Array.from(bytes);
}

/**
 * 扫描件 PDF 的兜底：提不出文字就渲染成页面，逐页 OCR。
 *
 * 为什么要这条路：文字层是「有没有」的问题，扫描件天生没有。
 * 模型看不了图时，这份 PDF 在它眼里就是一片空白 —— 而用户明明传了东西。
 * 只跑前两页：一页 OCR 要几秒，再多就变成让用户干等。
 */
export async function pdfOcrText(id: string, maxPages = 2): Promise<string> {
  const cached = ocrCache.get(`pdf-${id}`);
  if (cached) return cached;
  const pages = await pdfPageImages(id, maxPages);
  if (!pages.length) return "";

  const parts: string[] = [];
  for (let index = 0; index < pages.length; index += 1) {
    const text = await ocrBytes(dataUrlBytes(pages[index]), "jpg");
    // OCR 没配好时每页都会失败：第一页就停，别让同一句话重复两遍
    if (text.startsWith("（OCR 失败")) {
      ocrCache.set(`pdf-${id}`, text);
      return text;
    }
    parts.push(`【第 ${index + 1} 页】\n${text.trim()}`);
  }
  const joined = parts.join("\n\n");
  ocrCache.set(`pdf-${id}`, joined);
  return joined;
}

const errorText = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

const IMAGE_EXT = new Set(["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif"]);

export const isImage = (ext: string) => IMAGE_EXT.has(ext.toLowerCase());

/** 落盘前剥掉临时预览，避免把 object URL 写进会话文件 */
export const stripPreview = (file: Attachment): Attachment => {
  const { preview, ...rest } = file;
  void preview;
  return rest;
};

/** 浏览器 File → 附件元数据（同时后台读取可读文本；图片附带缩略图） */
export function toAttachment(file: File): Attachment {
  const id = `f-${crypto.randomUUID().slice(0, 8)}`;
  const ext = extOf(file.name);
  // PDF / 图片是「读不到正文」的重灾区，各自的通道见上面的函数注释。
  // fileCache 无条件存：OCR 懒执行，跑的时候才需要字节。
  fileCache.set(id, file);
  pending.set(
    id,
    (async () => {
      await cacheBody(id, file);
      // PDF 的文字要等 Rust 解析，几百毫秒到几秒；把它记成任务，
      // 用户拖进来立刻回车时发送侧能等它，而不是拿到 undefined 当「没内容」
      if (ext === "pdf") await extractPdf(id, file);
      if (isImage(ext)) await cacheImage(id, file);
    })(),
  );
  return {
    id,
    name: file.name,
    size: file.size,
    preview: isImage(ext) ? URL.createObjectURL(file) : undefined,
    ext: extOf(file.name),
  };
}

/** 取已缓存的正文；非文本类型或还没读完返回 undefined */
export const attachmentText = (id: string): string | undefined => bodyCache.get(id);

/**
 * 等这份附件读完再取正文。
 *
 * 桌面端的材料段是一条字符串（发给 Rust），发送时必须拿到正文本身；
 * 而 PDF 解析是异步的、可能还没回来。这里等任务收尾，读不出就返回 undefined，
 * 由调用方写明「读不到」，别让模型以为用户什么都没传。
 */
export async function attachmentBody(id: string): Promise<string | undefined> {
  const job = pending.get(id);
  if (job) {
    try {
      await job;
    } catch {
      /* 失败已经写进 bodyCache 的错误文案里了 */
    }
  }
  return bodyCache.get(id);
}

/** 等这份附件读完（正文 / 图片 data URL 都好了）再往下走 */
export async function attachmentReady(id: string): Promise<void> {
  const job = pending.get(id);
  if (!job) return;
  try {
    await job;
  } catch {
    /* 同上：错误已经落在文案里 */
  }
}

/** 原始 File：PDF 渲染成图片、OCR 兜底都要原始字节 */
export const attachmentFile = (id: string): File | undefined => fileCache.get(id);

/**
 * PDF 前几页 → 图片 data URL。
 *
 * 两条路，先走跨平台的：
 *   1. pdf.js 在 WebView 里渲染 —— macOS / Linux 也照样能用，不依赖系统组件；
 *   2. 失败再交给 Rust 侧（Windows 用系统自带的 Windows.Data.Pdf，其余平台用
 *      系统里装了的 pdftoppm / gs / mutool）。
 * 都失败就返回空数组：调用方按「只有文字」继续，一次渲染失败不该毁掉整轮对话。
 */
export async function pdfPageImages(id: string, maxPages = 3): Promise<string[]> {
  const file = fileCache.get(id);
  if (!file) return [];
  try {
    const pages = await renderPdfPages(await file.arrayBuffer(), maxPages);
    if (pages.length) return pages;
  } catch {
    /* pdf.js 也认不出的 PDF 很少见，交给后端再试一次 */
  }
  if (!isTauri() || file.size > MAX_PDF_BYTES) return [];
  try {
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
    const pages = await invoke<{ dataUrl: string }[]>("pdf_page_images", { bytes, maxPages });
    return pages.map((page) => page.dataUrl);
  } catch {
    return [];
  }
}

/** 取图片的 data URL；非图片、超大或浏览器调试模式返回 undefined */
export const attachmentImage = (id: string): string | undefined => imageCache.get(id);

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
