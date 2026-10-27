/**
 * 全选的作用域。
 *
 * 问题：浏览器里 Ctrl/Cmd+A 是「选中整个 document」，桌面应用就会把侧栏、
 * 页签、工具栏、纸面、右栏一次全选中，满屏蓝底 —— 这在网页里没人觉得怪，
 * 在编辑器里就是错的。
 *
 * 规则（由窄到宽，命中即停）：
 *   1. 焦点在 input / textarea / 可编辑元素里 → 不拦，交给浏览器原生全选（只选这个框）
 *   2. 最后一次按下的位置落在 [data-selectable] 里 → 只选那一块（AI 回复、用户消息、纸面）
 *   3. 其余情况 → 拦掉，什么都不选（点的是侧栏、页签、按钮这些 UI）
 *
 * 为什么靠 pointerdown 记位置：点击一个 div 不会让它成为 activeElement，
 * 事后无法从 document.activeElement 反推「用户刚才在看哪块」。
 */
export function installSelectAll(): () => void {
  let area: HTMLElement | null = null;

  const onPointerDown = (event: PointerEvent) => {
    const target = event.target as Element | null;
    area = target?.closest?.<HTMLElement>("[data-selectable]") ?? null;
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (!(event.ctrlKey || event.metaKey) || event.altKey) return;
    if (event.key.toLowerCase() !== "a") return;

    const active = document.activeElement as HTMLElement | null;
    if (
      active &&
      (active.tagName === "INPUT" || active.tagName === "TEXTAREA" || active.isContentEditable)
    ) {
      return;
    }

    // 不拦住的话，浏览器会把整个窗口的 UI 文字选中
    event.preventDefault();

    // 那一块可能已经因为重渲染被换掉了（流式回复尤其常见）
    if (!area || !area.isConnected) return;

    const range = document.createRange();
    range.selectNodeContents(area);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
  };

  document.addEventListener("pointerdown", onPointerDown, true);
  document.addEventListener("keydown", onKeyDown);
  return () => {
    document.removeEventListener("pointerdown", onPointerDown, true);
    document.removeEventListener("keydown", onKeyDown);
  };
}
