import { ACCENTS, TEXT_SCALES } from "@/types";
import { useWorkspace } from "@/store/workspace";
import { isTauri } from "@/lib/storage";
import { useToast } from "@/components/ui/toast-context";
import { Dropdown, MenuLabel, MenuSep } from "@/components/ui/menu";
import { PanelLeft, PanelRight, WinClose, WinMax, WinMin } from "@/components/ui/icons";
import styles from "./title-bar.module.css";
import menu from "@/components/ui/menu.module.css";

async function windowAction(action: "minimize" | "maximize" | "close") {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const win = getCurrentWindow();
  if (action === "minimize") await win.minimize();
  else if (action === "maximize") await win.toggleMaximize();
  else await win.close();
}

export function TitleBar() {
  const { state, resume, dispatch, textScale, systemTextScale, setTextScale } = useWorkspace();
  const toast = useToast();
  const desktop = isTauri();
  const { accent, mode } = state.ui;
  const autoScale = state.ui.textScale == null;
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
      <span className={styles.glyph} />
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

        <Dropdown
          width={222}
          trigger={
            <button type="button" className={styles.appearance} aria-label="外观">
              <span
                className={styles.dot}
                data-accent={accent}
                data-mode={mode}
              />
              外观
            </button>
          }
        >
          <MenuLabel>明暗</MenuLabel>
          <div className={menu.seg}>
            <button
              type="button"
              aria-pressed={mode === "light"}
              onClick={() => dispatch({ type: "setMode", mode: "light" })}
            >
              浅色
            </button>
            <button
              type="button"
              aria-pressed={mode === "dark"}
              onClick={() => dispatch({ type: "setMode", mode: "dark" })}
            >
              深色
            </button>
          </div>
          <MenuSep />
          <MenuLabel>强调色</MenuLabel>
          <div className={menu.swatches}>
            {ACCENTS.map((item) => (
              <button
                key={item.key}
                type="button"
                className={menu.sw}
                aria-pressed={accent === item.key}
                onClick={() => dispatch({ type: "setAccent", accent: item.key })}
              >
                <i style={{ background: item.color }} />
                {item.label}
              </button>
            ))}
          </div>
          <MenuSep />
          <MenuLabel>字号 · {Math.round(textScale * 100)}%</MenuLabel>
          <div className={menu.seg}>
            <button
              type="button"
              aria-pressed={autoScale}
              onClick={() => setTextScale(null)}
              title={`跟随系统文本大小（当前 ${Math.round(systemTextScale * 100)}%）`}
            >
              自动
            </button>
            {TEXT_SCALES.map((item) => (
              <button
                key={item.key}
                type="button"
                aria-pressed={state.ui.textScale === item.value}
                onClick={() => setTextScale(item.value)}
              >
                {item.label}
              </button>
            ))}
          </div>
        </Dropdown>

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
    </div>
  );
}
