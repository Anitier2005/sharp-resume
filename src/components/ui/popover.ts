/**
 * 浮层的落地位置。
 *
 * 两件事每个浮层都要做对一次：**挂在哪**、**摆在哪**。各组件自己写一遍的结果
 * 是「取色器能翻上去、下拉不会」这种玄学差异，所以抽成工具函数。
 */

/**
 * 浮层挂载点。
 *
 * 必须优先挂进 `.win`：`--bg-elev` / `--acc` 这些令牌都定义在它身上，
 * 挂到 body 时 `var()` 全部解析成空串 —— 浮层会变成透明底、黑字，深浅主题全错。
 * body 只是兜底（比如设置面板在测试环境里单独渲染时）。
 */
export function portalHost(): HTMLElement {
  return document.querySelector<HTMLElement>(".win") ?? document.body;
}

/** 贴边留白：浮层不许贴着窗口边缘，看起来像被截断 */
const EDGE = 8;

/**
 * 优先放在锚点下方，放不下就翻到上方；水平方向夹在视口内。
 *
 * 返回的是 fixed 定位坐标（配合 portal 使用）。
 */
export function placeBelow(
  anchor: DOMRect,
  width: number,
  height: number,
  gap = 4,
): { left: number; top: number } {
  const vw = window.innerWidth;
  const vh = window.innerHeight;

  let left = anchor.left;
  if (left + width > vw - EDGE) left = vw - width - EDGE;
  if (left < EDGE) left = EDGE;

  let top = anchor.bottom + gap;
  if (top + height > vh - EDGE) {
    // 下方放不下：翻到上方。上方也放不下时贴住视口顶部，宁可盖住锚点
    top = Math.max(EDGE, anchor.top - gap - height);
  }

  return { left, top };
}

/** 上方没有空间时改用下方；用于提示气泡这类默认朝上的浮层 */
export function placeAbove(
  anchor: DOMRect,
  width: number,
  height: number,
  gap = 6,
): { left: number; top: number } {
  const vw = window.innerWidth;
  const vh = window.innerHeight;

  let left = anchor.left + anchor.width / 2 - width / 2;
  if (left + width > vw - EDGE) left = vw - width - EDGE;
  if (left < EDGE) left = EDGE;

  let top = anchor.top - gap - height;
  if (top < EDGE) top = Math.min(vh - height - EDGE, anchor.bottom + gap);

  return { left, top };
}
