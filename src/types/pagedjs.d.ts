/**
 * Paged.js 0.4.3 不带类型声明（包里没有 `types` 字段）。
 * 这里只声明本项目用到的那一小块：`Previewer`。
 *
 * 声明成最小的形状而不是 `any`：分页参数写错（比如样式表传成字符串）时，
 * Paged.js 会去 `fetch` 一个不存在的 URL，静默失败成 0 页 —— 那种错误
 * 只有类型能在这里挡住。
 */
declare module "pagedjs" {
  export class Previewer {
    constructor(options?: Record<string, unknown>);
    /**
     * `stylesheets` 只接受「{ href: cssText }」这种对象形式：
     * 传字符串会被当成 URL 去请求。
     */
    preview(
      content: HTMLElement | string,
      stylesheets: Record<string, string>[],
      renderTo?: HTMLElement,
    ): Promise<unknown>;
  }
}
