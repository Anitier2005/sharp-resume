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
import { configureKnowledge } from "@/lib/knowledge";

/** 栏宽范围；中间会话区永远留够可读宽度。
    COLLAPSE 是「松手时低于它就当作想收起」的阈值 —— 刻意压得很低（80px 只剩一条缝），
    因为拖动过程中用户经常只是想把栏拉窄看看，把阈值设在 260 会让面板在仍可读的时候
    突然消失。真要收起就拖到底，或者用顶栏的开关。 */
/**
 * 拖动时允许的最小**可用**宽度。
 *
 * 以前是「拖到 80px 以下才收起」，于是 80–200px 这段会拖出一个半死不活的面板：
 * 「新增候选人」折成两行、页签挤碎、状态栏换行。宁可整栏收起来，
 * 也不给一个读不了的宽度 —— 所以低于这个值就直接当收起处理。
 */
const SIDE_MIN = 210;
const SIDE_MAX = 420;
const DOC_MIN = 300;
const DOC_MAX = 720;
const CENTER_MIN = 360;

const clamp = (value: number, min: number, max: number) =>
  Math.min(Math.max(value, min), Math.max(min, max));

/**
 * 拖动用的「中途吸附」：低于最小宽度直接归零（收起），否则夹在 [min, max]。
 * 与 clamp 的区别在于它不保留 0–min 之间的尴尬宽度。
 */
const snap = (value: number, min: number, max: number) =>
  value < min ? 0 : Math.min(Math.max(value, min), Math.max(min, max));

function Shell() {
  const { state, dispatch } = useWorkspace();

  // 启动就把嵌入模型配置推给后端：知识库工具据此决定要不要算 query 向量。
  // 放在这里而不是面板里，是因为「用户没打开过知识库面板」才是常态。
  useEffect(() => {
    configureKnowledge();
  }, []);
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

      /**
       * 拖动中就按「低于最小宽度 = 收起」表现：鼠标一过门槛整栏消失，
       * 用户立刻看得懂发生了什么，而不是先看到一团折行的字再松手才塌掉。
       */
      const onMove = (ev: PointerEvent) => {
        const dx = ev.clientX - startX;
        const next =
          edge === "side"
            ? {
                side: snap(
                  startSide + dx,
                  SIDE_MIN,
                  Math.max(SIDE_MIN, Math.min(SIDE_MAX, total - startDoc - CENTER_MIN)),
                ),
                doc: startDoc,
              }
            : {
                side: startSide,
                doc: snap(
                  startDoc - dx,
                  DOC_MIN,
                  Math.max(DOC_MIN, Math.min(DOC_MAX, total - startSide - CENTER_MIN)),
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
            patch.showSide = next.side >= SIDE_MIN;
            if (patch.showSide) patch.sideW = Math.round(clamp(next.side, SIDE_MIN, SIDE_MAX));
          } else {
            patch.showDoc = next.doc >= DOC_MIN;
            if (patch.showDoc) patch.docW = Math.round(clamp(next.doc, DOC_MIN, DOC_MAX));
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
    // Toast 在外层：工作区里也会弹提示（排版失败、保存出错），
    // 反过来套的话 provider 拿不到 toast，只能 console.error
    <ToastProvider>
      <WorkspaceProvider>
        <Shell />
      </WorkspaceProvider>
    </ToastProvider>
  );
}
