import workerSrc from "pdfjs-dist/build/pdf.worker.min.mjs?url";

/**
 * PDF → 页面图片（前端渲染）。
 *
 * 为什么主路径放在前端：这是唯一**跨平台**的实现。
 * Windows 上系统自带 `Windows.Data.Pdf`（Rust 侧那条路，零依赖），
 * macOS / Linux 没有等价的系统接口，而 pdfium、mupdf 都要额外带二进制。
 * pdf.js 跑在 WebView 里，三个平台同一份代码，也不必为分发多打一份 native 库。
 *
 * 代价是它得走 JS：一份 1 页的简历约 200–400ms，比 WinRT 慢，
 * 但只在「拖进 PDF 且模型能看图」这一件事上发生，用户感知不到。
 */

/** 与 Rust 侧对齐：1240px 宽约等于 A4 150dpi，小四号字看得清 */
const DEFAULT_WIDTH = 1240;
/** 75 是「文字边缘不糊、体积可接受」的折中：单页约 350–400KB */
const JPEG_QUALITY = 0.75;

/** 渲染前几页，返回 JPEG data URL。失败直接抛，由调用方决定是否退回 Rust */
export async function renderPdfPages(
  source: ArrayBuffer | Uint8Array,
  maxPages = 3,
  width = DEFAULT_WIDTH,
): Promise<string[]> {
  // 动态 import：pdf.js 有 1MB 出头，不该进首屏包
  const pdfjs = await import("pdfjs-dist");
  pdfjs.GlobalWorkerOptions.workerSrc = workerSrc;

  // pdf.js 会接管并 detach 这块 buffer，传副本，别让调用方的数据被动掉
  const data = source instanceof Uint8Array ? new Uint8Array(source) : new Uint8Array(source);
  const task = pdfjs.getDocument({ data });
  const doc = await task.promise;
  const total = Math.min(doc.numPages, Math.max(1, maxPages));
  const pages: string[] = [];
  try {
    for (let index = 1; index <= total; index += 1) {
      const page = await doc.getPage(index);
      const base = page.getViewport({ scale: 1 });
      const viewport = page.getViewport({ scale: width / base.width });
      const canvas = document.createElement("canvas");
      canvas.width = Math.round(viewport.width);
      canvas.height = Math.round(viewport.height);
      const context = canvas.getContext("2d");
      if (!context) throw new Error("画布不可用");
      // 页面底色是白的：PDF 没有背景色，透明底在 JPEG 里会变成黑
      context.fillStyle = "#ffffff";
      context.fillRect(0, 0, canvas.width, canvas.height);
      await page.render({ canvas, canvasContext: context, viewport }).promise;
      pages.push(canvas.toDataURL("image/jpeg", JPEG_QUALITY));
      page.cleanup();
    }
  } finally {
    // 销毁 loading task 才会连 worker 一起收掉；PDFDocumentProxy 上只有 cleanup
    await task.destroy();
  }
  return pages;
}
