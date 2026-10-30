import { describe, expect, it } from "vitest";
import { A4_H, A4_W, A4_W_PX, PAGE_MARGIN_MM, pageSheetCss } from "./paper-size";

describe("A4 尺寸常量", () => {
  it("793.7px 就是 210mm（96dpi 下 1in = 96px）", () => {
    // CSS 的 mm 与 px 是固定换算，跟屏幕 DPI 无关；两处写法必须落在同一个数上，
    // 否则离屏量和屏幕上看到的不是同一张纸。
    expect(A4_W_PX).toBeCloseTo((210 / 25.4) * 96, 0);
    expect(A4_W).toBe("210mm");
    expect(A4_H).toBe("297mm");
  });

  it("页边距与 print.css 的 @page 取同一个值", () => {
    expect(PAGE_MARGIN_MM).toBe(16);
  });
});

describe("pageSheetCss", () => {
  it("给出页尺寸与页边距，Paged.js 才知道该切多大的纸", () => {
    const css = pageSheetCss(A4_W, A4_H);
    expect(css).toContain("size: 210mm 297mm;");
    expect(css).toContain("margin: 16mm;");
  });

  it("内容层不再留第二次页边距（与 @media print 同一口径）", () => {
    // 双倍边距会让行宽凭空少 20mm，一页的内容被推成两页
    expect(pageSheetCss(A4_W, A4_H)).toContain(".pagedjs_page .doc { padding: 0; }");
  });

  it("换纸型只改参数，不靠调用方拼字符串", () => {
    const css = pageSheetCss("148mm", "210mm", 10);
    expect(css).toContain("size: 148mm 210mm;");
    expect(css).toContain("margin: 10mm;");
  });
});
