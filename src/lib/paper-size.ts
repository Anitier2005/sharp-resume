/**
 * A4 的物理尺寸，以及它在屏幕上（96dpi）折成多少 px。
 *
 * 为什么单独一个文件：纸面宽度这一个数被四处用到 —— CSS Modules 的 `.wrap`、
 * 离屏测量宿主的行内宽度、缩放倍率的换算、分页虚线。以前它是 374 一个魔法数字，
 * 四处各抄一遍，谁改漏一处就悄悄漂移。
 *
 * **纸必须是 1:1 真 A4**：字号、间距、页边距全是绝对 px（`render/paper.css` 的
 * `--fs` / `--pad`）。纸宽 374px 时一行只排得下 30 来个字，真 A4 宽能排 60 多个，
 * 同一份内容量出来的页数是真打印的两倍 —— 这是「排版不忠实」的根因，不是
 * 分页算法的问题。纸先对齐物理尺寸，后面所有测量才有意义。
 */

/** 纸宽：A4 的短边 */
export const A4_W = "210mm";
/** 纸高：A4 的长边 */
export const A4_H = "297mm";

/**
 * `210mm` 在 96dpi 下的 px 值（210 / 25.4 × 96 = 793.70…）。
 *
 * CSS 的 mm 与 px 是固定换算（1in = 96px），跟屏幕 DPI 无关，
 * 所以离屏宿主写 793.7 和 `.wrap` 写 210mm 是同一个宽度 —— 两边必须相等，
 * 否则量出来的纸和看到的纸不是一张。
 */
export const A4_W_PX = 793.7;

/**
 * 打印页边距（mm），与 `src/styles/print.css` 的 `@page { margin: 16mm }` 同一个值。
 * 改一处就必须改另一处，否则预览里能排下的行，打出来会掉到第二页。
 */
export const PAGE_MARGIN_MM = 16;

/**
 * 交给 Paged.js 的那份样式：定页尺寸、给页边距，并把内容层的 padding 去掉。
 *
 * 去掉 padding 是刻意的：`@page` 已经给了 16mm，内容层（`.doc` 的 `--pad`）
 * 再留一次就是双倍边距 —— 与 `@media print { .doc { padding: 0 } }` 同一口径。
 * 选择器带上 `.pagedjs_page` 前缀，权重高过 `.doc`，也不依赖样式插入顺序。
 */
export function pageSheetCss(
  pageW: string,
  pageH: string,
  marginMm: number = PAGE_MARGIN_MM,
): string {
  return [
    "@page {",
    `  size: ${pageW} ${pageH};`,
    `  margin: ${marginMm}mm;`,
    "}",
    "/* 与 @media print 同一口径：页边距归 @page，内容层不再留第二次 */",
    ".pagedjs_page .doc { padding: 0; }",
  ].join("\n");
}
