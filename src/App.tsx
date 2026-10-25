import { useCallback, useEffect, useRef, useState } from "react";
import { WorkspaceProvider, useWorkspace } from "@/store/workspace";
import { ToastProvider } from "@/components/ui/toast";
import { TitleBar } from "@/components/title-bar";
import { Sidebar } from "@/components/sidebar";
import { ChatPanel } from "@/components/chat/panel";
import { ResumePanel } from "@/components/resume/panel";

/** 栏宽取值范围；中间会话区永远留够可读宽度 */
const SIDE_MIN = 180;
const SIDE_MAX = 420;
const DOC_MIN = 320;
const DOC_MAX = 720;
const CENTER_MIN = 360;

const clamp = (value: number, min: number, max: number) =>
  Math.min(Math.max(value, min), Math.max(min, max));

function Shell() {
  const { state, dispatch } = useWorkspace();
  const { accent, mode } = state.ui;

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

  const startDrag = useCallback(
    (edge: "side" | "doc") => (event: React.PointerEvent) => {
      if (event.button !== 0) return;
      event.preventDefault();
      const startX = event.clientX;
      const startSide = sideW;
      const startDoc = docW;
      const total = window.innerWidth;

      const onMove = (ev: PointerEvent) => {
        const dx = ev.clientX - startX;
        const next =
          edge === "side"
            ? {
                side: clamp(startSide + dx, SIDE_MIN, Math.min(SIDE_MAX, total - startDoc - CENTER_MIN)),
                doc: startDoc,
              }
            : {
                side: startSide,
                doc: clamp(startDoc - dx, DOC_MIN, Math.min(DOC_MAX, total - startSide - CENTER_MIN)),
              };
        liveRef.current = next;
        setLive(next);
      };

      const onUp = () => {
        const next = liveRef.current;
        if (next) dispatch({ type: "patchUi", patch: { sideW: next.side, docW: next.doc } });
        liveRef.current = null;
        setLive(null);
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        document.body.classList.remove("dragging");
      };

      document.body.classList.add("dragging");
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
    },
    [sideW, docW, dispatch],
  );

  const width = { side: live?.side ?? sideW, doc: live?.doc ?? docW };

  return (
    // .win 与 .app 刻意用全局类名，不进 CSS Modules：
    // 1) .win 是整棵树的主题令牌作用域，tokens.css 里
    //    `html[data-accent="x"] .win{--acc}` 必须能选中它，哈希后主题会失效；
    // 2) .app 是唯一的布局根，只有一个实例，模块化没有收益，
    //    而窄屏断点与打印规则需要稳定选择器。
    <div className="win" data-mode={mode}>
      <TitleBar />
      <div
        className="app"
        style={
          {
            "--w-side": `${showSide ? width.side : 0}px`,
            "--w-doc": `${showDoc ? width.doc : 0}px`,
          } as React.CSSProperties
        }
      >
        {showSide && <Sidebar />}
        <ChatPanel />
        {showDoc && <ResumePanel />}

        {/* 拖拽分隔条：视觉只有 1px，命中区 7px，压在列的接缝上 */}
        {showSide && (
          <div
            className="gutter"
            data-edge="side"
            role="separator"
            aria-orientation="vertical"
            aria-label="调整侧栏宽度"
            onPointerDown={startDrag("side")}
          />
        )}
        {showDoc && (
          <div
            className="gutter"
            data-edge="doc"
            role="separator"
            aria-orientation="vertical"
            aria-label="调整简历区宽度"
            onPointerDown={startDrag("doc")}
          />
        )}
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
