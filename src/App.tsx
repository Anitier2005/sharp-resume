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
