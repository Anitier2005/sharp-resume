import { useEffect } from "react";
import { WorkspaceProvider, useWorkspace } from "@/store/workspace";
import { ToastProvider } from "@/components/ui/toast";
import { TitleBar } from "@/components/title-bar";
import { Sidebar } from "@/components/sidebar";
import { ChatPanel } from "@/components/chat/panel";
import { ResumePanel } from "@/components/resume/panel";

function Shell() {
  const { state } = useWorkspace();
  const { accent, mode } = state.ui;

  useEffect(() => {
    document.documentElement.dataset.accent = accent;
    document.documentElement.dataset.mode = mode;
  }, [accent, mode]);

  return (
    // .win 与 .app 刻意用全局类名，不进 CSS Modules：
    // 1) .win 是整棵树的主题令牌作用域，tokens.css 里
    //    `html[data-accent="x"] .win{--acc}` 必须能选中它，哈希后主题会失效；
    // 2) .app 是唯一的布局根，只有一个实例，模块化没有收益，
    //    而窄屏断点与打印规则需要稳定选择器。
    <div className="win" data-mode={mode}>
      <TitleBar />
      <div className="app">
        <Sidebar />
        <ChatPanel />
        <ResumePanel />
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
