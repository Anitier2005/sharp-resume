import { useEffect, type RefObject } from "react";

/**
 * 遮罩上的滚轮拦截。
 *
 * 遮罩挡得住点击，挡不住滚轮 —— 弹窗开着时滚一下，底下的纸面照滚，
 * 用户会以为「窗跟着内容动了」。标了 `[data-scroll]` 的区域放行，列表本身还能滚。
 *
 * 用原生监听而不是 onWheel：React 给 wheel 挂的是 passive，preventDefault 无效。
 *
 * 单独成文件（而不是塞在 modal.tsx 里）有两个理由：
 *   1. 大面板（设置 560、资料 764）不走 `Modal`，但这条行为必须一致，
 *      否则「有的窗能滚底下的纸、有的不能」就成了玄学；
 *   2. 组件文件只导出组件，Fast Refresh 才不会失效。
 */
export function useVeilWheelLock(ref: RefObject<HTMLElement | null>, enabled = true) {
  useEffect(() => {
    if (!enabled) return;
    const veil = ref.current;
    if (!veil) return;
    const onWheel = (event: WheelEvent) => {
      const scroller = (event.target as HTMLElement | null)?.closest?.("[data-scroll]");
      if (scroller) return;
      event.preventDefault();
    };
    veil.addEventListener("wheel", onWheel, { passive: false });
    return () => veil.removeEventListener("wheel", onWheel);
  }, [ref, enabled]);
}
