import { useCallback, useEffect, useRef, useState } from "react";
import type { Workspace } from "@/types";
import { WorkspaceProvider, useWorkspace } from "@/store/workspace";
import { ToastProvider } from "@/components/ui/toast";
import { TitleBar } from "@/components/title-bar";
import { Sidebar } from "@/components/sidebar";
import { ChatPanel } from "@/components/chat/panel";
import { ResumePanel } from "@/components/resume/panel";
import { readableOn } from "@/lib/color";
import { installSelectAll } from "@/lib/shortcuts";

/** 栏宽范围；中间会话区永远留够可读宽度。
    FLOOR 是「再窄就当作想收起」的阈值 —— 松手时低于它就折叠，
    高于它就停在用户拖到的位置（不回弹，避免「拖到 150 跳回 180」）。 */
const SIDE_FLOOR = 140;
const SIDE_MAX = 420;
const DOC_FLOOR = 260;
const DOC_MAX = 720;
const CENTER_MIN = 360;

const clamp = (value: number, min: number, max: number) =>
  Math.min(Math.max(value, min), Math.max(min, max));

function Shell() {
  const { state, dispatch } = useWorkspace();
  const { accent, mode } = state.ui;

  const accentHex = state.ui.accentHex;
  const sideW = state.ui.sideW ?? 240;
  const docW = state.ui.docW ?? 420;
  const showSide = state.ui.showSide !== false;
  const showDoc = state.ui.showDoc !== false;

  /** 拖动中的临时宽度：不进 store，避免每帧 dispatch 触发全树重渲染 */
  const [live, setLive] = useState<{ side: number; doc: number } | null>(null);
  /** 同一份数据留个 ref：pointerup 里要读「最后落在哪」，只能从事件里写，不能在渲染期写 */
  const liveRef = useRef<{ side: number; doc: number } | null>(null);

  useEffect(() => {
    document.documentElement.dataset.accent = accent;
    document.documentElement.dataset.mode = mode;
  }, [accent, mode]);

  // Ctrl/Cmd+A 只作用于正在读的那块内容，不是整个窗口
  useEffect(() => installSelectAll(), []);

  const startDrag = useCallback(
    (edge: "side" | "doc") => (event: React.PointerEvent) => {
      if (event.button !== 0) return;
      event.preventDefault();
      const startX = event.clientX;
      /** 起点用「当前实际渲染宽度」：栏已隐藏时是 0，往外拖就能拉出来 */
      const startSide = showSide ? sideW : 0;
      const startDoc = showDoc ? docW : 0;
      const total = window.innerWidth;

      /** 拖动中允许一路到 0（= 视觉上已经收起），松手时再决定是折叠还是停在拖到的位置 */
      const onMove = (ev: PointerEvent) => {
        const dx = ev.clientX - startX;
        const next =
          edge === "side"
            ? {
                // 窗口太窄时宁可挤中间区，也要让侧栏停在 FLOOR 之上，不然一拖就折叠
                side: clamp(
                  startSide + dx,
                  0,
                  Math.max(SIDE_FLOOR, Math.min(SIDE_MAX, total - startDoc - CENTER_MIN)),
                ),
                doc: startDoc,
              }
            : {
                side: startSide,
                doc: clamp(
                  startDoc - dx,
                  0,
                  Math.max(DOC_FLOOR, Math.min(DOC_MAX, total - startSide - CENTER_MIN)),
                ),
              };
        liveRef.current = next;
        setLive(next);
      };

      const onUp = () => {
        const next = liveRef.current;
        if (next) {
          const patch: Partial<Workspace["ui"]> = {};
          if (edge === "side") {
            patch.showSide = next.side >= SIDE_FLOOR;
            if (patch.showSide) patch.sideW = Math.round(clamp(next.side, SIDE_FLOOR, SIDE_MAX));
          } else {
            patch.showDoc = next.doc >= DOC_FLOOR;
            if (patch.showDoc) patch.docW = Math.round(clamp(next.doc, DOC_FLOOR, DOC_MAX));
          }
          // 折叠时故意不改 sideW/docW：下次展开还是原来的宽度
          dispatch({ type: "patchUi", patch });
        }
        liveRef.current = null;
        setLive(null);
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        window.removeEventListener("pointercancel", onUp);
        window.removeEventListener("blur", onUp);
        document.body.classList.remove("dragging");
      };

      document.body.classList.add("dragging");
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
      // 触摸被打断、指针移出窗口、窗口失焦：都要把 dragging 收掉，
      // 否则整窗光标会永远卡在 col-resize
      window.addEventListener("pointercancel", onUp);
      window.addEventListener("blur", onUp);
    },
    [sideW, docW, showSide, showDoc, dispatch],
  );

  /** 拖动中以 live 为准，静止时以 store 为准：两边共用一条「宽度 > 0 才算显示」的判据 */
  const width = {
    side: live ? live.side : showSide ? sideW : 0,
    doc: live ? live.doc : showDoc ? docW : 0,
  };
  const sideOn = width.side > 0;
  const docOn = width.doc > 0;

  return (
    // .win 与 .app 刻意用全局类名，不进 CSS Modules：
    // 1) .win 是整棵树的主题令牌作用域，tokens.css 里
    //    `html[data-accent="x"] .win{--acc}` 必须能选中它，哈希后主题会失效；
    // 2) .app 是唯一的布局根，只有一个实例，模块化没有收益，
    //    而窄屏断点与打印规则需要稳定选择器。
    <div
      className="win"
      data-mode={mode}
      style={
        accentHex
          ? ({ "--acc": accentHex, "--acc-fg": readableOn(accentHex) } as React.CSSProperties)
          : undefined
      }
    >
      <TitleBar />
      <div
        className="app"
        style={
          {
            "--w-side": `${width.side}px`,
            "--w-doc": `${width.doc}px`,
          } as React.CSSProperties
        }
      >
        {sideOn && <Sidebar />}
        <ChatPanel />
        {docOn && <ResumePanel />}

        {/* 拖拽分隔条：视觉只有 1px，命中区 7px，压在列的接缝上。
            两条永远渲染 —— 栏收起来之后，还得能从这里把它拖回来，
            所以折叠态用 data-hidden 把它们贴到窗口左右边缘。 */}
        <div
          className="gutter"
          data-edge="side"
          data-hidden={!sideOn}
          role="separator"
          aria-orientation="vertical"
          aria-label={sideOn ? "调整侧栏宽度，拖到底可收起" : "拖出侧栏"}
          onPointerDown={startDrag("side")}
        />
        <div
          className="gutter"
          data-edge="doc"
          data-hidden={!docOn}
          role="separator"
          aria-orientation="vertical"
          aria-label={docOn ? "调整简历区宽度，拖到底可收起" : "拖出简历区"}
          onPointerDown={startDrag("doc")}
        />
      </div>
    </div>
  );
}

export default function App() {
  return (
    <WorkspaceProvider>
      <ToastProvider>
        <Shell />
      </ToastProvider>
    </WorkspaceProvider>
  );
}
