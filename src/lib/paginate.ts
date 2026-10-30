/* ==========================================================================
   真分页：把一页成品切成若干张 A4 纸

   为什么用 Paged.js：预览里原来只有一条按实测高度画的虚线，它说的是「大约在这儿
   被切断」，而不是「切断之后长什么样」—— 段落会不会被劈成两半、小标题会不会
   孤零零留在上一页，全都看不见。分页得真做一遍，预览和打印才是一回事。

   只用 `Previewer`，不用 `paged.polyfill.js`：polyfill 一引就接管整个文档的
   DOMContentLoaded，而这里要的是「按需分一次」。
   ========================================================================== */

import { pageSheetCss } from "./paper-size";

/**
 * Paged.js 会把解析过的样式插进 `document.head`，而且**从不自己回收**。
 * 每分一次页就多一份，head 里的 `<style>` 会越堆越多，最后一次分页的样式也会
 * 一直作用在页面上（比如把 `.doc` 的 padding 抹掉，回退档的单张纸就没有页边距了）。
 */
export function resetPagination(): void {
  for (const el of document.querySelectorAll("style[data-pagedjs-inserted-styles]")) {
    el.remove();
  }
}

/**
 * 给文档里所有现存的 `<style>` 打上 ignore 标记。
 *
 * Paged.js 在**没显式传样式表**时会调用 `removeStyles()`，把页面上每一张
 * `<style>`（含 React 渲染的那份预设样式）从 DOM 里摘走、塞进自己的样式表。
 * 我们走的是「显式传样式表」这条分支，本来不会触发它；但这条保底不能省 ——
 * 一旦哪天调用方式变了，被摘走的就是 React 手里那个节点，下一次重渲染直接错乱。
 */
function markStylesIgnored(): void {
  for (const el of document.querySelectorAll("style:not([data-pagedjs-ignore])")) {
    el.setAttribute("data-pagedjs-ignore", "");
  }
}

/**
 * 纸面自带的预设 CSS 必须**手动**交给 Paged.js。
 *
 * 它只接收「样式表」参数，不会去 DOM 里捡 `<style>`（捡的那条分支就是 removeStyles，
 * 会连锅端走）。这里从 `.paper` 里取那唯一一份预设样式，顺序也保持原样。
 */
function presetSheets(source: HTMLElement): Record<string, string>[] {
  const paper = source.closest('[data-print="sheet"]') ?? document.body;
  const sheets: Record<string, string>[] = [];
  for (const el of paper.querySelectorAll("style")) {
    // 上一轮插进 head 的那份不算「纸面自带」，否则会把旧页尺寸再喂一遍
    if (el.hasAttribute("data-pagedjs-inserted-styles")) continue;
    const text = el.textContent ?? "";
    if (text.trim()) sheets.push({ [window.location.href]: text });
  }
  return sheets;
}

/**
 * 把 `source` 切成 A4 纸，结果挂进 `target`。
 *
 * 分页分两步：先在**离屏但仍有布局**的暂存区里排完，再整棵搬进 `target`。
 * 直接往可见容器里排会让用户看见纸一张张往外冒（Paged.js 是边排边追加的）；
 * 而离屏不能图省事用 `display: none` —— 没有布局就没有高度，Paged.js 会量出 0，
 * 一页都切不出来。所以暂存区是「挪到画布外 + visibility: hidden」：
 * 尺寸照算，只是看不见。
 *
 * `source` 会被 `cloneNode` 一份再喂进去（Paged.js 自己也会克隆节点到纸上），
 * 因此原树上挂的 React 事件一个都不会跟过来 —— 调用方不许指望分页结果可交互。
 */
export async function paginate(
  source: HTMLElement,
  target: HTMLElement,
  pageW: string,
  pageH: string,
  /** 调用方在等结果期间换了页 / 关了预览：返回 true 就丢弃这一轮 */
  cancelled?: () => boolean,
): Promise<void> {
  markStylesIgnored();

  // 上一轮的样式留到最后再删：换页瞬间旧纸还挂在 target 上，
  // 先删会让它裸奔一帧。
  const stale = Array.from(document.querySelectorAll("style[data-pagedjs-inserted-styles]"));

  const stage = document.createElement("div");
  // 暂存区本身也要打 ignore：它在 body 里，下一轮 removeStyles 不该把它算进去
  stage.setAttribute("data-pagedjs-ignore", "");
  stage.setAttribute("aria-hidden", "true");
  stage.style.cssText = [
    "position: absolute",
    "top: 0",
    "left: -20000px",
    `width: ${pageW}`,
    "visibility: hidden",
    "pointer-events: none",
    "z-index: -1",
  ].join(";");

  const flow = source.cloneNode(true) as HTMLElement;
  // data-ref 是 Paged.js 给节点编的号，跨轮复用会把两份文档的引用对错
  flow.removeAttribute("data-ref");

  /**
   * **必须再套一层壳**：Paged.js 只从「传进去那个元素的 firstChild」开始搬，
   * 根元素本身永远进不了纸 —— 直接传 `.doc`，切出来的纸上就没有 `.doc` 这层，
   * 而 `paper.css` 里所有规则（font-size / line-height / 页边距）全都挂在 `.doc` 上。
   * 结果就是内容按 16px 默认字号排一遍，明明半页的东西被切成两页。
   * 这一层壳让 `.doc` 变成「子节点」，它才会被完整克隆进每一张纸。
   */
  const shell = document.createElement("div");
  shell.appendChild(flow);
  stage.appendChild(shell);
  document.body.appendChild(stage);

  try {
    // 按需加载：分页器只服务「预览」这一条支路，让它跟着首屏一起下载
    // 会给启动白加两百多 KB 的解析时间 —— 不打开预览的用户一次都用不上。
    const { Previewer } = await import("pagedjs");
    const previewer = new Previewer();
    await previewer.preview(
      shell,
      [...presetSheets(source), { [window.location.href]: pageSheetCss(pageW, pageH) }],
      stage,
    );

    // 切页是异步的：这一轮排到一半用户可能已经翻页了。
    // 不挡一下，慢的那一轮会把新页的纸盖成旧页的。
    if (cancelled?.()) return;

    const pages = stage.querySelector(".pagedjs_pages");
    target.replaceChildren();
    // 搬一整个 `.pagedjs_pages`：纸的尺寸全是 mm / px 绝对值，
    // 换挂载点不会重排，所以搬完和离线时长得一模一样。
    if (pages) target.appendChild(pages);
  } finally {
    stage.remove();
    for (const el of stale) el.remove();
  }
}
