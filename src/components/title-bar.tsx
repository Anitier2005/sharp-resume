import { useState } from "react";
import { useWorkspace } from "@/store/workspace";
import { isTauri } from "@/lib/storage";
import { useToast } from "@/components/ui/toast-context";
import { Logo } from "@/components/ui/logo";
import { PanelLeft, PanelRight, WinClose, WinMax, WinMin } from "@/components/ui/icons";
import { SettingsSheet } from "@/components/settings/sheet";
import styles from "./title-bar.module.css";

async function windowAction(action: "minimize" | "maximize" | "close") {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const win = getCurrentWindow();
  if (action === "minimize") await win.minimize();
  else if (action === "maximize") await win.toggleMaximize();
  else await win.close();
}

export function TitleBar() {
  const { state, resume, dispatch } = useWorkspace();
  const toast = useToast();
  const desktop = isTauri();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const { accent, mode } = state.ui;
  const showSide = state.ui.showSide !== false;
  const showDoc = state.ui.showDoc !== false;

  const run = (action: "minimize" | "maximize" | "close") => {
    if (!desktop) {
      toast("窗口控制在打包后可用");
      return;
    }
    void windowAction(action);
  };

  return (
    <div
      className={styles.titlebar}
      data-tauri-drag-region
      data-print="hide"
      onDoubleClick={() => run("maximize")}
    >
      <span className={styles.glyph} data-tauri-drag-region>
        <Logo size={14} />
      </span>
      <span className={styles.docName}>{resume?.title ?? "未选择简历"}</span>
      <span>— Sharp Resume</span>

      <div className={styles.right}>
        {/* 面板显隐：两栏都能收起来，专心看会话或专心看简历 */}
        <button
          type="button"
          className={styles.toggle}
          aria-pressed={showSide}
          title={showSide ? "隐藏候选人与经历库" : "显示候选人与经历库"}
          onClick={() => dispatch({ type: "patchUi", patch: { showSide: !showSide } })}
        >
          <PanelLeft size={12} />
        </button>
        <button
          type="button"
          className={styles.toggle}
          aria-pressed={showDoc}
          title={showDoc ? "隐藏右侧简历区" : "显示右侧简历区"}
          onClick={() => dispatch({ type: "patchUi", patch: { showDoc: !showDoc } })}
        >
          <PanelRight size={12} />
        </button>

        <button
          type="button"
          className={styles.appearance}
          aria-label="设置"
          onClick={() => setSettingsOpen(true)}
        >
          <span className={styles.dot} data-accent={accent} data-mode={mode} />
          设置
        </button>

        <div className={styles.winbtns}>
          <button type="button" aria-label="最小化" onClick={() => run("minimize")}>
            <WinMin />
          </button>
          <button type="button" aria-label="最大化" onClick={() => run("maximize")}>
            <WinMax />
          </button>
          <button type="button" aria-label="关闭" onClick={() => run("close")}>
            <WinClose />
          </button>
        </div>
      </div>

      <SettingsSheet open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  );
}
